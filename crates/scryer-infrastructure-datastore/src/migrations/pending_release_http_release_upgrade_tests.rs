//! A database last opened by 0.21.20 holds version 304 as "rules disabled
//! unless experimental". The HTTP release column lands after every migration
//! that release shipped, so that database upgrades without a checksum
//! mismatch and gains the column.

use sqlx::sqlite::SqlitePoolOptions;

/// Version 304 as 0.21.20 published it.
const SHIPPED_304_DESCRIPTION: &str = "rules disabled unless experimental";
/// The checksum a 0.21.20 database recorded for version 304.
const SHIPPED_304_CHECKSUM: &str =
    "f15403f21ce0e48c4cb2640c192ecc5ed43598e7048a6cc73df67f1e0497d606";

async fn pool_through(version: i64) -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::replay_source_catalog_for_fresh_install(&pool, Some(version), true)
        .await
        .expect("pre-upgrade catalog should apply");
    pool
}

async fn has_http_release_column(pool: &sqlx::SqlitePool) -> bool {
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('pending_releases')")
            .fetch_all(pool)
            .await
            .unwrap();
    columns.iter().any(|name| name == "http_release_json")
}

#[test]
fn version_304_keeps_its_shipped_identity() {
    let catalog = super::embedded_catalog().unwrap();
    let shipped = catalog.find_migration(304).expect("304 is in the catalog");
    assert_eq!(shipped.description, SHIPPED_304_DESCRIPTION);
    assert_eq!(
        super::migration_assets::checksum_hex(&shipped.checksum),
        SHIPPED_304_CHECKSUM
    );
    let http = catalog
        .migrations
        .iter()
        .find(|migration| migration.description == "pending release http release")
        .expect("the HTTP release migration is in the catalog");
    assert!(http.version > 304, "{}", http.version);
}

#[tokio::test]
async fn a_0_21_20_database_upgrades_and_gains_the_http_release_column() {
    let pool = pool_through(304).await;
    assert!(!has_http_release_column(&pool).await);
    let applied: String =
        sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 304")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(applied, SHIPPED_304_DESCRIPTION);

    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("upgrade must retain the published migration checksums");

    assert!(has_http_release_column(&pool).await);
    let ceiling: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ceiling, super::embedded_catalog().unwrap().max_version());
}
