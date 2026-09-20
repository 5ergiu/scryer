//! A cour's own name is a name the title answers to, but the catalog keeps it
//! only inside the anime numbering bridge. Every lane that decides which title
//! a release or a file belongs to has to see those names, or a romanized
//! cour-numbered release is discarded before numbering is ever consulted.

use super::*;

fn fma_final_cour_bridge() -> scryer_domain::AnimeNumberingBridge {
    scryer_domain::AnimeNumberingBridge {
        source: Default::default(),
        generated_on: "2026-01-01".into(),
        corroborating_order: None,
        seasons: vec![scryer_domain::AnimeCommunitySeason {
            index: 4,
            titles: vec![
                "Hagane no Renkinjutsushi Saigo no Gassho o Utau Toki no Hikari to Kage no Uta"
                    .into(),
            ],
            absolute_start: Some(37),
            ..Default::default()
        }],
    }
}

const ROMANIZED_COUR_RELEASE: &str = "Hagane no Renkinjutsushi Saigo no Gasshou wo Utau Toki no Hikari to Kage no Uta - 23.720p.WEB-DL.AV1.AAC2.0-NTb";

/// The import lane resolves a file against the monitored-title matcher. Until
/// that matcher carries the bridge cour names, an imported romanized cour file
/// belongs to no title at all, so it can never land on the official episode.
#[tokio::test]
async fn the_import_matcher_answers_to_anime_bridge_cour_names() {
    let (app, user) = bootstrap();
    let title = app
        .add_title(
            &user,
            NewTitle {
                name: "Fullmetal Alchemist Brotherhood".into(),
                facet: MediaFacet::Anime,
                monitored: true,
                ..Default::default()
            },
        )
        .await
        .expect("create anime title");

    app.services
        .catalog
        .shows
        .replace_anime_numbering_bridge(&title.id, Some(&fma_final_cour_bridge()))
        .await
        .expect("store the anime numbering bridge");

    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");
    let parsed = crate::release_parser::parse_release_metadata(ROMANIZED_COUR_RELEASE);
    let resolved = matcher
        .resolve_episode(&parsed, Some("anime"))
        .await
        .expect("resolve episode");

    assert_eq!(
        resolved.map(|resolved| resolved.title.id.clone()),
        Some(title.id.clone()),
        "an imported file named after a bridge cour must resolve to its title"
    );
}

/// The matcher used to be rebuilt by walking the catalog and reading every
/// title's anime numbering bridge. A bridge read that failed transiently then
/// looked exactly like "this title has no bridge", so every scan until the
/// next catalog write ran against a title bank missing every cour alias — and
/// a cour-named file belonged to nobody.
///
/// Cour names are now written into the persisted title index when the bridge
/// itself is written, so the matching lane never reads the shows store: a
/// bridge-read outage cannot reach title matching at all.
#[tokio::test]
async fn a_bridge_read_outage_cannot_reach_title_matching() {
    let shows = std::sync::Arc::new(super::support_library_show::MockShowRepo::default());
    let (app, user) = bootstrap();
    let app = app.with_test_overrides({
        let shows = shows.clone();
        move |services| services.with_shows(shows)
    });
    let title = app
        .add_title(
            &user,
            NewTitle {
                name: "Synthetic Alchemy Chronicle".into(),
                facet: MediaFacet::Anime,
                monitored: true,
                ..Default::default()
            },
        )
        .await
        .expect("create anime title");
    assert_eq!(title.facet, MediaFacet::Anime);

    *shows.fail_anime_bridge.lock().await = true;
    // The outage is live: the shows store itself still fails.
    let error = app
        .services
        .catalog
        .shows
        .get_anime_numbering_bridge(&title.id)
        .await
        .expect_err("the bridge read must fail while the store is down");
    assert!(
        error.to_string().contains("anime numbering bridge"),
        "the store's own failure is what surfaces: {error}"
    );

    // Matching is unaffected: no bridge read stands between a release and its
    // title.
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("a bridge outage must not fail the matcher");
    let parsed = crate::release_parser::parse_release_metadata(
        "Synthetic.Alchemy.Chronicle.S01E01.1080p.WEB-DL.H264-GRP",
    );
    let resolved = matcher
        .resolve_episode(&parsed, Some("anime"))
        .await
        .expect("resolve episode");
    assert_eq!(
        resolved.map(|resolved| resolved.title.id.clone()),
        Some(title.id.clone()),
        "a title still resolves by its own name during a bridge outage"
    );
}
