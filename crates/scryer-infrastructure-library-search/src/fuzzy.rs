//! The bounded-distance lane of the title name index.
//!
//! The exact lanes — same match form, same romanization, equal under a
//! collation profile, same lookup key, same external id — are SQL equality
//! over `title_search_terms` and are exhaustive by construction. What they
//! cannot answer is "within `n` edits of", which is not an equality and which
//! no index on a text column can serve. That lane lives here, in a tantivy
//! index built from the same projection rows, so there is exactly one place a
//! fuzzy title lookup happens for the UI library search, for release matching
//! and for import matching.
//!
//! Operational contract:
//!
//! * The index is **derived state**. It is never the source of truth and
//!   losing it costs a rebuild and nothing else. Backups are table-scoped —
//!   they carry rows out of the catalog, never files out of the data
//!   directory — so this directory is excluded by construction, and a
//!   restore bumps the projection generation, which invalidates whatever
//!   index the restoring installation happened to have. Opening and reads
//!   wait for required rebuilds. An unavailable index is an error, never
//!   evidence that there are no competing titles.
//! * The directory is opened through [`MmapDirectory`] only. No other
//!   directory implementation is compiled in.
//! * A writer is created per batch and dropped when the batch commits. A
//!   separate process lock protects the directory throughout its lifetime,
//!   including recovery. The OS releases ownership after a crash.
//! * `scryer_fuzzy.json` beside the segments records the schema version this
//!   build writes and the projection stamp the contents were built from. A
//!   mismatch, a missing file or an unreadable one requires a blocking rebuild.
//! * Incremental freshness rides on `title_search_index_queue`, which the
//!   projection writer fills in the *same database transaction* as the
//!   projection row. A queued title is therefore never lost to a rollback,
//!   and draining is idempotent: a drain rewrites a title's documents from
//!   the projection as it stands at drain time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use scryer_application::{AppError, AppResult};
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    FAST, Field, INDEXED, IndexRecordOption, STORED, Schema, TextFieldIndexing, TextOptions, Value,
};
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, RawTokenizer, SimpleTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, ReloadPolicy, TantivyDocument, Term};

/// Bumped whenever the schema or the analysis below changes in a way that
/// makes existing segments answer differently. A changed value is a rebuild.
pub const FUZZY_SCHEMA_VERSION: u32 = 1;

const META_FILE: &str = "scryer_fuzzy.json";
/// The directory name under the data dir. Named so an operator can delete it
/// without wondering what it is.
pub const FUZZY_INDEX_DIR: &str = "title-fuzzy-index";

/// Terms per writer batch, both for the paged rebuild and for queue drains.
const BATCH_TERMS: usize = 20_000;
/// Titles read out of the queue per drain pass.
const QUEUE_DRAIN_BATCH: i64 = 512;
/// Writer heap per batch. Tantivy's floor is 15 MB; a batch is small and
/// short-lived, so nothing is gained by giving it more.
const WRITER_HEAP_BYTES: usize = 15_000_000;
/// The widest distance tantivy's automaton can be asked for. Beyond it the
/// gram filter below carries the lane.
const MAX_AUTOMATON_DISTANCE: u8 = 2;

/// The projection row as the index stores it. Every field here is already on
/// `title_search_terms`; the index adds no derivation of its own, which is
/// what keeps it rebuildable from the database alone.
#[derive(Clone, Debug)]
pub struct IndexedTerm {
    pub term_id: i64,
    pub title_id: String,
    pub facet: String,
    pub term_kind: String,
    pub weight: i64,
    pub literal_term: String,
    pub match_term: String,
    pub normalized_term: String,
    pub script: String,
    pub numbers_key: String,
    pub char_length: i64,
}

/// One queued title, with the queue row that claimed it.
#[derive(Clone, Debug)]
pub struct QueuedTitle {
    pub seq: i64,
    pub title_id: String,
}

/// What the projection stamp says the index should have been built from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectionStamp {
    pub collation_version: String,
    pub projection_generation: i64,
}

