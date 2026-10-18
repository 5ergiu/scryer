use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use scryer_application::{
    AppError, AppResult, DelayProfile, DelayProfileRepository, PreferredProtocol,
};

use crate::queries::sql_runtime::{SqlArg, SqlRow, SqlRuntime, SqlTx, StoreDatastore};

const DELAY_PROFILE_COLUMNS: &str = "id, name, usenet_delay_minutes, torrent_delay_minutes,
    preferred_protocol, min_age_minutes, bypass_score_threshold, enable_usenet, enable_torrent,
    bypass_if_highest_quality, priority, enabled";

const DELAY_PROFILE_UPSERT_SQL: &str = "INSERT INTO delay_profiles (
    id, name, usenet_delay_minutes, torrent_delay_minutes, preferred_protocol, min_age_minutes,
    bypass_score_threshold, enable_usenet, enable_torrent, bypass_if_highest_quality, priority,
    enabled, created_at, updated_at
) VALUES (
    {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}
)
ON CONFLICT(id) DO UPDATE SET
    name = excluded.name,
    usenet_delay_minutes = excluded.usenet_delay_minutes,
    torrent_delay_minutes = excluded.torrent_delay_minutes,
    preferred_protocol = excluded.preferred_protocol,
    min_age_minutes = excluded.min_age_minutes,
    bypass_score_threshold = excluded.bypass_score_threshold,
    enable_usenet = excluded.enable_usenet,
    enable_torrent = excluded.enable_torrent,
    bypass_if_highest_quality = excluded.bypass_if_highest_quality,
    priority = excluded.priority,
    enabled = excluded.enabled,
    updated_at = excluded.updated_at";

/// The three sets a profile owns, each stored as `(profile_id, value,
/// position)` rows so a profile reads back in the order it was saved.
const DELAY_PROFILE_SETS: [(&str, &str); 3] = [
    ("delay_profile_facets", "facet"),
    ("delay_profile_tags", "label"),
    ("delay_profile_libraries", "library_id"),
];

#[derive(Clone)]
pub struct DelayProfileStore {
    datastore: StoreDatastore,
}

impl DelayProfileStore {
    pub fn new(datastore: StoreDatastore) -> Self {
        Self { datastore }
    }
}

#[async_trait]
impl DelayProfileRepository for DelayProfileStore {
    async fn list(&self) -> AppResult<Vec<DelayProfile>> {
        let exec = || self.datastore.read_exec();
        let rows = SqlRuntime::fetch_all(
            exec(),
            &format!("SELECT {DELAY_PROFILE_COLUMNS} FROM delay_profiles ORDER BY priority, id"),
            &[],
        )
        .await?;
        let mut profiles = rows
            .iter()
            .map(row_to_delay_profile)
            .collect::<AppResult<Vec<_>>>()?;

        let mut sets = Vec::with_capacity(DELAY_PROFILE_SETS.len());
        for (table, column) in DELAY_PROFILE_SETS {
            let rows = SqlRuntime::fetch_all(
                exec(),
                &format!(
                    "SELECT profile_id, {column} AS value FROM {table}
                     ORDER BY profile_id, position, {column}"
                ),
                &[],
            )
            .await?;
            let mut by_profile = HashMap::<String, Vec<String>>::new();
            for row in rows {
                by_profile
                    .entry(row.text("profile_id")?)
                    .or_default()
                    .push(row.text("value")?);
            }
            sets.push(by_profile);
        }
        let [mut facets, mut tags, mut libraries]: [HashMap<String, Vec<String>>; 3] = sets
            .try_into()
            .map_err(|_| AppError::Repository("delay profile sets did not load".to_string()))?;
        for profile in &mut profiles {
            profile.applies_to_facets = facets.remove(&profile.id).unwrap_or_default();
            profile.tags = tags.remove(&profile.id).unwrap_or_default();
            profile.library_ids = libraries.remove(&profile.id).unwrap_or_default();
        }
        Ok(profiles)
    }

    async fn save_many(&self, profiles: Vec<DelayProfile>) -> AppResult<()> {
        if profiles.is_empty() {
            return Ok(());
        }
        SqlRuntime::run_in_transaction(&self.datastore, "save_delay_profiles", move |tx| {
            let profiles = profiles.clone();
            Box::pin(async move {
                for profile in &profiles {
                    save_delay_profile_tx(tx, profile).await?;
                }
                Ok(())
            })
        })
        .await
    }

