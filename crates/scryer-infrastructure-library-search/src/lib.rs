use std::collections::HashSet;

use scryer_application::{AppError, AppResult};
use scryer_domain::{MediaFacet, TaggedAlias, Title, title_spelling};
use sqlx::{Postgres, QueryBuilder, Row, Sqlite, SqlitePool, Transaction};

const TERM_KIND_NAME: &str = "name";
const TERM_KIND_ALIAS: &str = "alias";
const TERM_KIND_TAGGED_ALIAS: &str = "tagged_alias";
const TERM_KIND_SORT_TITLE: &str = "sort_title";
const TERM_KIND_SLUG: &str = "slug";
const TERM_KIND_NAME_TOKEN: &str = "name_token";
const TERM_KIND_ALIAS_TOKEN: &str = "alias_token";
const TERM_KIND_TAGGED_ALIAS_TOKEN: &str = "tagged_alias_token";
const TERM_KIND_SORT_TITLE_TOKEN: &str = "sort_title_token";
const TERM_KIND_SLUG_TOKEN: &str = "slug_token";

const TERM_WEIGHT_NAME: i64 = 0;
const TERM_WEIGHT_ALIAS: i64 = 100;
const TERM_WEIGHT_TAGGED_ALIAS: i64 = 200;
const TERM_WEIGHT_SORT_TITLE: i64 = 300;
const TERM_WEIGHT_SLUG: i64 = 400;

const DIRECT_EXACT_BASE_RANK: i64 = 0;
const DIRECT_PREFIX_BASE_RANK: i64 = 1_000;
const DIRECT_CONTAINS_BASE_RANK: i64 = 2_000;
const TYPO_BASE_RANK: i64 = 3_000;
const TYPO_TOP_LIMIT: i64 = 50;
const MAX_NORMALIZED_QUERY_CHARS: usize = 512;
const MAX_TYPO_QUERY_TOKENS: usize = 16;

#[derive(Clone, Debug)]
pub struct TitleSearchPlan {
    normalized_query: String,
    query_tokens: Vec<String>,
    facets: Vec<MediaFacet>,
}

