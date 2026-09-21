//! Release comparison across the spellings a group uses for a diacritic name.
//!
//! A catalog name keeps its diacritics. A release name usually does not, and
//! groups disagree about what to put in their place. The same German series
//! ships as `Die.Höhle.der.Löwen`, as `Die.Hohle.der.Lowen` with the marks
//! dropped, and as `Die.Hoehle.der.Loewen` with each mark written out the way
//! the language spells it on a typewriter; `Straße` ships as `Strasse`. French
//! loses its ligature and its accent in one step, `cœur` becoming `coeur`.
//! All of them name one subject.
//!
//! What these pin is that a folded spelling is an *equivalence* and not a
//! typo: the identity gate accepts it when the release also carries the
//! title's year, and it keeps rejecting a genuine misspelling under exactly
//! the same corroboration. The year is not incidental. A folded spelling is
//! not a literal match, and the matcher requires a year or an asserted indexer
//! id before accepting one; the last test here pins that rule and records what
//! it costs an undated series release.
//!
//! These go through the identity gate — the monitored-title matcher the import
//! path, the tracked-download sweep and the acquisition lanes all resolve
//! through — so what is asserted is the comparison itself rather than any one
//! caller's wrapper. The persisted lanes that feed it candidates are covered
//! where they are real, in `scryer-infrastructure-runtime`'s
//! `title_name_candidates` and `title_fuzzy_index` tests.

use super::*;

const SUBJECT_YEAR: i32 = 2019;

/// A subject: the name the catalog stores, and the spellings a release uses
/// for it. The first spelling is always the catalog's own, because a group
/// that keeps the diacritics must not be the case that breaks.
struct Subject {
    catalog: &'static str,
    spellings: &'static [&'static str],
}

/// German, Portuguese, French and Spanish names, each with the marks kept,
/// dropped, and — where the language has one — written out. The German pair
/// carries the partial case too, one mark kept and one dropped, which is what
/// a group produces by hand-typing half a name.
const SUBJECTS: &[Subject] = &[
    Subject {
        catalog: "Die Höhle der Löwen",
        spellings: &[
            "Die Höhle der Löwen",
            "Die Hohle der Lowen",
            "Die Hoehle der Loewen",
            "Die Höhle der Lowen",
        ],
    },
    Subject {
        catalog: "Die Müller Straße",
        spellings: &[
            "Die Müller Straße",
            "Die Muller Strasse",
            "Die Mueller Strasse",
        ],
    },
    Subject {
        catalog: "Coração de Açúcar",
        spellings: &["Coração de Açúcar", "Coracao de Acucar"],
    },
    Subject {
        catalog: "Le cœur de Chloé",
        spellings: &["Le cœur de Chloé", "Le coeur de Chloe"],
    },
    Subject {
        catalog: "El último día",
        spellings: &["El último día", "El ultimo dia"],
    },
];

fn scene_name(spelling: &str) -> String {
    spelling.replace(' ', ".")
}

/// The catalog under test, and the id each stored name was given.
type Catalog = (AppUseCase, std::collections::BTreeMap<String, String>);

async fn catalog_with(subjects: &[(&str, MediaFacet, Option<i32>)]) -> Catalog {
    let (app, user) = bootstrap();
    let mut ids = std::collections::BTreeMap::new();
    for (name, facet, year) in subjects {
        let title = app
            .add_title(
                &user,
                NewTitle {
                    name: (*name).into(),
                    facet: facet.clone(),
                    monitored: true,
                    year: *year,
                    ..Default::default()
                },
            )
            .await
            .expect("create title");
        ids.insert((*name).to_string(), title.id);
    }
    (app, ids)
}

async fn catalog_of_subjects(facet: MediaFacet) -> Catalog {
    let subjects = SUBJECTS
        .iter()
        .map(|subject| (subject.catalog, facet.clone(), Some(SUBJECT_YEAR)))
        .collect::<Vec<_>>();
    catalog_with(&subjects).await
}

