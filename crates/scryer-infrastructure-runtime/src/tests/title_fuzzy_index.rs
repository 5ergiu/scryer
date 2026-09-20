//! The tantivy index itself: freshness, invalidation and degradation.
//!
//! Every other search test goes through a store. These go at the index
//! directly, because its contract — a queue drained before every fuzzy read,
//! a stamp that invalidates the whole thing, and silence plus empty results
//! whenever it cannot serve — is what the exact lanes are allowed to rely on.

use super::*;
use scryer_infrastructure_library::media::titles::fuzzy_source::DatastoreTitleTermSource;
use scryer_infrastructure_library_search::TitleFuzzyIndex;
use scryer_infrastructure_library_search::fuzzy::{ProjectionStamp, ResolverFuzzyQuery};

async fn anime_title(catalog: &TitleStore, id: &str, name: &str) -> Title {
    let mut title = make_test_title(id, None);
    title.name = name.to_string();
    title.facet = MediaFacet::Anime;
    title.library_id = scryer_domain::default_library_id_for_facet(&MediaFacet::Anime);
    TitleRepository::create(catalog, title.clone())
        .await
        .expect("title should insert");
    title
}

async fn open_index(services: &SqliteServices, dir: &std::path::Path) -> Arc<TitleFuzzyIndex> {
    TitleFuzzyIndex::open(
        dir,
        Arc::new(DatastoreTitleTermSource::new(services.datastore())),
    )
    .await
}

/// The resolver's own query shape, spelled the way the port spells it.
struct ObservedName {
    match_term: String,
    script: String,
    numbers_key: String,
}

fn observed(name: &str) -> ObservedName {
    ObservedName {
        match_term: scryer_domain::title_spelling::title_match_form(name, name, None).0,
        script: scryer_domain::title_spelling::title_script(name)
            .as_str()
            .to_string(),
        numbers_key: scryer_domain::title_spelling::title_numbers_key(name),
    }
}

fn resolver_query<'a>(name: &'a ObservedName, distance: u8) -> ResolverFuzzyQuery<'a> {
    ResolverFuzzyQuery {
        match_term: &name.match_term,
        script: &name.script,
        numbers_key: &name.numbers_key,
        facet: Some("anime"),
        distance,
        limit: 64,
    }
}

#[tokio::test]
async fn a_queued_title_is_visible_to_the_next_fuzzy_read() {
    let (services, db) = temp_services("scryer_fuzzy_index_queue_drain").await;
    let dir = tempfile::tempdir().unwrap();
    let catalog = title_store(&services);
    let index = open_index(&services, dir.path()).await;
    index
        .rebuild(index_stamp(&services).await)
        .await
        .expect("the initial rebuild must succeed");

    // Created after the rebuild, so only the queue can carry it.
    let aokumo = anime_title(&catalog, "fuzzy-aokumo", "Aokumo").await;
    index.sync().await.expect("sync must succeed");

    let candidates = index
        .resolver_candidates(resolver_query(&observed("Akumo"), 1))
        .await;
    assert!(
        !candidates.is_empty(),
        "a one-edit misspelling must reach the title queued a moment ago"
    );
    let hydrated = title_ids_for_terms(&services, &candidates).await;
    assert!(hydrated.contains(&aokumo.id.to_string()), "{hydrated:?}");

    let _ = std::fs::remove_file(db);
}

#[tokio::test]
async fn a_deleted_title_leaves_the_index_on_the_next_sync() {
    let (services, db) = temp_services("scryer_fuzzy_index_delete").await;
    let dir = tempfile::tempdir().unwrap();
    let catalog = title_store(&services);
    let index = open_index(&services, dir.path()).await;

    let aokumo = anime_title(&catalog, "fuzzy-aokumo-delete", "Aokumo").await;
    index
        .rebuild(index_stamp(&services).await)
        .await
        .expect("rebuild must succeed");
    assert!(
        !index
            .resolver_candidates(resolver_query(&observed("Akumo"), 1))
            .await
            .is_empty()
    );

    TitleRepository::delete(&catalog, aokumo.id.as_str())
        .await
        .expect("delete should succeed");
    index.sync().await.expect("sync must succeed");
    assert!(
        index
            .resolver_candidates(resolver_query(&observed("Akumo"), 1))
            .await
            .is_empty(),
        "a deleted title must not keep answering from the index"
    );

    let _ = std::fs::remove_file(db);
}

#[tokio::test]
async fn a_stamp_from_another_projection_marks_the_directory_for_rebuild() {
    let (services, db) = temp_services("scryer_fuzzy_index_stamp").await;
    let dir = tempfile::tempdir().unwrap();
    let catalog = title_store(&services);
    anime_title(&catalog, "fuzzy-aokumo-stamp", "Aokumo").await;

    let index = open_index(&services, dir.path()).await;
    let current = index_stamp(&services).await;
    // A stamp from a different projection generation. The contents may be
    // perfectly good, but nothing on disk can prove they were built from the
    // projection as it now stands, so the directory must not be trusted.
    index
        .rebuild(ProjectionStamp {
            collation_version: "not-the-current-one".to_string(),
            projection_generation: -1,
        })
        .await
        .expect("rebuild must succeed");
    assert!(
        !index.stamp_matches(&current),
        "a foreign stamp must send the next open into a rebuild"
    );

    index
        .rebuild(current.clone())
        .await
        .expect("rebuild must succeed");
    assert!(index.stamp_matches(&current));
    assert!(
        !index
            .resolver_candidates(resolver_query(&observed("Akumo"), 1))
            .await
            .is_empty(),
        "the rebuilt index must answer again"
    );

    let _ = std::fs::remove_file(db);
}

