//! A database that still carries the retired `spellfix1` virtual table must
//! survive an upgrade, on a build that has no `spellfix1` module at all.
//!
//! The fixture here is the real hazard, not an approximation of it: a
//! `sqlite_master` row describing a virtual table whose module is gone. Every
//! statement that touches such an object fails with `no such module`,
//! including `DROP TABLE`, and the already-checksummed migrations 0092 and
//! 0236 still name the table. The retirement step rewrites the schema under
//! `PRAGMA writable_schema` and leaves a plain stand-in behind, and this
//! asserts both halves plus the integrity of the resulting schema.

use scryer_infrastructure_datastore::retire_spellfix_virtual_table;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite should open")
}

/// Write the schema row by hand, which is the only way to produce a virtual
/// table whose module this binary cannot load.
async fn install_orphaned_virtual_table(pool: &SqlitePool) {
    sqlx::raw_sql(
        "PRAGMA writable_schema = ON;
         INSERT INTO sqlite_master (type, name, tbl_name, rootpage, sql)
         VALUES ('table', 'title_search_spellfix', 'title_search_spellfix', 0,
                 'CREATE VIRTUAL TABLE title_search_spellfix USING spellfix1');
         PRAGMA writable_schema = RESET;",
    )
    .execute(pool)
    .await
    .expect("the schema row should be writable");

    // The shadow table the module used to own. It is an ordinary table, so it
    // outlives the module and has to be cleaned up too.
    sqlx::query("CREATE TABLE title_search_spellfix_vocab (id INTEGER PRIMARY KEY NOT NULL)")
        .execute(pool)
        .await
        .expect("the shadow table should be created");
}

async fn object_sql(pool: &SqlitePool, name: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT COALESCE(sql, '') FROM sqlite_master WHERE name = ?1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .expect("sqlite_master should be readable")
}

#[tokio::test]
async fn retirement_removes_the_module_less_virtual_table_and_lets_old_migrations_replay() {
    let pool = pool().await;
    install_orphaned_virtual_table(&pool).await;

    // The fixture is only worth anything if it really is unusable first.
    let error = sqlx::query("SELECT COUNT(*) FROM title_search_spellfix")
        .fetch_one(&pool)
        .await
        .expect_err("a virtual table without its module must not be queryable");
    assert!(
        error.to_string().contains("no such module"),
        "unexpected failure: {error}"
    );

    retire_spellfix_virtual_table(&pool)
        .await
        .expect("retirement must succeed");

    let sql = object_sql(&pool, "title_search_spellfix")
        .await
        .expect("the stand-in table should exist");
    assert!(
        !sql.to_ascii_lowercase().contains("spellfix1"),
        "the virtual table must be gone, found: {sql}"
    );
    assert!(
        object_sql(&pool, "title_search_spellfix_vocab")
            .await
            .is_none(),
        "the shadow table must be gone"
    );

    let integrity = sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .expect("integrity_check should run");
    assert_eq!(
        integrity, "ok",
        "rewriting the schema must leave the database intact"
    );

    // What migrations 0092 and 0236 do, verbatim in shape: they are already
    // applied and checksummed, so they still have to run against the stand-in.
    sqlx::raw_sql(
        "CREATE VIRTUAL TABLE IF NOT EXISTS title_search_spellfix USING spellfix1;
         DELETE FROM title_search_spellfix;
         INSERT INTO title_search_spellfix (rowid, word, rank, langid)
         VALUES (1, 'example', 1, 0);
         DELETE FROM title_search_spellfix WHERE rowid = 1;",
    )
    .execute(&pool)
    .await
    .expect("the pre-removal migrations must replay against the stand-in");

    // Idempotent: a second pass changes nothing and still succeeds.
    retire_spellfix_virtual_table(&pool)
        .await
        .expect("retirement must be repeatable");
    assert!(
        object_sql(&pool, "title_search_spellfix").await.is_some(),
        "the stand-in stays until migration 0252 drops it"
    );
}

#[tokio::test]
async fn retirement_is_a_no_op_on_a_database_that_never_had_the_table() {
    let pool = pool().await;
    sqlx::raw_sql(
        "CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY NOT NULL);
         INSERT INTO _sqlx_migrations (version) VALUES (252);",
    )
    .execute(&pool)
    .await
    .unwrap();

    retire_spellfix_virtual_table(&pool)
        .await
        .expect("retirement must succeed");

    assert!(
        object_sql(&pool, "title_search_spellfix").await.is_none(),
        "a database already past 0252 must not get the stand-in back"
    );
}

#[tokio::test]
async fn a_fresh_install_carries_no_spellfix_objects_and_has_the_queue() {
    let pool = pool().await;
    scryer_infrastructure_datastore::migrations::replay_source_catalog_for_fresh_install(
        &pool, None, true,
    )
    .await
    .expect("fresh migrations should apply");

    let leftovers: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE name LIKE 'title_search_spellfix%'
         OR name = 'title_search_terms_delete_spellfix' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        leftovers.is_empty(),
        "migration 0252 must leave nothing behind: {leftovers:?}"
    );

    let queue: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_search_index_queue")
        .fetch_one(&pool)
        .await
        .expect("the fuzzy index queue must exist");
    assert_eq!(queue, 0);
}
