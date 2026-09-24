//! The fuzzy index's claim queue, against the real merge engine.
//!
//! `title_search_index_queue` is what keeps the tantivy index honest, and it
//! only works if a title is claimed exactly when its projection changes and
//! never when a write rolls back. Both halves are exercised here: the Rust
//! projection writer's enqueue, which rides in the caller's transaction, and
//! the `titles` delete trigger, which covers the rows that
//! `ON DELETE CASCADE` removes without any Rust running.

use super::*;
use scryer_application::TitleRepository;
use scryer_infrastructure_library_search::replace_title_search_projection_tx;
use sqlx::SqlitePool;

fn pool(datastore: &StoreDatastore) -> &SqlitePool {
    let StoreDatastore::Sqlite { pool, .. } = datastore else {
        panic!("SQLite fixture required")
    };
    pool
}

async fn claimed_titles(datastore: &StoreDatastore) -> Vec<String> {
    sqlx::query_scalar("SELECT title_id FROM title_search_index_queue ORDER BY seq")
        .fetch_all(pool(datastore))
        .await
        .unwrap()
}

async fn drain_queue(datastore: &StoreDatastore) {
    sqlx::query("DELETE FROM title_search_index_queue")
        .execute(pool(datastore))
        .await
        .unwrap();
}

async fn seed_search(datastore: &StoreDatastore) {
    insert_title(datastore, SOURCE, "library-a", &[]).await;
    insert_title(datastore, DESTINATION, "library-b", &[]).await;
    run(datastore,
        "INSERT INTO libraries (id, facet, name, slug, is_default, created_at, updated_at)
         VALUES ('library-a', 'series', 'A', 'a', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        vec![]).await;
    let titles = crate::media::titles::store::TitleStore::new(datastore.clone());
    for id in [SOURCE, DESTINATION] {
        let title = titles.get_by_id(id).await.unwrap().unwrap();
        let mut tx = pool(datastore).begin().await.unwrap();
        replace_title_search_projection_tx(&mut tx, &title)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
    assert_eq!(
        claimed_titles(datastore).await,
        vec![SOURCE.to_string(), DESTINATION.to_string()],
        "writing a projection must claim its title for the index"
    );
}

async fn transfer_survivor(datastore: &StoreDatastore) -> AppResult<()> {
    crate::media::titles::store::TitleStore::new(datastore.clone())
        .transfer_to_library(DESTINATION, "library-a", "root-library-a", None, &[])
        .await
}

#[tokio::test]
async fn index_claims_follow_merge_rollback_retry_and_title_cascade() {
    let (store, datastore) = test_store().await;
    seed_search(&datastore).await;
    drain_queue(&datastore).await;

    let plan = plan_for(&store).await;
    run(
        &datastore,
        "CREATE TRIGGER fail_title_retirement BEFORE DELETE ON titles
         BEGIN SELECT RAISE(ABORT, 'injected retirement failure'); END",
        vec![],
    )
    .await;
    store.execute_title_merge(&plan).await.unwrap_err();
    assert!(
        claimed_titles(&datastore).await.is_empty(),
        "a merge that rolled back must claim nothing: the index would rebuild \
         a title whose projection never changed"
    );
    run(&datastore, "DROP TRIGGER fail_title_retirement", vec![]).await;

    store.execute_title_merge(&plan).await.unwrap();
    let claims = claimed_titles(&datastore).await;
    assert!(
        claims.iter().any(|claimed| claimed == SOURCE),
        "the merge retires the source, so its projection rows go and the \
         index must be told: {claims:?}"
    );
    assert_eq!(
        scalar(
            &datastore,
            "SELECT COUNT(*) AS row_count FROM title_search_terms
             WHERE title_id NOT IN (SELECT id FROM titles)",
            vec![]
        )
        .await,
        0,
        "the retired title's projection rows must be gone"
    );

    transfer_survivor(&datastore).await.unwrap();
    transfer_survivor(&datastore).await.unwrap();
    assert_eq!(
        text(
            &datastore,
            "SELECT library_id AS value FROM titles WHERE id = {}",
            vec![SqlArg::Text(DESTINATION.into())]
        )
        .await
        .as_deref(),
        Some("library-a")
    );

    // The cascade case: deleting the title removes its projection rows through
    // ON DELETE CASCADE, so no Rust writer runs and the trigger is the only
    // thing that can claim it.
    drain_queue(&datastore).await;
    run(
        &datastore,
        "DELETE FROM titles WHERE id = {}",
        vec![SqlArg::Text(DESTINATION.into())],
    )
    .await;
    assert_eq!(
        claimed_titles(&datastore).await,
        vec![DESTINATION.to_string()],
        "a cascade-deleted title must still be claimed for the index"
    );
    assert_eq!(
        scalar(
            &datastore,
            "SELECT COUNT(*) AS row_count FROM title_search_terms",
            vec![]
        )
        .await,
        0
    );
}
