//! The monitored-title matcher is a catalog-sized structure cached behind a
//! dirty flag. It used to also carry a 60 second freshness window, so a write
//! path that forgot to invalidate merely served a stale matcher for up to a
//! minute instead of forever. That window is gone: invalidation is now the
//! only thing that rebuilds it, which makes a missed write path a permanent
//! correctness bug rather than a delay.
//!
//! One test per write-path family, plus the cache-hit test that proves the
//! matcher is genuinely reused when nothing wrote.

use super::*;

/// Identity of the cached matcher, so a test can tell "rebuilt" from "reused"
/// without reaching into the cache.
async fn matcher_ptr(app: &AppUseCase) -> usize {
    std::sync::Arc::as_ptr(
        &app.monitored_title_matcher()
            .await
            .expect("build the monitored title matcher"),
    ) as usize
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

/// Nothing wrote between the two calls, so the second must be the very same
/// `Arc` — no rebuild, no catalog read. This is the property the removed age
/// fallback used to break once a minute regardless of catalog activity.
#[tokio::test]
async fn two_calls_with_no_write_between_them_return_the_same_matcher() {
    let (app, user) = bootstrap();
    series(&app, &user, "Harbour Lights").await;

    let first = app
        .monitored_title_matcher()
        .await
        .expect("build the matcher");
    let second = app
        .monitored_title_matcher()
        .await
        .expect("reuse the matcher");

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "an unwritten catalog must not rebuild the matcher"
    );
}

/// Creating a title is the write path a library scan takes.
#[tokio::test]
async fn adding_a_title_invalidates_the_matcher() {
    let (app, user) = bootstrap();
    series(&app, &user, "Harbour Lights").await;
    let before = matcher_ptr(&app).await;

    let added = series(&app, &user, "Signal Fire").await;

    assert_ne!(
        before,
        matcher_ptr(&app).await,
        "a new title must dirty the matcher"
    );
    let parsed = crate::release_parser::parse_release_metadata("Signal.Fire.S01E01.1080p.WEB-DL");
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the matcher");
    assert_eq!(
        matcher
            .resolve_episode(&parsed, Some("series"))
            .map(|resolved| resolved.title.id.clone()),
        Some(added.id),
        "the rebuilt matcher resolves the title that was just added"
    );
}

/// A rename changes every key the matcher indexes for that identity.
#[tokio::test]
async fn renaming_a_title_invalidates_the_matcher() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;
    let before = matcher_ptr(&app).await;

    app.update_title_metadata(&user, &title.id, Some("Lantern Bay".into()), None, None)
        .await
        .expect("rename the title");

    assert_ne!(
        before,
        matcher_ptr(&app).await,
        "a rename must dirty the matcher"
    );
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the matcher");
    let renamed = crate::release_parser::parse_release_metadata("Lantern.Bay.S01E01.1080p.WEB-DL");
    assert_eq!(
        matcher
            .resolve_episode(&renamed, Some("series"))
            .map(|resolved| resolved.title.id.clone()),
        Some(title.id.clone()),
        "the rebuilt matcher answers to the new name"
    );
    let old = crate::release_parser::parse_release_metadata("Harbour.Lights.S01E01.1080p.WEB-DL");
    assert!(
        matcher.resolve_episode(&old, Some("series")).is_none(),
        "and no longer to the old one"
    );
}

/// The monitored flag decides whether the title is a resolution candidate at
/// all, so a toggle has to reach the matcher immediately.
#[tokio::test]
async fn toggling_monitoring_invalidates_the_matcher() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;
    let parsed =
        crate::release_parser::parse_release_metadata("Harbour.Lights.S01E01.1080p.WEB-DL");
    assert!(
        app.monitored_title_matcher()
            .await
            .expect("build the matcher")
            .resolve_episode(&parsed, Some("series"))
            .is_some(),
        "a monitored title resolves to begin with"
    );
    let before = matcher_ptr(&app).await;

    app.set_title_monitored(&user, &title.id, false)
        .await
        .expect("unmonitor the title");

    assert_ne!(
        before,
        matcher_ptr(&app).await,
        "a monitor toggle must dirty the matcher"
    );
    assert!(
        app.monitored_title_matcher()
            .await
            .expect("build the matcher")
            .resolve_episode(&parsed, Some("series"))
            .is_none(),
        "an unmonitored title is not a resolution candidate"
    );
}

/// Deleting a title removes the identity; a stale matcher would keep handing
/// out a title id that no longer exists.
#[tokio::test]
async fn deleting_a_title_invalidates_the_matcher() {
    let (app, user) = bootstrap();
    let title = series(&app, &user, "Harbour Lights").await;
    let before = matcher_ptr(&app).await;

    app.delete_title(&user, &title.id, false, None)
        .await
        .expect("delete the title");

    assert_ne!(
        before,
        matcher_ptr(&app).await,
        "a delete must dirty the matcher"
    );
    let parsed =
        crate::release_parser::parse_release_metadata("Harbour.Lights.S01E01.1080p.WEB-DL");
    assert!(
        app.monitored_title_matcher()
            .await
            .expect("build the matcher")
            .resolve_episode(&parsed, Some("series"))
            .is_none(),
        "a deleted title is gone from the matcher"
    );
}

/// The anime numbering bridge carries cour names that reach the matcher only
/// through `title_with_bridge_cour_titles`, so a bridge write is a matcher
/// write.
#[tokio::test]
async fn replacing_the_numbering_bridge_invalidates_the_matcher() {
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
    let before = matcher_ptr(&app).await;

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

    assert_ne!(
        before,
        matcher_ptr(&app).await,
        "a bridge write must dirty the matcher"
    );
    let parsed =
        crate::release_parser::parse_release_metadata("Renkinjutsushi no Yoake - 03.1080p.WEB-DL");
    assert_eq!(
        app.monitored_title_matcher()
            .await
            .expect("build the matcher")
            .resolve_episode(&parsed, Some("anime"))
            .map(|resolved| resolved.title.id.clone()),
        Some(title.id),
        "the rebuilt matcher answers to the cour name the bridge just added"
    );
}
