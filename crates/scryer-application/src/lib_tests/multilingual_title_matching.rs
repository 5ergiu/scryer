//! One title, many spellings, every release shape.
//!
//! A title is rarely released under the name the catalog stores. An anime
//! carries a native name, one or more romanizations that disagree about long
//! vowels and particles, an English licence title and a cour name that only
//! the numbering bridge knows; a release then wraps whichever of those the
//! group preferred in a season, cour, pack or absolute-numbering shape. Every
//! consumer that decides "which title is this" has to answer the same for all
//! of them, because they are all the same subject.
//!
//! These cases go through the identity gate — the monitored-title matcher the
//! import path, the tracked-download sweep and the acquisition lanes all
//! resolve through — so what is asserted is the resolution itself rather than
//! any one caller's wrapper. The index behind it is covered where it is real:
//! `scryer-infrastructure-runtime`'s `title_fuzzy_index` and
//! `title_name_candidates` tests run the same lanes against tantivy and SQL.

use super::*;

/// The subject: an anime whose catalog name is the native one, with the
/// romanization and licence names as aliases, exactly as a metadata provider
/// would leave it.
const NATIVE_NAME: &str = "蒼雲の記録";
const ROMANIZED_NAME: &str = "Aokumo no Kiroku";
const ROMANIZED_LONG_VOWEL: &str = "Aokumo no Kirokuu";
const LICENCE_NAME: &str = "Record of the Blue Cloud";

/// The cour names a community source actually publishes for a second cour:
/// its own subtitle, and the季/期 and "2nd Season" shapes groups copy from it.
fn second_cour_bridge() -> scryer_domain::AnimeNumberingBridge {
    scryer_domain::AnimeNumberingBridge {
        source: Default::default(),
        generated_on: "2026-01-01".into(),
        corroborating_order: None,
        seasons: vec![scryer_domain::AnimeCommunitySeason {
            index: 2,
            titles: vec![
                "Aokumo no Kiroku: Hikari no Shou".into(),
                "Aokumo no Kiroku 2nd Season".into(),
                "Record of the Blue Cloud Part 2".into(),
                "蒼雲の記録 第2期".into(),
            ],
            absolute_start: Some(13),
            ..Default::default()
        }],
    }
}

async fn multilingual_subject() -> (AppUseCase, User, scryer_domain::Title) {
    let (app, user) = bootstrap();
    let title = app
        .add_title(
            &user,
            NewTitle {
                name: NATIVE_NAME.into(),
                facet: MediaFacet::Anime,
                monitored: true,
                ..Default::default()
            },
        )
        .await
        .expect("create anime title");

    app.services
        .catalog
        .titles
        .update_title_hydrated_metadata(
            &title.id,
            TitleMetadataUpdate {
                aliases: vec![
                    ROMANIZED_NAME.to_string(),
                    ROMANIZED_LONG_VOWEL.to_string(),
                    LICENCE_NAME.to_string(),
                ],
                ..Default::default()
            },
        )
        .await
        .expect("store the provider's alias set");

    app.services
        .catalog
        .shows
        .replace_anime_numbering_bridge(&title.id, Some(&second_cour_bridge()))
        .await
        .expect("store the anime numbering bridge");

    let title = app
        .services
        .catalog
        .titles
        .get_by_id(&title.id)
        .await
        .expect("reload the title")
        .expect("the title exists");
    (app, user, title)
}