    async fn delete(&self, id: &str) -> AppResult<bool> {
        let id = id.to_string();
        SqlRuntime::run_in_transaction(&self.datastore, "delete_delay_profile", move |tx| {
            let id = id.clone();
            Box::pin(async move {
                // The set tables cascade from the profile row.
                let rows = tx
                    .execute(
                        "DELETE FROM delay_profiles WHERE id = {}",
                        &[SqlArg::Text(id)],
                    )
                    .await?;
                Ok(rows > 0)
            })
        })
        .await
    }
}

async fn save_delay_profile_tx(tx: &mut SqlTx<'_>, profile: &DelayProfile) -> AppResult<()> {
    let now = Utc::now();
    tx.execute(
        DELAY_PROFILE_UPSERT_SQL,
        &[
            SqlArg::Text(profile.id.clone()),
            SqlArg::Text(profile.name.clone()),
            SqlArg::I64(profile.usenet_delay_minutes),
            SqlArg::I64(profile.torrent_delay_minutes),
            SqlArg::Text(profile.preferred_protocol.as_str().to_string()),
            SqlArg::I64(profile.min_age_minutes),
            SqlArg::OptI32(profile.bypass_score_threshold),
            SqlArg::Bool(profile.enable_usenet),
            SqlArg::Bool(profile.enable_torrent),
            SqlArg::Bool(profile.bypass_if_highest_quality),
            SqlArg::I32(profile.priority),
            SqlArg::Bool(profile.enabled),
            SqlArg::Timestamp(now),
            SqlArg::Timestamp(now),
        ],
    )
    .await?;

    let values = [
        &profile.applies_to_facets,
        &profile.tags,
        &profile.library_ids,
    ];
    for ((table, column), values) in DELAY_PROFILE_SETS.into_iter().zip(values) {
        tx.execute(
            &format!("DELETE FROM {table} WHERE profile_id = {{}}"),
            &[SqlArg::Text(profile.id.clone())],
        )
        .await?;
        for (position, value) in values.iter().enumerate() {
            tx.execute(
                &format!(
                    "INSERT INTO {table} (profile_id, {column}, position) VALUES ({{}}, {{}}, {{}})"
                ),
                &[
                    SqlArg::Text(profile.id.clone()),
                    SqlArg::Text(value.clone()),
                    SqlArg::I32(i32::try_from(position).unwrap_or(i32::MAX)),
                ],
            )
            .await?;
        }
    }
    Ok(())
}

