use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use scryer_application::{
    AppError, AppResult, ClientJobLocator, DownloadClientBindingRecord, DownloadOrigin,
    DownloadRecord, DownloadRegistryRepository, ObservationResolution, ObservedClientJob,
};
use scryer_domain::download_identity::DownloadId;

use super::unique_violation::is_unique_violation;
use super::{normalize_download_client_id, opt_timestamp_string, timestamp_string};
use crate::queries::sql_runtime::{SqlArg, SqlExec, SqlRow, SqlRuntime, SqlTx, StoreDatastore};

/// Maximum native item ids bound into one `IN (...)` list. Kept well under
/// SQLite's historical 999-parameter ceiling so a title with a long grab
/// history never trips the statement limit on either dialect.
const ACTIVE_BINDING_LOOKUP_CHUNK: usize = 200;

#[derive(Clone)]
pub struct DownloadRegistryStore {
    datastore: StoreDatastore,
}

struct ObservationState {
    first_observed_at: Option<DateTime<Utc>>,
    last_observed_at: Option<DateTime<Utc>>,
}

struct ActiveObservationBinding {
    binding: DownloadClientBindingRecord,
    state: ObservationState,
}

impl DownloadRegistryStore {
    pub fn new(datastore: StoreDatastore) -> Self {
        Self { datastore }
    }

    async fn resolve_observation_attempt(
        &self,
        observation: &ObservedClientJob,
    ) -> AppResult<ObservationResolution> {
        let observation = observation.clone();
        SqlRuntime::run_in_transaction(&self.datastore, "resolve_download_observation", move |tx| {
            let observation = observation.clone();
            Box::pin(async move { resolve_observation_tx(tx, &observation).await })
        })
        .await
    }

    /// Insert-or-load recovery for a first observation that lost the race for
    /// its locator.
    ///
    /// The attempt's transaction is already gone: a failed statement poisons a
    /// postgres transaction, and `run_in_transaction` propagates the error
    /// without committing, so the whole attempt (including its `downloads`
    /// insert) has rolled back and nothing can be salvaged in place. Recovery
    /// therefore runs in a *fresh* transaction that simply re-reads the active
    /// binding the winner committed — the winner is durable by the time the
    /// loser's unique violation surfaces, so a single retry is enough.
    ///
    /// `conflict` is the original unique violation and is returned unchanged
    /// when no winner is visible, so a genuine constraint failure is never
    /// swallowed or turned into an invented identity.
    async fn converge_on_locator_winner(
        &self,
        observation: &ObservedClientJob,
        conflict: AppError,
    ) -> AppResult<ObservationResolution> {
        let locator = observation.locator.clone();
        let observation = observation.clone();
        let resolution = SqlRuntime::run_in_transaction(
            &self.datastore,
            "converge_download_observation_on_locator_winner",
            move |tx| {
                let observation = observation.clone();
                Box::pin(async move {
                    let token_id = wire_token_id(&observation);
                    resolve_against_active_locator_tx(tx, &observation, token_id).await
                })
            },
        )
        .await?;
        let Some(resolution) = resolution else {
            return Err(conflict);
        };
        tracing::debug!(
            client_id = locator.client_id.as_deref().unwrap_or_default(),
            client_type = %locator.client_type,
            item_id = %locator.item_id,
            resolution = ?resolution,
            "converged a racing first observation onto the locator's committed winner"
        );
        Ok(resolution)
    }
}

#[async_trait]
impl DownloadRegistryRepository for DownloadRegistryStore {
    async fn resolve_observation(
        &self,
        observation: &ObservedClientJob,
    ) -> AppResult<ObservationResolution> {
        match self.resolve_observation_attempt(observation).await {
            // A concurrent writer committed this locator's active binding
            // between the pre-insert re-check and the insert, so the insert hit
            // the active-locator unique index (or, for a token-bearing first
            // sighting of an already-adopted token, the `downloads` primary
            // key). Converge on the winner instead of failing the cycle. On
            // sqlite the writer gate serializes transactions, so this arm is
            // unreachable there.
            Err(error) if is_unique_violation(&error) => {
                self.converge_on_locator_winner(observation, error).await
            }
            result => result,
        }
    }

