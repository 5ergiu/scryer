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

pub(super) async fn set_experimental_features(harness: &MediaRequestTestHarness, enabled: bool) {
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

fn refused_as_experimental<T: std::fmt::Debug>(result: AppResult<T>) {
    let error = result.expect_err("lists are off");
    assert!(
        matches!(error, AppError::Validation(ref message) if message.contains("experimental")),
        "unexpected error: {error:?}"
    );
}

#[tokio::test]
async fn list_reads_and_writes_are_refused_while_experimental_features_are_off() {
    let harness = bootstrap_media_request_app();
    let manager = list_manager();
    *harness.lists.subscriptions.lock().unwrap() =
        vec![crate::lists::test_support::subscription("public-list-one")];
    harness
        .lists
        .exclusions
        .lock()
        .unwrap()
        .push(fixture_exclusion(
            "exclusion-one",
            MediaFacet::Movie,
            scryer_domain::ListExclusionScope::AllLists,
        ));
    let app = &harness.app;

    refused_as_experimental(app.list_provider_catalog(&manager).await);
    refused_as_experimental(app.list_provider_settings(&manager).await);
    refused_as_experimental(app.public_list_subscriptions(&manager).await);
    refused_as_experimental(
        app.public_list_subscription(&manager, "public-list-one")
            .await,
    );
    refused_as_experimental(
        app.public_list_memberships(&manager, "public-list-one", 10, 0)
            .await,
    );
    refused_as_experimental(
        app.public_list_sync_runs(&manager, "public-list-one", 10)
            .await,
    );
    refused_as_experimental(app.preview_public_list(&manager, "public-list-one").await);
    refused_as_experimental(app.list_exclusions(&manager).await);
    refused_as_experimental(app.member_list_policies(&manager).await);
    refused_as_experimental(
        app.set_public_list_enabled(&manager, "public-list-one", false)
            .await,
    );
    refused_as_experimental(app.remove_list_exclusion(&manager, "exclusion-one").await);
    refused_as_experimental(
        app.unsubscribe_public_list(&manager, "public-list-one")
            .await,
    );

    assert_eq!(
        harness.lists.subscriptions.lock().unwrap().len(),
        1,
        "a refused unfollow keeps the list"
    );
    assert_eq!(
        harness.lists.exclusions.lock().unwrap().len(),
        1,
        "a refused removal keeps the exclusion"
    );

    set_experimental_features(&harness, true).await;
    assert_eq!(
        app.public_list_subscriptions(&manager).await.unwrap().len(),
        1
    );
    assert_eq!(app.list_exclusions(&manager).await.unwrap().len(), 1);
}

#[tokio::test]
async fn list_data_on_titles_and_requests_is_empty_while_lists_are_off() {
    let harness = bootstrap_media_request_app();
    *harness.lists.subscriptions.lock().unwrap() =
        vec![crate::lists::test_support::subscription("public-list-one")];
    harness
        .titles
        .store
        .lock()
        .await
        .push(make_due_hydration_title(
            "title-alpha",
            MediaFacet::Movie,
            9061,
        ));
    let mut row = crate::lists::test_support::membership(
        "public-list-one",
        "item-one",
        scryer_domain::ListMembershipState::Added,
    );
    row.title_id = Some("title-alpha".to_string());
    harness.lists.insert_rows(vec![row]);
    let request = crate::lists::rejection::tests::rejected_request(
        "request-one",
        scryer_domain::MediaRequestOrigin::PublicList {
            subscription_id: "public-list-one".to_string(),
        },
    );
    let titles = ["title-alpha".to_string()];

    let memberships = harness
        .app
        .public_list_memberships_for_titles(&harness.manager, &titles)
        .await
        .expect("a title page does not fail while lists are off");
    assert!(memberships.is_empty());
    let facts = harness
        .app
        .media_request_policy_facts(&harness.manager, std::slice::from_ref(&request))
        .await
        .expect("a request page does not fail while lists are off");
    assert_eq!(facts["request-one"].public_list_name, None);

    set_experimental_features(&harness, true).await;
    let memberships = harness
        .app
        .public_list_memberships_for_titles(&harness.manager, &titles)
        .await
        .unwrap();
    assert_eq!(memberships["title-alpha"].len(), 1);
    let facts = harness
        .app
        .media_request_policy_facts(&harness.manager, std::slice::from_ref(&request))
        .await
        .unwrap();
    assert_eq!(
        facts["request-one"].public_list_name.as_deref(),
        Some("Fixture list public-list-one")
    );
}

fn movie_library_viewer() -> User {
    let mut viewer = User::new_admin("list-viewer");
    viewer.authorization = scryer_domain::UserAuthorization {
        libraries: HashMap::from([(
            scryer_domain::default_library_id_for_facet(&MediaFacet::Movie),
            scryer_domain::LibraryPermissionMask::from_permissions([
                scryer_domain::LibraryPermission::View,
            ]),
        )]),
        loaded: true,
        ..Default::default()
    };
    viewer
}

#[tokio::test]
async fn a_reader_sees_list_titles_only_in_libraries_they_may_view() {
    use crate::lists::test_support::{membership, route, subscription};
    use scryer_domain::ListMembershipState;

    let harness = bootstrap_media_request_app();
    set_experimental_features(&harness, true).await;
    let movies = scryer_domain::default_library_id_for_facet(&MediaFacet::Movie);
    let series = scryer_domain::default_library_id_for_facet(&MediaFacet::Series);
    let mut followed = subscription("public-list-one");
    followed.kinds = vec![MediaFacet::Movie, MediaFacet::Series, MediaFacet::Anime];
    let mut series_route = route(MediaFacet::Series, &series);
    series_route.quality_profile_id = Some("profile-hidden".to_string());
    series_route.root_folder_id = Some("root-hidden".to_string());
    series_route.tags = vec!["tag-hidden".to_string()];
    let mut movie_route = route(MediaFacet::Movie, &movies);
    movie_route.quality_profile_id = Some("profile-shown".to_string());
    followed.routes = vec![movie_route, series_route];
    *harness.lists.subscriptions.lock().unwrap() = vec![followed];

    let mut in_series_library = make_due_hydration_title("title-hidden", MediaFacet::Series, 9062);
    in_series_library.library_id = series.clone();
    let in_movie_library = make_due_hydration_title("title-shown", MediaFacet::Movie, 9063);
    {
        let mut store = harness.titles.store.lock().await;
        store.push(in_series_library);
        store.push(in_movie_library);
    }
    let row = |key: &str, kind: MediaFacet, title_id: Option<&str>| {
        let mut row = membership("public-list-one", key, ListMembershipState::Added);
        row.kind = kind;
        row.title_id = title_id.map(str::to_string);
        row.request_id = title_id.map(|id| format!("request-{id}"));
        row
    };
    harness.lists.insert_rows(vec![
        row("movie-unadded", MediaFacet::Movie, None),
        row("series-unadded", MediaFacet::Series, None),
        // The title's own library decides, not the route.
        row(
            "movie-in-series-library",
            MediaFacet::Movie,
            Some("title-hidden"),
        ),
        row(
            "series-in-movie-library",
            MediaFacet::Series,
            Some("title-shown"),
        ),
        // No route and no title: only what the provider's list shows.
        row("anime-unrouted", MediaFacet::Anime, None),
    ]);
    let viewer = movie_library_viewer();

    let page = harness
        .app
        .public_list_memberships(&viewer, "public-list-one", 100, 0)
        .await
        .expect("memberships load");
    let mut keys = page
        .items
        .iter()
        .map(|row| row.item_key.as_str())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["anime-unrouted", "movie-unadded", "series-in-movie-library"]
    );
    assert_eq!(page.total_count, 3);

    let seen = harness
        .app
        .public_list_subscription(&viewer, "public-list-one")
        .await
        .unwrap()
        .expect("the list stays visible");
    let shown = &seen.routes[0];
    assert_eq!(shown.library_id, movies);
    assert_eq!(shown.quality_profile_id.as_deref(), Some("profile-shown"));
    let hidden = &seen.routes[1];
    assert_eq!(hidden.library_id, series, "the library id stays");
    assert_eq!(hidden.quality_profile_id, None);
    assert_eq!(hidden.root_folder_id, None);
    assert!(hidden.tags.is_empty());

    let listed = harness
        .app
        .public_list_subscriptions(&viewer)
        .await
        .unwrap();
    assert_eq!(listed[0].routes[1].quality_profile_id, None);

    let everything = harness
        .app
        .public_list_memberships(&harness.manager, "public-list-one", 100, 0)
        .await
        .unwrap();
    assert_eq!(
        everything.total_count, 5,
        "a reader of every library sees all"
    );
}

fn fixture_exclusion(
    id: &str,
    kind: MediaFacet,
    scope: scryer_domain::ListExclusionScope,
) -> scryer_domain::ListExclusion {
    scryer_domain::ListExclusion {
        id: id.to_string(),
        kind,
        external_ids: vec![crate::lists::test_support::tmdb("excluded-id")],
        display_title: "Fixture Excluded Title".to_string(),
        year: None,
        scope,
        created_by_user_id: None,
        created_at: crate::lists::test_support::at(0),
    }
}