/// The database side of the index, dialect-agnostic on purpose: SQLite and
/// PostgreSQL implement it over the same projection tables, so the index and
/// every behaviour built on it are identical on both.
#[async_trait]
pub trait TitleTermSource: Send + Sync {
    /// Projected terms with `term_id > after_term_id`, ordered by `term_id`,
    /// at most `limit` of them. Paging by key, not by offset, so a rebuild
    /// does not degrade quadratically on a large library.
    async fn page_terms(&self, after_term_id: i64, limit: i64) -> AppResult<Vec<IndexedTerm>>;
    /// Every projected term of these titles.
    async fn terms_for_titles(&self, title_ids: &[String]) -> AppResult<Vec<IndexedTerm>>;
    /// Oldest queued titles first.
    async fn queued_titles(&self, limit: i64) -> AppResult<Vec<QueuedTitle>>;
    /// Drop exactly these queue rows. Rows enqueued after the drain read them
    /// carry later sequence numbers and survive, so a write racing a drain is
    /// picked up by the next one rather than lost.
    async fn clear_queued(&self, seqs: &[i64]) -> AppResult<()>;
    async fn projection_stamp(&self) -> AppResult<ProjectionStamp>;
}

#[derive(Clone, Copy)]
struct Fields {
    term_id: Field,
    title_id: Field,
    facet: Field,
    script: Field,
    numbers_key: Field,
    term_kind: Field,
    token_row: Field,
    weight: Field,
    char_length: Field,
    match_raw: Field,
    literal_raw: Field,
    lenient_raw: Field,
    grams: Field,
}

const TOKENIZER_RAW: &str = "scryer_raw";
const TOKENIZER_WORDS: &str = "scryer_words";
const TOKENIZER_GRAMS: &str = "scryer_grams";

fn raw_text_options(tokenizer: &str) -> TextOptions {
    TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(tokenizer)
            .set_index_option(IndexRecordOption::Basic),
    )
}

fn build_schema() -> (Schema, Fields) {
    let mut schema = Schema::builder();
    let fields = Fields {
        term_id: schema.add_i64_field("term_id", INDEXED | STORED | FAST),
        title_id: schema.add_text_field("title_id", raw_text_options(TOKENIZER_RAW) | STORED),
        facet: schema.add_text_field("facet", raw_text_options(TOKENIZER_RAW)),
        script: schema.add_text_field("script", raw_text_options(TOKENIZER_RAW)),
        numbers_key: schema.add_text_field("numbers_key", raw_text_options(TOKENIZER_RAW)),
        term_kind: schema.add_text_field("term_kind", raw_text_options(TOKENIZER_RAW)),
        token_row: schema.add_text_field("token_row", raw_text_options(TOKENIZER_RAW)),
        weight: schema.add_i64_field("weight", STORED | FAST),
        char_length: schema.add_i64_field("char_length", STORED | FAST),
        match_raw: schema.add_text_field("match_raw", raw_text_options(TOKENIZER_RAW)),
        literal_raw: schema.add_text_field("literal_raw", raw_text_options(TOKENIZER_RAW)),
        // Stored as well as indexed: the UI lane re-measures the edit
        // distance against the token it actually matched, so the precision
        // rules that used to be SQL predicates still apply.
        lenient_raw: schema.add_text_field("lenient_raw", raw_text_options(TOKENIZER_RAW) | STORED),
        grams: schema.add_text_field("grams", raw_text_options(TOKENIZER_GRAMS)),
    };
    (schema.build(), fields)
}

fn register_tokenizers(index: &Index) -> AppResult<()> {
    let manager = index.tokenizers();
    manager.register(
        TOKENIZER_RAW,
        TextAnalyzer::builder(RawTokenizer::default()).build(),
    );
    manager.register(
        TOKENIZER_WORDS,
        TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(LowerCaser)
            .build(),
    );
    // Both sizes, so one field serves two gram lanes: bigrams for scripts
    // that carry a word in two characters, trigrams for everything else.
    manager.register(
        TOKENIZER_GRAMS,
        TextAnalyzer::builder(
            NgramTokenizer::new(2, 3, false)
                .map_err(|error| AppError::Repository(error.to_string()))?,
        )
        .build(),
    );
    Ok(())
}

