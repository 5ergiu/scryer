use sqlx::sqlite::SqlitePoolOptions;

const SETTING_PAIRS: [(&str, &str); 3] = [
    (
        "acquisition.default_movie_availability",
        "movie.default_availability",
    ),
    ("acquisition.movie_release_market", "movie.release_market"),
    (
        "acquisition.movie_availability_delay_days",
        "movie.availability_delay_days",
    ),
];

/// Seeds the acquisition definitions as bootstrap wrote them in 0.21.18, with
/// an instance value for each and an unrelated library-scoped value.
async fn seed_acquisition_settings(pool: &sqlx::SqlitePool, values: [&str; 3]) {
    for ((old_key, _), value) in SETTING_PAIRS.iter().zip(values) {
        let definition_id = format!("acquisition:system:{old_key}");
        sqlx::query(
            "INSERT INTO settings_definitions
                (id, category, scope, key_name, data_type, default_value_json, is_sensitive,
                 created_at, updated_at)
             VALUES (?, 'acquisition', 'system', ?, 'string', '\"\"', 0,
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(&definition_id)
        .bind(old_key)
        .execute(pool)
        .await
        .unwrap();
        for (suffix, scope_id) in [("instance", None), ("library", Some("library-fixture"))] {
            sqlx::query(
                "INSERT INTO settings_values
                    (id, setting_definition_id, scope, scope_id, value_json, source,
                     created_at, updated_at)
                 VALUES (?, ?, 'system', ?, ?, 'user',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            )
            .bind(format!("{definition_id}:{suffix}"))
            .bind(&definition_id)
            .bind(scope_id)
            .bind(value)
            .execute(pool)
            .await
            .unwrap();
        }
    }
}

async fn movie_facet_value(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT v.value_json FROM settings_values v
         JOIN settings_definitions d ON d.id = v.setting_definition_id
         WHERE d.category = 'media' AND d.scope = 'system' AND d.key_name = ?
           AND v.scope = 'system' AND v.scope_id = 'movie'",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .unwrap()
}

async fn acquisition_rows(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT
            (SELECT COUNT(*) FROM settings_definitions WHERE key_name LIKE 'acquisition.%movie%')
          + (SELECT COUNT(*) FROM settings_values WHERE setting_definition_id LIKE 'acquisition:%movie%')",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn pool_before_upgrade() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::replay_source_catalog_for_fresh_install(&pool, Some(301), true)
        .await
        .expect("pre-upgrade catalog should apply");
    pool
}

#[tokio::test]
async fn movie_availability_settings_move_to_the_movie_facet() {
    let pool = pool_before_upgrade().await;
    seed_acquisition_settings(&pool, ["\"in_cinemas\"", "\"GB\"", "14"]).await;

    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("upgrade must retain the published migration checksums");

    for ((_, new_key), expected) in SETTING_PAIRS.iter().zip(["\"in_cinemas\"", "\"GB\"", "14"]) {
        assert_eq!(
            movie_facet_value(&pool, new_key).await.as_deref(),
            Some(expected),
            "{new_key}"
        );
    }
    assert_eq!(
        acquisition_rows(&pool).await,
        0,
        "acquisition keys are retired"
    );
    let library_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM settings_values WHERE scope_id = 'library-fixture'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(library_rows, 0, "only the instance value carries over");

    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("later startup must be idempotent");
    assert_eq!(
        movie_facet_value(&pool, "movie.release_market")
            .await
            .as_deref(),
        Some("\"GB\"")
    );
}

#[tokio::test]
async fn an_existing_movie_facet_value_wins_over_the_instance_value() {
    let pool = pool_before_upgrade().await;
    seed_acquisition_settings(&pool, ["\"announced\"", "\"DE\"", "3"]).await;
    sqlx::raw_sql(
        "INSERT INTO settings_definitions
            (id, category, scope, key_name, data_type, default_value_json, is_sensitive,
             created_at, updated_at)
         VALUES ('media:system:movie.release_market', 'media', 'system', 'movie.release_market',
             'string', '\"US\"', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO settings_values
            (id, setting_definition_id, scope, scope_id, value_json, source, created_at, updated_at)
         VALUES ('existing-movie-market', 'media:system:movie.release_market', 'system', 'movie',
             '\"FR\"', 'user', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
    )
    .execute(&pool)
    .await
    .unwrap();

    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("upgrade should apply");

    assert_eq!(
        movie_facet_value(&pool, "movie.release_market")
            .await
            .as_deref(),
        Some("\"FR\"")
    );
    assert_eq!(
        movie_facet_value(&pool, "movie.default_availability")
            .await
            .as_deref(),
        Some("\"announced\"")
    );
    assert_eq!(acquisition_rows(&pool).await, 0);
}

#[tokio::test]
async fn movie_availability_facet_settings_apply_on_fresh_install() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("fresh migration catalog should apply");
    assert_eq!(movie_facet_value(&pool, "movie.release_market").await, None);
    let definitions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM settings_definitions
         WHERE category = 'media' AND key_name LIKE 'movie.%availability%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(definitions, 2);
}