fn title_id_for<'a>(ids: &'a std::collections::BTreeMap<String, String>, name: &str) -> &'a str {
    ids.get(name)
        .unwrap_or_else(|| panic!("the catalog holds {name}"))
        .as_str()
}

async fn resolve_episode(app: &AppUseCase, release: &str) -> Option<String> {
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");
    let parsed = crate::release_parser::parse_release_metadata(release);
    matcher
        .resolve_episode(&parsed, Some("series"))
        .await
        .expect("resolution must not fail")
        .map(|resolved| resolved.title.id)
}

async fn resolve_movie(app: &AppUseCase, release: &str) -> Option<String> {
    let matcher = app
        .monitored_title_matcher()
        .await
        .expect("build the monitored title matcher");
    let parsed = crate::release_parser::parse_release_metadata(release);
    matcher
        .resolve_movie(&parsed)
        .await
        .expect("resolution must not fail")
        .map(|resolved| resolved.title.id)
}

/// Every episodic shape a group ships, each carrying the subject's year.
const SERIES_SHAPES: &[(&str, fn(&str) -> String)] = &[
    ("single episode", |name| {
        format!("{name}.{SUBJECT_YEAR}.S01E01.1080p.WEB-DL.H264-Group")
    }),
    ("season pack", |name| {
        format!("{name}.{SUBJECT_YEAR}.S01.1080p.WEB-DL.AAC2.0.H.264-Group")
    }),
    ("multi-episode range", |name| {
        format!("{name}.{SUBJECT_YEAR}.S01E01-E03.1080p.WEB-DL.H264-Group")
    }),
];

#[tokio::test]
async fn every_diacritic_spelling_of_a_dated_series_release_reaches_its_title() {
    let (app, ids) = catalog_of_subjects(MediaFacet::Series).await;

    let mut unresolved = Vec::new();
    for subject in SUBJECTS {
        let expected = title_id_for(&ids, subject.catalog);
        for spelling in subject.spellings {
            for (shape, build) in SERIES_SHAPES {
                let release = build(&scene_name(spelling));
                if resolve_episode(&app, &release).await.as_deref() != Some(expected) {
                    unresolved.push(format!("{} / {shape}: {release}", subject.catalog));
                }
            }
        }
    }

    assert!(
        unresolved.is_empty(),
        "every spelling of every shape names one subject; these did not reach it:\n{}",
        unresolved.join("\n")
    );
}

/// Movies carry their year by convention, so the whole matrix holds for them
/// without the caveat the series tests above have to make.
#[tokio::test]
async fn every_diacritic_spelling_of_a_movie_release_reaches_its_title() {
    let (app, ids) = catalog_of_subjects(MediaFacet::Movie).await;

    let mut unresolved = Vec::new();
    for subject in SUBJECTS {
        let expected = title_id_for(&ids, subject.catalog);
        for spelling in subject.spellings {
            let release = format!(
                "{}.{SUBJECT_YEAR}.1080p.BluRay.x264-Group",
                scene_name(spelling)
            );
            if resolve_movie(&app, &release).await.as_deref() != Some(expected) {
                unresolved.push(format!("{}: {release}", subject.catalog));
            }
        }
    }

    assert!(
        unresolved.is_empty(),
        "every spelling names one movie; these did not reach it:\n{}",
        unresolved.join("\n")
    );
}