pub struct TitleFuzzyIndex {
    dir: PathBuf,
    index: Index,
    reader: IndexReader,
    fields: Fields,
    source: Arc<dyn TitleTermSource>,
    /// One writer at a time, always. Tantivy enforces this with a lock file;
    /// taking it in process turns a hard error into a wait.
    write_lock: tokio::sync::Mutex<()>,
    rebuild_complete: AtomicBool,
    // Keep directory ownership until the index and reader have been dropped.
    _ownership: std::fs::File,
}

/// A resolver-lane request: one bucket of the name index, the same
/// `(facet, script, numbers)` bucket the exact lanes use, and a distance.
#[derive(Clone, Debug)]
pub struct ResolverFuzzyQuery<'a> {
    pub facet: Option<&'a str>,
    pub script: &'a str,
    pub numbers_key: &'a str,
    pub match_term: &'a str,
    pub distance: u8,
    pub limit: usize,
}

/// A UI-lane hit: the same shape the SQL typo lane produced, so the ranking
/// arithmetic around it is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiFuzzyHit {
    pub title_id: String,
    /// The query token this hit answers.
    pub token_key: String,
    /// The projected token that matched, so the caller can re-measure the
    /// distance and apply the boundary rules the lane has always applied.
    pub matched_term: String,
    pub weight: i64,
}

impl TitleFuzzyIndex {
    /// Open the index, completing any required recovery and rebuild first.
    /// Failure to provide the complete index is an error, never an empty match.
    pub async fn open(data_dir: &Path, source: Arc<dyn TitleTermSource>) -> AppResult<Arc<Self>> {
        let dir = data_dir.join(FUZZY_INDEX_DIR);
        Self::open_inner(dir, source).await
    }

    async fn open_inner(dir: PathBuf, source: Arc<dyn TitleTermSource>) -> AppResult<Arc<Self>> {
        let (_, fields) = build_schema();
        let open_dir = dir.clone();
        let (ownership, index, reader) =
            tokio::task::spawn_blocking(move || open_owned_index(&open_dir))
                .await
                .map_err(|error| AppError::Repository(error.to_string()))??;

        let handle = Arc::new(Self {
            dir,
            _ownership: ownership,
            index,
            reader,
            fields,
            source,
            write_lock: tokio::sync::Mutex::new(()),
            rebuild_complete: AtomicBool::new(false),
        });

        let stamp = handle.source.projection_stamp().await?;
        if handle.read_meta().is_some_and(|meta| meta == stamp) {
            handle.rebuild_complete.store(true, Ordering::SeqCst);
        } else {
            handle.rebuild(stamp).await?;
        }
        handle.sync().await?;
        Ok(handle)
    }

    /// Drop every document and page the projection back in, one writer per
    /// batch. Readers wait for the complete replacement before proceeding.
    pub async fn rebuild(&self, stamp: ProjectionStamp) -> AppResult<()> {
        let _guard = self.write_lock.lock().await;
        self.rebuild_locked(stamp).await
    }