#[tokio::test]
async fn an_unusable_directory_answers_nothing_instead_of_failing() {
    let (services, db) = temp_services("scryer_fuzzy_index_unusable").await;
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("blocked");
    // A file where the index directory should be: opening it cannot succeed.
    std::fs::write(&blocked, b"not a directory").unwrap();

    let index = open_index(&services, &blocked).await;
    assert!(
        index
            .resolver_candidates(resolver_query(&observed("Akumo"), 1))
            .await
            .is_empty(),
        "an index that cannot open must answer nothing, not panic"
    );
    assert!(index.sync().await.is_ok(), "sync must stay quiet too");

    let _ = std::fs::remove_file(db);
}

async fn index_stamp(services: &SqliteServices) -> ProjectionStamp {
    use scryer_infrastructure_library_search::fuzzy::TitleTermSource;
    DatastoreTitleTermSource::new(services.datastore())
        .projection_stamp()
        .await
        .expect("the projection stamp should load")
}

async fn title_ids_for_terms(services: &SqliteServices, term_ids: &[i64]) -> Vec<String> {
    if term_ids.is_empty() {
        return Vec::new();
    }
    let placeholders = term_ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let mut query = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
        "SELECT DISTINCT title_id FROM title_search_terms WHERE term_id IN ({placeholders})"
    )));
    for term_id in term_ids {
        query = query.bind(term_id);
    }
    query.fetch_all(services.pool()).await.unwrap()
}

/// What one release costs against a library-sized catalog.
///
/// Ignored by default: it builds 12,000 titles and runs 1,000 lookups, which
/// is minutes of work and a real file on disk, not something a normal test
/// run should pay for. Run it with
/// `cargo test -p scryer-infrastructure-runtime --release fuzzy_lookup_timing
/// -- --ignored --nocapture` when the lane's cost is the question.
///
/// It reports the median and the p99 rather than a mean, because the mean of
/// a lookup distribution with a rare pathological bucket says nothing useful
/// about either the normal case or the bad one.
#[tokio::test]
#[ignore = "timing measurement over a 12,000-title catalog"]
async fn fuzzy_lookup_timing_over_a_library_sized_catalog() {
    const TITLES: usize = 12_000;
    const RELEASES: usize = 1_000;

    let (services, db) = temp_services("scryer_fuzzy_timing").await;
    let dir = tempfile::tempdir().unwrap();
    let catalog = title_store(&services);

    // A catalog with the shapes that actually collide: shared prefixes,
    // shared numbers, and a native-script tail that shares no Latin letters
    // with any of it.
    for index in 0..TITLES {
        let name = match index % 4 {
            0 => format!("Harbor Lights {}", index / 4),
            1 => format!("Harbour Lights Season {}", index / 4),
            2 => format!("Aokumo no Kiroku {}", index / 4),
            _ => format!("蒼雲の記録 {}", index / 4),
        };
        let mut title = make_test_title(&format!("timing-title-{index}"), None);
        title.name = name;
        title.facet = MediaFacet::Anime;
        title.library_id = scryer_domain::default_library_id_for_facet(&MediaFacet::Anime);
        TitleRepository::create(&catalog, title)
            .await
            .expect("title should insert");
    }

    let index = open_index(&services, dir.path()).await;
    let rebuild_start = std::time::Instant::now();
    index
        .rebuild(index_stamp(&services).await)
        .await
        .expect("rebuild must succeed");
    let rebuild = rebuild_start.elapsed();
    let catalog = catalog.with_fuzzy_index(index);

    let mut elapsed = Vec::with_capacity(RELEASES);
    for release in 0..RELEASES {
        // One edit away from a name that exists, which is the case the lane
        // is for: an exact hit would never reach it.
        let release_name = match release % 4 {
            0 => format!("Harbor Ligths {release}"),
            1 => format!("Harbour Lihgts Season {release}"),
            2 => format!("Aokumo no Kirouk {release}"),
            _ => format!("蒼雲の記緑 {release}"),
        };
        let name = observed(&release_name);
        let start = std::time::Instant::now();
        let candidates = TitleRepository::find_title_name_candidates(
            &catalog,
            scryer_application::TitleNameBucketQuery {
                facet: Some("anime"),
                script: &name.script,
                numbers_key: &name.numbers_key,
                typo_distance: Some(4),
                match_term: &name.match_term,
                romanization_key: None,
                collation_keys: &[],
                limit: 256,
            },
        )
        .await
        .expect("candidate lookup must succeed");
        elapsed.push(start.elapsed());
        std::hint::black_box(candidates);
    }

    elapsed.sort();
    let median = elapsed[elapsed.len() / 2];
    let p99 = elapsed[(elapsed.len() * 99) / 100];
    println!(
        "fuzzy lookup over {TITLES} titles: rebuild {rebuild:?}, \
         median {median:?}, p99 {p99:?} over {RELEASES} releases"
    );

    let _ = std::fs::remove_file(db);
}