    async fn load_download(&self, id: &DownloadId) -> AppResult<Option<DownloadRecord>> {
        SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT id, origin, created_at, first_observed_at, last_observed_at, terminal_at
             FROM downloads
             WHERE id = {}",
            &[SqlArg::Text(id.to_string())],
        )
        .await?
        .map(download_from_row)
        .transpose()
    }

    async fn load_binding(
        &self,
        id: &DownloadId,
    ) -> AppResult<Option<DownloadClientBindingRecord>> {
        SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                    native_item_id, created_at, last_seen_at, ended_at
             FROM download_client_bindings
             WHERE download_id = {}",
            &[SqlArg::Text(id.to_string())],
        )
        .await?
        .map(binding_from_row)
        .transpose()
    }

    async fn find_active_binding_by_locator(
        &self,
        locator: &ClientJobLocator,
    ) -> AppResult<Option<DownloadClientBindingRecord>> {
        let client_id = locator.client_id.clone().unwrap_or_default();
        // One statement, two ways to match:
        //
        // 1. the exact locator, as before; or
        // 2. a *legacy* row (migration 0179 backfilled it from a submission
        //    that predates per-client attribution, so its `client_config_id` is
        //    NULL or blank) with the same type and item id, but only when the
        //    requested client is this install's single configured client of
        //    that type. Same attribution rule as the cleanup store's
        //    `resolve_legacy_cleanup_client_tx`; disabled clients still count,
        //    so a second configured instance makes the owner ambiguous and the
        //    legacy arm matches nothing.
        //
        // The `ORDER BY` keeps an exact match strictly ahead of a legacy one,
        // and the legacy arm is inert for a locator with no client id, which is
        // what the first arm already resolves.
        SqlRuntime::fetch_optional(
            self.datastore.read_exec(),
            "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                    native_item_id, created_at, last_seen_at, ended_at
             FROM download_client_bindings
             WHERE ended_at IS NULL
               AND native_item_id IS NOT NULL
               AND LOWER(TRIM(COALESCE(client_type_snapshot, ''))) = {}
               AND native_item_id = {}
               AND (
                     COALESCE(client_config_id, '') = {}
                  OR (
                       {} <> ''
                       AND TRIM(COALESCE(client_config_id, '')) = ''
                       AND EXISTS (
                             SELECT 1 FROM download_clients dc
                             WHERE dc.id = {}
                               AND LOWER(TRIM(COALESCE(dc.client_type, ''))) = {}
                           )
                       AND (
                             SELECT COUNT(*) FROM download_clients dc_all
                             WHERE LOWER(TRIM(COALESCE(dc_all.client_type, ''))) = {}
                           ) = 1
                     )
               )
             ORDER BY CASE WHEN COALESCE(client_config_id, '') = {} THEN 0 ELSE 1 END,
                      created_at, download_id
             LIMIT 1",
            &[
                SqlArg::Text(locator.client_type.clone()),
                SqlArg::Text(locator.item_id.clone()),
                SqlArg::Text(client_id.clone()),
                SqlArg::Text(client_id.clone()),
                SqlArg::Text(client_id.clone()),
                SqlArg::Text(locator.client_type.clone()),
                SqlArg::Text(locator.client_type.clone()),
                SqlArg::Text(client_id),
            ],
        )
        .await?
        .map(binding_from_row)
        .transpose()
    }

    async fn list_active_bindings_for_native_item_ids(
        &self,
        native_item_ids: &[String],
    ) -> AppResult<Vec<DownloadClientBindingRecord>> {
        let mut seen = std::collections::HashSet::new();
        let wanted: Vec<String> = native_item_ids
            .iter()
            .map(|item_id| item_id.trim())
            .filter(|item_id| !item_id.is_empty())
            .filter(|item_id| seen.insert(item_id.to_string()))
            .map(str::to_string)
            .collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }

        let mut bindings = Vec::new();
        for chunk in wanted.chunks(ACTIVE_BINDING_LOOKUP_CHUNK) {
            let placeholders = (0..chunk.len())
                .map(|_| "{}")
                .collect::<Vec<_>>()
                .join(", ");
            let args: Vec<SqlArg> = chunk.iter().cloned().map(SqlArg::Text).collect();
            let rows = SqlRuntime::fetch_all(
                self.datastore.read_exec(),
                &format!(
                    "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                            native_item_id, created_at, last_seen_at, ended_at
                     FROM download_client_bindings
                     WHERE ended_at IS NULL
                       AND native_item_id IS NOT NULL
                       AND native_item_id IN ({placeholders})
                     ORDER BY created_at, download_id"
                ),
                &args,
            )
            .await?;
            for row in rows {
                bindings.push(binding_from_row(row)?);
            }
        }
        Ok(bindings)
    }

    async fn list_active_bindings_for_client_before(
        &self,
        client_config_id: &str,
        client_type: &str,
        observed_before: DateTime<Utc>,
        after: Option<(DateTime<Utc>, DownloadId)>,
        limit: usize,
    ) -> AppResult<Vec<DownloadClientBindingRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut args = vec![
            SqlArg::Text(client_config_id.to_string()),
            SqlArg::Text(client_type.trim().to_ascii_lowercase()),
            SqlArg::Timestamp(observed_before),
            SqlArg::Timestamp(observed_before),
        ];
        // Keyset over the result's own `(created_at, download_id)` ordering, so
        // a pass resumes where the previous one stopped instead of re-reading
        // the oldest prefix that the disposition rules deliberately preserve.
        let keyset = match after {
            Some((cursor_created_at, cursor_download_id)) => {
                args.push(SqlArg::Timestamp(cursor_created_at));
                args.push(SqlArg::Timestamp(cursor_created_at));
                args.push(SqlArg::Text(cursor_download_id.to_string()));
                "AND (created_at > {} OR (created_at = {} AND download_id > {}))"
            }
            None => "",
        };
        args.push(SqlArg::I64(limit as i64));

        SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            &format!(
                "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                        native_item_id, created_at, last_seen_at, ended_at
                 FROM download_client_bindings
                 WHERE ended_at IS NULL
                   AND native_item_id IS NOT NULL
                   AND client_config_id = {{}}
                   AND LOWER(TRIM(COALESCE(client_type_snapshot, ''))) = {{}}
                   AND created_at <= {{}}
                   AND (last_seen_at IS NULL OR last_seen_at <= {{}})
                   {keyset}
                 ORDER BY created_at, download_id
                 LIMIT {{}}"
            ),
            &args,
        )
        .await?
        .into_iter()
        .map(binding_from_row)
        .collect()
    }

    async fn list_active_legacy_client_bindings_before(
        &self,
        observed_before: DateTime<Utc>,
        after: Option<(DateTime<Utc>, DownloadId)>,
        limit: usize,
    ) -> AppResult<Vec<DownloadClientBindingRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut args = vec![
            SqlArg::Timestamp(observed_before),
            SqlArg::Timestamp(observed_before),
        ];
        // Same keyset contract as the per-client listing above.
        let keyset = match after {
            Some((cursor_created_at, cursor_download_id)) => {
                args.push(SqlArg::Timestamp(cursor_created_at));
                args.push(SqlArg::Timestamp(cursor_created_at));
                args.push(SqlArg::Text(cursor_download_id.to_string()));
                "AND (created_at > {} OR (created_at = {} AND download_id > {}))"
            }
            None => "",
        };
        args.push(SqlArg::I64(limit as i64));

        SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            &format!(
                "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                        native_item_id, created_at, last_seen_at, ended_at
                 FROM download_client_bindings
                 WHERE ended_at IS NULL
                   AND native_item_id IS NOT NULL
                   AND TRIM(COALESCE(client_config_id, '')) = ''
                   AND created_at <= {{}}
                   AND (last_seen_at IS NULL OR last_seen_at <= {{}})
                   {keyset}
                 ORDER BY created_at, download_id
                 LIMIT {{}}"
            ),
            &args,
        )
        .await?
        .into_iter()
        .map(binding_from_row)
        .collect()
    }

    async fn attribute_legacy_binding_client(
        &self,
        id: &DownloadId,
        client_config_id: &str,
    ) -> AppResult<()> {
        let client_config_id = client_config_id.trim().to_string();
        if client_config_id.is_empty() {
            return Ok(());
        }
        let id = id.to_string();
        SqlRuntime::run_in_transaction(
            &self.datastore,
            "attribute_legacy_download_client_binding",
            move |tx| {
                let id = id.clone();
                let client_config_id = client_config_id.clone();
                Box::pin(async move {
                    // Only fills a blank attribution, and never one that would
                    // collide with `idx_download_client_bindings_active_locator_unique`:
                    // if another live binding already holds this
                    // (client, type, item) the legacy row stays as it is rather
                    // than failing the pass.
                    SqlRuntime::execute(
                        SqlExec::Tx(tx),
                        "UPDATE download_client_bindings
                         SET client_config_id = {}
                         WHERE download_id = {}
                           AND TRIM(COALESCE(client_config_id, '')) = ''
                           AND NOT EXISTS (
                                 SELECT 1 FROM download_client_bindings other
                                 WHERE other.download_id <> download_client_bindings.download_id
                                   AND other.ended_at IS NULL
                                   AND other.native_item_id = download_client_bindings.native_item_id
                                   AND LOWER(TRIM(COALESCE(other.client_type_snapshot, '')))
                                       = LOWER(TRIM(COALESCE(download_client_bindings.client_type_snapshot, '')))
                                   AND COALESCE(other.client_config_id, '') = {}
                               )",
                        &[
                            SqlArg::Text(client_config_id.clone()),
                            SqlArg::Text(id),
                            SqlArg::Text(client_config_id),
                        ],
                    )
                    .await?;
                    Ok(())
                })
            },
        )
        .await
    }

    async fn list_active_binding_clients(&self) -> AppResult<Vec<(String, String)>> {
        SqlRuntime::fetch_all(
            self.datastore.read_exec(),
            "SELECT DISTINCT client_config_id,
                    LOWER(TRIM(COALESCE(client_type_snapshot, ''))) AS client_type
             FROM download_client_bindings
             WHERE ended_at IS NULL
               AND native_item_id IS NOT NULL
               AND client_config_id IS NOT NULL
               AND client_config_id <> ''
             ORDER BY client_config_id, client_type",
            &[],
        )
        .await?
        .into_iter()
        .map(|row| Ok((row.text("client_config_id")?, row.text("client_type")?)))
        .collect()
    }

    async fn end_binding(&self, id: &DownloadId) -> AppResult<()> {
        let id = id.to_string();
        SqlRuntime::run_in_transaction(&self.datastore, "end_download_client_binding", move |tx| {
            let id = id.clone();
            Box::pin(async move {
                SqlRuntime::execute(
                    SqlExec::Tx(tx),
                    "UPDATE download_client_bindings
                     SET ended_at = {}
                     WHERE download_id = {}
                       AND ended_at IS NULL",
                    &[SqlArg::Timestamp(Utc::now()), SqlArg::Text(id)],
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
}

async fn resolve_observation_tx(
    tx: &mut SqlTx<'_>,
    observation: &ObservedClientJob,
) -> AppResult<ObservationResolution> {
    let token_id = wire_token_id(observation);
    if let Some(resolution) = resolve_against_active_locator_tx(tx, observation, token_id).await? {
        return Ok(resolution);
    }

    if let Some(token_id) = token_id {
        if let Some(state) = observation_state_for_download_tx(tx, token_id).await? {
            let mut attached = false;
            let binding = match binding_for_download_tx(tx, token_id).await? {
                // An unbound binding may only adopt an observation from the
                // client it was submitted to; a mismatch falls through to the
                // catch-all conflict arm below instead of writing a binding
                // that mixes one client's config with another's native item.
                Some(binding)
                    if binding.ended_at.is_none()
                        && binding.native_item_id.is_none()
                        && unbound_binding_accepts_locator(&binding, &observation.locator) =>
                {
                    attach_unbound_binding_tx(tx, token_id, observation).await?;
                    attached = true;
                    Some(binding)
                }
                Some(binding) if binding_matches_locator(&binding, &observation.locator) => {
                    Some(binding)
                }
                Some(binding)
                    if binding.ended_at.is_some()
                        && binding_locator_fields_match(&binding, &observation.locator) =>
                {
                    return Ok(ObservationResolution::BindingAlreadyEnded);
                }
                Some(binding) if binding.ended_at.is_none() && binding.native_item_id.is_some() => {
                    return Ok(locator_conflict(token_id, binding.download_id));
                }
                Some(binding) => {
                    return Ok(locator_conflict(token_id, binding.download_id));
                }
                None => {
                    create_bound_binding_tx(tx, token_id, observation).await?;
                    attached = true;
                    None
                }
            };
            if attached {
                touch_observation_tx(tx, token_id, observation).await?;
            } else if let Some(binding) = binding {
                touch_observation_if_stale_tx(tx, token_id, &state, &binding, observation).await?;
            }
            return Ok(ObservationResolution::Resolved {
                download_id: token_id,
                newly_foreign: false,
                attached: true,
            });
        }

        // The writer gate serializes SQLite writers, but re-check immediately
        // before the insert for datastore implementations with concurrent writers.
        if let Some(active_binding) =
            active_observation_binding_by_locator_tx(tx, &observation.locator).await?
        {
            let binding = active_binding.binding;
            return Ok(locator_conflict(token_id, binding.download_id));
        }
        create_foreign_observation_tx(tx, token_id, observation).await?;
        return Ok(ObservationResolution::Resolved {
            download_id: token_id,
            newly_foreign: true,
            attached: false,
        });
    }

    let candidates = ambiguous_submission_candidates_tx(tx, observation).await?;
    if let [download_id] = candidates.as_slice() {
        attach_unbound_binding_tx(tx, *download_id, observation).await?;
        touch_observation_tx(tx, *download_id, observation).await?;
        return Ok(ObservationResolution::Resolved {
            download_id: *download_id,
            newly_foreign: false,
            attached: true,
        });
    }

    // The active-locator unique index (0180) backstops this, but re-check in
    // this transaction so serialized-writer engines converge without erroring.
    if let Some(active_binding) =
        active_observation_binding_by_locator_tx(tx, &observation.locator).await?
    {
        let ActiveObservationBinding { binding, state } = active_binding;
        touch_observation_if_stale_tx(tx, binding.download_id, &state, &binding, observation)
            .await?;
        return Ok(ObservationResolution::Resolved {
            download_id: binding.download_id,
            newly_foreign: false,
            attached: false,
        });
    }

    let download_id = DownloadId::new();
    create_foreign_observation_tx(tx, download_id, observation).await?;
    Ok(ObservationResolution::Resolved {
        download_id,
        newly_foreign: true,
        attached: false,
    })
}

fn wire_token_id(observation: &ObservedClientJob) -> Option<DownloadId> {
    observation
        .wire_token
        .as_deref()
        .and_then(DownloadId::from_wire)
}

/// Resolve an observation against the active binding that already owns its
/// locator, or `None` when no such binding exists.
///
/// This is both the first step of [`resolve_observation_tx`] and the whole of
/// the insert-or-load recovery: a loser of the active-locator race re-runs
/// exactly this step in a fresh transaction and lands on the winner.
async fn resolve_against_active_locator_tx(
    tx: &mut SqlTx<'_>,
    observation: &ObservedClientJob,
    token_id: Option<DownloadId>,
) -> AppResult<Option<ObservationResolution>> {
    let Some(ActiveObservationBinding { binding, state }) =
        active_observation_binding_by_locator_tx(tx, &observation.locator).await?
    else {
        return Ok(None);
    };
    if let Some(token_id) = token_id
        && token_id != binding.download_id
    {
        return Ok(Some(locator_conflict(token_id, binding.download_id)));
    }
    touch_observation_if_stale_tx(tx, binding.download_id, &state, &binding, observation).await?;
    Ok(Some(ObservationResolution::Resolved {
        download_id: binding.download_id,
        newly_foreign: false,
        attached: false,
    }))
}

async fn active_observation_binding_by_locator_tx(
    tx: &mut SqlTx<'_>,
    locator: &ClientJobLocator,
) -> AppResult<Option<ActiveObservationBinding>> {
    SqlRuntime::fetch_optional(
        SqlExec::Tx(tx),
        "SELECT b.download_id, b.client_config_id, b.client_type_snapshot, b.client_name_snapshot,
                b.native_item_id, b.created_at, b.last_seen_at, b.ended_at,
                d.first_observed_at, d.last_observed_at
         FROM download_client_bindings b
         JOIN downloads d ON d.id = b.download_id
         WHERE b.ended_at IS NULL
           AND b.native_item_id IS NOT NULL
           AND COALESCE(b.client_config_id, '') = {}
           AND LOWER(TRIM(COALESCE(b.client_type_snapshot, ''))) = {}
           AND b.native_item_id = {}
         ORDER BY b.created_at, b.download_id
         LIMIT 1",
        &[
            SqlArg::Text(locator.client_id.clone().unwrap_or_default()),
            SqlArg::Text(locator.client_type.clone()),
            SqlArg::Text(locator.item_id.clone()),
        ],
    )
    .await?
    .map(|row| {
        Ok(ActiveObservationBinding {
            state: ObservationState {
                first_observed_at: optional_timestamp_from_row(&row, "first_observed_at")?,
                last_observed_at: optional_timestamp_from_row(&row, "last_observed_at")?,
            },
            binding: binding_from_row(row)?,
        })
    })
    .transpose()
}

async fn binding_for_download_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
) -> AppResult<Option<DownloadClientBindingRecord>> {
    SqlRuntime::fetch_optional(
        SqlExec::Tx(tx),
        "SELECT download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                native_item_id, created_at, last_seen_at, ended_at
         FROM download_client_bindings
         WHERE download_id = {}",
        &[SqlArg::Text(download_id.to_string())],
    )
    .await?
    .map(binding_from_row)
    .transpose()
}

async fn observation_state_for_download_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
) -> AppResult<Option<ObservationState>> {
    SqlRuntime::fetch_optional(
        SqlExec::Tx(tx),
        "SELECT first_observed_at, last_observed_at
         FROM downloads
         WHERE id = {}",
        &[SqlArg::Text(download_id.to_string())],
    )
    .await?
    .map(|row| {
        Ok(ObservationState {
            first_observed_at: optional_timestamp_from_row(&row, "first_observed_at")?,
            last_observed_at: optional_timestamp_from_row(&row, "last_observed_at")?,
        })
    })
    .transpose()
}

async fn ambiguous_submission_candidates_tx(
    tx: &mut SqlTx<'_>,
    observation: &ObservedClientJob,
) -> AppResult<Vec<DownloadId>> {
    let Some(observed_name) = normalized_observed_name(observation.observed_name.as_deref()) else {
        return Ok(Vec::new());
    };
    let rows = SqlRuntime::fetch_all(
        SqlExec::Tx(tx),
        "SELECT b.download_id
         FROM download_client_bindings b
         JOIN download_submissions s ON s.id = b.download_id
         WHERE b.ended_at IS NULL
           AND b.native_item_id IS NULL
           AND COALESCE(b.client_config_id, '') = {}
           AND LOWER(TRIM(COALESCE(b.client_type_snapshot, ''))) = {}
           AND s.download_client_item_id IS NULL
           AND LOWER(TRIM(COALESCE(s.source_title, ''))) = {}
         ORDER BY b.created_at, b.download_id
         LIMIT 2",
        &[
            SqlArg::Text(observation.locator.client_id.clone().unwrap_or_default()),
            SqlArg::Text(observation.locator.client_type.clone()),
            SqlArg::Text(observed_name),
        ],
    )
    .await?;
    rows.into_iter()
        .map(|row| download_id_from_column(&row, "download_id"))
        .collect()
}

async fn attach_unbound_binding_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
    observation: &ObservedClientJob,
) -> AppResult<()> {
    // Callers gate this on the binding's configured client either matching the
    // observation or being blank, so the fills below can only ever complete a
    // legacy binding that never captured its client identity.
    let client_name_snapshot = observed_client_name_snapshot_tx(tx, &observation.locator).await?;
    let rows_affected = SqlRuntime::execute(
        SqlExec::Tx(tx),
        "UPDATE download_client_bindings
         SET client_config_id = CASE
                 WHEN TRIM(COALESCE(client_config_id, '')) = '' THEN {}
                 ELSE client_config_id
             END,
             client_type_snapshot = CASE
                 WHEN TRIM(COALESCE(client_type_snapshot, '')) = '' THEN {}
                 ELSE client_type_snapshot
             END,
             client_name_snapshot = CASE
                 WHEN TRIM(COALESCE(client_name_snapshot, '')) = '' THEN {}
                 ELSE client_name_snapshot
             END,
             native_item_id = {}
         WHERE download_id = {}
           AND native_item_id IS NULL
           AND ended_at IS NULL",
        &[
            SqlArg::OptText(observation.locator.client_id.clone()),
            SqlArg::Text(observation.locator.client_type.clone()),
            SqlArg::OptText(client_name_snapshot),
            SqlArg::Text(observation.locator.item_id.clone()),
            SqlArg::Text(download_id.to_string()),
        ],
    )
    .await?;
    if rows_affected != 1 {
        return Err(AppError::Repository(format!(
            "canonical download binding {download_id} was no longer an active unbound binding"
        )));
    }

    // The canonical binding is authoritative, but compatibility readers still
    // select the locator columns from download_submissions. Fill them in the
    // same transaction so an ambiguity that later reconciles immediately
    // participates in normal title/signature conflict checks.
    SqlRuntime::execute(
        SqlExec::Tx(tx),
        "UPDATE download_submissions
            SET download_client_id = {},
                download_client_type = {},
                download_client_item_id = {}
          WHERE id = {}
            AND download_client_item_id IS NULL",
        &[
            SqlArg::Text(normalize_download_client_id(
                observation.locator.client_id.as_deref(),
            )),
            SqlArg::Text(observation.locator.client_type.clone()),
            SqlArg::Text(observation.locator.item_id.clone()),
            SqlArg::Text(download_id.to_string()),
        ],
    )
    .await?;
    Ok(())
}

async fn create_foreign_observation_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
    observation: &ObservedClientJob,
) -> AppResult<()> {
    SqlRuntime::execute(
        SqlExec::Tx(tx),
        "INSERT INTO downloads (id, origin, created_at, first_observed_at, last_observed_at)
         VALUES ({}, 'foreign_observation', {}, {}, {})",
        &[
            SqlArg::Text(download_id.to_string()),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
        ],
    )
    .await?;
    create_bound_binding_tx(tx, download_id, observation).await
}

async fn create_bound_binding_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
    observation: &ObservedClientJob,
) -> AppResult<()> {
    let client_name_snapshot = observed_client_name_snapshot_tx(tx, &observation.locator).await?;
    SqlRuntime::execute(
        SqlExec::Tx(tx),
        "INSERT INTO download_client_bindings (
            download_id, client_config_id, client_type_snapshot, client_name_snapshot,
            native_item_id, created_at, last_seen_at, ended_at
         ) VALUES ({}, {}, {}, {}, {}, {}, {}, NULL)",
        &[
            SqlArg::Text(download_id.to_string()),
            SqlArg::OptText(observation.locator.client_id.clone()),
            SqlArg::Text(observation.locator.client_type.clone()),
            SqlArg::OptText(client_name_snapshot),
            SqlArg::Text(observation.locator.item_id.clone()),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
        ],
    )
    .await?;
    Ok(())
}