    async fn rebuild_locked(&self, stamp: ProjectionStamp) -> AppResult<()> {
        self.rebuild_complete.store(false, Ordering::SeqCst);
        self.remove_meta()?;

        {
            let index = self.index.clone();
            tokio::task::spawn_blocking(move || -> AppResult<()> {
                let mut writer = index
                    .writer_with_num_threads::<TantivyDocument>(1, WRITER_HEAP_BYTES)
                    .map_err(|error| AppError::Repository(error.to_string()))?;
                writer
                    .delete_all_documents()
                    .map_err(|error| AppError::Repository(error.to_string()))?;
                writer
                    .commit()
                    .map_err(|error| AppError::Repository(error.to_string()))?;
                Ok(())
            })
            .await
            .map_err(|error| AppError::Repository(error.to_string()))??;
        }

        let mut after = 0i64;
        loop {
            let page = self.source.page_terms(after, BATCH_TERMS as i64).await?;
            if page.is_empty() {
                break;
            }
            after = page.iter().map(|term| term.term_id).max().unwrap_or(after);
            self.write_batch(page, Vec::new()).await?;
        }

        // Anything enqueued while the rebuild ran is already reflected by the
        // page that read it, or is still queued and will be drained. Either
        // way the stamp below describes the projection the pages came from.
        self.write_meta(&stamp)?;
        self.reload()?;
        self.rebuild_complete.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Apply everything `title_search_index_queue` holds and check that the
    /// projection generation has not changed while draining it.
    ///
    /// Called before a fuzzy read rather than from a timer, so a caller never
    /// sees a title the database has already accepted but the index has not.
    pub async fn sync(&self) -> AppResult<()> {
        let _guard = self.write_lock.lock().await;
        self.sync_locked().await
    }

    async fn sync_locked(&self) -> AppResult<()> {
        loop {
            let stamp = self.source.projection_stamp().await?;
            if !self.ready() || !self.stamp_matches(&stamp) {
                self.rebuild_locked(stamp).await?;
                continue;
            }
            let queued = self.source.queued_titles(QUEUE_DRAIN_BATCH).await?;
            if queued.is_empty() {
                if self.source.projection_stamp().await? == stamp {
                    return Ok(());
                }
                continue;
            }
            let title_ids = queued
                .iter()
                .map(|entry| entry.title_id.clone())
                .collect::<Vec<_>>();
            let terms = self.source.terms_for_titles(&title_ids).await?;
            self.write_batch(terms, title_ids).await?;
            self.reload()?;
            let seqs = queued.iter().map(|entry| entry.seq).collect::<Vec<_>>();
            self.source.clear_queued(&seqs).await?;
        }
    }

    /// One writer, one commit, dropped. `replace_title_ids` are deleted first,
    /// so a rename cannot leave the previous spelling matchable.
    async fn write_batch(
        &self,
        terms: Vec<IndexedTerm>,
        replace_title_ids: Vec<String>,
    ) -> AppResult<()> {
        if terms.is_empty() && replace_title_ids.is_empty() {
            return Ok(());
        }
        let index = self.index.clone();
        let fields = self.fields;
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let mut writer = index
                .writer_with_num_threads::<TantivyDocument>(1, WRITER_HEAP_BYTES)
                .map_err(|error| AppError::Repository(error.to_string()))?;
            for title_id in &replace_title_ids {
                writer.delete_term(Term::from_field_text(fields.title_id, title_id));
            }
            for chunk in terms.chunks(BATCH_TERMS) {
                for term in chunk {
                    writer
                        .add_document(document_for(&fields, term))
                        .map_err(|error| AppError::Repository(error.to_string()))?;
                }
            }
            writer
                .commit()
                .map_err(|error| AppError::Repository(error.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|error| AppError::Repository(error.to_string()))??;
        Ok(())
    }

    fn reload(&self) -> AppResult<()> {
        self.reader
            .reload()
            .map_err(|error| AppError::Repository(error.to_string()))
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join(META_FILE)
    }

    /// Whether the directory on disk was built from this projection stamp.
    /// This is the invalidation rule in one call: a reopen that answers
    /// `false` rebuilds, and answers `false` for a missing, unreadable or
    /// older-schema meta file too.
    pub fn stamp_matches(&self, stamp: &ProjectionStamp) -> bool {
        self.read_meta().is_some_and(|meta| &meta == stamp)
    }

    fn read_meta(&self) -> Option<ProjectionStamp> {
        #[derive(serde::Deserialize)]
        struct Meta {
            schema_version: u32,
            stamp: ProjectionStamp,
        }
        let bytes = std::fs::read(self.meta_path()).ok()?;
        let meta = serde_json::from_slice::<Meta>(&bytes).ok()?;
        (meta.schema_version == FUZZY_SCHEMA_VERSION).then_some(meta.stamp)
    }

    fn write_meta(&self, stamp: &ProjectionStamp) -> AppResult<()> {
        let payload = serde_json::json!({
            "schema_version": FUZZY_SCHEMA_VERSION,
            "stamp": stamp,
        });
        std::fs::write(
            self.meta_path(),
            serde_json::to_vec(&payload)
                .map_err(|error| AppError::Repository(error.to_string()))?,
        )
        .map_err(|error| AppError::Repository(error.to_string()))
    }

    fn remove_meta(&self) -> AppResult<()> {
        // A rebuild that dies halfway must not leave a stamp claiming the
        // segments are complete.
        match std::fs::remove_file(self.meta_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Repository(error.to_string())),
        }
    }

    fn ready(&self) -> bool {
        self.rebuild_complete.load(Ordering::SeqCst)
    }

    /// The resolver lane: term ids of projected names within `distance` edits
    /// of `match_term`, inside the same bucket the exact lanes use.
    ///
    /// Returns term ids rather than hydrated candidates so the caller reads
    /// the candidate columns from `title_search_terms` — one source of truth
    /// for what a candidate *is*, whichever lane found it.
    pub async fn resolver_candidates(&self, query: ResolverFuzzyQuery<'_>) -> AppResult<Vec<i64>> {
        let _guard = self.write_lock.lock().await;
        self.sync_locked().await?;
        if query.match_term.is_empty() {
            return Ok(Vec::new());
        }
        let fields = self.fields;
        // Pin a complete generation before allowing another rebuild to start.
        let searcher = self.reader.searcher();
        drop(_guard);
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![
            (Occur::Must, term_clause(fields.script, query.script)),
            (
                Occur::Must,
                term_clause(fields.numbers_key, query.numbers_key),
            ),
            (Occur::Must, term_clause(fields.token_row, "0")),
        ];
        if let Some(facet) = query.facet {
            clauses.push((Occur::Must, term_clause(fields.facet, facet)));
        }
        // The projection also holds sort-title and slug rows; the resolver
        // lane matches on names the catalog answers to, exactly as the SQL
        // bucket does with its `term_kind IN (...)`.
        clauses.push((
            Occur::Must,
            Box::new(BooleanQuery::new(
                ["name", "alias", "tagged_alias"]
                    .into_iter()
                    .map(|kind| (Occur::Should, term_clause(fields.term_kind, kind)))
                    .collect(),
            )),
        ));
        clauses.push((
            Occur::Must,
            spelling_clause(&fields, query.match_term, query.distance),
        ));

        let limit = query.limit;
        let boolean = BooleanQuery::new(clauses);
        let hits = tokio::task::spawn_blocking(move || -> tantivy::Result<Vec<i64>> {
            let docs = searcher.search(
                &boolean,
                &TopDocs::with_limit(limit.max(1)).order_by_score(),
            )?;
            let mut term_ids = Vec::with_capacity(docs.len());
            for (_score, address) in docs {
                let doc = searcher.doc::<TantivyDocument>(address)?;
                if let Some(value) = doc
                    .get_first(fields.term_id)
                    .and_then(|value| value.as_i64())
                {
                    term_ids.push(value);
                }
            }
            Ok(term_ids)
        })
        .await;

        hits.map_err(|error| AppError::Repository(error.to_string()))?
            .map_err(|error| AppError::Repository(error.to_string()))
    }

    /// The UI lane: per query token, the titles holding a projected token
    /// within the distance the token's length allows.
    pub async fn ui_candidates(
        &self,
        tokens: &[String],
        facets: &[&str],
        distance_for: impl Fn(usize) -> u8,
        limit_per_token: usize,
    ) -> AppResult<Vec<UiFuzzyHit>> {
        let _guard = self.write_lock.lock().await;
        self.sync_locked().await?;
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        // All tokens must search the same complete generation.
        let searcher = self.reader.searcher();
        drop(_guard);
        let fields = self.fields;
        let mut hits = Vec::new();
        for token in tokens {
            let distance = distance_for(token.chars().count());
            let mut clauses: Vec<(Occur, Box<dyn Query>)> =
                vec![(Occur::Must, term_clause(fields.token_row, "1"))];
            if facets.len() < 3 {
                clauses.push((
                    Occur::Must,
                    Box::new(BooleanQuery::new(
                        facets
                            .iter()
                            .map(|facet| (Occur::Should, term_clause(fields.facet, facet)))
                            .collect(),
                    )),
                ));
            }
            clauses.push((
                Occur::Must,
                Box::new(FuzzyTermQuery::new(
                    Term::from_field_text(fields.lenient_raw, token),
                    distance,
                    true,
                )),
            ));
            let boolean = BooleanQuery::new(clauses);
            let searcher = searcher.clone();
            let token_key = token.clone();
            let found = tokio::task::spawn_blocking(move || -> tantivy::Result<Vec<UiFuzzyHit>> {
                let docs = searcher.search(
                    &boolean,
                    &TopDocs::with_limit(limit_per_token.max(1)).order_by_score(),
                )?;
                let mut found = Vec::with_capacity(docs.len());
                for (_score, address) in docs {
                    let doc = searcher.doc::<TantivyDocument>(address)?;
                    let (Some(title_id), Some(weight), Some(matched_term)) = (
                        doc.get_first(fields.title_id).and_then(|v| v.as_str()),
                        doc.get_first(fields.weight).and_then(|v| v.as_i64()),
                        doc.get_first(fields.lenient_raw).and_then(|v| v.as_str()),
                    ) else {
                        continue;
                    };
                    found.push(UiFuzzyHit {
                        title_id: title_id.to_string(),
                        token_key: token_key.clone(),
                        matched_term: matched_term.to_string(),
                        weight,
                    });
                }
                Ok(found)
            })
            .await;
            hits.extend(
                found
                    .map_err(|error| AppError::Repository(error.to_string()))?
                    .map_err(|error| AppError::Repository(error.to_string()))?,
            );
        }
        Ok(hits)
    }
}

fn reject_symlink(path: &Path) -> AppResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::Repository(format!(
            "title index path must not be a symlink: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Repository(error.to_string())),
    }
}

fn open_index_files(dir: &Path) -> tantivy::Result<(Index, IndexReader)> {
    std::fs::create_dir_all(dir)?;
    if dir.join(META_FILE).exists() && !dir.join("meta.json").exists() {
        return Err(tantivy::directory::error::OpenReadError::FileDoesNotExist(
            dir.join("meta.json"),
        )
        .into());
    }
    let directory = MmapDirectory::open(dir)?;
    let index = Index::open_or_create(directory, build_schema().0)?;
    register_tokenizers(&index)
        .map_err(|error| tantivy::TantivyError::InvalidArgument(error.to_string()))?;
    if !index.validate_checksum()?.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "title index checksum mismatch",
        )
        .into());
    }
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    Ok((index, reader))
}

