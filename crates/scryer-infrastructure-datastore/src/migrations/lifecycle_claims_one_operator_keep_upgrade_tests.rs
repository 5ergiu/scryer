use sqlx::sqlite::SqlitePoolOptions;

async fn pool_before_upgrade() -> sqlx::SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    super::replay_source_catalog_for_fresh_install(&pool, Some(302), true)
        .await
        .expect("pre-upgrade catalog should apply");
    pool
}

async fn seed_claim(
    pool: &sqlx::SqlitePool,
    id: &str,
    title_id: &str,
    producer: &str,
    state: &str,
    created_at: &str,
) {
    sqlx::query(
        "INSERT INTO lifecycle_claims
            (id, title_id, producer, kind, state, created_at, updated_at)
         VALUES (?, ?, ?, 'keep', ?, ?, ?)",
    )
    .bind(id)
    .bind(title_id)
    .bind(producer)
    .bind(state)
    .bind(created_at)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

async fn claim_state(pool: &sqlx::SqlitePool, id: &str) -> (String, Option<String>) {
    sqlx::query_as("SELECT state, released_reason FROM lifecycle_claims WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn duplicate_operator_keeps_collapse_to_the_oldest() {
    let pool = pool_before_upgrade().await;
    seed_claim(
        &pool,
        "keep-old",
        "title-1",
        "operator_keep",
        "active",
        "2026-01-01T00:00:00Z",
    )
    .await;
    seed_claim(
        &pool,
        "keep-new",
        "title-1",
        "operator_keep",
        "active",
        "2026-02-01T00:00:00Z",
    )
    .await;
    seed_claim(
        &pool,
        "keep-dormant",
        "title-1",
        "operator_keep",
        "dormant",
        "2026-03-01T00:00:00Z",
    )
    .await;
    seed_claim(
        &pool,
        "keep-gone",
        "title-1",
        "operator_keep",
        "released",
        "2025-12-01T00:00:00Z",
    )
    .await;
    seed_claim(
        &pool,
        "keep-other",
        "title-2",
        "operator_keep",
        "active",
        "2026-02-01T00:00:00Z",
    )
    .await;
    seed_claim(
        &pool,
        "lease",
        "title-1",
        "request_permanent",
        "active",
        "2025-11-01T00:00:00Z",
    )
    .await;

    super::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("upgrade should apply");

    assert_eq!(
        claim_state(&pool, "keep-old").await,
        ("active".into(), None)
    );
    for id in ["keep-new", "keep-dormant"] {
        assert_eq!(
            claim_state(&pool, id).await,
            ("released".into(), Some("duplicate_operator_keep".into())),
            "{id}"
        );
    }
    assert_eq!(claim_state(&pool, "keep-gone").await.0, "released");
    assert_eq!(
        claim_state(&pool, "keep-other").await,
        ("active".into(), None)
    );
    assert_eq!(
        claim_state(&pool, "lease").await,
        ("active".into(), None),
        "only operator keeps are collapsed"
    );

    let second = sqlx::query(
        "INSERT INTO lifecycle_claims (id, title_id, producer, kind, state)
         VALUES ('keep-race', 'title-1', 'operator_keep', 'keep', 'active')",
    )
    .execute(&pool)
    .await;
    assert!(second.is_err(), "a second live operator keep is refused");
}