/// Mirrors `client_name_snapshot_tx` in the submission store: prefer the
/// configured client's display name and fall back to the client type, so an
/// observation-created binding carries the same snapshot a submission would.
async fn observed_client_name_snapshot_tx(
    tx: &mut SqlTx<'_>,
    locator: &ClientJobLocator,
) -> AppResult<Option<String>> {
    let configured_name = match locator.client_id.as_deref() {
        Some(client_id) if !client_id.trim().is_empty() => SqlRuntime::fetch_optional(
            SqlExec::Tx(tx),
            "SELECT name FROM download_clients WHERE id = {} LIMIT 1",
            &[SqlArg::Text(client_id.to_string())],
        )
        .await?
        .map(|row| row.text("name"))
        .transpose()?,
        _ => None,
    };
    Ok(configured_name
        .or_else(|| (!locator.client_type.trim().is_empty()).then(|| locator.client_type.clone())))
}

async fn touch_observation_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
    observation: &ObservedClientJob,
) -> AppResult<()> {
    SqlRuntime::execute(
        SqlExec::Tx(tx),
        "UPDATE downloads
         SET first_observed_at = COALESCE(first_observed_at, {}),
             last_observed_at = CASE
                 WHEN last_observed_at IS NULL OR last_observed_at < {} THEN {}
                 ELSE last_observed_at
             END
         WHERE id = {}",
        &[
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Text(download_id.to_string()),
        ],
    )
    .await?;
    SqlRuntime::execute(
        SqlExec::Tx(tx),
        "UPDATE download_client_bindings
         SET last_seen_at = CASE
                 WHEN last_seen_at IS NULL OR last_seen_at < {} THEN {}
                 ELSE last_seen_at
             END
         WHERE download_id = {}",
        &[
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Timestamp(observation.observed_at),
            SqlArg::Text(download_id.to_string()),
        ],
    )
    .await?;
    Ok(())
}

async fn touch_observation_if_stale_tx(
    tx: &mut SqlTx<'_>,
    download_id: DownloadId,
    state: &ObservationState,
    binding: &DownloadClientBindingRecord,
    observation: &ObservedClientJob,
) -> AppResult<()> {
    if observation_timestamp_write_required(state, binding, observation.observed_at) {
        touch_observation_tx(tx, download_id, observation).await?;
    }
    Ok(())
}

fn observation_timestamp_write_required(
    state: &ObservationState,
    binding: &DownloadClientBindingRecord,
    observed_at: DateTime<Utc>,
) -> bool {
    state.first_observed_at.is_none()
        || state.last_observed_at.is_none()
        || binding.last_seen_at.is_none_or(|last_seen_at| {
            observed_at.signed_duration_since(last_seen_at) > Duration::seconds(60)
        })
}

fn normalized_observed_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn binding_matches_locator(
    binding: &DownloadClientBindingRecord,
    locator: &ClientJobLocator,
) -> bool {
    binding.ended_at.is_none() && binding_locator_fields_match(binding, locator)
}

fn binding_locator_fields_match(
    binding: &DownloadClientBindingRecord,
    locator: &ClientJobLocator,
) -> bool {
    binding.client_config_id.as_deref().map(str::trim) == locator.client_id.as_deref()
        && binding
            .client_type_snapshot
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
            == Some(locator.client_type.as_str())
        && binding.native_item_id.as_deref().map(str::trim) == Some(locator.item_id.as_str())
}

/// Whether an active, still-unbound binding may adopt this observation.
///
/// Configured-client identity is compared the same way `binding_matches_locator`
/// compares it: trimmed and case-sensitive for the config id, trimmed and
/// case-insensitive for the client type. A blank (NULL or empty) column is
/// treated as "never captured" — legacy bindings predate client capture, so they
/// accept the observation and adopt the observed client instead of blocking.
fn unbound_binding_accepts_locator(
    binding: &DownloadClientBindingRecord,
    locator: &ClientJobLocator,
) -> bool {
    let config_matches = match blank_as_absent(binding.client_config_id.as_deref()) {
        None => true,
        Some(config_id) => Some(config_id) == blank_as_absent(locator.client_id.as_deref()),
    };
    let type_matches = match blank_as_absent(binding.client_type_snapshot.as_deref()) {
        None => true,
        Some(client_type) => client_type.eq_ignore_ascii_case(locator.client_type.trim()),
    };
    config_matches && type_matches
}

