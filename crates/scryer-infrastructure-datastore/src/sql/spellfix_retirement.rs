//! Retiring the `spellfix1` virtual table, and letting the migrations that
//! predate its removal still replay.
//!
//! The fuzzy title lane is a tantivy index now, so the vendored `spellfix.c`
//! is gone and no SQLite connection this process opens can instantiate a
//! `spellfix1` module. Two consequences have to be handled here rather than
//! in a migration file, because they happen *before* the migration that
//! removes the table could run:
//!
//! 1. **An existing database still holds the virtual table.** Its definition
//!    lives in `sqlite_master`, and any statement that touches it — including
//!    the `DELETE FROM title_search_spellfix` in migration 0236, a `VACUUM`,
//!    or a backup's `integrity_check` — fails with `no such module: spellfix1`.
//!    `DROP TABLE` fails for the same reason, so the row is removed by
//!    rewriting the schema under `PRAGMA writable_schema`, which is the one
//!    supported way to delete an object whose module is unavailable.
//!
//! 2. **Migrations 0092, 0236 and the 0198 baseline are already applied and
//!    checksummed.** Editing them would fail every existing installation's
//!    checksum verification, so they still contain their `title_search_spellfix`
//!    statements. A plain table of that name satisfies all of them —
//!    `CREATE VIRTUAL TABLE IF NOT EXISTS` becomes a no-op when the name is
//!    taken, and the `DELETE`s and the trigger work against an ordinary table
//!    — so this installs one as a stand-in for exactly as long as the replay
//!    needs it. Migration 0252 drops it together with the 0236 trigger.
//!
//! Both steps are idempotent and both are no-ops on a database that has
//! already passed 0252.

use scryer_application::{AppError, AppResult};
use sqlx::{Row, SqlitePool};

/// The migration that drops the stand-in table and the 0236 trigger. A
/// database at or past this version needs neither step below.
const SPELLFIX_REMOVAL_VERSION: i64 = 252;

const SPELLFIX_TABLE: &str = "title_search_spellfix";

fn repo_err(error: impl std::fmt::Display) -> AppError {
    AppError::Repository(error.to_string())
}

/// Remove any `spellfix1` virtual table, then install the plain stand-in the
/// pre-removal migrations need, if they still have to run.
pub async fn retire_spellfix_virtual_table(pool: &SqlitePool) -> AppResult<()> {
    if let Some(sql) = spellfix_object_sql(pool).await?
        && sql.to_ascii_lowercase().contains("using spellfix1")
    {
        drop_virtual_table_via_writable_schema(pool).await?;
    }

    if applied_migration_ceiling(pool).await? < SPELLFIX_REMOVAL_VERSION
        && spellfix_object_sql(pool).await?.is_none()
    {
        install_stand_in_table(pool).await?;
    }
    Ok(())
}

async fn spellfix_object_sql(pool: &SqlitePool) -> AppResult<Option<String>> {
    let row = sqlx::query("SELECT COALESCE(sql, '') AS sql FROM sqlite_master WHERE name = ?1")
        .bind(SPELLFIX_TABLE)
        .fetch_optional(pool)
        .await
        .map_err(repo_err)?;
    row.map(|row| row.try_get::<String, _>("sql").map_err(repo_err))
        .transpose()
}

/// The highest migration version recorded as applied, or 0 when the ledger
/// does not exist yet (a fresh database, which replays everything above the
/// baseline it starts from).
async fn applied_migration_ceiling(pool: &SqlitePool) -> AppResult<i64> {
    let ledger_exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_one(pool)
    .await
    .map_err(repo_err)?;
    if ledger_exists == 0 {
        return Ok(0);
    }
    sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .map(|value| value.unwrap_or(0))
        .map_err(repo_err)
}

/// The only way to delete an object whose module is missing.
///
/// `writable_schema` is left on for the shortest possible window and is
/// always turned off again, including on the error path: a connection that
/// keeps it on would let any later statement corrupt the schema. `RESET`
/// rather than `OFF` so the schema cache is reloaded immediately instead of
/// at the next connection.
async fn drop_virtual_table_via_writable_schema(pool: &SqlitePool) -> AppResult<()> {
    let mut connection = pool.acquire().await.map_err(repo_err)?;

    let result = async {
        sqlx::query("PRAGMA writable_schema = ON")
            .execute(&mut *connection)
            .await
            .map_err(repo_err)?;
        sqlx::query("DELETE FROM sqlite_master WHERE name = ?1 OR tbl_name = ?1")
            .bind(SPELLFIX_TABLE)
            .execute(&mut *connection)
            .await
            .map_err(repo_err)?;
        Ok::<(), AppError>(())
    }
    .await;

    // RESET runs whatever happened above, and its own failure must not mask
    // the original one.
    let reset = sqlx::query("PRAGMA writable_schema = RESET")
        .execute(&mut *connection)
        .await
        .map_err(repo_err);
    result?;
    reset?;

    sqlx::query("DROP TABLE IF EXISTS title_search_spellfix_vocab")
        .execute(&mut *connection)
        .await
        .map_err(repo_err)?;

    tracing::info!("removed the retired spellfix1 virtual table from the schema");
    Ok(())
}

/// The columns the pre-removal migrations reference: `rowid` for the 0236
/// trigger and its repair `DELETE`, and `word` because the old typo lane
/// selected it. Nothing reads this table any more; it exists only so the
/// replay of already-applied SQL does not need a module that is gone.
async fn install_stand_in_table(pool: &SqlitePool) -> AppResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS title_search_spellfix (
             rowid INTEGER PRIMARY KEY NOT NULL,
             word TEXT,
             rank INTEGER,
             langid INTEGER
         )",
    )
    .execute(pool)
    .await
    .map_err(repo_err)?;
    Ok(())
}
