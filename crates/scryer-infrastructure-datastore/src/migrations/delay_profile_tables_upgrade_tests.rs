//! The JSON delay profile catalog moves into the 0293 tables and the setting
//! is removed.

use sqlx::SqlitePool;

/// Version the catalog is replayed to before 0293 is applied.
const PRE_UPGRADE_VERSION: i64 = 292;

type MigratedDelayProfileRow = (
    String,
    i64,
    i64,
    String,
    i64,
    Option<i64>,
    bool,
    bool,
    bool,
    i64,
    bool,
);

async fn pre_upgrade_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("in-memory SQLite should open");
    crate::migrations::replay_source_catalog_for_fresh_install(
        &pool,
        Some(PRE_UPGRADE_VERSION),
        true,
    )
    .await
    .expect("pre-0293 migration fixture should apply");
    pool
}

async fn seed_catalog(pool: &SqlitePool, catalog: &str) {
    sqlx::query(
        "INSERT INTO settings_definitions
             (id, category, scope, key_name, data_type, default_value_json, is_sensitive,
              created_at, updated_at)
         VALUES ('delay-profiles', 'acquisition', 'system', 'acquisition.delay_profiles',
                 'json', '[]', 0, '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
    )
    .execute(pool)
    .await
    .expect("definition should insert");
    sqlx::query(
        "INSERT INTO settings_values
             (id, setting_definition_id, scope, scope_id, value_json, source, created_at,
              updated_at)
         VALUES ('delay-profiles-value', 'delay-profiles', 'system', NULL, ?1, 'test',
                 '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
    )
    .bind(catalog)
    .execute(pool)
    .await
    .expect("value should insert");
}

async fn strings(pool: &SqlitePool, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_all(pool)
        .await
        .unwrap_or_else(|error| panic!("query failed: {error}\n{sql}"))
}

#[tokio::test]
async fn the_json_catalog_moves_into_the_tables_and_the_setting_is_removed() {
    let pool = pre_upgrade_pool().await;
    seed_catalog(
        &pool,
        r#"[
            {"id":"anime-hold","name":"Anime hold","usenet_delay_minutes":30,
             "torrent_delay_minutes":90,"preferred_protocol":"torrent","min_age_minutes":5,
             "bypass_score_threshold":1200,"enable_usenet":false,"bypass_if_highest_quality":true,
             "applies_to_facets":["anime","series"],"tags":["fansub","keep"],"priority":2},
            {"id":"catch-all","name":"Catch all","usenet_delay_minutes":60,"enabled":false}
        ]"#,
    )
    .await;

    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("0293 upgrade should apply");

    let row: MigratedDelayProfileRow = sqlx::query_as(
        "SELECT name, usenet_delay_minutes, torrent_delay_minutes, preferred_protocol,
                    min_age_minutes, bypass_score_threshold, enable_usenet, enable_torrent,
                    bypass_if_highest_quality, priority, enabled
               FROM delay_profiles WHERE id = 'anime-hold'",
    )
    .fetch_one(&pool)
    .await
    .expect("imported profile should exist");
    assert_eq!(
        row,
        (
            "Anime hold".to_string(),
            30,
            90,
            "torrent".to_string(),
            5,
            Some(1200),
            false,
            true,
            true,
            2,
            true
        )
    );
    assert_eq!(
        strings(
            &pool,
            "SELECT facet FROM delay_profile_facets WHERE profile_id = 'anime-hold'
              ORDER BY position"
        )
        .await,
        vec!["anime", "series"]
    );
    assert_eq!(
        strings(
            &pool,
            "SELECT label FROM delay_profile_tags WHERE profile_id = 'anime-hold'
              ORDER BY position"
        )
        .await,
        vec!["fansub", "keep"]
    );
    assert!(
        strings(&pool, "SELECT library_id FROM delay_profile_libraries")
            .await
            .is_empty(),
        "imported profiles keep applying to every library"
    );

    let catch_all: (i64, bool) = sqlx::query_as(
        "SELECT usenet_delay_minutes, enabled FROM delay_profiles WHERE id = 'catch-all'",
    )
    .fetch_one(&pool)
    .await
    .expect("second profile should exist");
    assert_eq!(catch_all, (60, false));

    assert!(
        strings(
            &pool,
            "SELECT id FROM settings_definitions WHERE key_name = 'acquisition.delay_profiles'"
        )
        .await
        .is_empty(),
        "the setting definition is gone"
    );
    assert!(
        strings(
            &pool,
            "SELECT id FROM settings_values WHERE id = 'delay-profiles-value'"
        )
        .await
        .is_empty(),
        "the setting value is gone"
    );
}

#[tokio::test]
async fn a_fresh_upgrade_without_a_catalog_creates_empty_tables() {
    let pool = pre_upgrade_pool().await;
    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("0293 upgrade should apply");
    assert!(
        strings(&pool, "SELECT id FROM delay_profiles")
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_malformed_catalog_stops_the_upgrade_and_keeps_the_setting() {
    let pool = pre_upgrade_pool().await;
    seed_catalog(&pool, "not json").await;

    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect_err("a malformed catalog should fail the upgrade");

    assert_eq!(
        strings(
            &pool,
            "SELECT value_json FROM settings_values WHERE id = 'delay-profiles-value'"
        )
        .await,
        vec!["not json"],
        "the failed migration rolls back and leaves the stored catalog alone"
    );
}
