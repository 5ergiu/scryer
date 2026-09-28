//! Lists ship behind the experimental-features switch: while it is off, a
//! manager cannot follow or sync a public list and the sync job idles.

use super::*;
use crate::lists::sync::ListSyncReport;

fn list_manager() -> User {
    let mut manager = User::new_admin("list-manager");
    manager.authorization = scryer_domain::UserAuthorization {
        app: AppPermissionMask::MANAGE_LISTS,
        loaded: true,
        ..Default::default()
    };
    manager
}

async fn set_experimental_features(harness: &MediaRequestTestHarness, enabled: bool) {
    harness
        .app
        .services
        .config
        .settings
        .upsert_setting_json(
            SETTINGS_SCOPE_SYSTEM,
            crate::settings::keys::EXPERIMENTAL_FEATURES_ENABLED_KEY,
            None,
            enabled.to_string(),
            "test",
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn public_list_sync_is_refused_until_experimental_features_are_on() {
    let harness = bootstrap_media_request_app();
    let manager = list_manager();

    let refused = harness
        .app
        .sync_all_public_lists(&manager)
        .await
        .expect_err("lists stay off by default");
    assert!(
        matches!(refused, AppError::Validation(ref message) if message.contains("experimental")),
        "unexpected error: {refused:?}"
    );
    assert_eq!(
        harness.app.run_list_sync_job(None).await.unwrap(),
        ListSyncReport::default(),
        "the sync job idles while lists are off"
    );

    set_experimental_features(&harness, true).await;
    assert_eq!(
        harness
            .app
            .sync_all_public_lists(&manager)
            .await
            .expect("lists open once the switch is on"),
        Vec::<String>::new()
    );

    set_experimental_features(&harness, false).await;
    harness
        .app
        .sync_all_public_lists(&manager)
        .await
        .expect_err("turning the switch back off closes lists again");
}

async fn list_sync_starts(harness: &MediaRequestTestHarness) -> usize {
    harness
        .domain_events
        .events
        .lock()
        .await
        .iter()
        .filter(|event| {
            matches!(
                &event.payload,
                DomainEventPayload::JobRunStarted(data) if data.job_key == "list_sync"
            )
        })
        .count()
}

#[tokio::test]
async fn sync_now_clears_the_stored_fingerprint_so_the_list_is_read_in_full() {
    let harness = bootstrap_media_request_app();
    set_experimental_features(&harness, true).await;
    let mut followed = crate::lists::test_support::subscription("public-list-one");
    followed.sync.fetch_fingerprint = Some("fingerprint-one".to_string());
    *harness.lists.subscriptions.lock().unwrap() = vec![followed];

    let queued = harness
        .app
        .sync_public_list_now(&list_manager(), "public-list-one")
        .await
        .expect("sync queued");

    assert_eq!(queued, vec!["public-list-one".to_string()]);
    let stored = harness.lists.subscription("public-list-one");
    assert_eq!(stored.sync.fetch_fingerprint, None);
    assert!(stored.sync.next_at.is_some());
    assert_eq!(list_sync_starts(&harness).await, 1);
}

#[tokio::test]
async fn following_a_list_starts_its_first_sync_at_once() {
    let harness = bootstrap_media_request_app();
    set_experimental_features(&harness, true).await;

    let followed = harness
        .app
        .subscribe_public_list(
            &list_manager(),
            crate::lists::public::PublicListInput {
                provider: Some(crate::lists::catalog::LIST_PROVIDER_IMDB.to_string()),
                source_type: Some(
                    crate::lists::catalog::LIST_SOURCE_TYPE_IMDB_USER_LIST.to_string(),
                ),
                params: std::collections::BTreeMap::from([(
                    scryer_domain::LIST_SOURCE_IMDB_LIST_ID_PARAM.to_string(),
                    "ls0000001".to_string(),
                )]),
                mode: scryer_domain::ListMode::Add,
                ..Default::default()
            },
        )
        .await
        .expect("list followed");

    let stored = harness.lists.subscription(&followed.id);
    assert!(stored.sync.next_at.is_some(), "the new list is due now");
    assert_eq!(
        list_sync_starts(&harness).await,
        1,
        "following starts the first sync instead of waiting for the schedule"
    );
}