fn recoverable_index_error(error: &tantivy::TantivyError) -> bool {
    use tantivy::TantivyError;
    use tantivy::directory::error::OpenReadError;
    match error {
        TantivyError::DataCorruption(_)
        | TantivyError::SchemaError(_)
        | TantivyError::IncompatibleIndex(_)
        | TantivyError::DeserializeError(_)
        | TantivyError::OpenReadError(OpenReadError::FileDoesNotExist(_))
        | TantivyError::OpenReadError(OpenReadError::IncompatibleIndex(_)) => true,
        TantivyError::IoError(error)
        | TantivyError::OpenReadError(OpenReadError::IoError {
            io_error: error, ..
        }) => error.kind() == std::io::ErrorKind::InvalidData,
        _ => false,
    }
}

fn open_owned_index(dir: &Path) -> AppResult<(std::fs::File, Index, IndexReader)> {
    let parent = dir
        .parent()
        .ok_or_else(|| AppError::Repository("title index has no parent".into()))?;
    std::fs::create_dir_all(parent).map_err(|error| AppError::Repository(error.to_string()))?;
    let lock_path = parent.join("title-fuzzy-index.lock");
    reject_symlink(&lock_path)?;
    let ownership = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| AppError::Repository(error.to_string()))?;
    ownership.try_lock().map_err(|error| {
        AppError::Repository(format!(
            "title index is already owned or cannot be locked: {error}"
        ))
    })?;
    reject_symlink(dir)?;
    if dir.exists() && !dir.is_dir() {
        return Err(AppError::Repository(
            "title index path is not a directory".into(),
        ));
    }
    let (index, reader) = match open_index_files(dir) {
        Ok(opened) => opened,
        Err(error) if recoverable_index_error(&error) => {
            // Reserve a new sibling without replacing anything already there.
            // Keep the entire old directory, including files Tantivy does not own.
            let mut suffix = 0u64;
            let preserved = loop {
                let candidate = parent.join(format!("{FUZZY_INDEX_DIR}.recovery-{suffix}"));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => suffix += 1,
                    Err(error) => return Err(AppError::Repository(error.to_string())),
                }
            };
            std::fs::rename(dir, preserved.join(FUZZY_INDEX_DIR))
                .map_err(|error| AppError::Repository(error.to_string()))?;
            tracing::warn!(%error, path = %preserved.display(), "preserved unreadable title index; rebuilding before use");
            open_index_files(dir).map_err(|error| AppError::Repository(error.to_string()))?
        }
        Err(error) => return Err(AppError::Repository(error.to_string())),
    };
    Ok((ownership, index, reader))
}

