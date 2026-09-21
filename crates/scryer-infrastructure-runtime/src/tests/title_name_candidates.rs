//! The title-matching port, proven on both dialects against the derivation
//! test fakes answer with.
//!
//! Release and import matching no longer walks the catalog: it asks the title
//! repository a handful of indexed questions. Each of those questions has two
//! implementations — SQL over the persisted projection in `TitleStore`, and an
//! in-memory free function the trait's default methods (and therefore every
//! test fake) use. If the two ever disagree, matching answers one way under
//! test and another in production, which is precisely the class of silent
//! lookup miss this index was built to remove. So every assertion below runs
//! the SQL lane and the in-memory lane over the same titles and compares them.

use super::*;
use scryer_application::{
    TitleNameBucketQuery, lookup_keys_claimed_by_others, name_candidates_in_bucket,
    title_lookup_forms, titles_matching_external_id, titles_matching_lookup_key_shapes,
    titles_matching_lookup_keys,
};
use scryer_domain::title_spelling;

fn matching_title(
    id: &str,
    name: &str,
    facet: MediaFacet,
    language: Option<&str>,
    year: Option<i32>,
    aliases: &[&str],
) -> Title {
    let mut title = make_test_title(id, None);
    title.name = name.to_string();
    title.facet = facet.clone();
    title.library_id = scryer_domain::default_library_id_for_facet(&facet);
    title.metadata_language = language.map(str::to_string);
    title.year = year;
    title.aliases = aliases.iter().map(|alias| alias.to_string()).collect();
    title
}

fn ids(titles: &[Title]) -> Vec<String> {
    let mut ids = titles
        .iter()
        .map(|title| title.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn candidate_pairs(candidates: &[scryer_application::TitleNameCandidate]) -> Vec<(String, String)> {
    let mut pairs = candidates
        .iter()
        .map(|candidate| (candidate.title_id.clone(), candidate.match_term.clone()))
        .collect::<Vec<_>>();
    pairs.sort();
    pairs.dedup();
    pairs
}

/// The SQL bucket narrows by key and by the fuzzy index; the in-memory bucket a fake
/// answers with returns the whole (facet, script, numbers) bucket and lets the
/// spelling comparison filter it. The fake may therefore hold more names, but
/// never fewer: a name the store can reach must be one a fake can reach too,
/// or matching would accept under test what it refuses in production.
fn assert_within(
    fetched: &[scryer_application::TitleNameCandidate],
    derived: &[scryer_application::TitleNameCandidate],
) {
    let derived = candidate_pairs(derived);
    for pair in candidate_pairs(fetched) {
        assert!(
            derived.contains(&pair),
            "the store returned {pair:?}, which the in-memory bucket does not hold: {derived:?}"
        );
    }
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

/// The bucket query the spelling lane builds for one observed spelling.
fn bucket_query<'a>(
    facet: Option<&'a str>,
    anchor: &'a str,
    numbers_key: &'a str,
    collation_keys: &'a [(&'static str, Vec<u8>)],
    romanization_key: Option<&'a str>,
    typo_distance: Option<u8>,
    limit: i64,
) -> TitleNameBucketQuery<'a> {
    TitleNameBucketQuery {
        facet,
        script: title_spelling::title_script(anchor).as_str(),
        numbers_key,
        typo_distance,
        match_term: anchor,
        romanization_key,
        collation_keys,
        limit,
    }
}

fn collation_keys_for(anchor: &str) -> Vec<(&'static str, Vec<u8>)> {
    title_spelling::COLLATION_PROFILES
        .iter()
        .filter_map(|profile| {
            title_spelling::title_spelling_key(anchor, profile).map(|key| (*profile, key))
        })
        .collect()
}