/// Release shapes, each in the three spellings the subject answers to.
///
/// Every one of these names the same title. A consumer that resolves one and
/// not another is not doing bounded-distance matching, it is matching the one
/// spelling somebody happened to type into the catalog.
/// One release shape: a label, and how it wraps a name.
type ReleaseShape = (&'static str, fn(&str) -> String);

fn release_matrix() -> Vec<(&'static str, Vec<String>)> {
    let spellings = [NATIVE_NAME, ROMANIZED_NAME, LICENCE_NAME];
    let shapes: Vec<ReleaseShape> = vec![
        ("second season as S2", |name| {
            format!("{name} S2 - 03 [1080p]-Group")
        }),
        ("season pack", |name| {
            format!("{name}.S02.1080p.WEB-DL.AAC2.0.H.264-Group")
        }),
        ("complete series pack", |name| {
            format!("{name}.COMPLETE.Series.1080p.BluRay.x264-Group")
        }),
        ("multi-episode range", |name| {
            format!("{name}.S02E01-E03.1080p.WEB-DL.H264-Group")
        }),
        ("absolute-numbered batch", |name| {
            format!("[Group] {name} - 13-24 [1080p][Batch]")
        }),
    ];

    shapes
        .into_iter()
        .map(|(label, build)| {
            (
                label,
                spellings
                    .iter()
                    .map(|spelling| build(spelling))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

#[tokio::test]
async fn every_release_shape_resolves_from_every_spelling_the_title_answers_to() {
    let (app, _user, title) = multilingual_subject().await;
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    let mut unresolved = Vec::new();
    for (shape, releases) in release_matrix() {
        for release in releases {
            let parsed = crate::release_parser::parse_release_metadata(&release);
            let resolved = matcher
                .resolve_episode(&parsed, Some("anime"))
                .await
                .expect("resolution must not fail");
            if resolved.map(|resolved| resolved.title.id.clone()) != Some(title.id.clone()) {
                unresolved.push(format!("{shape}: {release}"));
            }
        }
    }

    assert!(
        unresolved.is_empty(),
        "every spelling of every shape names one subject; these did not reach it:\n{}",
        unresolved.join("\n")
    );
}

/// The cour name is not on the title at all — it lives in the numbering
/// bridge — and a release that uses it is still this title's release. The
/// romanized variant with the long vowel spelled differently is the same
/// claim made by a group with a different transliteration habit.
#[tokio::test]
async fn a_bridged_cour_name_and_a_rival_romanization_both_reach_the_title() {
    let (app, _user, title) = multilingual_subject().await;
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    for release in [
        "[Group] Aokumo no Kiroku: Hikari no Shou - 03 [1080p]",
        "[Group] Aokumo no Kirokuu - 15 [1080p]",
        "Aokumo no Kiroku Hikari no Shou S01E03 1080p WEB-DL H264-Group",
    ] {
        let parsed = crate::release_parser::parse_release_metadata(release);
        let resolved = matcher
            .resolve_episode(&parsed, Some("anime"))
            .await
            .expect("resolution must not fail");
        assert_eq!(
            resolved.map(|resolved| resolved.title.id.clone()),
            Some(title.id.clone()),
            "{release} names this title"
        );
    }
}

/// Tolerance is bounded, not generous. A different subject whose name is a
/// few edits away must not be absorbed, in any script: the whole point of a
/// bounded-distance lane is that the bound is real.
#[tokio::test]
async fn a_neighbouring_subject_is_not_absorbed_in_any_script() {
    let (app, user, title) = multilingual_subject().await;
    let rival = app
        .add_title(
            &user,
            NewTitle {
                name: "赤雲の記録".into(),
                facet: MediaFacet::Anime,
                monitored: true,
                ..Default::default()
            },
        )
        .await
        .expect("create the rival title");
    app.services
        .catalog
        .titles
        .update_title_hydrated_metadata(
            &rival.id,
            TitleMetadataUpdate {
                aliases: vec!["Akakumo no Kiroku".to_string()],
                ..Default::default()
            },
        )
        .await
        .expect("store the rival's alias");

    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    for (release, expected) in [
        ("[Group] Akakumo no Kiroku - 03 [1080p]", &rival.id),
        ("[Group] Aokumo no Kiroku - 03 [1080p]", &title.id),
        ("赤雲の記録 - 03 [1080p]", &rival.id),
        ("蒼雲の記録 - 03 [1080p]", &title.id),
    ] {
        let parsed = crate::release_parser::parse_release_metadata(release);
        let resolved = matcher
            .resolve_episode(&parsed, Some("anime"))
            .await
            .expect("resolution must not fail");
        assert_eq!(
            resolved.map(|resolved| resolved.title.id.clone()),
            Some(expected.clone()),
            "{release} must reach its own subject and no other"
        );
    }
}

/// The season qualifier a group writes is part of the name it claims, and the
/// only place that name exists is the numbering bridge's cour titles. Each of
/// these is one of this title's second-cour names, in the script the group
/// chose, so each must reach the title.
#[tokio::test]
async fn every_second_cour_spelling_reaches_the_title() {
    let (app, _user, title) = multilingual_subject().await;
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    for release in [
        "[Group] 蒼雲の記録 第2期 - 03 [1080p]",
        "[Group] Aokumo no Kiroku: Hikari no Shou - 03 [1080p]",
        "Aokumo.no.Kiroku.2nd.Season.S02.1080p.WEB-DL.AAC2.0.H.264-Group",
        "Record.of.the.Blue.Cloud.Part.2.S02E03.1080p.WEB-DL.H264-Group",
    ] {
        let parsed = crate::release_parser::parse_release_metadata(release);
        let resolved = matcher
            .resolve_episode(&parsed, Some("anime"))
            .await
            .expect("resolution must not fail");
        assert_eq!(
            resolved.map(|resolved| resolved.title.id.clone()),
            Some(title.id.clone()),
            "{release} is one of this title's own cour names"
        );
    }
}

/// A release-parser defect, recorded rather than worked around.
///
/// `[Group] Aokumo no Kiroku 2nd Season - 03 [1080p][HEVC][AAC]` parses as
///
/// ```text
/// title  = "AOKUMO NO KIROKU 2ND"
/// season = Some(3), full_season = true, release_type = SeasonPack
/// ```
///
/// Two things are wrong and neither is a matching problem. The `- 03` after
/// the word `Season` is read as the season number, so episode 3 of the second
/// cour becomes a pack of a third season that does not exist; and the word
/// `Season` is consumed while `2nd` is left glued to the title, so the name
/// the matcher is handed is a name nothing in the catalog can carry — not the
/// title's own, not the cour alias `Aokumo no Kiroku 2nd Season`, not the
/// bare `Aokumo no Kiroku`.
///
/// `Record of the Blue Cloud Part 2 - 03 (1080p) [Group]` fails the same way
/// from the other side: the title keeps `PART 2` (with a `... 2` variant) but
/// the episode comes back `None`, so a numbered release arrives unnumbered.
///
/// Both spellings are ordinary fansub naming, and the title index resolves
/// the names correctly once the parse hands it a name that was actually
/// claimed — the sibling test above proves the same cour names resolve when
/// they survive parsing. Ignored until the parser is fixed; un-ignore then,
/// do not weaken.
#[tokio::test]
#[ignore = "release parser reads `Season - 03` as a season number and keeps `2nd` in the title"]
async fn a_dash_numbered_episode_after_a_spelled_out_season_keeps_its_cour_name() {
    let (app, _user, title) = multilingual_subject().await;
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    for release in [
        "[Group] Aokumo no Kiroku 2nd Season - 03 [1080p][HEVC][AAC]",
        "Record of the Blue Cloud Part 2 - 03 (1080p) [Group]",
    ] {
        let parsed = crate::release_parser::parse_release_metadata(release);
        let resolved = matcher
            .resolve_episode(&parsed, Some("anime"))
            .await
            .expect("resolution must not fail");
        assert_eq!(
            resolved.map(|resolved| resolved.title.id.clone()),
            Some(title.id.clone()),
            "{release} is one of this title's own cour names"
        );
    }
}

/// The RSS lane asks the same question of the same index, from a feed item
/// instead of a file. A name the identity gate resolves and the feed lane
/// does not is a title that silently never gets grabbed, so the lanes are
/// asserted against the same spellings.
#[tokio::test]
async fn the_rss_lane_resolves_the_same_spellings_as_the_identity_gate() {
    let (app, _user, title) = multilingual_subject().await;

    for release in [
        "[Group] 蒼雲の記録 - 03 [1080p]",
        "[Group] Aokumo no Kiroku - 03 [1080p]",
        "Record.of.the.Blue.Cloud.S02E03.1080p.WEB-DL.H264-Group",
        "Aokumo.no.Kiroku.S02.1080p.WEB-DL.AAC2.0.H.264-Group",
        "[Group] Aokumo no Kiroku: Hikari no Shou - 03 [1080p]",
    ] {
        let matched = crate::app_usecase_rss::match_rss_release_to_catalog_title(
            app.services.catalog.titles.clone(),
            release,
        )
        .await
        .expect("the feed lane must not fail");
        assert_eq!(
            matched.as_deref(),
            Some(title.id.as_str()),
            "the feed lane must reach the same subject: {release}"
        );
    }
}