fn term_clause(field: Field, value: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, value),
        IndexRecordOption::Basic,
    ))
}

/// One spelling, two ways of being close to it.
///
/// Tantivy's Levenshtein automaton is exact but stops at
/// [`MAX_AUTOMATON_DISTANCE`], and it counts *bytes*: in a script whose
/// characters are three bytes wide, "one character wrong" is three edits and
/// is indistinguishable from a different title. Both gaps are covered by the
/// same device — character n-grams with a shared-gram floor:
///
/// * A wide script (Han, Kana, Hangul) always goes through bigrams, because
///   the automaton cannot express a character-level tolerance there at all.
/// * A distance above the automaton's ceiling goes through trigrams. `k`
///   edits destroy at most `n * k` n-grams, so a candidate within `k` edits
///   shares at least `G - n * k` of the query's `G` grams. That is a floor,
///   not a heuristic: nothing within the distance can fall below it.
///
/// Either way this lane only has to *find* candidates. Every one of them is
/// then measured exactly by the caller's spelling comparison, so a loose gram
/// hit costs a comparison and can never become a match on its own.
fn spelling_clause(fields: &Fields, match_term: &str, distance: u8) -> Box<dyn Query> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    let wide = is_wide_script(match_term);

    if !wide {
        let automaton_distance = distance.min(MAX_AUTOMATON_DISTANCE);
        for field in [fields.match_raw, fields.literal_raw] {
            clauses.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(
                    Term::from_field_text(field, match_term),
                    automaton_distance,
                    true,
                )),
            ));
        }
    }

    let gram_size = if wide { 2 } else { 3 };
    if wide || distance > MAX_AUTOMATON_DISTANCE {
        let grams = character_ngrams(match_term, gram_size);
        if !grams.is_empty() {
            let destroyed = gram_size * distance as usize;
            let required = grams.len().saturating_sub(destroyed).max(1);
            clauses.push((
                Occur::Should,
                Box::new(BooleanQuery::with_minimum_required_clauses(
                    grams
                        .into_iter()
                        .map(|gram| (Occur::Should, term_clause(fields.grams, gram.as_str())))
                        .collect(),
                    required,
                )),
            ));
        }
    }

    Box::new(BooleanQuery::new(clauses))
}