/// One body, run against SQLite and PostgreSQL.
async fn assert_title_matching_port(catalog: &TitleStore) -> AppResult<()> {
    // A German title and an unmonitored ASCII-spelled rival. The rival is the
    // collider every ambiguity guard has to see, monitored or not.
    let hoehle = matching_title(
        "de-hoehle",
        "Die Höhle der Löwen",
        MediaFacet::Series,
        Some("de"),
        Some(2024),
        &[],
    );
    let mut rival = matching_title(
        "de-hoehle-rival",
        "Die Hoehle der Loewen",
        MediaFacet::Series,
        Some("de"),
        Some(2020),
        &[],
    );
    rival.monitored = false;

    // The year-suffixed alias shape: `Tide Chart` must reach both, and the
    // suffixed alias must read as a shared key rather than a unique one.
    let tide_series = matching_title(
        "tide-series",
        "Tide Chart",
        MediaFacet::Series,
        Some("en"),
        Some(2023),
        &["Tide Chart (2023)"],
    );
    let mut tide_anime = matching_title(
        "tide-anime",
        "Tide Chart",
        MediaFacet::Anime,
        Some("en"),
        Some(1999),
        &[],
    );
    tide_anime.monitored = false;

    // A romanized anime alias: romanization is an equality lane, not an edit
    // distance, so it has to be fetched by key.
    let mut romaji = matching_title(
        "anime-romaji",
        "鋼の錬金術師",
        MediaFacet::Anime,
        Some("ja"),
        Some(2009),
        &[],
    );
    // Unmonitored, so the anime scope below has no monitored title at all.
    romaji.monitored = false;
    romaji.tagged_aliases = vec![TaggedAlias {
        name: "Hagane no Renkinjutsushi".to_string(),
        language: "x-jat".to_string(),
    }];

    let mut movie = matching_title(
        "movie-ids",
        "Signal Runner",
        MediaFacet::Movie,
        Some("en"),
        Some(2017),
        &[],
    );
    movie.imdb_id = Some("tt1856101".to_string());
    movie.external_ids = vec![
        ExternalId {
            source: "imdb".to_string(),
            kind: None,
            value: "tt1856101".to_string(),
        },
        ExternalId {
            source: "tvdb".to_string(),
            kind: None,
            value: "445123".to_string(),
        },
    ];

    let titles = vec![
        hoehle.clone(),
        rival.clone(),
        tide_series.clone(),
        tide_anime.clone(),
        romaji.clone(),
        movie.clone(),
    ];
    for title in &titles {
        TitleRepository::create(catalog, title.clone()).await?;
    }

    // --- exact lookup keys ------------------------------------------------
    let keys = vec![title_spelling::title_lookup_form("Die Höhle der Löwen")];
    let found = TitleRepository::find_titles_by_lookup_keys(catalog, &keys).await?;
    assert_eq!(ids(&found), vec!["de-hoehle".to_string()]);
    assert_eq!(
        ids(&found),
        ids(&titles_matching_lookup_keys(titles.clone(), &keys)),
        "the SQL lane and the in-memory lane must name the same titles"
    );

    // --- year-stripped key shapes ----------------------------------------
    let shapes = vec!["tide chart".to_string()];
    let found = TitleRepository::find_titles_by_lookup_key_shapes(catalog, &shapes).await?;
    assert_eq!(
        ids(&found),
        vec!["tide-anime".to_string(), "tide-series".to_string()],
        "a bare release name must reach the year-suffixed alias too"
    );
    assert_eq!(
        ids(&found),
        ids(&titles_matching_lookup_key_shapes(titles.clone(), &shapes)),
    );

    // --- collision guard --------------------------------------------------
    let tide_keys = title_lookup_forms(&tide_series);
    let shared =
        TitleRepository::lookup_keys_claimed_by_other_titles(catalog, &tide_series.id, &tide_keys)
            .await?;
    assert!(
        shared.contains(&"tide chart".to_string()),
        "the unmonitored twin is still a collider: {shared:?}"
    );
    assert!(
        shared.contains(&"tide chart 2023".to_string()),
        "a year-suffixed alias is a shared shape, not a unique one: {shared:?}"
    );
    assert_eq!(
        sorted(shared),
        sorted(lookup_keys_claimed_by_others(
            &titles,
            &tide_series.id,
            &tide_keys
        )),
    );
    // A title that shares nothing claims nothing.
    let movie_keys = title_lookup_forms(&movie);
    assert!(
        TitleRepository::lookup_keys_claimed_by_other_titles(catalog, &movie.id, &movie_keys)
            .await?
            .is_empty()
    );

    // --- external ids -----------------------------------------------------
    for (source, value) in [("imdb", "tt1856101"), ("tvdb", "445123")] {
        let found = TitleRepository::find_titles_by_external_id(catalog, source, value).await?;
        assert_eq!(ids(&found), vec!["movie-ids".to_string()], "{source}");
        assert_eq!(
            ids(&found),
            ids(&titles_matching_external_id(titles.clone(), source, value)),
            "{source}"
        );
    }

    // --- monitored scopes -------------------------------------------------
    let scopes = TitleRepository::monitored_library_scopes(catalog).await?;
    assert!(
        scopes.contains(&(
            scryer_domain::default_library_id_for_facet(&MediaFacet::Series),
            "series".to_string()
        )),
        "{scopes:?}"
    );
    assert!(
        !scopes.contains(&(
            scryer_domain::default_library_id_for_facet(&MediaFacet::Anime),
            "anime".to_string()
        )),
        "every anime title here is unmonitored, so the scope is not polled: {scopes:?}"
    );
    let mut in_memory_scopes = titles
        .iter()
        .filter(|title| title.monitored)
        .map(|title| (title.library_id.clone(), title.facet.as_str().to_string()))
        .collect::<Vec<_>>();
    in_memory_scopes.sort();
    in_memory_scopes.dedup();
    assert_eq!(scopes, in_memory_scopes);

    // --- the spelling buckets --------------------------------------------
    // Equality on the compared form.
    let anchor = title_spelling::title_lookup_form("Die Höhle der Löwen");
    let numbers = title_spelling::title_numbers_key("Die Höhle der Löwen");
    let collation = collation_keys_for(&anchor);
    let query = bucket_query(
        Some("series"),
        &anchor,
        &numbers,
        &collation,
        None,
        None,
        64,
    );
    let candidates = TitleRepository::find_title_name_candidates(catalog, query).await?;
    assert!(
        candidate_pairs(&candidates)
            .iter()
            .any(|(id, term)| id == "de-hoehle" && term == &anchor),
        "{candidates:?}"
    );
    // The ASCII-spelled rival is not literally equal and no fuzzy lane was
    // asked for here, but the German phonebook collation key equates the two
    // spellings — which is exactly how a competitor is found.
    assert!(
        candidate_pairs(&candidates)
            .iter()
            .any(|(id, _)| id == "de-hoehle-rival"),
        "the locale-equal rival must come back on the collation lane: {candidates:?}"
    );
    let query = bucket_query(
        Some("series"),
        &anchor,
        &numbers,
        &collation,
        None,
        None,
        64,
    );
    assert_within(&candidates, &name_candidates_in_bucket(&titles, &query));

    // The romanization lane: a romaji spelling reaches the tagged alias
    // without any edit-distance band.
    let observed = title_spelling::title_lookup_form("Hagane no Renkinjutsushi");
    let romanization = title_spelling::japanese_romanization_key(&observed, Some("ja"));
    let numbers = title_spelling::title_numbers_key("Hagane no Renkinjutsushi");
    let collation = collation_keys_for(&observed);
    let query = bucket_query(
        Some("anime"),
        &observed,
        &numbers,
        &collation,
        romanization.as_deref(),
        None,
        64,
    );
    let candidates = TitleRepository::find_title_name_candidates(catalog, query).await?;
    assert!(
        candidate_pairs(&candidates)
            .iter()
            .any(|(id, _)| id == "anime-romaji"),
        "{candidates:?}"
    );
    let query = bucket_query(
        Some("anime"),
        &observed,
        &numbers,
        &collation,
        romanization.as_deref(),
        None,
        64,
    );
    assert_within(&candidates, &name_candidates_in_bucket(&titles, &query));

    // The typo lane: a misspelling equals nothing, so only the fuzzy index
    // can reach the name.
    let observed = title_spelling::title_lookup_form("Die Höhle der Lowen");
    let numbers = title_spelling::title_numbers_key("Die Höhle der Lowen");
    let collation = collation_keys_for(&observed);
    let query = bucket_query(
        Some("series"),
        &observed,
        &numbers,
        &collation,
        None,
        Some(4),
        64,
    );
    let candidates = TitleRepository::find_title_name_candidates(catalog, query).await?;
    assert!(
        candidate_pairs(&candidates)
            .iter()
            .any(|(id, _)| id == "de-hoehle"),
        "a typo must still reach the name through the fuzzy index: {candidates:?}"
    );
    let query = bucket_query(
        Some("series"),
        &observed,
        &numbers,
        &collation,
        None,
        Some(4),
        64,
    );
    assert_within(&candidates, &name_candidates_in_bucket(&titles, &query));

    // Every spelling a group ships for the umlaut name reaches it through the
    // persisted lanes, asked exactly as the resolver asks: the distance is the
    // one a 16-letter anchor can consume (one edit, plus one for the
    // competitor check), not a blanket ceiling. Marks kept is equality, marks
    // dropped and marks written out are collation equivalences, and the
    // half-typed spelling is the fuzzy index's.
    for spelling in [
        "Die Höhle der Löwen",
        "Die Hohle der Lowen",
        "Die Hoehle der Loewen",
        "Die Höhle der Lowen",
    ] {
        let observed = title_spelling::title_lookup_form(spelling);
        let numbers = title_spelling::title_numbers_key(spelling);
        let collation = collation_keys_for(&observed);
        let candidates = TitleRepository::find_title_name_candidates(
            catalog,
            bucket_query(
                Some("series"),
                &observed,
                &numbers,
                &collation,
                None,
                Some(2),
                2000,
            ),
        )
        .await?;
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.title_id == "de-hoehle"),
            "{spelling} must reach the umlaut title: {candidates:?}"
        );
    }

    // The cap bounds the fuzzy lane only. Asking for one row still returns
    // the equality matches, which are what identity is actually proven
    // against.
    let anchor = title_spelling::title_lookup_form("Die Höhle der Löwen");
    let numbers = title_spelling::title_numbers_key("Die Höhle der Löwen");
    let collation = collation_keys_for(&anchor);
    let capped = TitleRepository::find_title_name_candidates(
        catalog,
        bucket_query(Some("series"), &anchor, &numbers, &collation, None, None, 1),
    )
    .await?;
    assert!(
        capped
            .iter()
            .any(|candidate| candidate.title_id == "de-hoehle"),
        "the equality lanes are never capped: {capped:?}"
    );

    // What the cap does to the fuzzy lane, on the other hand, is not a short
    // list. A bounded-distance lane that fills its cap was cut off somewhere
    // arbitrary, and a caller reading that as "no competing name" would
    // attribute a release to the wrong title, so it is an error instead.
    let saturated = TitleRepository::find_title_name_candidates(
        catalog,
        bucket_query(
            Some("series"),
            &anchor,
            &numbers,
            &collation,
            None,
            Some(4),
            1,
        ),
    )
    .await;
    let Err(error) = saturated else {
        panic!("a fuzzy lane that fills its cap must not answer short: {saturated:?}");
    };
    assert!(
        error.to_string().contains("incomplete"),
        "the cap error must say the answer is incomplete: {error}"
    );

    // A facet hint narrows the bucket, as the in-memory index's facet key did.
    let anchor = title_spelling::title_lookup_form("Tide Chart");
    let numbers = title_spelling::title_numbers_key("Tide Chart");
    let collation = collation_keys_for(&anchor);
    let series_only = TitleRepository::find_title_name_candidates(
        catalog,
        bucket_query(
            Some("series"),
            &anchor,
            &numbers,
            &collation,
            None,
            None,
            64,
        ),
    )
    .await?;
    assert!(
        series_only
            .iter()
            .all(|candidate| candidate.facet == "series"),
        "{series_only:?}"
    );
    let every_facet = TitleRepository::find_title_name_candidates(
        catalog,
        bucket_query(None, &anchor, &numbers, &collation, None, None, 64),
    )
    .await?;
    assert!(
        every_facet
            .iter()
            .any(|candidate| candidate.title_id == "tide-anime"),
        "no facet hint means every facet: {every_facet:?}"
    );

    Ok(())
}