/// One projected name. `normalized_term` is the lenient (diacritic-folded)
/// form the UI query builder matches against; `literal_term` is the
/// diacritic-preserving lookup form release and import resolution key on.
/// Everything else is what the in-memory resolver index used to compute per
/// process: the bucket key (facet is on the row, script and numbers here), the
/// length band, and the equality keys that are not bounded edit distances.
#[derive(Clone, Debug)]
pub struct TitleSearchTerm {
    pub term_kind: &'static str,
    pub raw_term: String,
    pub normalized_term: String,
    pub literal_term: String,
    /// `literal_term` with a trailing `19xx`/`20xx` removed. Collision
    /// counting groups on this, so `Tide Chart` and `Tide Chart 2023` are one
    /// identity shape.
    pub stripped_year_key: String,
    /// The form the spelling lane compares: the lookup form without a name's
    /// own trailing year. `match_year` is the year that name then asserts,
    /// falling back to the title's.
    pub match_term: String,
    pub match_year: Option<i32>,
    pub script: &'static str,
    pub numbers_key: String,
    pub char_length: i64,
    /// Romanization variance is not a bounded edit distance, so a
    /// Levenshtein-shaped filter can miss it; it needs its own equality key.
    pub romanization_key: Option<String>,
    pub language_tag: Option<String>,
    /// ICU sort keys, one per collation profile the name qualifies for. Only
    /// comparable against keys written by the same collation-data version,
    /// which `title_search_meta.collation_version` records.
    pub collation_keys: Vec<(&'static str, Vec<u8>)>,
    pub weight: i64,
    pub title_year: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct TitleSearchProjectionSource {
    pub title_id: String,
    pub facet: MediaFacet,
    pub name: String,
    pub sort_title: Option<String>,
    pub slug: Option<String>,
    pub aliases: Vec<String>,
    pub tagged_aliases: Vec<TaggedAlias>,
    /// The language the untagged names are in. Tagged aliases carry their own,
    /// and the collation profile and the romanization key both depend on it.
    pub metadata_language: Option<String>,
    pub year: Option<i32>,
}

impl From<&Title> for TitleSearchProjectionSource {
    fn from(title: &Title) -> Self {
        Self {
            title_id: title.id.clone(),
            facet: title.facet.clone(),
            name: title.name.clone(),
            sort_title: title.sort_title.clone(),
            slug: title.slug.clone(),
            aliases: title.aliases.clone(),
            tagged_aliases: title.tagged_aliases.clone(),
            metadata_language: title.metadata_language.clone(),
            year: title.year,
        }
    }
}

impl TitleSearchProjectionSource {
    /// The same source with more tagged aliases — the cour names a numbering
    /// bridge carries. `build_title_search_terms` deduplicates, so a cour name
    /// the title already answers to costs nothing here.
    fn with_tagged_aliases(&self, extra: Vec<TaggedAlias>) -> Self {
        let mut source = self.clone();
        source.tagged_aliases.extend(extra);
        source
    }
}

#[derive(Clone, Copy, Debug)]
enum DirectLane {
    Exact,
    Prefix,
    Contains,
}

impl DirectLane {
    fn base_rank(self) -> i64 {
        match self {
            Self::Exact => DIRECT_EXACT_BASE_RANK,
            Self::Prefix => DIRECT_PREFIX_BASE_RANK,
            Self::Contains => DIRECT_CONTAINS_BASE_RANK,
        }
    }
}

/// The UI's lenient form. One normalizer: this is
/// [`scryer_domain::title_spelling::title_search_lenient_form`], which is
/// `normalize_title_spelling` with diacritics folded away, `&` spelled out,
/// stray symbols treated as separators and initialisms joined. The projection
/// stores it beside the diacritic-preserving literal that release and import
/// resolution key on, so both consumers see one normalization of a name.
pub fn normalize_title_search_text(raw: &str) -> String {
    scryer_domain::title_spelling::title_search_lenient_form(raw)
}

fn facet_langid(facet: &MediaFacet) -> i64 {
    match facet {
        MediaFacet::Movie => 1,
        MediaFacet::Series => 2,
        MediaFacet::Anime => 3,
    }
}

fn truncate_chars(value: String, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value;
    }
    value.chars().take(max_chars).collect()
}

fn max_typo_distance(query_char_count: usize) -> i64 {
    match query_char_count {
        0..=5 => 100,
        6..=10 => 150,
        _ => 200,
    }
}

fn max_typo_length_delta(query_char_count: usize) -> i64 {
    match query_char_count {
        0..=5 => 1,
        6..=10 => 2,
        _ => 3,
    }
}

fn typo_scope(query_char_count: usize) -> i64 {
    match query_char_count {
        0..=8 => 3,
        _ => 2,
    }
}

fn spellfix_rank_for_weight(weight: i64) -> i64 {
    match weight {
        TERM_WEIGHT_NAME => 10_000,
        TERM_WEIGHT_ALIAS => 5_000,
        TERM_WEIGHT_TAGGED_ALIAS => 4_000,
        TERM_WEIGHT_SORT_TITLE => 2_000,
        TERM_WEIGHT_SLUG => 1_000,
        _ => 1,
    }
}

fn typo_boundary_chars(query_token: &str) -> Option<(String, String)> {
    let mut chars = query_token.chars();
    let first = chars.next()?;
    let last = query_token.chars().last()?;
    Some((first.to_string(), last.to_string()))
}

pub fn build_title_search_plan(facet: Option<MediaFacet>, query: &str) -> Option<TitleSearchPlan> {
    let normalized_query = truncate_chars(
        normalize_title_search_text(query),
        MAX_NORMALIZED_QUERY_CHARS,
    );
    if normalized_query.is_empty() {
        return None;
    }

    let query_tokens = normalized_query
        .split_whitespace()
        .filter(|token| token.chars().count() >= 4)
        .take(MAX_TYPO_QUERY_TOKENS)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let facets = facet
        .map(|facet| vec![facet])
        .unwrap_or_else(|| vec![MediaFacet::Movie, MediaFacet::Series, MediaFacet::Anime]);

    Some(TitleSearchPlan {
        normalized_query,
        query_tokens,
        facets,
    })
}

pub fn push_ranked_title_matches_cte(builder: &mut QueryBuilder<Sqlite>, plan: &TitleSearchPlan) {
    builder.push("WITH direct_title_matches(title_id, rank) AS (");
    push_direct_match_select(
        builder,
        plan,
        DirectLane::Exact,
        plan.normalized_query.clone(),
    );
    builder.push(" UNION ALL ");
    push_direct_match_select(
        builder,
        plan,
        DirectLane::Prefix,
        format!("{}%", plan.normalized_query),
    );
    builder.push(" UNION ALL ");
    push_direct_match_select(
        builder,
        plan,
        DirectLane::Contains,
        format!("%{}%", plan.normalized_query),
    );
    builder.push(
        "), typo_candidate_matches(title_id, token_key, candidate_weight, candidate_distance) AS (",
    );
    push_typo_candidate_matches(builder, plan);
    builder.push("), typo_token_matches(title_id, token_key, best_weight, best_distance) AS (");
    push_typo_token_matches(builder, plan);
    builder.push(
        "), typo_title_matches(title_id, rank) AS (
             SELECT title_id,
                    ",
    );
    builder.push_bind(TYPO_BASE_RANK);
    builder.push(" + (");
    builder.push_bind(plan.query_tokens.len() as i64);
    builder.push(
        " - COUNT(DISTINCT token_key)) * 50
                    + SUM(best_distance) * 100
                    + MIN(best_weight) AS rank
             FROM typo_token_matches
             GROUP BY title_id
             HAVING COUNT(DISTINCT token_key) >= ",
    );
    builder.push_bind(required_typo_token_matches(plan.query_tokens.len()));
    builder.push(
        "), ranked_title_matches(title_id, rank) AS (
             SELECT title_id, MIN(rank) AS rank
             FROM (
                 SELECT title_id, rank FROM direct_title_matches
                 UNION ALL
                 SELECT title_id, rank FROM typo_title_matches
             )
             GROUP BY title_id
         ) ",
    );
}