fn character_ngrams(value: &str, size: usize) -> Vec<String> {
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() < size {
        return Vec::new();
    }
    chars
        .windows(size)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

/// Characters outside the Basic Multilingual Plane's single-byte and
/// two-byte ranges: Han, Kana, Hangul and the CJK punctuation around them.
fn is_wide_script(value: &str) -> bool {
    value.chars().any(|character| {
        matches!(character as u32,
            0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
    })
}

fn document_for(fields: &Fields, term: &IndexedTerm) -> TantivyDocument {
    let mut doc = TantivyDocument::default();
    doc.add_i64(fields.term_id, term.term_id);
    doc.add_text(fields.title_id, &term.title_id);
    doc.add_text(fields.facet, &term.facet);
    doc.add_text(fields.script, &term.script);
    doc.add_text(fields.numbers_key, &term.numbers_key);
    doc.add_text(fields.term_kind, &term.term_kind);
    doc.add_text(
        fields.token_row,
        if term.term_kind.ends_with("_token") {
            "1"
        } else {
            "0"
        },
    );
    doc.add_i64(fields.weight, term.weight);
    doc.add_i64(fields.char_length, term.char_length);
    doc.add_text(fields.match_raw, &term.match_term);
    doc.add_text(fields.literal_raw, &term.literal_term);
    doc.add_text(fields.lenient_raw, &term.normalized_term);
    doc.add_text(fields.grams, &term.match_term);
    doc
}

/// Terms grouped by title, for callers that rebuild a title's documents.
pub fn group_terms_by_title(terms: Vec<IndexedTerm>) -> HashMap<String, Vec<IndexedTerm>> {
    let mut grouped: HashMap<String, Vec<IndexedTerm>> = HashMap::new();
    for term in terms {
        grouped.entry(term.title_id.clone()).or_default().push(term);
    }
    grouped
}
