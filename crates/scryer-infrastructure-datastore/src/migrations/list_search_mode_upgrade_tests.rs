//! "Add and search" lists become "Add" lists. Their unmonitored routes take
//! the monitoring that keeps the search they had; nothing else changes.

use sqlx::SqlitePool;

/// Version the catalog is replayed to before 0291 is applied.
const PRE_UPGRADE_VERSION: i64 = 290;

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
    .expect("pre-0291 migration fixture should apply");
    pool
}

async fn exec(pool: &SqlitePool, sql: &str, binds: &[&str]) {
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.to_string()));
    for bind in binds {
        query = query.bind(*bind);
    }
    query
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("fixture statement failed: {error}\n{sql}"));
}

async fn seed_follow(pool: &SqlitePool, id: &str, mode: &str, routes: &[(&str, &str)]) {
    exec(
        pool,
        "INSERT INTO list_subscriptions (id, scope, owner_user_id, provider, source_type,
             source_origin, name, mode, interval_seconds, created_at, updated_at)
         VALUES (?1, 'public', 'list-manager', 'tmdb', 'smg_chart:tmdb.fixture:global',
                 'smg_chart', 'Synthetic list ' || ?1, ?2, 43200, '2026-09-01',
                 '2026-09-01')",
        &[id, mode],
    )
    .await;
    for (kind, monitor_type) in routes {
        exec(
            pool,
            "INSERT INTO list_subscription_routes (subscription_id, kind, library_id,
                 monitor_type)
             SELECT ?1, ?2, id, ?3 FROM libraries WHERE facet = ?2 AND is_default = 1",
            &[id, kind, monitor_type],
        )
        .await;
    }
}

async fn mode(pool: &SqlitePool, id: &str) -> String {
    sqlx::query_scalar("SELECT mode FROM list_subscriptions WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("follow should load")
}

async fn monitor_type(pool: &SqlitePool, id: &str, kind: &str) -> String {
    sqlx::query_scalar(
        "SELECT monitor_type FROM list_subscription_routes
         WHERE subscription_id = ?1 AND kind = ?2",
    )
    .bind(id)
    .bind(kind)
    .fetch_one(pool)
    .await
    .expect("route should load")
}

#[tokio::test]
async fn search_lists_become_add_lists_and_unmonitored_routes_keep_their_search() {
    let pool = pre_upgrade_pool().await;
    exec(
        &pool,
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES ('list-manager', 'list-manager', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z')",
        &[],
    )
    .await;
    seed_follow(
        &pool,
        "search-unmonitored",
        "search",
        &[
            ("movie", "UNMONITORED"),
            ("series", "NONE"),
            ("anime", "none"),
        ],
    )
    .await;
    seed_follow(
        &pool,
        "search-monitored",
        "search",
        &[
            ("movie", "MONITORED"),
            ("series", "ALL_EPISODES"),
            ("anime", "MISSING_AND_FUTURE_EPISODES"),
        ],
    )
    .await;
    seed_follow(
        &pool,
        "add-unmonitored",
        "add",
        &[("movie", "UNMONITORED"), ("series", "NONE")],
    )
    .await;
    seed_follow(
        &pool,
        "hold-unmonitored",
        "hold",
        &[("movie", "UNMONITORED")],
    )
    .await;

    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("0291 upgrade should apply");

    for id in ["search-unmonitored", "search-monitored", "add-unmonitored"] {
        assert_eq!(mode(&pool, id).await, "add", "{id} becomes an Add list");
    }
    assert_eq!(mode(&pool, "hold-unmonitored").await, "hold");

    let expected = [
        ("search-unmonitored", "movie", "MONITORED"),
        ("search-unmonitored", "series", "FUTURE_EPISODES"),
        ("search-unmonitored", "anime", "FUTURE_EPISODES"),
        ("search-monitored", "movie", "MONITORED"),
        ("search-monitored", "series", "ALL_EPISODES"),
        ("search-monitored", "anime", "MISSING_AND_FUTURE_EPISODES"),
        ("add-unmonitored", "movie", "UNMONITORED"),
        ("add-unmonitored", "series", "NONE"),
        ("hold-unmonitored", "movie", "UNMONITORED"),
    ];
    for (id, kind, monitor) in expected {
        assert_eq!(
            monitor_type(&pool, id, kind).await,
            monitor,
            "{id} {kind} route monitoring"
        );
    }
}