fn required_typo_token_matches(token_count: usize) -> i64 {
    if token_count <= 1 { 1 } else { 2 }
}

fn push_direct_match_select(
    builder: &mut QueryBuilder<Sqlite>,
    plan: &TitleSearchPlan,
    lane: DirectLane,
    pattern: String,
) {
    builder.push("SELECT title_id, MIN(");
    builder.push_bind(lane.base_rank());
    builder.push(
        " + weight) AS rank
         FROM title_search_terms
         WHERE term_kind NOT LIKE '%_token' AND ",
    );
    match lane {
        DirectLane::Exact => {
            builder.push("normalized_term = ");
        }
        DirectLane::Prefix | DirectLane::Contains => {
            builder.push("normalized_term LIKE ");
        }
    }
    builder.push_bind(pattern);
    push_facet_filter(builder, &plan.facets);
    builder.push(" GROUP BY title_id");
}

fn push_typo_token_matches(builder: &mut QueryBuilder<Sqlite>, plan: &TitleSearchPlan) {
    if plan.query_tokens.is_empty() || plan.normalized_query.chars().count() < 4 {
        builder.push("SELECT NULL, NULL, NULL, NULL WHERE 0");
        return;
    }

    builder.push(
        "SELECT title_id,
                token_key,
                MIN(candidate_weight) AS best_weight,
                MIN(candidate_distance) AS best_distance
         FROM typo_candidate_matches
         GROUP BY title_id, token_key",
    );
}

fn push_typo_candidate_matches(builder: &mut QueryBuilder<Sqlite>, plan: &TitleSearchPlan) {
    if plan.query_tokens.is_empty() {
        builder.push("SELECT NULL, NULL, NULL, NULL WHERE 0");
        return;
    }

    let mut first = true;
    for query_token in &plan.query_tokens {
        for facet in &plan.facets {
            if !first {
                builder.push(" UNION ALL ");
            }
            first = false;
            push_spellfix_token_candidate_select(builder, plan, query_token, facet);
            builder.push(" UNION ALL ");
            push_edit_distance_token_candidate_select(builder, plan, query_token, facet);
        }
    }
}

fn push_spellfix_token_candidate_select(
    builder: &mut QueryBuilder<Sqlite>,
    plan: &TitleSearchPlan,
    query_token: &str,
    facet: &MediaFacet,
) {
    builder.push(
        "SELECT terms.title_id AS title_id,
                ",
    );
    builder.push_bind(query_token.to_string());
    builder.push(
        " AS token_key,
                MIN(terms.weight) AS candidate_weight,
                MIN(spellfix.distance) AS candidate_distance
         FROM title_search_spellfix spellfix
         JOIN title_search_terms terms ON terms.term_id = spellfix.rowid
         WHERE spellfix.word MATCH ",
    );
    builder.push_bind(query_token.to_string());
    builder.push(" AND spellfix.top = ");
    builder.push_bind(TYPO_TOP_LIMIT);
    builder.push(" AND spellfix.scope = ");
    builder.push_bind(typo_scope(query_token.chars().count()));
    builder.push(" AND spellfix.distance <= ");
    builder.push_bind(max_typo_distance(query_token.chars().count()));
    builder.push(" AND ABS(length(terms.normalized_term) - ");
    builder.push_bind(query_token.chars().count() as i64);
    builder.push(") <= ");
    builder.push_bind(max_typo_length_delta(query_token.chars().count()));
    if let Some((first_char, last_char)) = typo_boundary_chars(query_token) {
        builder.push(" AND substr(terms.normalized_term, 1, 1) = ");
        builder.push_bind(first_char);
        if query_token.chars().count() <= 5 || plan.query_tokens.len() == 1 {
            builder.push(" AND substr(terms.normalized_term, -1, 1) = ");
            builder.push_bind(last_char);
        }
    }
    builder.push(" AND spellfix.langid = ");
    builder.push_bind(facet_langid(facet));
    builder.push(" AND terms.facet = ");
    builder.push_bind(facet.as_str());
    builder.push(" AND terms.term_kind LIKE '%_token' GROUP BY terms.title_id");
}

