//! The monitored-title matcher used to be a catalog-sized structure cached
//! behind a dirty flag: every write path had to remember to invalidate it, and
//! a path that forgot served a stale matcher forever.
//!
//! The matcher now holds nothing but a repository handle and asks the
//! persisted title index per release, so there is no cache to miss and no
//! invalidation to forget. These tests keep the behaviour each invalidation
//! test guarded — one per write-path family — and assert it the strict way:
//! the very next resolution reflects the write, with no invalidation call in
//! between.

use super::*;

async fn matcher(
    app: &AppUseCase,
) -> std::sync::Arc<crate::import_title_resolution::MonitoredTitleMatcher> {
    app.monitored_title_matcher()
        .await
        .expect("build the monitored title matcher")
}

async fn resolved_episode_id(app: &AppUseCase, release: &str, facet: &str) -> Option<String> {
    let parsed = crate::release_parser::parse_release_metadata(release);
    matcher(app)
        .await
        .resolve_episode(&parsed, Some(facet))
        .await
        .expect("resolve episode")
        .map(|resolved| resolved.title.id)
}

async fn series(app: &AppUseCase, user: &User, name: &str) -> Title {
    app.add_title(
        user,
        NewTitle {
            name: name.into(),
            facet: MediaFacet::Series,
            monitored: true,
            ..Default::default()
        },
    )
    .await
    .expect("create title")
}

/// A matcher handle taken before a write must still see the write: it holds no
/// snapshot of the catalog, so there is no window in which it can answer from
/// stale state.
#[tokio::test]
async fn a_matcher_taken_before_a_write_still_sees_the_write() {
    let (app, user) = bootstrap();
    series(&app, &user, "Harbour Lights").await;
    let held = matcher(&app).await;

    let added = series(&app, &user, "Signal Fire").await;

    let parsed = crate::release_parser::parse_release_metadata("Signal.Fire.S01E01.1080p.WEB-DL");
    assert_eq!(
        held.resolve_episode(&parsed, Some("series"))
            .await
            .expect("resolve episode")
            .map(|resolved| resolved.title.id),
        Some(added.id),
        "a handle taken before the write must not hold a stale catalog"
    );
}

/// Creating a title is the write path a library scan takes.
#[tokio::test]
async fn adding_a_title_is_immediately_matchable() {
    let (app, user) = bootstrap();
    series(&app, &user, "Harbour Lights").await;

    let added = series(&app, &user, "Signal Fire").await;

    assert_eq!(
        resolved_episode_id(&app, "Signal.Fire.S01E01.1080p.WEB-DL", "series").await,
        Some(added.id),
        "the matcher resolves the title that was just added"
    );
}

/// A rename changes every key the matcher looks up for that identity.
#[tokio::test]
async fn renaming_a_title_is_immediately_matchable() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;

    app.update_title_metadata(&user, &title.id, Some("Lantern Bay".into()), None, None)
        .await
        .expect("rename the title");

    assert_eq!(
        resolved_episode_id(&app, "Lantern.Bay.S01E01.1080p.WEB-DL", "series").await,
        Some(title.id.clone()),
        "the matcher answers to the new name"
    );
    assert_eq!(
        resolved_episode_id(&app, "Harbour.Lights.S01E01.1080p.WEB-DL", "series").await,
        None,
        "and no longer to the old one"
    );
}

/// The monitored flag decides whether the title is a resolution candidate at
/// all, so a toggle has to reach the matcher immediately.
#[tokio::test]
async fn toggling_monitoring_is_immediately_visible() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;
    assert_eq!(
        resolved_episode_id(&app, "Harbour.Lights.S01E01.1080p.WEB-DL", "series").await,
        Some(title.id.clone()),
        "a monitored title resolves to begin with"
    );

    app.set_title_monitored(&user, &title.id, false)
        .await
        .expect("unmonitor the title");

    assert_eq!(
        resolved_episode_id(&app, "Harbour.Lights.S01E01.1080p.WEB-DL", "series").await,
        None,
        "an unmonitored title is not a resolution candidate"
    );
}

/// Deleting a title removes the identity; a stale matcher would keep handing
/// out a title id that no longer exists.
#[tokio::test]
async fn deleting_a_title_is_immediately_visible() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;

    app.delete_title(&user, &title.id, false, None)
        .await
        .expect("delete the title");

    assert_eq!(
        resolved_episode_id(&app, "Harbour.Lights.S01E01.1080p.WEB-DL", "series").await,
        None,
        "a deleted title is gone from the matcher"
    );
}

/// The anime numbering bridge carries cour names the title answers to. They
/// reach matching through the persisted title index, written with the bridge,
/// so a bridge write is immediately a matchable name.
#[tokio::test]
async fn replacing_the_numbering_bridge_is_immediately_matchable() {
    let (app, user) = bootstrap();
    let title = app
        .add_title(
            &user,
            NewTitle {
                name: "Alchemy Chronicle".into(),
                facet: MediaFacet::Anime,
                monitored: true,
                ..Default::default()
            },
        )
        .await
        .expect("create anime title");

    app.replace_numbering_bridge_after_hydration(
        &title,
        Some(&scryer_domain::AnimeNumberingBridge {
            source: Default::default(),
            generated_on: "2026-01-01".into(),
            corroborating_order: None,
            seasons: vec![scryer_domain::AnimeCommunitySeason {
                index: 2,
                titles: vec!["Renkinjutsushi no Yoake".into()],
                absolute_start: Some(13),
                ..Default::default()
            }],
        }),
        &[],
    )
    .await;

    assert_eq!(
        resolved_episode_id(&app, "Renkinjutsushi no Yoake - 03.1080p.WEB-DL", "anime").await,
        Some(title.id),
        "the matcher answers to the cour name the bridge just added"
    );
}