/// Folding a mark away must not fold two subjects together. These two German
/// series differ in one word, and that word is the one carrying the umlaut, so
/// a comparison sloppy about diacritics answers the wrong title for both
/// transliterations of both names. They share a year, which removes the year
/// as a tiebreaker and leaves the names to do the work.
#[tokio::test]
async fn a_neighbouring_diacritic_subject_is_not_absorbed() {
    let (app, ids) = catalog_with(&[
        (
            "Die Höhle der Löwen",
            MediaFacet::Series,
            Some(SUBJECT_YEAR),
        ),
        (
            "Die Höhle der Bären",
            MediaFacet::Series,
            Some(SUBJECT_YEAR),
        ),
    ])
    .await;

    let lions = title_id_for(&ids, "Die Höhle der Löwen").to_string();
    let bears = title_id_for(&ids, "Die Höhle der Bären").to_string();

    for (release, expected, subject) in [
        (
            "Die.Hoehle.der.Loewen.2019.S01E01.1080p.WEB-DL.H264-Group",
            &lions,
            "Löwen",
        ),
        (
            "Die.Hohle.der.Lowen.2019.S01E01.1080p.WEB-DL.H264-Group",
            &lions,
            "Löwen",
        ),
        (
            "Die.Hoehle.der.Baeren.2019.S01E01.1080p.WEB-DL.H264-Group",
            &bears,
            "Bären",
        ),
        (
            "Die.Hohle.der.Baren.2019.S01E01.1080p.WEB-DL.H264-Group",
            &bears,
            "Bären",
        ),
    ] {
        assert_eq!(
            resolve_episode(&app, release).await.as_deref(),
            Some(expected.as_str()),
            "{release} names the {subject} subject and no other"
        );
    }
}

/// A dropped diacritic is an equivalence; a mangled word is not. The year that
/// lets every folded spelling above through does not let this through, which
/// is what keeps the tolerance bounded rather than generous.
#[tokio::test]
async fn a_corroborating_year_does_not_admit_a_mangled_name() {
    let (app, _ids) =
        catalog_with(&[("Harbor Lights", MediaFacet::Series, Some(SUBJECT_YEAR))]).await;

    assert!(
        resolve_episode(&app, "Harbor.Lights.2019.S01E01.1080p.WEB-DL.H264-Group")
            .await
            .is_some(),
        "the control resolves, so a None below is the name and not the shape"
    );

    assert_eq!(
        resolve_episode(&app, "Harbor.Ligths.2019.S01E01.1080p.WEB-DL.H264-Group").await,
        None,
        "a transposed word is a misspelling, not another spelling"
    );
}

/// The corroboration rule, asserted from the outside. A folded spelling is not
/// a literal match, and `find_spelling_match` requires a year or an asserted
/// indexer id before it will accept one. Nothing corroborates a bare
/// `Die.Hoehle.der.Loewen.S01E01`, so it reaches nothing — while the same
/// release with `.2019.` in it reaches the title, as the dated test above
/// pins.
///
/// This is deliberate and only one equivalence is exempt: a Japanese
/// romanization, because a catalog carries the romanized alias precisely so a
/// release named in romaji is recognizable, and an anime episode carries
/// neither a year nor usually an indexer id. Every other locale equivalence,
/// including every diacritic fold here, keeps the requirement. That exemption
/// is pinned from the outside in `romaji_release_matching`.
///
/// The cost is worth stating plainly for whoever revisits the rule: a series
/// release rarely carries a year, `S01E01` being the disambiguator a group
/// reaches for, so a German or Portuguese series is reachable under the
/// spelling groups actually ship only when the group also dates it. Movies are
/// unaffected because a movie release carries its year by convention. Widening
/// the exemption is a product decision, not a bug fix, which is why this is an
/// assertion of current behaviour rather than an ignored wish.
#[tokio::test]
async fn an_undated_series_release_needs_the_catalog_spelling() {
    let (app, ids) = catalog_of_subjects(MediaFacet::Series).await;

    for subject in SUBJECTS {
        let expected = title_id_for(&ids, subject.catalog);
        for spelling in subject.spellings {
            let release = format!("{}.S01E01.1080p.WEB-DL.H264-Group", scene_name(spelling));
            let reached = resolve_episode(&app, &release).await;
            if spelling == &subject.catalog {
                assert_eq!(
                    reached.as_deref(),
                    Some(expected),
                    "{release} is the catalogued spelling and needs no corroboration"
                );
            } else {
                assert_eq!(
                    reached, None,
                    "{release} is a folded spelling with nothing to corroborate it"
                );
            }
        }
    }
}