fn push_edit_distance_token_candidate_select(
    builder: &mut QueryBuilder<Sqlite>,
    plan: &TitleSearchPlan,
    query_token: &str,
    facet: &MediaFacet,
) {
    builder.push(
        "SELECT terms.title_id AS title_id,
                ",
    );
    builder.push_bind(query_token.to_string());
    builder.push(
        " AS token_key,
                MIN(terms.weight) AS candidate_weight,
                MIN(editdist3(terms.normalized_term, ",
    );
    builder.push_bind(query_token.to_string());
    builder.push(
        ")) AS candidate_distance
         FROM title_search_terms terms
         WHERE terms.facet = ",
    );
    builder.push_bind(facet.as_str());
    builder.push(" AND terms.term_kind LIKE '%_token'");
    builder.push(" AND ABS(length(terms.normalized_term) - ");
    builder.push_bind(query_token.chars().count() as i64);
    builder.push(") <= ");
    builder.push_bind(max_typo_length_delta(query_token.chars().count()));
    if let Some((first_char, last_char)) = typo_boundary_chars(query_token) {
        builder.push(" AND substr(terms.normalized_term, 1, 1) = ");
        builder.push_bind(first_char);
        if query_token.chars().count() <= 5 || plan.query_tokens.len() == 1 {
            builder.push(" AND substr(terms.normalized_term, -1, 1) = ");
            builder.push_bind(last_char);
        }
    }
    builder.push(" AND editdist3(terms.normalized_term, ");
    builder.push_bind(query_token.to_string());
    builder.push(") <= ");
    builder.push_bind(max_typo_distance(query_token.chars().count()));
    builder.push(" GROUP BY terms.title_id");
}

fn push_facet_filter(builder: &mut QueryBuilder<Sqlite>, facets: &[MediaFacet]) {
    if facets.len() == 3 {
        return;
    }

    builder.push(" AND facet IN (");
    let mut separated = builder.separated(", ");
    for facet in facets {
        separated.push_bind(facet.as_str());
    }
    separated.push_unseparated(")");
}

/// One row's spelling facts, computed once from the raw name and shared by
/// the full-name row and its token rows.
struct SpellingFacts {
    literal: String,
    stripped_year_key: String,
    script: &'static str,
    numbers_key: String,
    romanization_key: Option<String>,
    collation_keys: Vec<(&'static str, Vec<u8>)>,
}

/// `match_term` is the form the spelling lane compares (see
/// [`title_spelling::title_match_form`]); the equality keys are computed from
/// it, not from the literal, because a name that dates itself is compared
/// without its year.
fn spelling_facts(raw_term: &str, language: Option<&str>, match_term: &str) -> SpellingFacts {
    let literal = title_spelling::title_lookup_form(raw_term);
    let collation_keys = title_spelling::title_spelling_profiles(match_term, language)
        .into_iter()
        .filter_map(|profile| {
            title_spelling::title_spelling_key(match_term, profile).map(|key| (profile, key))
        })
        .collect();
    SpellingFacts {
        stripped_year_key: title_spelling::strip_trailing_year(&literal).to_string(),
        script: title_spelling::title_script(match_term).as_str(),
        // From the term as written, not the lowercased literal: the
        // Roman-numeral rule reads letter case.
        numbers_key: title_spelling::title_numbers_key(raw_term),
        romanization_key: title_spelling::japanese_romanization_key(match_term, language),
        collation_keys,
        literal,
    }
}

#[allow(clippy::too_many_arguments)]
fn push_term_with_tokens(
    terms: &mut Vec<TitleSearchTerm>,
    seen: &mut HashSet<(&'static str, String)>,
    term_kind: &'static str,
    token_term_kind: &'static str,
    weight: i64,
    raw_term: &str,
    language: Option<&str>,
    title_name: &str,
    year: Option<i32>,
) {
    let raw_term = raw_term.trim();
    if raw_term.is_empty() {
        return;
    }

    let normalized_term = normalize_title_search_text(raw_term);
    if normalized_term.is_empty() {
        return;
    }

    let (match_term, match_year) = title_spelling::title_match_form(raw_term, title_name, year);
    let facts = spelling_facts(raw_term, language, &match_term);

    if seen.insert((term_kind, normalized_term.clone())) {
        terms.push(TitleSearchTerm {
            term_kind,
            raw_term: raw_term.to_string(),
            normalized_term: normalized_term.clone(),
            literal_term: facts.literal.clone(),
            match_term: match_term.clone(),
            match_year,
            stripped_year_key: facts.stripped_year_key.clone(),
            script: facts.script,
            numbers_key: facts.numbers_key.clone(),
            char_length: match_term.chars().count() as i64,
            romanization_key: facts.romanization_key.clone(),
            language_tag: language.map(str::to_string),
            collation_keys: facts.collation_keys.clone(),
            weight,
            title_year: year,
        });
    }

    // Token rows exist for the UI's per-word typo lane. They are words, not
    // identities, so they carry the token's own spelling facts and never a
    // year: a word inside a name does not date the name.
    for token in normalized_term
        .split_whitespace()
        .filter(|token| token.chars().count() >= 4)
    {
        let token = token.to_string();
        if !seen.insert((token_term_kind, token.clone())) {
            continue;
        }
        let token_facts = spelling_facts(&token, language, &token);
        terms.push(TitleSearchTerm {
            term_kind: token_term_kind,
            raw_term: token.clone(),
            literal_term: token_facts.literal.clone(),
            match_term: token_facts.literal.clone(),
            match_year: None,
            stripped_year_key: token_facts.stripped_year_key,
            script: token_facts.script,
            numbers_key: token_facts.numbers_key,
            char_length: token_facts.literal.chars().count() as i64,
            romanization_key: token_facts.romanization_key,
            language_tag: language.map(str::to_string),
            collation_keys: token_facts.collation_keys,
            normalized_term: token,
            weight,
            title_year: None,
        });
    }
}

pub fn build_title_search_terms(source: &TitleSearchProjectionSource) -> Vec<TitleSearchTerm> {
    let mut seen = HashSet::<(&'static str, String)>::new();
    let mut terms = Vec::new();
    let language = source.metadata_language.as_deref();

    push_term_with_tokens(
        &mut terms,
        &mut seen,
        TERM_KIND_NAME,
        TERM_KIND_NAME_TOKEN,
        TERM_WEIGHT_NAME,
        &source.name,
        language,
        &source.name,
        source.year,
    );

    if let Some(sort_title) = source.sort_title.as_deref() {
        push_term_with_tokens(
            &mut terms,
            &mut seen,
            TERM_KIND_SORT_TITLE,
            TERM_KIND_SORT_TITLE_TOKEN,
            TERM_WEIGHT_SORT_TITLE,
            sort_title,
            language,
            &source.name,
            source.year,
        );
    }

    if let Some(slug) = source.slug.as_deref() {
        push_term_with_tokens(
            &mut terms,
            &mut seen,
            TERM_KIND_SLUG,
            TERM_KIND_SLUG_TOKEN,
            TERM_WEIGHT_SLUG,
            slug,
            language,
            &source.name,
            source.year,
        );
    }

    for alias in &source.aliases {
        push_term_with_tokens(
            &mut terms,
            &mut seen,
            TERM_KIND_ALIAS,
            TERM_KIND_ALIAS_TOKEN,
            TERM_WEIGHT_ALIAS,
            alias,
            language,
            &source.name,
            source.year,
        );
    }

    // A tagged alias carries its own language, which is the whole point of the
    // tag: `x-jat` is what makes a Latin-script name a Japanese romanization.
    for tagged_alias in &source.tagged_aliases {
        push_term_with_tokens(
            &mut terms,
            &mut seen,
            TERM_KIND_TAGGED_ALIAS,
            TERM_KIND_TAGGED_ALIAS_TOKEN,
            TERM_WEIGHT_TAGGED_ALIAS,
            &tagged_alias.name,
            Some(tagged_alias.language.as_str()),
            &source.name,
            source.year,
        );
    }

    terms
}

/// How many titles one rebuild page reads. The catalog is never loaded whole:
/// a rebuild on a large library used to materialize every title row at once.
const REBUILD_PAGE_SIZE: i64 = 500;

pub async fn delete_title_search_projection_tx(
    tx: &mut Transaction<'_, Sqlite>,
    title_id: &str,
) -> AppResult<()> {
    delete_title_search_projection_on_connection(tx, title_id).await
}

async fn delete_title_search_projection_on_connection(
    connection: &mut sqlx::SqliteConnection,
    title_id: &str,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM title_search_spellfix
         WHERE rowid IN (
             SELECT term_id
             FROM title_search_terms
             WHERE title_id = ?
         )",
    )
    .bind(title_id)
    .execute(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;

    // The collation keys cascade from the term rows, but SQLite only enforces
    // that when foreign keys are on for this connection, which is not
    // guaranteed for every caller. Deleting them by hand costs one statement.
    sqlx::query(
        "DELETE FROM title_search_collation_keys
         WHERE term_id IN (
             SELECT term_id
             FROM title_search_terms
             WHERE title_id = ?
         )",
    )
    .bind(title_id)
    .execute(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;

    sqlx::query("DELETE FROM title_search_terms WHERE title_id = ?")
        .bind(title_id)
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    Ok(())
}

/// Cour names the title's numbering bridge carries, as tagged aliases.
///
/// A release or an import can be named after a cour rather than after the
/// title, so those names have to be in the projection: it is the one place the
/// resolver looks. The matcher used to fold them in per process, which cost a
/// bridge read per anime title on every rebuild of a catalog-sized index.
///
/// A bridge whose payload no longer parses is treated as absent, matching
/// `get_anime_numbering_bridge`: the bridge is a cache, and refusing to project
/// a title because a stored blob went stale would be worse than projecting it
/// without its cour names.
fn bridge_cour_aliases_from_json(title_id: &str, seasons_json: Option<String>) -> Vec<TaggedAlias> {
    let Some(seasons_json) = seasons_json else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<scryer_domain::AnimeCommunitySeason>>(&seasons_json) {
        Ok(seasons) => scryer_domain::AnimeNumberingBridge {
            seasons,
            ..Default::default()
        }
        .cour_title_aliases(),
        Err(error) => {
            tracing::warn!(
                title_id,
                error = %error,
                "stored anime numbering bridge is unreadable; projecting without its cour names"
            );
            Vec::new()
        }
    }
}

async fn bridge_cour_aliases(
    connection: &mut sqlx::SqliteConnection,
    title_id: &str,
) -> AppResult<Vec<TaggedAlias>> {
    let seasons_json: Option<String> = sqlx::query_scalar(
        "SELECT seasons_json FROM title_anime_numbering_bridges WHERE title_id = ?",
    )
    .bind(title_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;
    Ok(bridge_cour_aliases_from_json(title_id, seasons_json))
}

async fn bridge_cour_aliases_pg(
    connection: &mut sqlx::PgConnection,
    title_id: &str,
) -> AppResult<Vec<TaggedAlias>> {
    let seasons_json: Option<String> = sqlx::query_scalar(
        "SELECT seasons_json FROM title_anime_numbering_bridges WHERE title_id = $1",
    )
    .bind(title_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;
    Ok(bridge_cour_aliases_from_json(title_id, seasons_json))
}

pub async fn replace_title_search_projection_tx(
    tx: &mut Transaction<'_, Sqlite>,
    title: &Title,
) -> AppResult<()> {
    replace_title_search_projection_source_tx(tx, &TitleSearchProjectionSource::from(title)).await
}

pub async fn replace_title_search_projection_pg_tx(
    tx: &mut Transaction<'_, Postgres>,
    title: &Title,
) -> AppResult<()> {
    replace_title_search_projection_pg_source_tx(tx, &TitleSearchProjectionSource::from(title))
        .await
}

pub async fn replace_title_search_projection_pg_source_tx(
    tx: &mut Transaction<'_, Postgres>,
    source: &TitleSearchProjectionSource,
) -> AppResult<()> {
    replace_title_search_projection_pg_on_connection(&mut *tx, source).await
}

async fn replace_title_search_projection_pg_on_connection(
    connection: &mut sqlx::PgConnection,
    source: &TitleSearchProjectionSource,
) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM title_search_collation_keys
         WHERE term_id IN (SELECT term_id FROM title_search_terms WHERE title_id = $1)",
    )
    .bind(&source.title_id)
    .execute(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;

    sqlx::query("DELETE FROM title_search_terms WHERE title_id = $1")
        .bind(&source.title_id)
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    let bridged = source
        .with_tagged_aliases(bridge_cour_aliases_pg(&mut *connection, &source.title_id).await?);

    let facet = source.facet.as_str();
    for term in build_title_search_terms(&bridged) {
        let term_id: i64 = sqlx::query_scalar(
            "INSERT INTO title_search_terms
             (title_id, facet, term_kind, raw_term, normalized_term, weight,
              literal_term, match_term, match_year, stripped_year_key, script,
              numbers_key, char_length, romanization_key, language_tag, title_year)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16)
             ON CONFLICT (title_id, term_kind, normalized_term) DO UPDATE SET
                raw_term = EXCLUDED.raw_term,
                weight = EXCLUDED.weight,
                literal_term = EXCLUDED.literal_term,
                match_term = EXCLUDED.match_term,
                match_year = EXCLUDED.match_year,
                stripped_year_key = EXCLUDED.stripped_year_key,
                script = EXCLUDED.script,
                numbers_key = EXCLUDED.numbers_key,
                char_length = EXCLUDED.char_length,
                romanization_key = EXCLUDED.romanization_key,
                language_tag = EXCLUDED.language_tag,
                title_year = EXCLUDED.title_year
             RETURNING term_id",
        )
        .bind(&source.title_id)
        .bind(facet)
        .bind(term.term_kind)
        .bind(&term.raw_term)
        .bind(&term.normalized_term)
        .bind(term.weight)
        .bind(&term.literal_term)
        .bind(&term.match_term)
        .bind(term.match_year.map(i64::from))
        .bind(&term.stripped_year_key)
        .bind(term.script)
        .bind(&term.numbers_key)
        .bind(term.char_length)
        .bind(&term.romanization_key)
        .bind(&term.language_tag)
        .bind(term.title_year.map(i64::from))
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

        for (profile, key) in &term.collation_keys {
            sqlx::query(
                "INSERT INTO title_search_collation_keys (term_id, profile, collation_key)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (term_id, profile) DO UPDATE SET
                    collation_key = EXCLUDED.collation_key",
            )
            .bind(term_id)
            .bind(*profile)
            .bind(key.as_slice())
            .execute(&mut *connection)
            .await
            .map_err(|err| AppError::Repository(err.to_string()))?;
        }
    }

    Ok(())
}

async fn replace_title_search_projection_source_tx(
    connection: &mut sqlx::SqliteConnection,
    source: &TitleSearchProjectionSource,
) -> AppResult<()> {
    delete_title_search_projection_on_connection(connection, &source.title_id).await?;

    let bridged =
        source.with_tagged_aliases(bridge_cour_aliases(&mut *connection, &source.title_id).await?);

    let facet = source.facet.as_str();
    let langid = facet_langid(&source.facet);

    for term in build_title_search_terms(&bridged) {
        let term_id: i64 = sqlx::query_scalar(
            "INSERT INTO title_search_terms
             (title_id, facet, term_kind, raw_term, normalized_term, weight,
              literal_term, match_term, match_year, stripped_year_key, script,
              numbers_key, char_length, romanization_key, language_tag, title_year)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             RETURNING term_id",
        )
        .bind(&source.title_id)
        .bind(facet)
        .bind(term.term_kind)
        .bind(&term.raw_term)
        .bind(&term.normalized_term)
        .bind(term.weight)
        .bind(&term.literal_term)
        .bind(&term.match_term)
        .bind(term.match_year.map(i64::from))
        .bind(&term.stripped_year_key)
        .bind(term.script)
        .bind(&term.numbers_key)
        .bind(term.char_length)
        .bind(&term.romanization_key)
        .bind(&term.language_tag)
        .bind(term.title_year.map(i64::from))
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

        for (profile, key) in &term.collation_keys {
            sqlx::query(
                "INSERT OR REPLACE INTO title_search_collation_keys
                 (term_id, profile, collation_key)
                 VALUES (?, ?, ?)",
            )
            .bind(term_id)
            .bind(*profile)
            .bind(key.as_slice())
            .execute(&mut *connection)
            .await
            .map_err(|err| AppError::Repository(err.to_string()))?;
        }

        sqlx::query(
            "INSERT INTO title_search_spellfix(rowid, word, rank, langid)
             VALUES (?, ?, ?, ?)",
        )
        .bind(term_id)
        .bind(&term.normalized_term)
        .bind(spellfix_rank_for_weight(term.weight))
        .bind(langid)
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;
    }

    Ok(())
}

/// Rebuild when the projection is empty, or when it was written with different
/// collation data than this build produces.
///
/// The second case is what lets the projection persist ICU sort keys at all: a
/// key written by one collation-data version is not comparable against a key
/// computed by another, and the mismatch is silent — the lookup simply stops
/// finding rows. Stamping the version and rebuilding on a difference turns
/// that into a one-off cost at start instead of a matching outage.
pub async fn seed_title_search_projection_if_stale(pool: &SqlitePool) -> AppResult<()> {
    let stored_version: Option<String> =
        sqlx::query_scalar("SELECT collation_version FROM title_search_meta WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(|err| AppError::Repository(err.to_string()))?;
    let existing_term_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_search_terms")
        .fetch_one(pool)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    if existing_term_count != 0
        && stored_version.as_deref() == Some(title_spelling::title_collation_data_version())
    {
        return Ok(());
    }

    rebuild_title_search_projection(pool).await
}

/// PostgreSQL half of [`seed_title_search_projection_if_stale`].
pub async fn seed_title_search_projection_if_stale_pg(pool: &sqlx::PgPool) -> AppResult<()> {
    let stored_version: Option<String> =
        sqlx::query_scalar("SELECT collation_version FROM title_search_meta WHERE id = 1")
            .fetch_optional(pool)
            .await
            .map_err(|err| AppError::Repository(err.to_string()))?;
    let existing_term_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_search_terms")
        .fetch_one(pool)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    if existing_term_count != 0
        && stored_version.as_deref() == Some(title_spelling::title_collation_data_version())
    {
        return Ok(());
    }

    rebuild_title_search_projection_pg(pool).await
}

pub async fn rebuild_title_search_projection(pool: &SqlitePool) -> AppResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;
    rebuild_title_search_projection_on_connection(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|err| AppError::Repository(err.to_string()))
}

pub async fn rebuild_title_search_projection_pg(pool: &sqlx::PgPool) -> AppResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;
    rebuild_title_search_projection_pg_on_connection(&mut tx).await?;
    tx.commit()
        .await
        .map_err(|err| AppError::Repository(err.to_string()))
}

/// Rebuild on the caller's active transaction, including the catalog read.
/// The caller must commit or roll back this connection; restore uses its
/// existing BEGIN IMMEDIATE transaction so the catalog and index stay atomic.
///
/// The catalog is read in pages of [`REBUILD_PAGE_SIZE`] keyed on the last id
/// seen, never with one `fetch_all` of every title.
pub async fn rebuild_title_search_projection_on_connection(
    connection: &mut sqlx::SqliteConnection,
) -> AppResult<()> {
    sqlx::query("DELETE FROM title_search_collation_keys")
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    sqlx::query("DELETE FROM title_search_terms")
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    sqlx::query("DELETE FROM title_search_spellfix")
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    let mut after_id = String::new();
    loop {
        let rows = sqlx::query(
            "SELECT id, name, facet, sort_title, slug, aliases, tagged_aliases_json,
                    metadata_language, year
             FROM titles
             WHERE id > ?
             ORDER BY id ASC
             LIMIT ?",
        )
        .bind(&after_id)
        .bind(REBUILD_PAGE_SIZE)
        .fetch_all(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

        if rows.is_empty() {
            break;
        }

        for row in &rows {
            let source = projection_source_from_row(row)?;
            after_id = source.title_id.clone();
            replace_title_search_projection_source_tx(connection, &source).await?;
        }
    }

    stamp_collation_version_sqlite(connection).await
}

/// PostgreSQL half of [`rebuild_title_search_projection_on_connection`].
pub async fn rebuild_title_search_projection_pg_on_connection(
    connection: &mut sqlx::PgConnection,
) -> AppResult<()> {
    sqlx::query("DELETE FROM title_search_collation_keys")
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    sqlx::query("DELETE FROM title_search_terms")
        .execute(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

    let mut after_id = String::new();
    loop {
        let rows = sqlx::query(
            "SELECT id, name, facet, sort_title, slug, aliases, tagged_aliases_json,
                    metadata_language, year
             FROM titles
             WHERE id > $1
             ORDER BY id ASC
             LIMIT $2",
        )
        .bind(&after_id)
        .bind(REBUILD_PAGE_SIZE)
        .fetch_all(&mut *connection)
        .await
        .map_err(|err| AppError::Repository(err.to_string()))?;

        if rows.is_empty() {
            break;
        }

        for row in &rows {
            let source = projection_source_from_pg_row(row)?;
            after_id = source.title_id.clone();
            replace_title_search_projection_pg_on_connection(connection, &source).await?;
        }
    }

    stamp_collation_version_pg(connection).await
}

async fn stamp_collation_version_sqlite(connection: &mut sqlx::SqliteConnection) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO title_search_meta (id, collation_version, projection_generation)
         VALUES (1, ?, 1)
         ON CONFLICT(id) DO UPDATE SET
            collation_version = excluded.collation_version,
            projection_generation = title_search_meta.projection_generation + 1",
    )
    .bind(title_spelling::title_collation_data_version())
    .execute(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;
    Ok(())
}

async fn stamp_collation_version_pg(connection: &mut sqlx::PgConnection) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO title_search_meta (id, collation_version, projection_generation)
         VALUES (1, $1, 1)
         ON CONFLICT (id) DO UPDATE SET
            collation_version = EXCLUDED.collation_version,
            projection_generation = title_search_meta.projection_generation + 1",
    )
    .bind(title_spelling::title_collation_data_version())
    .execute(&mut *connection)
    .await
    .map_err(|err| AppError::Repository(err.to_string()))?;
    Ok(())
}

fn projection_source_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> AppResult<TitleSearchProjectionSource> {
    let facet_raw: String = row
        .try_get("facet")
        .map_err(|err| AppError::Repository(err.to_string()))?;
    let aliases_json: String = row.try_get("aliases").unwrap_or_else(|_| "[]".to_string());
    let tagged_aliases_json: String = row
        .try_get("tagged_aliases_json")
        .unwrap_or_else(|_| "[]".to_string());

    Ok(TitleSearchProjectionSource {
        title_id: row
            .try_get("id")
            .map_err(|err| AppError::Repository(err.to_string()))?,
        facet: MediaFacet::parse(&facet_raw).unwrap_or_default(),
        name: row
            .try_get("name")
            .map_err(|err| AppError::Repository(err.to_string()))?,
        sort_title: row.try_get("sort_title").unwrap_or(None),
        slug: row.try_get("slug").unwrap_or(None),
        aliases: serde_json::from_str(&aliases_json)
            .map_err(|err| AppError::Repository(err.to_string()))?,
        tagged_aliases: serde_json::from_str(&tagged_aliases_json)
            .map_err(|err| AppError::Repository(err.to_string()))?,
        metadata_language: row.try_get("metadata_language").unwrap_or(None),
        year: row.try_get("year").unwrap_or(None),
    })
}

/// PostgreSQL keeps `aliases` and `tagged_aliases_json` as `jsonb`, so they
/// decode as a JSON value rather than as a string.
fn pg_json_column<T: serde::de::DeserializeOwned + Default>(
    row: &sqlx::postgres::PgRow,
    column: &str,
) -> AppResult<T> {
    let Ok(value) = row.try_get::<serde_json::Value, _>(column) else {
        return Ok(T::default());
    };
    serde_json::from_value(value).map_err(|err| AppError::Repository(err.to_string()))
}

fn projection_source_from_pg_row(
    row: &sqlx::postgres::PgRow,
) -> AppResult<TitleSearchProjectionSource> {
    let facet_raw: String = row
        .try_get("facet")
        .map_err(|err| AppError::Repository(err.to_string()))?;

    Ok(TitleSearchProjectionSource {
        title_id: row
            .try_get("id")
            .map_err(|err| AppError::Repository(err.to_string()))?,
        facet: MediaFacet::parse(&facet_raw).unwrap_or_default(),
        name: row
            .try_get("name")
            .map_err(|err| AppError::Repository(err.to_string()))?,
        sort_title: row.try_get("sort_title").unwrap_or(None),
        slug: row.try_get("slug").unwrap_or(None),
        aliases: pg_json_column(row, "aliases")?,
        tagged_aliases: pg_json_column(row, "tagged_aliases_json")?,
        metadata_language: row.try_get("metadata_language").unwrap_or(None),
        year: row.try_get("year").unwrap_or(None),
    })
}
