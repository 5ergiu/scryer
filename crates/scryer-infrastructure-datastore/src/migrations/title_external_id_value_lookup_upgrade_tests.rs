use sqlx::sqlite::SqlitePoolOptions;

const SQLITE: &str =
    include_str!("../../../scryer/src/db/migrations/0301_title_external_id_value_lookup.sql");
const POSTGRES: &str = include_str!(
    "../../../scryer/src/db/postgres/migrations/0301_title_external_id_value_lookup.sql"
);
const INDEX_NAME: &str = "idx_title_external_ids_source_value_nocase";
/// The cross-library lookup the title store runs for each id a release carries.
const LOOKUP_PLAN: &str = "EXPLAIN QUERY PLAN SELECT DISTINCT title_id FROM title_external_ids \
     WHERE source = 'imdb' AND LOWER(external_id) = LOWER('tt0004242')";

async fn assert_sqlite_index(pool: &sqlx::SqlitePool) {
    let definition: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?")
            .bind(INDEX_NAME)
            .fetch_one(pool)
            .await
            .expect("lookup index should exist");
    assert!(definition.contains("LOWER(external_id)"), "{definition}");

    let rows: Vec<(i64, i64, i64, String)> = sqlx::query_as(LOOKUP_PLAN)
        .fetch_all(pool)
        .await
        .expect("query plan should load");
    let plan = rows.into_iter().map(|row| row.3).collect::<Vec<_>>();
    assert!(
        plan.iter().any(|detail| detail.contains(&format!(
            "SEARCH title_external_ids USING INDEX {INDEX_NAME} (source=? AND <expr>=?)"
        ))),
        "the external id lookup should search the source/value index: {plan:?}"
    );
}

#[tokio::test]
async fn title_external_id_value_lookup_applies_on_fresh_install() {
    assert_eq!(SQLITE, POSTGRES, "both engines create the same index");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("fresh migration catalog should apply");
    assert_sqlite_index(&pool).await;
}

#[tokio::test]
async fn title_external_id_value_lookup_upgrades_from_the_previous_catalog() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::replay_source_catalog_for_fresh_install(&pool, Some(300), true)
        .await
        .expect("pre-upgrade catalog should apply");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = ?")
        .bind(INDEX_NAME)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "{INDEX_NAME} must be introduced after 300");
    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("upgrade must retain the published migration checksums");
    assert_sqlite_index(&pool).await;
    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("later startup must be idempotent");
    assert_sqlite_index(&pool).await;
}

#[tokio::test]
async fn title_external_id_value_lookup_postgres_applies_idempotently() {
    let Ok(url) = std::env::var("SCRYER_TEST_POSTGRES_URL") else {
        eprintln!("skipped: SCRYER_TEST_POSTGRES_URL must name an isolated test database");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    // A temporary table inside a rolled-back transaction keeps the fixture
    // isolated from the database the URL names.
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "CREATE TEMP TABLE title_external_ids (
            id TEXT PRIMARY KEY, title_id TEXT NOT NULL, source TEXT NOT NULL,
            external_id TEXT NOT NULL);
         INSERT INTO title_external_ids VALUES
            ('upper', 'fixture-upper', 'imdb', 'TT0004242'),
            ('lower', 'fixture-lower', 'imdb', 'tt0004242'),
            ('other', 'fixture-other', 'tvdb', '0004242');",
    )
    .execute(&mut *tx)
    .await
    .unwrap();
    for _ in 0..2 {
        sqlx::raw_sql(POSTGRES).execute(&mut *tx).await.unwrap();
    }
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT indexname FROM pg_indexes
         WHERE schemaname = (SELECT nspname FROM pg_namespace WHERE oid = pg_my_temp_schema())
           AND indexname = $1",
    )
    .bind(INDEX_NAME)
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(names, [INDEX_NAME]);
    let titles: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT title_id FROM title_external_ids
         WHERE source = 'imdb' AND LOWER(external_id) = LOWER('tt0004242')
         ORDER BY title_id",
    )
    .fetch_all(&mut *tx)
    .await
    .unwrap();
    assert_eq!(titles, ["fixture-lower", "fixture-upper"]);
    tx.rollback().await.unwrap();
}
