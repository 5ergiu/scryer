//! Upgrade coverage for migration 0264.
//!
//! Manage Lists joined the full-admin permission set after these masks were
//! stored, and startup refuses to boot without a full admin. The upgrade must
//! hand the new bit to every account that already held the earlier four, and
//! only to those: a narrower mask was never a full admin.
//!
//! The scenario drives the real migration runner over a database built at the
//! pre-0264 state by the real catalog.

use sqlx::SqlitePool;

/// Version the catalog is replayed to before 0264 is applied.
const PRE_UPGRADE_VERSION: i64 = 263;

/// The four full-admin bits that predate Manage Lists.
const LEGACY_FULL_ADMIN: i64 = 15;
const MANAGE_LISTS: i64 = 16;

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
    .expect("pre-0264 migration fixture should apply");
    pool
}

async fn seed_user(pool: &SqlitePool, id: &str, permission_mask: i64) {
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (?1, ?1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .execute(pool)
    .await
    .expect("user should insert");
    sqlx::query(
        "INSERT INTO user_app_permission_masks (user_id, permission_mask, updated_at)
         VALUES (?1, ?2, '2026-01-01T00:00:00Z')",
    )
    .bind(id)
    .bind(permission_mask)
    .execute(pool)
    .await
    .expect("permission mask should insert");
}

async fn mask(pool: &SqlitePool, id: &str) -> (i64, String) {
    sqlx::query_as(
        "SELECT permission_mask, updated_at FROM user_app_permission_masks WHERE user_id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("permission mask should load")
}

#[tokio::test]
async fn the_upgrade_grants_manage_lists_to_existing_full_admins_only() {
    let pool = pre_upgrade_pool().await;
    seed_user(&pool, "full-admin", LEGACY_FULL_ADMIN).await;
    seed_user(&pool, "already-current", LEGACY_FULL_ADMIN | MANAGE_LISTS).await;
    seed_user(&pool, "user-manager", 3).await;
    seed_user(&pool, "member", 0).await;

    crate::migrations::run_migrations(&pool, crate::MigrationMode::Apply)
        .await
        .expect("0264 upgrade should apply");

    let (full_admin, touched_at) = mask(&pool, "full-admin").await;
    assert_eq!(
        full_admin,
        LEGACY_FULL_ADMIN | MANAGE_LISTS,
        "an administrator keeps full-admin status across the upgrade"
    );
    assert_ne!(touched_at, "2026-01-01T00:00:00Z", "the grant is dated");

    let (current, untouched_at) = mask(&pool, "already-current").await;
    assert_eq!(current, LEGACY_FULL_ADMIN | MANAGE_LISTS);
    assert_eq!(untouched_at, "2026-01-01T00:00:00Z", "a current mask is left alone");

    assert_eq!(mask(&pool, "user-manager").await.0, 3, "a partial mask is not promoted");
    assert_eq!(mask(&pool, "member").await.0, 0);
}
