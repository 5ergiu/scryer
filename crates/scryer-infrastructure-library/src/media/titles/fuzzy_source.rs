//! The database side of the title fuzzy index.
//!
//! Everything the index holds comes from `title_search_terms`, and everything
//! it has to catch up on comes from `title_search_index_queue`. Both are read
//! through the dialect-agnostic runtime, so SQLite and PostgreSQL build the
//! same index from the same rows and every consumer of the fuzzy lane behaves
//! identically on either.

use async_trait::async_trait;
use scryer_application::AppResult;
use scryer_infrastructure_library_search::fuzzy::{
    IndexedTerm, ProjectionStamp, QueuedTitle, TitleTermSource,
};
use scryer_infrastructure_sql::runtime::{SqlArg, SqlRow, SqlRuntime, StoreDatastore};

const TERM_COLUMNS: &str = "term_id, title_id, facet, term_kind, weight, literal_term, \
                            match_term, normalized_term, script, numbers_key, char_length";

pub struct DatastoreTitleTermSource {
    datastore: StoreDatastore,
}

impl DatastoreTitleTermSource {
    pub fn new(datastore: StoreDatastore) -> Self {
        Self { datastore }
    }
}

fn indexed_term(row: &SqlRow) -> AppResult<IndexedTerm> {
    Ok(IndexedTerm {
        term_id: row.i64("term_id")?,
        title_id: row.text("title_id")?,
        facet: row.text("facet")?,
        term_kind: row.text("term_kind")?,
        weight: row.i64("weight")?,
        literal_term: row.text("literal_term")?,
        match_term: row.text("match_term")?,
        normalized_term: row.text("normalized_term")?,
        script: row.text("script")?,
        numbers_key: row.text("numbers_key")?,
        char_length: row.i64("char_length")?,
    })
}

#[async_trait]
impl TitleTermSource for DatastoreTitleTermSource {
    async fn page_terms(&self, after_term_id: i64, limit: i64) -> AppResult<Vec<IndexedTerm>> {
        let sql = format!(
            "SELECT {TERM_COLUMNS} FROM title_search_terms \
             WHERE term_id > {{}} ORDER BY term_id LIMIT {{}}"
        );
        let rows = SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            &sql,
            &[SqlArg::I64(after_term_id), SqlArg::I64(limit)],
        )
        .await?;
        rows.iter().map(indexed_term).collect()
    }

    async fn terms_for_titles(&self, title_ids: &[String]) -> AppResult<Vec<IndexedTerm>> {
        if title_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("{}", title_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT {TERM_COLUMNS} FROM title_search_terms WHERE title_id IN ({placeholders})"
        );
        let args = title_ids
            .iter()
            .cloned()
            .map(SqlArg::Text)
            .collect::<Vec<_>>();
        let rows = SqlRuntime::fetch_all(self.datastore.read_exec(), &sql, &args).await?;
        rows.iter().map(indexed_term).collect()
    }

    async fn queued_titles(&self, limit: i64) -> AppResult<Vec<QueuedTitle>> {
        let rows = SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            "SELECT seq, title_id FROM title_search_index_queue ORDER BY seq LIMIT {}",
            &[SqlArg::I64(limit)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                Ok(QueuedTitle {
                    seq: row.i64("seq")?,
                    title_id: row.text("title_id")?,
                })
            })
            .collect()
    }

    async fn clear_queued(&self, seqs: &[i64]) -> AppResult<()> {
        if seqs.is_empty() {
            return Ok(());
        }
        let placeholders = std::iter::repeat_n("{}", seqs.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM title_search_index_queue WHERE seq IN ({placeholders})");
        let args = seqs.iter().copied().map(SqlArg::I64).collect::<Vec<_>>();
        SqlRuntime::execute_write(&self.datastore, "clear_title_fuzzy_index_queue", &sql, args)
            .await?;
        Ok(())
    }

    async fn projection_stamp(&self) -> AppResult<ProjectionStamp> {
        let row = SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT collation_version, projection_generation \
             FROM title_search_meta WHERE id = 1",
            &[],
        )
        .await?;
        let Some(row) = row else {
            // No stamp is a projection that has never been built. Treating it
            // as its own generation keeps the index and the projection in the
            // same state: both empty, both about to be seeded.
            return Ok(ProjectionStamp {
                collation_version: String::new(),
                projection_generation: 0,
            });
        };
        Ok(ProjectionStamp {
            collation_version: row.text("collation_version")?,
            projection_generation: row.i64("projection_generation")?,
        })
    }
}