fn row_to_delay_profile(row: &SqlRow) -> AppResult<DelayProfile> {
    let id = row.text("id")?;
    let preferred_protocol = row.text("preferred_protocol")?;
    Ok(DelayProfile {
        preferred_protocol: PreferredProtocol::parse(&preferred_protocol).ok_or_else(|| {
            AppError::Repository(format!(
                "delay profile {id} has an unknown preferred protocol '{preferred_protocol}'"
            ))
        })?,
        name: row.text("name")?,
        usenet_delay_minutes: row.i64("usenet_delay_minutes")?,
        torrent_delay_minutes: row.i64("torrent_delay_minutes")?,
        min_age_minutes: row.i64("min_age_minutes")?,
        bypass_score_threshold: row.opt_i32("bypass_score_threshold")?,
        enable_usenet: row.bool("enable_usenet")?,
        enable_torrent: row.bool("enable_torrent")?,
        bypass_if_highest_quality: row.bool("bypass_if_highest_quality")?,
        applies_to_facets: Vec::new(),
        tags: Vec::new(),
        library_ids: Vec::new(),
        priority: row.i32("priority")?,
        enabled: row.bool("enabled")?,
        id,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn store() -> (DelayProfileStore, sqlx::SqlitePool) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should open");
        sqlx::raw_sql(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE libraries (id TEXT PRIMARY KEY NOT NULL);
             INSERT INTO libraries (id) VALUES ('library-a'), ('library-b');",
        )
        .execute(&pool)
        .await
        .expect("library fixture should apply");
        sqlx::raw_sql(include_str!(
            "../../scryer/src/db/migrations/0293_delay_profile_tables.sql"
        ))
        .execute(&pool)
        .await
        .expect("delay profile schema should apply");
        let store = DelayProfileStore::new(StoreDatastore::Sqlite {
            pool: pool.clone(),
            writer_gate: Arc::new(tokio::sync::Mutex::new(())),
        });
        (store, pool)
    }

    fn profile(id: &str, priority: i32) -> DelayProfile {
        DelayProfile {
            id: id.to_string(),
            name: format!("Profile {id}"),
            usenet_delay_minutes: 60,
            torrent_delay_minutes: 120,
            preferred_protocol: PreferredProtocol::Torrent,
            min_age_minutes: 15,
            bypass_score_threshold: Some(900),
            enable_usenet: true,
            enable_torrent: false,
            bypass_if_highest_quality: true,
            applies_to_facets: vec!["series".to_string(), "movie".to_string()],
            tags: vec!["keep".to_string(), "needs review".to_string()],
            library_ids: vec!["library-b".to_string(), "library-a".to_string()],
            priority,
            enabled: true,
        }
    }

    #[tokio::test]
    async fn delay_profiles_round_trip_with_their_sets_in_saved_order() {
        let (store, _pool) = store().await;
        store
            .save_many(vec![profile("second", 20), profile("first", 10)])
            .await
            .expect("profiles should save");

        let loaded = store.list().await.expect("profiles should load");
        assert_eq!(
            loaded.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["first", "second"],
            "profiles load in priority order"
        );
        let first = &loaded[0];
        assert_eq!(first.name, "Profile first");
        assert_eq!(first.usenet_delay_minutes, 60);
        assert_eq!(first.torrent_delay_minutes, 120);
        assert_eq!(first.preferred_protocol, PreferredProtocol::Torrent);
        assert_eq!(first.min_age_minutes, 15);
        assert_eq!(first.bypass_score_threshold, Some(900));
        assert!(first.enable_usenet);
        assert!(!first.enable_torrent);
        assert!(first.bypass_if_highest_quality);
        assert!(first.enabled);
        assert_eq!(first.applies_to_facets, vec!["series", "movie"]);
        assert_eq!(first.tags, vec!["keep", "needs review"]);
        assert_eq!(first.library_ids, vec!["library-b", "library-a"]);
    }

    #[tokio::test]
    async fn saving_a_profile_again_replaces_its_sets() {
        let (store, _pool) = store().await;
        store
            .save_many(vec![profile("only", 10)])
            .await
            .expect("profile should save");
        let mut edited = profile("only", 10);
        edited.applies_to_facets.clear();
        edited.tags = vec!["fresh".to_string()];
        edited.library_ids = vec!["library-a".to_string()];
        edited.bypass_score_threshold = None;
        edited.enabled = false;
        store
            .save_many(vec![edited])
            .await
            .expect("profile should update");

        let loaded = store.list().await.expect("profiles should load");
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].applies_to_facets.is_empty());
        assert_eq!(loaded[0].tags, vec!["fresh"]);
        assert_eq!(loaded[0].library_ids, vec!["library-a"]);
        assert_eq!(loaded[0].bypass_score_threshold, None);
        assert!(!loaded[0].enabled);
    }

    #[tokio::test]
    async fn deleting_a_profile_removes_its_sets() {
        let (store, pool) = store().await;
        store
            .save_many(vec![profile("gone", 10), profile("kept", 20)])
            .await
            .expect("profiles should save");

        assert!(store.delete("gone").await.expect("delete should run"));
        assert!(!store.delete("gone").await.expect("delete should run"));

        let loaded = store.list().await.expect("profiles should load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "kept");
        for table in [
            "delay_profile_facets",
            "delay_profile_tags",
            "delay_profile_libraries",
        ] {
            let orphans: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*) FROM {table} WHERE profile_id = 'gone'"
            )))
            .fetch_one(&pool)
            .await
            .expect("count should run");
            assert_eq!(orphans, 0, "{table} keeps no rows for a deleted profile");
        }
    }

    #[tokio::test]
    async fn an_unknown_library_is_refused_and_nothing_is_saved() {
        let (store, _pool) = store().await;
        let mut stray = profile("stray", 10);
        stray.library_ids = vec!["library-missing".to_string()];
        store
            .save_many(vec![profile("good", 5), stray])
            .await
            .expect_err("an unknown library id should fail the save");
        assert!(
            store.list().await.expect("profiles should load").is_empty(),
            "the batch is one transaction"
        );
    }
}