fn blank_as_absent(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn locator_conflict(
    token_id: DownloadId,
    binding_download_id: DownloadId,
) -> ObservationResolution {
    ObservationResolution::Conflict {
        token_id,
        binding_download_id,
    }
}

fn download_from_row(row: SqlRow) -> AppResult<DownloadRecord> {
    let id = download_id_from_column(&row, "id")?;
    let origin = match row.text("origin")?.as_str() {
        "scryer_submission" => DownloadOrigin::ScryerSubmission,
        "foreign_observation" => DownloadOrigin::ForeignObservation,
        value => {
            return Err(AppError::Repository(format!(
                "invalid canonical download origin {value:?} for download {id}"
            )));
        }
    };
    Ok(DownloadRecord {
        id,
        origin,
        created_at: timestamp_from_row(&row, "created_at")?,
        first_observed_at: optional_timestamp_from_row(&row, "first_observed_at")?,
        last_observed_at: optional_timestamp_from_row(&row, "last_observed_at")?,
        terminal_at: optional_timestamp_from_row(&row, "terminal_at")?,
    })
}

fn binding_from_row(row: SqlRow) -> AppResult<DownloadClientBindingRecord> {
    Ok(DownloadClientBindingRecord {
        download_id: download_id_from_column(&row, "download_id")?,
        client_config_id: row.opt_text("client_config_id")?,
        client_type_snapshot: row.opt_text("client_type_snapshot")?,
        client_name_snapshot: row.opt_text("client_name_snapshot")?,
        native_item_id: row.opt_text("native_item_id")?,
        created_at: timestamp_from_row(&row, "created_at")?,
        last_seen_at: optional_timestamp_from_row(&row, "last_seen_at")?,
        ended_at: optional_timestamp_from_row(&row, "ended_at")?,
    })
}

fn download_id_from_column(row: &SqlRow, column: &str) -> AppResult<DownloadId> {
    let value = row.text(column)?;
    DownloadId::parse(&value).ok_or_else(|| {
        AppError::Repository(format!(
            "invalid canonical download id {value:?} in {column}"
        ))
    })
}

fn timestamp_from_row(row: &SqlRow, column: &str) -> AppResult<DateTime<Utc>> {
    parse_stored_timestamp(&timestamp_string(row, column)?, column)
}

fn optional_timestamp_from_row(row: &SqlRow, column: &str) -> AppResult<Option<DateTime<Utc>>> {
    opt_timestamp_string(row, column)?
        .map(|value| parse_stored_timestamp(&value, column))
        .transpose()
}

fn parse_stored_timestamp(value: &str, column: &str) -> AppResult<DateTime<Utc>> {
    // 0179's hook writes RFC3339 offsets, while legacy-copied SQLite timestamps
    // use strftime's `...Z` form; RFC3339 parsing deliberately accepts both.
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            AppError::Repository(format!(
                "invalid canonical download timestamp {value:?} in {column}: {error}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    const FIRST_ID: &str = "00000000-0000-4000-8000-000000000001";
    const SECOND_ID: &str = "00000000-0000-4000-8000-000000000002";
    const CREATED_AT: &str = "2026-08-24T12:34:56Z";

    async fn store() -> DownloadRegistryStore {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should open");
        sqlx::query(
            "CREATE TABLE downloads (
                 id TEXT PRIMARY KEY,
                 origin TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 first_observed_at TEXT,
                 last_observed_at TEXT,
                 terminal_at TEXT
             );
             CREATE TABLE download_client_bindings (
                 download_id TEXT PRIMARY KEY,
                 client_config_id TEXT,
                 client_type_snapshot TEXT,
                 client_name_snapshot TEXT,
                 native_item_id TEXT,
                 created_at TEXT NOT NULL,
                 last_seen_at TEXT,
                 ended_at TEXT
             );
             CREATE TABLE download_submissions (
                 id TEXT PRIMARY KEY,
                 download_client_id TEXT NOT NULL DEFAULT '',
                 download_client_type TEXT NOT NULL,
                 download_client_item_id TEXT,
                 source_title TEXT,
                 download_id TEXT
             );
             CREATE TABLE download_clients (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 client_type TEXT NOT NULL DEFAULT ''
             )",
        )
        .execute(&pool)
        .await
        .expect("canonical tables should be created");
        DownloadRegistryStore::new(StoreDatastore::Sqlite {
            pool,
            writer_gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    async fn insert_download(
        store: &DownloadRegistryStore,
        id: &str,
        origin: &str,
        first_observed_at: Option<&str>,
    ) {
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "INSERT INTO downloads (id, origin, created_at, first_observed_at)
             VALUES ({}, {}, {}, {})",
            &[
                SqlArg::Text(id.to_string()),
                SqlArg::Text(origin.to_string()),
                SqlArg::Text(CREATED_AT.to_string()),
                SqlArg::OptText(first_observed_at.map(str::to_string)),
            ],
        )
        .await
        .expect("download should insert");
    }

    async fn insert_binding(
        store: &DownloadRegistryStore,
        id: &str,
        config_id: Option<&str>,
        native_item_id: Option<&str>,
        ended_at: Option<&str>,
    ) {
        insert_binding_with_client(
            store,
            id,
            config_id,
            Some("qbittorrent"),
            Some("Primary"),
            native_item_id,
            ended_at,
        )
        .await;
    }

    async fn insert_binding_with_client(
        store: &DownloadRegistryStore,
        id: &str,
        config_id: Option<&str>,
        client_type: Option<&str>,
        client_name: Option<&str>,
        native_item_id: Option<&str>,
        ended_at: Option<&str>,
    ) {
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "INSERT INTO download_client_bindings (
                download_id, client_config_id, client_type_snapshot, client_name_snapshot,
                native_item_id, created_at, ended_at
             ) VALUES ({}, {}, {}, {}, {}, {}, {})",
            &[
                SqlArg::Text(id.to_string()),
                SqlArg::OptText(config_id.map(str::to_string)),
                SqlArg::OptText(client_type.map(str::to_string)),
                SqlArg::OptText(client_name.map(str::to_string)),
                SqlArg::OptText(native_item_id.map(str::to_string)),
                SqlArg::Text(CREATED_AT.to_string()),
                SqlArg::OptText(ended_at.map(str::to_string)),
            ],
        )
        .await
        .expect("binding should insert");
    }

    async fn insert_download_client(store: &DownloadRegistryStore, id: &str, name: &str) {
        insert_download_client_of_type(store, id, name, "qbittorrent").await;
    }

    async fn insert_download_client_of_type(
        store: &DownloadRegistryStore,
        id: &str,
        name: &str,
        client_type: &str,
    ) {
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "INSERT INTO download_clients (id, name, client_type) VALUES ({}, {}, {})",
            &[
                SqlArg::Text(id.to_string()),
                SqlArg::Text(name.to_string()),
                SqlArg::Text(client_type.to_string()),
            ],
        )
        .await
        .expect("download client should insert");
    }

    async fn attributed_client(store: &DownloadRegistryStore, download_id: &str) -> String {
        SqlRuntime::fetch_optional(
            store.datastore.read_exec(),
            "SELECT COALESCE(client_config_id, '') AS client_config_id
             FROM download_client_bindings WHERE download_id = {}",
            &[SqlArg::Text(download_id.to_string())],
        )
        .await
        .expect("the binding should read back")
        .expect("the binding row exists")
        .text("client_config_id")
        .expect("the column is present")
    }

    async fn insert_ambiguous_submission(
        store: &DownloadRegistryStore,
        id: &str,
        source_title: &str,
    ) {
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "INSERT INTO download_submissions (
                id, download_client_id, download_client_type, download_client_item_id,
                source_title, download_id
             ) VALUES ({}, 'client-1', 'qbittorrent', NULL, {}, {})",
            &[
                SqlArg::Text(id.to_string()),
                SqlArg::Text(source_title.to_string()),
                SqlArg::Text(DownloadId::parse(id).unwrap().to_wire()),
            ],
        )
        .await
        .expect("ambiguous submission should insert");
    }

    fn observation(
        native_item_id: &str,
        wire_token: Option<&str>,
        observed_name: Option<&str>,
        observed_at: &str,
    ) -> ObservedClientJob {
        ObservedClientJob {
            locator: ClientJobLocator::new(Some("client-1"), "qBittorrent", native_item_id),
            wire_token: wire_token.map(str::to_string),
            observed_name: observed_name.map(str::to_string),
            observed_at: DateTime::parse_from_rfc3339(observed_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    fn wire(id: &str) -> String {
        DownloadId::parse(id).unwrap().to_wire()
    }

    const DOWNLOAD_COLUMNS: &[&str] = &[
        "id",
        "origin",
        "created_at",
        "first_observed_at",
        "last_observed_at",
        "terminal_at",
    ];
    const BINDING_COLUMNS: &[&str] = &[
        "download_id",
        "client_config_id",
        "client_type_snapshot",
        "client_name_snapshot",
        "native_item_id",
        "created_at",
        "last_seen_at",
        "ended_at",
    ];

    /// Per-column `typeof()` plus the hex of the raw stored bytes.
    ///
    /// The decoded-struct comparisons the conflict tests already make cannot
    /// see a write that round-trips back to the same value: a timestamp
    /// rewritten in another textual form parses identically, and a column
    /// retyped (TEXT to BLOB, or to a NULL the decoder maps to `None`) can too.
    /// `hex(CAST(col AS BLOB))` is byte-exact, and the explicit `<null>` marker
    /// keeps SQL NULL distinct from an empty string.
    fn raw_column_projection(columns: &[&str]) -> String {
        columns
            .iter()
            .map(|column| {
                // `hex(NULL)` is the empty string in sqlite, which is exactly
                // what an empty TEXT column hexes to, so NULL gets its own
                // marker rather than relying on `typeof` alone.
                format!(
                    "typeof({column}) AS {column}_type, \
                     CASE WHEN {column} IS NULL THEN '<null>' \
                          ELSE hex(CAST({column} AS BLOB)) END AS {column}_bytes"
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    async fn raw_table_snapshot(
        store: &DownloadRegistryStore,
        table: &str,
        key_column: &str,
        key: &str,
        columns: &[&str],
    ) -> Vec<(String, String)> {
        let sql = format!(
            "SELECT {} FROM {table} WHERE {key_column} = {{}}",
            raw_column_projection(columns)
        );
        let Some(row) = SqlRuntime::fetch_optional(
            store.datastore.read_exec(),
            &sql,
            &[SqlArg::Text(key.to_string())],
        )
        .await
        .expect("raw snapshot should read") else {
            return vec![(format!("{table}[{key}]"), "<absent>".to_string())];
        };
        columns
            .iter()
            .flat_map(|column| {
                [
                    (
                        format!("{table}.{column}.type"),
                        row.text(&format!("{column}_type"))
                            .expect("column type should decode"),
                    ),
                    (
                        format!("{table}.{column}.bytes"),
                        row.text(&format!("{column}_bytes"))
                            .expect("column bytes should decode"),
                    ),
                ]
            })
            .collect()
    }

    /// Byte-level snapshot of one canonical identity: its `downloads` row and
    /// its `download_client_bindings` row.
    async fn raw_identity_snapshot(
        store: &DownloadRegistryStore,
        id: &str,
    ) -> Vec<(String, String)> {
        let mut snapshot = raw_table_snapshot(store, "downloads", "id", id, DOWNLOAD_COLUMNS).await;
        snapshot.extend(
            raw_table_snapshot(
                store,
                "download_client_bindings",
                "download_id",
                id,
                BINDING_COLUMNS,
            )
            .await,
        );
        snapshot
    }

    /// Guards the guard: a snapshot that could not see a write would make the
    /// conflict tests above pass vacuously.
    #[tokio::test]
    async fn the_raw_snapshot_sees_writes_that_survive_a_struct_comparison() {
        let store = store().await;
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;
        let before = raw_identity_snapshot(&store, FIRST_ID).await;
        assert_eq!(
            before.len(),
            (DOWNLOAD_COLUMNS.len() + BINDING_COLUMNS.len()) * 2,
            "every column contributes a type and a bytes entry"
        );

        // A trailing space the decoder would keep, but a `.trim()`-based
        // comparison would not.
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "UPDATE download_client_bindings
             SET client_name_snapshot = 'Primary '
             WHERE download_id = {}",
            &[SqlArg::Text(FIRST_ID.to_string())],
        )
        .await
        .expect("binding should update");
        let after_edit = raw_identity_snapshot(&store, FIRST_ID).await;
        assert_ne!(after_edit, before);

        // NULL and the empty string hex to the same bytes; the marker keeps
        // them apart.
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "UPDATE download_client_bindings
             SET client_name_snapshot = NULL
             WHERE download_id = {}",
            &[SqlArg::Text(FIRST_ID.to_string())],
        )
        .await
        .expect("binding should null out");
        let after_null = raw_identity_snapshot(&store, FIRST_ID).await;
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "UPDATE download_client_bindings
             SET client_name_snapshot = ''
             WHERE download_id = {}",
            &[SqlArg::Text(FIRST_ID.to_string())],
        )
        .await
        .expect("binding should blank out");
        assert_ne!(raw_identity_snapshot(&store, FIRST_ID).await, after_null);
    }

    #[tokio::test]
    async fn loads_canonical_rows_and_reports_absence() {
        let store = store().await;
        let first_id = DownloadId::parse(FIRST_ID).unwrap();
        assert_eq!(store.load_download(&first_id).await.unwrap(), None);
        assert_eq!(store.load_binding(&first_id).await.unwrap(), None);

        insert_download(
            &store,
            FIRST_ID,
            "scryer_submission",
            Some("2026-08-24T06:34:56-06:00"),
        )
        .await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;

        let download = store
            .load_download(&first_id)
            .await
            .unwrap()
            .expect("download should load");
        assert_eq!(download.origin, DownloadOrigin::ScryerSubmission);
        assert_eq!(download.first_observed_at.unwrap(), download.created_at);
        let binding = store
            .load_binding(&first_id)
            .await
            .unwrap()
            .expect("binding should load");
        assert_eq!(binding.client_name_snapshot.as_deref(), Some("Primary"));
        assert_eq!(binding.native_item_id.as_deref(), Some("job-1"));
    }

    #[tokio::test]
    async fn active_locator_lookup_excludes_ended_and_null_native_items() {
        let store = store().await;
        for id in [FIRST_ID, SECOND_ID, "00000000-0000-4000-8000-000000000003"] {
            insert_download(&store, id, "scryer_submission", None).await;
        }
        insert_binding(
            &store,
            "00000000-0000-4000-8000-000000000003",
            Some("client-1"),
            None,
            None,
        )
        .await;
        insert_binding(
            &store,
            SECOND_ID,
            Some("client-1"),
            Some("job-1"),
            Some(CREATED_AT),
        )
        .await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;

        let found = store
            .find_active_binding_by_locator(&ClientJobLocator::new(
                Some("client-1"),
                "qbittorrent",
                "job-1",
            ))
            .await
            .unwrap()
            .expect("active binding should load");
        assert_eq!(found.download_id, DownloadId::parse(FIRST_ID).unwrap());
    }

    #[tokio::test]
    async fn known_token_with_matching_ended_binding_skips_without_writes() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;
        store.end_binding(&id).await.unwrap();
        let before = raw_identity_snapshot(&store, FIRST_ID).await;

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    Some(&wire(FIRST_ID)),
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::BindingAlreadyEnded
        );
        assert_eq!(raw_identity_snapshot(&store, FIRST_ID).await, before);
    }

    #[tokio::test]
    async fn ending_a_binding_is_idempotent() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "foreign_observation", None).await;
        insert_binding(&store, FIRST_ID, None, Some("job-1"), None).await;

        store.end_binding(&id).await.unwrap();
        let first_end = store.load_binding(&id).await.unwrap().unwrap().ended_at;
        assert!(first_end.is_some());
        store.end_binding(&id).await.unwrap();
        assert_eq!(
            store.load_binding(&id).await.unwrap().unwrap().ended_at,
            first_end
        );
    }

    #[tokio::test]
    async fn known_token_with_matching_locator_throttles_observation_timestamp_writes() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;

        let first = observation(
            "job-1",
            Some(&wire(FIRST_ID)),
            Some("release"),
            "2026-08-24T13:00:00Z",
        );
        assert_eq!(
            store.resolve_observation(&first).await.unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: false,
            }
        );
        let first_download = store.load_download(&id).await.unwrap().unwrap();
        let first_binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(
            first_download.first_observed_at,
            Some(
                DateTime::parse_from_rfc3339("2026-08-24T13:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        assert_eq!(
            first_download.last_observed_at,
            first_download.first_observed_at
        );
        assert_eq!(first_binding.last_seen_at, first_download.first_observed_at);

        let immediate = observation(
            "job-1",
            Some(&wire(FIRST_ID)),
            Some("release"),
            "2026-08-24T13:00:01Z",
        );
        store.resolve_observation(&immediate).await.unwrap();
        let immediate_download = store.load_download(&id).await.unwrap().unwrap();
        let immediate_binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(
            immediate_download.last_observed_at,
            first_download.last_observed_at
        );
        assert_eq!(immediate_binding.last_seen_at, first_binding.last_seen_at);

        let later = observation(
            "job-1",
            Some(&wire(FIRST_ID)),
            Some("release"),
            "2026-08-24T13:01:01Z",
        );
        store.resolve_observation(&later).await.unwrap();
        let later_download = store.load_download(&id).await.unwrap().unwrap();
        let later_binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(
            later_download.first_observed_at,
            first_download.first_observed_at
        );
        assert_eq!(
            later_download.last_observed_at,
            Some(
                DateTime::parse_from_rfc3339("2026-08-24T13:01:01Z")
                    .unwrap()
                    .into()
            )
        );
        assert_eq!(later_binding.last_seen_at, later_download.last_observed_at);
    }

    #[tokio::test]
    async fn known_token_attaches_its_single_unbound_binding() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), None, None).await;

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                Some(&wire(FIRST_ID)),
                Some("different name"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        assert_eq!(
            resolution,
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: true,
            }
        );
        let binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(binding.native_item_id.as_deref(), Some("job-1"));
        assert_eq!(binding.client_type_snapshot.as_deref(), Some("qbittorrent"));
        assert_eq!(binding.client_config_id.as_deref(), Some("client-1"));
        assert_eq!(binding.client_name_snapshot.as_deref(), Some("Primary"));
    }

    #[tokio::test]
    async fn known_token_refuses_to_attach_another_clients_unbound_binding() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-2"), None, None).await;
        let before_download = store.load_download(&id).await.unwrap();
        let before_binding = store.load_binding(&id).await.unwrap();
        let before_bytes = raw_identity_snapshot(&store, FIRST_ID).await;

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                Some(&wire(FIRST_ID)),
                Some("release"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        assert_eq!(
            resolution,
            ObservationResolution::Conflict {
                token_id: id,
                binding_download_id: id,
            }
        );
        assert_eq!(store.load_download(&id).await.unwrap(), before_download);
        assert_eq!(store.load_binding(&id).await.unwrap(), before_binding);
        assert_eq!(raw_identity_snapshot(&store, FIRST_ID).await, before_bytes);
        assert_eq!(
            store
                .load_binding(&id)
                .await
                .unwrap()
                .unwrap()
                .native_item_id,
            None
        );
    }

    #[tokio::test]
    async fn known_token_refuses_to_attach_an_unbound_binding_of_another_client_type() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding_with_client(
            &store,
            FIRST_ID,
            Some("client-1"),
            Some("SABnzbd"),
            Some("Primary"),
            None,
            None,
        )
        .await;
        let before_binding = store.load_binding(&id).await.unwrap();
        let before_bytes = raw_identity_snapshot(&store, FIRST_ID).await;

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                Some(&wire(FIRST_ID)),
                Some("release"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        assert_eq!(
            resolution,
            ObservationResolution::Conflict {
                token_id: id,
                binding_download_id: id,
            }
        );
        assert_eq!(store.load_binding(&id).await.unwrap(), before_binding);
        assert_eq!(raw_identity_snapshot(&store, FIRST_ID).await, before_bytes);
    }

    #[tokio::test]
    async fn known_token_attaches_a_case_insensitively_matching_client_type() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding_with_client(
            &store,
            FIRST_ID,
            Some("client-1"),
            Some(" qBittorrent "),
            Some("Primary"),
            None,
            None,
        )
        .await;

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    Some(&wire(FIRST_ID)),
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: true,
            }
        );
        let binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(binding.native_item_id.as_deref(), Some("job-1"));
        assert_eq!(
            binding.client_type_snapshot.as_deref(),
            Some(" qBittorrent ")
        );
    }

    #[tokio::test]
    async fn known_token_lets_a_legacy_unbound_binding_adopt_the_observed_client() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding_with_client(&store, FIRST_ID, None, None, None, None, None).await;
        insert_download_client(&store, "client-1", "Primary Client").await;

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    Some(&wire(FIRST_ID)),
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: true,
            }
        );
        let binding = store.load_binding(&id).await.unwrap().unwrap();
        assert_eq!(binding.client_config_id.as_deref(), Some("client-1"));
        assert_eq!(binding.client_type_snapshot.as_deref(), Some("qbittorrent"));
        assert_eq!(
            binding.client_name_snapshot.as_deref(),
            Some("Primary Client")
        );
        assert_eq!(binding.native_item_id.as_deref(), Some("job-1"));
    }

    #[tokio::test]
    async fn foreign_observations_snapshot_the_configured_client_name() {
        let store = store().await;
        insert_download_client(&store, "client-1", "Primary Client").await;

        let ObservationResolution::Resolved { download_id, .. } = store
            .resolve_observation(&observation(
                "job-1",
                None,
                Some("release"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap()
        else {
            panic!("unbound observation should resolve");
        };

        let binding = store.load_binding(&download_id).await.unwrap().unwrap();
        assert_eq!(
            binding.client_name_snapshot.as_deref(),
            Some("Primary Client")
        );
        assert_eq!(binding.client_type_snapshot.as_deref(), Some("qbittorrent"));
    }

    #[tokio::test]
    async fn known_token_with_different_active_locator_reports_conflict_without_writes() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("other-job"), None).await;
        let before_download = store.load_download(&id).await.unwrap();
        let before_binding = store.load_binding(&id).await.unwrap();
        let before_bytes = raw_identity_snapshot(&store, FIRST_ID).await;

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                Some(&wire(FIRST_ID)),
                None,
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        assert_eq!(
            resolution,
            ObservationResolution::Conflict {
                token_id: id,
                binding_download_id: id,
            }
        );
        assert_eq!(store.load_download(&id).await.unwrap(), before_download);
        assert_eq!(store.load_binding(&id).await.unwrap(), before_binding);
        assert_eq!(raw_identity_snapshot(&store, FIRST_ID).await, before_bytes);
    }

    #[tokio::test]
    async fn unknown_valid_token_is_adopted_as_foreign_with_its_exact_id() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    Some(&wire(FIRST_ID)),
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: true,
                attached: false,
            }
        );
        assert_eq!(
            store.load_download(&id).await.unwrap().unwrap().origin,
            DownloadOrigin::ForeignObservation
        );
        assert_eq!(
            store
                .load_binding(&id)
                .await
                .unwrap()
                .unwrap()
                .native_item_id
                .as_deref(),
            Some("job-1")
        );
    }

    #[tokio::test]
    async fn malformed_wire_token_is_treated_as_absent() {
        let store = store().await;

        let ObservationResolution::Resolved {
            download_id,
            newly_foreign,
            attached,
        } = store
            .resolve_observation(&observation(
                "job-1",
                Some("SABnzbd_nzo_not_a_canonical_token"),
                Some("release"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap()
        else {
            panic!("malformed token observation should resolve");
        };

        assert!(newly_foreign);
        assert!(!attached);
        assert!(store.load_download(&download_id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn no_token_and_no_ambiguous_candidate_creates_a_foreign_download() {
        let store = store().await;

        let ObservationResolution::Resolved {
            download_id,
            newly_foreign,
            attached,
        } = store
            .resolve_observation(&observation(
                "job-1",
                None,
                Some("release"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap()
        else {
            panic!("unbound observation should resolve");
        };

        assert!(newly_foreign);
        assert!(!attached);
        assert_eq!(
            store
                .load_binding(&download_id)
                .await
                .unwrap()
                .unwrap()
                .native_item_id
                .as_deref(),
            Some("job-1")
        );
    }

    #[tokio::test]
    async fn no_token_uses_the_existing_active_locator() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    None,
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: false,
            }
        );
    }

    #[tokio::test]
    async fn no_token_attaches_exactly_one_matching_ambiguous_submission() {
        let store = store().await;
        let id = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), None, None).await;
        insert_ambiguous_submission(&store, FIRST_ID, "  Paper Lantern  ").await;

        assert_eq!(
            store
                .resolve_observation(&observation(
                    "job-1",
                    None,
                    Some("paper lantern"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap(),
            ObservationResolution::Resolved {
                download_id: id,
                newly_foreign: false,
                attached: true,
            }
        );
        assert_eq!(
            store
                .load_binding(&id)
                .await
                .unwrap()
                .unwrap()
                .native_item_id
                .as_deref(),
            Some("job-1")
        );
        let submission = SqlRuntime::fetch_optional(
            store.datastore.read_exec(),
            "SELECT download_client_id, download_client_type, download_client_item_id
               FROM download_submissions
              WHERE id = {}",
            &[SqlArg::Text(FIRST_ID.to_string())],
        )
        .await
        .expect("submission locator should load")
        .expect("ambiguous submission should remain");
        assert_eq!(
            submission.text("download_client_id").expect("client id"),
            "client-1"
        );
        assert_eq!(
            submission
                .text("download_client_type")
                .expect("client type"),
            "qbittorrent"
        );
        assert_eq!(
            submission
                .text("download_client_item_id")
                .expect("client item id"),
            "job-1"
        );
    }

    #[tokio::test]
    async fn ambiguous_name_collision_falls_through_to_a_foreign_download() {
        let store = store().await;
        for id in [FIRST_ID, SECOND_ID] {
            insert_download(&store, id, "scryer_submission", None).await;
            insert_binding(&store, id, Some("client-1"), None, None).await;
            insert_ambiguous_submission(&store, id, "Paper Lantern").await;
        }

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                None,
                Some("paper lantern"),
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        let ObservationResolution::Resolved {
            download_id,
            newly_foreign,
            attached,
        } = resolution
        else {
            panic!("ambiguous collision should create a foreign observation");
        };
        assert!(newly_foreign);
        assert!(!attached);
        assert_ne!(download_id, DownloadId::parse(FIRST_ID).unwrap());
        assert_ne!(download_id, DownloadId::parse(SECOND_ID).unwrap());
        assert_eq!(
            store
                .load_binding(&DownloadId::parse(FIRST_ID).unwrap())
                .await
                .unwrap()
                .unwrap()
                .native_item_id,
            None
        );
    }

    #[tokio::test]
    async fn conflicting_token_and_active_locator_reports_conflict_without_writes() {
        let store = store().await;
        let first = DownloadId::parse(FIRST_ID).unwrap();
        let second = DownloadId::parse(SECOND_ID).unwrap();
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_download(&store, SECOND_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;
        insert_binding(&store, SECOND_ID, Some("client-1"), Some("other-job"), None).await;
        let before_first = store.load_download(&first).await.unwrap();
        let before_second = store.load_download(&second).await.unwrap();
        // Both identities are involved — the token's and the locator owner's —
        // so neither may be touched on the way out.
        let before_first_bytes = raw_identity_snapshot(&store, FIRST_ID).await;
        let before_second_bytes = raw_identity_snapshot(&store, SECOND_ID).await;

        let resolution = store
            .resolve_observation(&observation(
                "job-1",
                Some(&wire(SECOND_ID)),
                None,
                "2026-08-24T13:00:00Z",
            ))
            .await
            .unwrap();

        assert_eq!(
            resolution,
            ObservationResolution::Conflict {
                token_id: second,
                binding_download_id: first,
            }
        );
        assert_eq!(store.load_download(&first).await.unwrap(), before_first);
        assert_eq!(store.load_download(&second).await.unwrap(), before_second);
        assert_eq!(
            raw_identity_snapshot(&store, FIRST_ID).await,
            before_first_bytes
        );
        assert_eq!(
            raw_identity_snapshot(&store, SECOND_ID).await,
            before_second_bytes
        );
    }

    #[tokio::test]
    async fn concurrent_unseen_locator_resolutions_converge_on_one_foreign_download() {
        let store = store().await;
        let first_store = store.clone();
        let second_store = store.clone();
        let first = tokio::spawn(async move {
            first_store
                .resolve_observation(&observation(
                    "job-1",
                    None,
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ))
                .await
                .unwrap()
        });
        let second = tokio::spawn(async move {
            second_store
                .resolve_observation(&observation(
                    "job-1",
                    None,
                    Some("release"),
                    "2026-08-24T13:00:01Z",
                ))
                .await
                .unwrap()
        });

        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let ObservationResolution::Resolved {
            download_id: first_id,
            ..
        } = first
        else {
            panic!("first concurrent observation should resolve");
        };
        let second = second.unwrap();
        let ObservationResolution::Resolved {
            download_id: second_id,
            ..
        } = second
        else {
            panic!("second concurrent observation should resolve");
        };
        assert_eq!(first_id, second_id);
        assert!(store.load_download(&first_id).await.unwrap().is_some());
    }

    /// Verbatim sqlite and postgres unique-violation texts, as `repo_err`
    /// flattens them into `AppError::Repository`.
    const SQLITE_UNIQUE_VIOLATION: &str = "error returned from database: (code: 2067) UNIQUE \
         constraint failed: index 'idx_download_client_bindings_active_locator_unique'";
    const POSTGRES_UNIQUE_VIOLATION: &str = "error returned from database: duplicate key value \
         violates unique constraint \"idx_download_client_bindings_active_locator_unique\"";

    #[test]
    fn unique_violation_matcher_accepts_both_dialects_and_rejects_other_failures() {
        for message in [
            SQLITE_UNIQUE_VIOLATION,
            POSTGRES_UNIQUE_VIOLATION,
            "error returned from database: (code: 1555) UNIQUE constraint failed: downloads.id",
            "error returned from database: duplicate key value violates unique constraint \
             \"downloads_pkey\"",
        ] {
            assert!(
                is_unique_violation(&AppError::Repository(message.to_string())),
                "expected unique violation: {message}"
            );
        }

        for error in [
            AppError::Repository(
                "error returned from database: (code: 5) database is locked".to_string(),
            ),
            AppError::Repository(
                "error returned from database: insert or update on table \
                 \"download_client_bindings\" violates foreign key constraint"
                    .to_string(),
            ),
            // Only repository errors carry datastore text; a validation error
            // that happens to quote one must not trigger the recovery path.
            AppError::Validation(SQLITE_UNIQUE_VIOLATION.to_string()),
        ] {
            assert!(!is_unique_violation(&error), "unexpected match: {error:?}");
        }
    }

    #[tokio::test]
    async fn locator_conflict_recovery_converges_on_the_committed_winner() {
        let store = store().await;
        let winner = DownloadId::parse(FIRST_ID).unwrap();
        insert_download(&store, FIRST_ID, "foreign_observation", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;

        let resolution = store
            .converge_on_locator_winner(
                &observation("job-1", None, Some("release"), "2026-08-24T13:00:00Z"),
                AppError::Repository(POSTGRES_UNIQUE_VIOLATION.to_string()),
            )
            .await
            .expect("recovery should converge on the winner");

        assert_eq!(
            resolution,
            ObservationResolution::Resolved {
                download_id: winner,
                newly_foreign: false,
                attached: false,
            }
        );
        // The loser still records the sighting against the winner's identity.
        let download = store.load_download(&winner).await.unwrap().unwrap();
        assert_eq!(
            download.last_observed_at,
            Some(
                DateTime::parse_from_rfc3339("2026-08-24T13:00:00Z")
                    .unwrap()
                    .into()
            )
        );
        assert_eq!(
            store
                .load_binding(&winner)
                .await
                .unwrap()
                .unwrap()
                .last_seen_at,
            download.last_observed_at
        );
    }

    #[tokio::test]
    async fn locator_conflict_recovery_reports_a_token_mismatch_as_conflict() {
        let store = store().await;
        let winner = DownloadId::parse(FIRST_ID).unwrap();
        let token = DownloadId::parse(SECOND_ID).unwrap();
        insert_download(&store, FIRST_ID, "foreign_observation", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-1"), None).await;
        let before_download = store.load_download(&winner).await.unwrap();

        let resolution = store
            .converge_on_locator_winner(
                &observation(
                    "job-1",
                    Some(&wire(SECOND_ID)),
                    Some("release"),
                    "2026-08-24T13:00:00Z",
                ),
                AppError::Repository(POSTGRES_UNIQUE_VIOLATION.to_string()),
            )
            .await
            .expect("recovery should classify the mismatch");

        assert_eq!(
            resolution,
            ObservationResolution::Conflict {
                token_id: token,
                binding_download_id: winner,
            }
        );
        assert_eq!(store.load_download(&winner).await.unwrap(), before_download);
    }

    #[tokio::test]
    async fn locator_conflict_recovery_surfaces_the_original_error_without_a_winner() {
        let store = store().await;

        let error = store
            .converge_on_locator_winner(
                &observation("job-1", None, Some("release"), "2026-08-24T13:00:00Z"),
                AppError::Repository(POSTGRES_UNIQUE_VIOLATION.to_string()),
            )
            .await
            .expect_err("recovery must not invent an identity");

        let AppError::Repository(message) = error else {
            panic!("recovery should surface the original repository error");
        };
        assert_eq!(message, POSTGRES_UNIQUE_VIOLATION);
        // Nothing was written on the way out.
        assert_eq!(
            store
                .find_active_binding_by_locator(&ClientJobLocator::new(
                    Some("client-1"),
                    "qbittorrent",
                    "job-1",
                ))
                .await
                .unwrap(),
            None
        );
    }

    fn synthetic_download_id(index: usize) -> String {
        format!("00000000-0000-4000-8000-{index:012}")
    }

    #[tokio::test]
    async fn bulk_active_binding_lookup_skips_ended_rows_and_ignores_unknown_item_ids() {
        let store = store().await;
        insert_download(&store, FIRST_ID, "scryer_submission", None).await;
        insert_binding(&store, FIRST_ID, Some("client-1"), Some("job-active"), None).await;
        insert_download(&store, SECOND_ID, "scryer_submission", None).await;
        insert_binding(
            &store,
            SECOND_ID,
            Some("client-1"),
            Some("job-ended"),
            Some("2026-08-24T13:00:00Z"),
        )
        .await;

        let bindings = store
            .list_active_bindings_for_native_item_ids(&[
                "job-active".to_string(),
                "job-ended".to_string(),
                "job-never-existed".to_string(),
                // Blank and duplicate entries must not widen the read.
                "  ".to_string(),
                "job-active".to_string(),
            ])
            .await
            .expect("bulk binding lookup should succeed");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].download_id.to_string(), FIRST_ID);
        assert_eq!(bindings[0].native_item_id.as_deref(), Some("job-active"));
        assert!(bindings[0].ended_at.is_none());

        assert!(
            store
                .list_active_bindings_for_native_item_ids(&[])
                .await
                .expect("an empty request should not touch the database")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn bulk_active_binding_lookup_spans_the_chunk_boundary() {
        let store = store().await;
        let total = ACTIVE_BINDING_LOOKUP_CHUNK + 1;
        let mut native_item_ids = Vec::with_capacity(total);
        for index in 0..total {
            let download_id = synthetic_download_id(index + 1);
            let native_item_id = format!("job-{index}");
            insert_download(&store, &download_id, "scryer_submission", None).await;
            insert_binding(
                &store,
                &download_id,
                Some("client-1"),
                Some(&native_item_id),
                None,
            )
            .await;
            native_item_ids.push(native_item_id);
        }

        let bindings = store
            .list_active_bindings_for_native_item_ids(&native_item_ids)
            .await
            .expect("a request larger than one chunk should still resolve");

        let mut found: Vec<String> = bindings
            .into_iter()
            .filter_map(|binding| binding.native_item_id)
            .collect();
        found.sort();
        let mut expected = native_item_ids;
        expected.sort();
        assert_eq!(found, expected);
    }

    /// The reconcile listing is a rotation: a pass resumes strictly after the
    /// `(created_at, download_id)` the previous pass stopped on, so bindings the
    /// reconciler preserves cannot pin it to the same oldest prefix forever.
    #[tokio::test]
    async fn active_binding_reconcile_listing_resumes_after_its_keyset_cursor() {
        let store = store().await;
        // Two rows share a `created_at`, so only the download-id tie-break can
        // order them; the third is strictly newer.
        let tie_at = DateTime::parse_from_rfc3339(CREATED_AT)
            .expect("fixture timestamp")
            .with_timezone(&Utc);
        let newer_at = tie_at + Duration::days(1);
        let tie_one = synthetic_download_id(1);
        let tie_two = synthetic_download_id(2);
        let newer = synthetic_download_id(3);
        for (download_id, native_item_id, created_at) in [
            (&tie_one, "keyset-tie-one", tie_at),
            (&tie_two, "keyset-tie-two", tie_at),
            (&newer, "keyset-newer", newer_at),
        ] {
            insert_download(&store, download_id, "scryer_submission", None).await;
            insert_binding(
                &store,
                download_id,
                Some("client-1"),
                Some(native_item_id),
                None,
            )
            .await;
            // The fixture inserts a literal timestamp string; rewrite it the way
            // production writes one so the stored text and the bound cursor share
            // a format.
            SqlRuntime::execute(
                store.datastore.read_exec(),
                "UPDATE download_client_bindings SET created_at = {} WHERE download_id = {}",
                &[
                    SqlArg::Timestamp(created_at),
                    SqlArg::Text(download_id.to_string()),
                ],
            )
            .await
            .expect("binding timestamp should normalize");
        }

        let observed_before = Utc::now();
        let item_ids = |bindings: Vec<DownloadClientBindingRecord>| {
            bindings
                .into_iter()
                .filter_map(|binding| binding.native_item_id)
                .collect::<Vec<_>>()
        };

        let first = store
            .list_active_bindings_for_client_before(
                "client-1",
                "qBittorrent",
                observed_before,
                None,
                1,
            )
            .await
            .expect("the first page should load");
        assert_eq!(
            item_ids(first),
            vec!["keyset-tie-one".to_string()],
            "equal created_at ties break by download_id"
        );

        let cursor = (
            tie_at,
            DownloadId::parse(&tie_one).expect("fixture download id"),
        );
        let resumed = store
            .list_active_bindings_for_client_before(
                "client-1",
                "qBittorrent",
                observed_before,
                Some(cursor),
                10,
            )
            .await
            .expect("the cursor page should load");
        assert_eq!(
            item_ids(resumed),
            vec!["keyset-tie-two".to_string(), "keyset-newer".to_string()],
            "a cursor page starts strictly after the cursor, in (created_at, download_id) order"
        );

        let bounded = store
            .list_active_bindings_for_client_before(
                "client-1",
                "qBittorrent",
                observed_before,
                Some(cursor),
                1,
            )
            .await
            .expect("the bounded cursor page should load");
        assert_eq!(
            item_ids(bounded),
            vec!["keyset-tie-two".to_string()],
            "the limit still bounds a cursor page"
        );

        let exhausted = store
            .list_active_bindings_for_client_before(
                "client-1",
                "qBittorrent",
                observed_before,
                Some((
                    newer_at,
                    DownloadId::parse(&newer).expect("fixture download id"),
                )),
                10,
            )
            .await
            .expect("the exhausted page should load");
        assert!(
            exhausted.is_empty(),
            "a cursor at the newest eligible row returns nothing, which wraps the rotation"
        );
    }

    /// Migration 0179 backfilled bindings from submissions that predate
    /// per-client attribution, so those rows carry no `client_config_id` and no
    /// exact locator can reach them. A locator that names the install's single
    /// configured client of the type must resolve one; anything ambiguous must
    /// not.
    #[tokio::test]
    async fn legacy_client_less_binding_resolves_only_for_a_single_configured_client_of_its_type() {
        let store = store().await;
        let legacy = synthetic_download_id(41);
        insert_download(&store, &legacy, "scryer_submission", None).await;
        insert_binding_with_client(
            &store,
            &legacy,
            None,
            Some("sabnzbd"),
            Some("Legacy Client"),
            Some("legacy-item-1"),
            None,
        )
        .await;
        insert_download_client_of_type(&store, "client-sab", "Primary Usenet", "sabnzbd").await;
        // A client of another type must not make the row resolvable for itself.
        insert_download_client_of_type(&store, "client-torrent", "Primary Torrent", "qbittorrent")
            .await;

        let resolved = store
            .find_active_binding_by_locator(&ClientJobLocator::new(
                Some("client-sab"),
                "sabnzbd",
                "legacy-item-1",
            ))
            .await
            .expect("the legacy lookup should run");
        let resolved = resolved.expect("the single configured sabnzbd client owns the legacy row");
        assert_eq!(resolved.download_id.to_string(), legacy);
        assert!(
            resolved.client_config_id.is_none(),
            "the row itself is still unattributed: {resolved:?}"
        );

        assert!(
            store
                .find_active_binding_by_locator(&ClientJobLocator::new(
                    Some("client-torrent"),
                    "sabnzbd",
                    "legacy-item-1",
                ))
                .await
                .expect("the mismatched lookup should run")
                .is_none(),
            "a client that is not configured for the row's type may not claim it"
        );

        // A second configured sabnzbd client makes the original owner
        // ambiguous, even though only one of them could be enabled.
        insert_download_client_of_type(&store, "client-sab-2", "Secondary Usenet", "sabnzbd").await;
        assert!(
            store
                .find_active_binding_by_locator(&ClientJobLocator::new(
                    Some("client-sab"),
                    "sabnzbd",
                    "legacy-item-1",
                ))
                .await
                .expect("the ambiguous lookup should run")
                .is_none(),
            "two configured clients of the type make the attribution ambiguous"
        );
        // The client-less locator still resolves it, exactly as before.
        assert_eq!(
            store
                .find_active_binding_by_locator(&ClientJobLocator::new(
                    None,
                    "sabnzbd",
                    "legacy-item-1",
                ))
                .await
                .expect("the client-less lookup should run")
                .map(|binding| binding.download_id.to_string()),
            Some(legacy)
        );
    }

    /// The legacy arm is a fallback, never a replacement: a row that actually
    /// names the client wins.
    #[tokio::test]
    async fn an_exact_binding_wins_over_a_legacy_one_for_the_same_locator() {
        let store = store().await;
        let legacy = synthetic_download_id(42);
        let exact = synthetic_download_id(43);
        insert_download(&store, &legacy, "scryer_submission", None).await;
        insert_download(&store, &exact, "scryer_submission", None).await;
        insert_binding_with_client(
            &store,
            &legacy,
            None,
            Some("sabnzbd"),
            Some("Legacy Client"),
            Some("shared-item-1"),
            None,
        )
        .await;
        insert_binding_with_client(
            &store,
            &exact,
            Some("client-sab"),
            Some("sabnzbd"),
            Some("Primary Usenet"),
            Some("shared-item-1"),
            None,
        )
        .await;
        insert_download_client_of_type(&store, "client-sab", "Primary Usenet", "sabnzbd").await;

        assert_eq!(
            store
                .find_active_binding_by_locator(&ClientJobLocator::new(
                    Some("client-sab"),
                    "sabnzbd",
                    "shared-item-1",
                ))
                .await
                .expect("the lookup should run")
                .map(|binding| binding.download_id.to_string()),
            Some(exact),
            "an exact attribution is preferred over a legacy one"
        );
    }

    /// The legacy listing sees only unattributed rows, respects the recency
    /// floor and the limit, and resumes strictly after its keyset cursor.
    #[tokio::test]
    async fn legacy_binding_listing_is_bounded_and_resumes_after_its_cursor() {
        let store = store().await;
        let floor_at = DateTime::parse_from_rfc3339(CREATED_AT)
            .expect("fixture timestamp")
            .with_timezone(&Utc);
        let first = synthetic_download_id(44);
        let second = synthetic_download_id(45);
        let attributed = synthetic_download_id(46);
        for (download_id, client, item_id) in [
            (&first, None, "legacy-listed-1"),
            (&second, None, "legacy-listed-2"),
            (&attributed, Some("client-sab"), "legacy-listed-3"),
        ] {
            insert_download(&store, download_id, "scryer_submission", None).await;
            insert_binding_with_client(
                &store,
                download_id,
                client,
                Some("sabnzbd"),
                Some("Primary Usenet"),
                Some(item_id),
                None,
            )
            .await;
            SqlRuntime::execute(
                store.datastore.read_exec(),
                "UPDATE download_client_bindings SET created_at = {} WHERE download_id = {}",
                &[
                    SqlArg::Timestamp(floor_at),
                    SqlArg::Text(download_id.to_string()),
                ],
            )
            .await
            .expect("binding timestamp should normalize");
        }
        // A blank (not NULL) attribution is just as unusable as a NULL one.
        let blank = synthetic_download_id(47);
        insert_download(&store, &blank, "scryer_submission", None).await;
        insert_binding_with_client(
            &store,
            &blank,
            Some("  "),
            Some("sabnzbd"),
            Some("Primary Usenet"),
            Some("legacy-listed-4"),
            None,
        )
        .await;
        SqlRuntime::execute(
            store.datastore.read_exec(),
            "UPDATE download_client_bindings SET created_at = {} WHERE download_id = {}",
            &[SqlArg::Timestamp(floor_at), SqlArg::Text(blank.clone())],
        )
        .await
        .expect("binding timestamp should normalize");

        let item_ids = |bindings: Vec<DownloadClientBindingRecord>| {
            bindings
                .into_iter()
                .filter_map(|binding| binding.native_item_id)
                .collect::<Vec<_>>()
        };

        let all = store
            .list_active_legacy_client_bindings_before(Utc::now(), None, 10)
            .await
            .expect("the legacy listing should load");
        assert_eq!(
            item_ids(all),
            vec![
                "legacy-listed-1".to_string(),
                "legacy-listed-2".to_string(),
                "legacy-listed-4".to_string(),
            ],
            "only rows with no usable client id are listed"
        );

        assert!(
            store
                .list_active_legacy_client_bindings_before(
                    floor_at - Duration::seconds(1),
                    None,
                    10
                )
                .await
                .expect("the floored listing should load")
                .is_empty(),
            "the recency floor still applies"
        );

        let page = store
            .list_active_legacy_client_bindings_before(Utc::now(), None, 1)
            .await
            .expect("the bounded page should load");
        assert_eq!(item_ids(page), vec!["legacy-listed-1".to_string()]);

        let resumed = store
            .list_active_legacy_client_bindings_before(
                Utc::now(),
                Some((
                    floor_at,
                    DownloadId::parse(&first).expect("fixture download id"),
                )),
                10,
            )
            .await
            .expect("the cursor page should load");
        assert_eq!(
            item_ids(resumed),
            vec!["legacy-listed-2".to_string(), "legacy-listed-4".to_string()],
            "a cursor page starts strictly after the cursor"
        );

        assert!(
            store
                .list_active_legacy_client_bindings_before(Utc::now(), None, 0)
                .await
                .expect("a zero budget should short-circuit")
                .is_empty()
        );
    }

    /// Persisting the derived attribution fills only a blank one, and never
    /// one that would collide with the active-locator unique index.
    #[tokio::test]
    async fn attributing_a_legacy_binding_fills_only_a_blank_and_never_collides() {
        let store = store().await;
        let legacy = synthetic_download_id(48);
        let already_owned = synthetic_download_id(49);
        let contended = synthetic_download_id(50);
        let holder = synthetic_download_id(51);
        for (download_id, client, item_id) in [
            (&legacy, None, "attribute-item-1"),
            (&already_owned, Some("client-other"), "attribute-item-2"),
            (&contended, None, "attribute-item-3"),
            (&holder, Some("client-sab"), "attribute-item-3"),
        ] {
            insert_download(&store, download_id, "scryer_submission", None).await;
            insert_binding_with_client(
                &store,
                download_id,
                client,
                Some("sabnzbd"),
                Some("Primary Usenet"),
                Some(item_id),
                None,
            )
            .await;
        }

        store
            .attribute_legacy_binding_client(
                &DownloadId::parse(&legacy).expect("fixture download id"),
                "client-sab",
            )
            .await
            .expect("attribution should apply");
        assert_eq!(attributed_client(&store, &legacy).await, "client-sab");

        store
            .attribute_legacy_binding_client(
                &DownloadId::parse(&already_owned).expect("fixture download id"),
                "client-sab",
            )
            .await
            .expect("attribution should run");
        assert_eq!(
            attributed_client(&store, &already_owned).await,
            "client-other",
            "a binding that already names a client is never re-pointed"
        );

        store
            .attribute_legacy_binding_client(
                &DownloadId::parse(&contended).expect("fixture download id"),
                "client-sab",
            )
            .await
            .expect("attribution should run");
        assert_eq!(
            attributed_client(&store, &contended).await,
            "",
            "attribution that would duplicate a live locator is skipped, not forced"
        );
    }

    /// Mirrors the canonical postgres DDL from migrations 0179/0180 — including
    /// the active-locator partial unique index that concurrent first
    /// observations race on.
    const POSTGRES_CANONICAL_SCHEMA: &[&str] = &[
        "CREATE TABLE downloads (
             id TEXT PRIMARY KEY,
             origin TEXT NOT NULL CHECK (origin IN ('scryer_submission', 'foreign_observation')),
             created_at TIMESTAMP WITH TIME ZONE NOT NULL,
             first_observed_at TIMESTAMP WITH TIME ZONE,
             last_observed_at TIMESTAMP WITH TIME ZONE,
             terminal_at TIMESTAMP WITH TIME ZONE
         )",
        "CREATE TABLE download_client_bindings (
             download_id TEXT PRIMARY KEY REFERENCES downloads(id),
             client_config_id TEXT,
             client_type_snapshot TEXT,
             client_name_snapshot TEXT,
             native_item_id TEXT,
             created_at TIMESTAMP WITH TIME ZONE NOT NULL,
             last_seen_at TIMESTAMP WITH TIME ZONE,
             ended_at TIMESTAMP WITH TIME ZONE
         )",
        "CREATE UNIQUE INDEX idx_download_client_bindings_active_locator_unique
             ON download_client_bindings(client_config_id, client_type_snapshot, native_item_id)
             WHERE native_item_id IS NOT NULL
               AND ended_at IS NULL",
        "CREATE TABLE download_submissions (
             id TEXT PRIMARY KEY,
             download_client_id TEXT NOT NULL DEFAULT '',
             download_client_type TEXT NOT NULL,
             download_client_item_id TEXT,
             source_title TEXT,
             download_id TEXT
         )",
        "CREATE TABLE download_clients (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             client_type TEXT NOT NULL DEFAULT ''
         )",
    ];

    const POSTGRES_CONCURRENT_OBSERVERS: usize = 6;

    /// Live-postgres counterpart to
    /// `concurrent_unseen_locator_resolutions_converge_on_one_foreign_download`.
    /// Sqlite serializes writers behind the store's writer gate, so only
    /// postgres can actually lose the active-locator race; this test is skipped
    /// unless `SCRYER_TEST_POSTGRES_URL` names a reachable server.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn postgres_concurrent_first_observations_converge_on_one_download() -> AppResult<()> {
        let Some(raw_url) = std::env::var("SCRYER_TEST_POSTGRES_URL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            eprintln!(
                "skipping PostgreSQL observation convergence test; SCRYER_TEST_POSTGRES_URL is not set"
            );
            return Ok(());
        };

        let admin_pool = sqlx::PgPool::connect(&raw_url).await.map_err(|error| {
            AppError::Repository(format!("failed to connect to postgres: {error}"))
        })?;
        let schema = format!(
            "scryer_test_{}_{}",
            std::process::id(),
            scryer_domain::Id::new().0.replace('-', "_")
        );
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
            .execute(&admin_pool)
            .await
            .map_err(|error| {
                AppError::Repository(format!("failed to create test schema: {error}"))
            })?;

        let result = postgres_convergence_case(&raw_url, &schema).await;

        sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
            .execute(&admin_pool)
            .await
            .map_err(|error| {
                AppError::Repository(format!("failed to drop test schema {schema}: {error}"))
            })?;
        result
    }

    async fn postgres_convergence_case(raw_url: &str, schema: &str) -> AppResult<()> {
        // Same shape `url::Url::append_pair` produces for the datastore crate's
        // PostgreSQL tests, without pulling `url` into this crate.
        let separator = if raw_url.contains('?') { '&' } else { '?' };
        let schema_url = format!("{raw_url}{separator}options=-csearch_path%3D{schema}");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(POSTGRES_CONCURRENT_OBSERVERS as u32 + 2)
            .connect(&schema_url)
            .await
            .map_err(|error| {
                AppError::Repository(format!("failed to connect with search_path: {error}"))
            })?;

        for statement in POSTGRES_CANONICAL_SCHEMA {
            sqlx::query(sqlx::AssertSqlSafe(*statement))
                .execute(&pool)
                .await
                .map_err(|error| {
                    AppError::Repository(format!("failed to create canonical schema: {error}"))
                })?;
        }

        let store = DownloadRegistryStore::new(StoreDatastore::Postgres { pool: pool.clone() });
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..POSTGRES_CONCURRENT_OBSERVERS {
            let store = store.clone();
            tasks.spawn(async move {
                store
                    .resolve_observation(&observation(
                        "job-1",
                        None,
                        Some("release"),
                        "2026-08-24T13:00:00Z",
                    ))
                    .await
            });
        }

        let mut resolved: Vec<DownloadId> = Vec::new();
        let mut newly_foreign_count = 0usize;
        while let Some(joined) = tasks.join_next().await {
            let resolution = joined
                .map_err(|error| AppError::Repository(error.to_string()))?
                .map_err(|error| {
                    AppError::Repository(format!("concurrent observation failed: {error}"))
                })?;
            let ObservationResolution::Resolved {
                download_id,
                newly_foreign,
                ..
            } = resolution
            else {
                return Err(AppError::Repository(format!(
                    "concurrent observation should resolve, got {resolution:?}"
                )));
            };
            if newly_foreign {
                newly_foreign_count += 1;
            }
            resolved.push(download_id);
        }

        let winner = resolved[0];
        assert!(
            resolved.iter().all(|id| *id == winner),
            "concurrent observations diverged: {resolved:?}"
        );
        assert_eq!(newly_foreign_count, 1, "exactly one writer may adopt");
        assert_eq!(
            store.load_download(&winner).await?.map(|row| row.origin),
            Some(DownloadOrigin::ForeignObservation)
        );

        let downloads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads")
            .fetch_one(&pool)
            .await
            .map_err(|error| AppError::Repository(error.to_string()))?;
        let bindings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM download_client_bindings")
            .fetch_one(&pool)
            .await
            .map_err(|error| AppError::Repository(error.to_string()))?;
        assert_eq!(downloads, 1, "losing transactions must roll back fully");
        assert_eq!(bindings, 1);
        Ok(())
    }
}
