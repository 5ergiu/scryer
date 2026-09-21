//! A fixed corpus, resolved and printed, so two builds can be compared.
//!
//! This is the parity harness: it builds one deterministic catalog, runs one
//! deterministic list of release names through title resolution, and prints
//! `release -> title` for each. Run it in this tree and in a tree at the
//! commit before the title-name index work, and diff the two outputs; every
//! difference has to be explainable, and the explanation has to be an
//! improvement.
//!
//! ```text
//! cargo test -p scryer-application --lib golden_resolution_corpus \
//!     -- --ignored --nocapture
//! ```
//!
//! Ignored by default: it is a comparison instrument, not an assertion. The
//! behaviours it covers are asserted, individually and with reasons, in
//! `multilingual_title_matching` and in the matcher's own tests.

use super::*;

/// The catalog. Deliberately full of the shapes that make resolution hard:
/// a near-spelled rival, a year-suffixed name whose stem is another title, a
/// pair that differ only by year, and an anime that answers to four names in
/// three scripts.
async fn golden_catalog() -> (AppUseCase, User) {
    let (app, user) = bootstrap();

    for (name, facet, year, aliases) in [
        ("Harbor Lights", MediaFacet::Series, Some(2019), vec![]),
        ("Harbour Lights", MediaFacet::Series, Some(2021), vec![]),
        ("Blade Runner", MediaFacet::Movie, Some(1982), vec![]),
        ("Blade Runner 2049", MediaFacet::Movie, Some(2017), vec![]),
        ("The Office", MediaFacet::Series, Some(2001), vec![]),
        ("The Office", MediaFacet::Series, Some(2005), vec![]),
        (
            "蒼雲の記録",
            MediaFacet::Anime,
            Some(2022),
            vec!["Aokumo no Kiroku", "Record of the Blue Cloud"],
        ),
        (
            "赤雲の記録",
            MediaFacet::Anime,
            Some(2023),
            vec!["Akakumo no Kiroku"],
        ),
        (
            "Frieren: Beyond Journey's End",
            MediaFacet::Anime,
            Some(2023),
            vec!["Sousou no Frieren", "葬送のフリーレン"],
        ),
    ] {
        let title = app
            .add_title(
                &user,
                NewTitle {
                    name: name.into(),
                    facet,
                    monitored: true,
                    year,
                    ..Default::default()
                },
            )
            .await
            .expect("create title");
        if !aliases.is_empty() {
            app.services
                .catalog
                .titles
                .update_title_hydrated_metadata(
                    &title.id,
                    TitleMetadataUpdate {
                        aliases: aliases.into_iter().map(str::to_string).collect(),
                        ..Default::default()
                    },
                )
                .await
                .expect("store aliases");
        }
    }

    (app, user)
}

/// The corpus: every release name, with the facet hint its lane would carry.
fn golden_releases() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        // Exact, in each script.
        ("Harbor.Lights.S01E01.1080p.WEB-DL.H264-GRP", Some("series")),
        ("[GRP] 蒼雲の記録 - 03 [1080p]", Some("anime")),
        ("[GRP] Aokumo no Kiroku - 03 [1080p]", Some("anime")),
        (
            "Record.of.the.Blue.Cloud.S01E03.1080p.WEB-DL.H264-GRP",
            Some("anime"),
        ),
        ("[GRP] 葬送のフリーレン - 12 [1080p]", Some("anime")),
        ("[GRP] Sousou no Frieren - 12 [1080p]", Some("anime")),
        // Typos, one and two edits, in each script.
        ("Harbor.Ligths.S01E01.1080p.WEB-DL.H264-GRP", Some("series")),
        ("[GRP] Aokumo no Kirouk - 03 [1080p]", Some("anime")),
        ("[GRP] Sosou no Frielen - 12 [1080p]", Some("anime")),
        ("[GRP] 蒼雲の記緑 - 03 [1080p]", Some("anime")),
        // The near-spelled rival, which must stay its own subject.
        (
            "Harbour.Lights.S01E01.1080p.WEB-DL.H264-GRP",
            Some("series"),
        ),
        ("[GRP] Akakumo no Kiroku - 03 [1080p]", Some("anime")),
        ("[GRP] 赤雲の記録 - 03 [1080p]", Some("anime")),
        // Year-stemmed collision.
        ("Blade.Runner.2049.2017.1080p.BluRay.x264-GRP", None),
        ("Blade.Runner.1982.1080p.BluRay.x264-GRP", None),
        ("Blade.Runner.1080p.BluRay.x264-GRP", None),
        // Same name, different year.
        (
            "The.Office.2005.S01E01.1080p.WEB-DL.H264-GRP",
            Some("series"),
        ),
        (
            "The.Office.2001.S01E01.1080p.WEB-DL.H264-GRP",
            Some("series"),
        ),
        ("The.Office.S01E01.1080p.WEB-DL.H264-GRP", Some("series")),
        // Packs and ranges.
        ("Harbor.Lights.S01.1080p.WEB-DL.H264-GRP", Some("series")),
        (
            "Harbor.Lights.COMPLETE.Series.1080p.BluRay.x264-GRP",
            Some("series"),
        ),
        (
            "Harbor.Lights.S01E01-E03.1080p.WEB-DL.H264-GRP",
            Some("series"),
        ),
        ("Aokumo.no.Kiroku.S02.1080p.WEB-DL.H264-GRP", Some("anime")),
        (
            "[GRP] Aokumo no Kiroku - 13-24 [1080p][Batch]",
            Some("anime"),
        ),
        (
            "[GRP] Sousou no Frieren - 01-28 [1080p][Batch]",
            Some("anime"),
        ),
        // Noise that must resolve to nothing.
        (
            "Some.Unrelated.Show.S01E01.1080p.WEB-DL.H264-GRP",
            Some("series"),
        ),
        ("[GRP] Kumo no Michi - 03 [1080p]", Some("anime")),
        ("Harbor.S01E01.1080p.WEB-DL.H264-GRP", Some("series")),
    ]
}

#[tokio::test]
#[ignore = "parity instrument: prints a snapshot, asserts nothing"]
async fn golden_resolution_corpus() {
    let (app, _user) = golden_catalog().await;
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");

    println!("GOLDEN-BEGIN");
    for (release, facet_hint) in golden_releases() {
        let parsed = crate::release_parser::parse_release_metadata(release);
        let resolved = if parsed.episode.is_some() {
            matcher
                .resolve_episode(&parsed, facet_hint)
                .await
                .expect("resolution must not fail")
        } else {
            matcher
                .resolve_movie(&parsed)
                .await
                .expect("resolution must not fail")
        };
        let answer = resolved
            .map(|resolved| {
                format!(
                    "{} ({}) [{:?}]",
                    resolved.title.name,
                    resolved
                        .title
                        .year
                        .map(|year| year.to_string())
                        .unwrap_or_else(|| "-".into()),
                    resolved.match_type
                )
            })
            .unwrap_or_else(|| "NONE".to_string());
        println!("GOLDEN {release} -> {answer}");
    }
    println!("GOLDEN-END");
}