#[tokio::test]
async fn title_matching_port_on_sqlite() -> AppResult<()> {
    let db = std::env::temp_dir().join(format!(
        "scryer_title_name_candidates_{}.db",
        chrono::Utc::now().timestamp_micros()
    ));
    let services = SqliteServices::new(db.to_string_lossy()).await?;
    let (catalog, _index_dir) = super::title_store_with_fuzzy_index(&services).await;
    let result = assert_title_matching_port(&catalog).await;
    services.pool().close().await;
    let _ = std::fs::remove_file(&db);
    result
}

#[tokio::test]
async fn title_matching_port_on_postgres() -> AppResult<()> {
    let Some(raw_url) = std::env::var("SCRYER_TEST_POSTGRES_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        eprintln!(
            "skipping PostgreSQL title matching port test; SCRYER_TEST_POSTGRES_URL is not set"
        );
        return Ok(());
    };

    let admin_pool = sqlx::PgPool::connect(&raw_url)
        .await
        .map_err(|error| AppError::Repository(format!("failed to connect to postgres: {error}")))?;
    let schema = format!(
        "scryer_test_{}_{}",
        std::process::id(),
        Id::new().0.replace('-', "_")
    );

    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin_pool)
        .await
        .map_err(|error| AppError::Repository(format!("failed to create schema: {error}")))?;

    let result = async {
        let mut url = url::Url::parse(&raw_url)
            .map_err(|error| AppError::Validation(format!("invalid postgres test URL: {error}")))?;
        url.query_pairs_mut()
            .append_pair("options", &format!("-csearch_path={schema}"));
        let services =
            crate::PostgresServices::new_with_mode(url.to_string(), crate::MigrationMode::Apply)
                .await?;
        let index_dir =
            tempfile::tempdir().map_err(|error| AppError::Repository(error.to_string()))?;
        let fuzzy = scryer_infrastructure_library_search::TitleFuzzyIndex::open(
            index_dir.path(),
            std::sync::Arc::new(
                scryer_infrastructure_library::media::titles::fuzzy_source::DatastoreTitleTermSource::new(
                    services.datastore(),
                ),
            ),
        )
        .await?;
        let catalog = TitleStore::new(services.datastore()).with_fuzzy_index(fuzzy.clone());
        let wanted = WantedStore::new(services.datastore()).with_fuzzy_index(fuzzy);
        let result = async {
            assert_title_matching_port(&catalog).await?;
            super::wanted_items_and_search::assert_catalog_search(&catalog, &wanted).await
        }.await;
        services.pool().close().await;
        result
    }
    .await;

    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin_pool)
        .await;
    admin_pool.close().await;
    cleanup.map_err(|error| AppError::Repository(format!("failed to drop schema: {error}")))?;
    result
}
