use super::*;

use crate::DownloadLifecycleDeferral;
use crate::acquisition_decision_helpers::is_download_submit_unavailable_error;
use crate::catalog::workflow::queue_item_matches_submission;
use crate::download_identity::{
    AcceptedDownloadIdentityInput, accepted_download_submission_identity,
};
use crate::services::{CanonicalSubmissionTitleState, UncertainDownloadSubmissionClaim};

/// A short, operator-readable name for the scope an acquisition asked for.
/// Used only to describe a refusal; nothing routes on it.
fn describe_submission_scope(scope: &SubmissionScope) -> String {
    match scope {
        SubmissionScope::Episode { episode_id } => format!("episode {episode_id}"),
        SubmissionScope::EpisodeSet { episode_ids } => {
            format!("episode set of {}", episode_ids.len())
        }
        SubmissionScope::SeriesMovie {
            series_movie_link_id,
        } => format!("series movie {series_movie_link_id}"),
        SubmissionScope::Collection { collection_id } => format!("collection {collection_id}"),
        SubmissionScope::Title => "title".to_string(),
        SubmissionScope::Orphan => "orphan".to_string(),
    }
}

#[derive(Clone)]
pub(crate) struct CanonicalDownloadSubmissionIntent {
    pub request: DownloadClientAddRequest,
    pub scope: SubmissionScope,
    pub conflict_policy: SubmissionConflictPolicy,
    pub request_signature: Option<String>,
    pub source_provider_name: Option<String>,
    pub release_size_bytes: Option<i64>,
}

pub(crate) enum CanonicalDownloadSubmissionOutcome {
    Accepted(CanonicalDownloadSubmission),
    Conflict(SubmissionScopeConflict),
}

pub(crate) struct CanonicalDownloadSubmission {
    pub grab: DownloadGrabResult,
    pub newly_submitted: bool,
}

fn accepted_existing(submission: DownloadSubmission) -> CanonicalDownloadSubmissionOutcome {
    let grab = DownloadGrabResult {
        job_id: submission.download_client_item_id.clone(),
        client_id: submission.download_client_id.clone(),
        client_type: submission.download_client_type.clone(),
        info_hash: None,
        download_id: Some(submission.download_id),
        seed_goals: None,
    };
    CanonicalDownloadSubmissionOutcome::Accepted(CanonicalDownloadSubmission {
        grab,
        newly_submitted: false,
    })
}

fn submission_for_grab(
    intent: &CanonicalDownloadSubmissionIntent,
    request: &DownloadClientAddRequest,
    download_id: scryer_domain::download_identity::DownloadId,
    grab: &DownloadGrabResult,
) -> DownloadSubmission {
    DownloadSubmission {
        download_id,
        title_id: request.title.id.clone(),
        facet: request.title.facet.as_str().to_string(),
        download_client_id: grab.client_id.clone(),
        download_client_type: grab.client_type.clone(),
        download_client_item_id: grab.job_id.clone(),
        source_hint: normalize_release_attempt_hint(intent.request.source_hint.as_deref()),
        source_provider_id: request.indexer_id.clone(),
        source_provider_name: intent.source_provider_name.clone(),
        source_kind: intent.request.source_kind,
        source_title: request.source_title.clone(),
        info_hash: request.info_hash_hint.clone(),
        release_size_bytes: intent.release_size_bytes,
        request_signature: intent.request_signature.clone(),
        purpose: request.purpose,
        scope: intent.scope.clone(),
    }
}

fn accepted_identity_for_grab(
    intent: &CanonicalDownloadSubmissionIntent,
    request: &DownloadClientAddRequest,
    download_id: scryer_domain::download_identity::DownloadId,
    grab: &DownloadGrabResult,
) -> DownloadSubmissionIdentity {
    let download_id_wire = download_id.to_wire();
    accepted_download_submission_identity(AcceptedDownloadIdentityInput {
        initial_download_id: Some(download_id_wire.as_str()),
        source_kind: intent.request.source_kind,
        source_hint: intent.request.source_hint.as_deref(),
        info_hash_hint: request.info_hash_hint.as_deref(),
        client_type: Some(grab.client_type.as_str()),
        client_item_id: Some(grab.job_id.as_str()),
        accepted_info_hash: grab.info_hash.as_deref(),
    })
}

fn submission_client_state_is_authoritative(
    snapshot: &DownloadClientSnapshotOutcome,
    submission: &DownloadSubmission,
) -> bool {
    submission
        .download_client_id
        .as_deref()
        .map(str::trim)
        .filter(|client_id| !client_id.is_empty())
        .is_some_and(|client_id| snapshot.authoritative_client_ids.contains(client_id))
}

/// Whether a bulk-read binding is the one `find_active_binding_by_locator`
/// would resolve for `locator`.
///
/// Mirrors that query's predicate exactly — non-ended, a present native item
/// id, and the same normalized client id / client type / item id — so the
/// bulk pre-filter never skips a row the authoritative per-locator lookup
/// would have found.
///
/// `legacy_attributed` says `locator` is a client-less submission's locator
/// that was resolved to the single configured client of its type. That is
/// precisely the case in which the per-locator lookup also accepts a binding
/// row whose own `client_config_id` is still blank, so the pre-filter accepts
/// one too; otherwise the client id must match exactly.
fn active_binding_matches_locator(
    binding: &DownloadClientBindingRecord,
    locator: &ClientJobLocator,
    legacy_attributed: bool,
) -> bool {
    let binding_client_id = binding.client_config_id.as_deref().unwrap_or_default();
    let binding_client_type = binding
        .client_type_snapshot
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let client_matches = binding_client_id == locator.client_id.as_deref().unwrap_or_default()
        || (legacy_attributed && binding_client_id.trim().is_empty());
    binding.ended_at.is_none()
        && binding.native_item_id.as_deref() == Some(locator.item_id.as_str())
        && client_matches
        && binding_client_type == locator.client_type
}

fn submission_matches_intent(
    submission: &DownloadSubmission,
    intent: &CanonicalDownloadSubmissionIntent,
) -> bool {
    intent.request_signature.is_some()
        && submission.request_signature == intent.request_signature
        && submission.purpose == intent.request.purpose
        && submission.scope == intent.scope
}

/// Whether an accepted grab that landed on a job the tracker only observed may
/// take that job over.
///
/// A client that already holds the release reports the job it has instead of a
/// new one, so the grab resolves to the job's foreign canonical identity, whose
/// only submission row is the tracker's title-less observation stub. That stub
/// belongs to no title, so it is not another title's grab: the grab claims it,
/// and the store then records the job as a Scryer submission under the
/// identity it already had.
enum ObservationStubAdoption {
    /// This call is the grab's own client mutation; claim the stub and freeze
    /// the grab's seed goals on it.
    ClaimForGrab(Option<crate::PersistedSeedGoals>),
    /// Leave a stub owner rejected like any other title mismatch.
    Refuse,
}

impl AppUseCase {
    /// Schedule bounded lifecycle reconciliation for a job the guard just
    /// proved absent while its binding was still active.
    ///
    /// Three ways this declines, all silent to the acquisition:
    /// no tracked-download loop is running (tests, partial assemblies), a
    /// reconciliation for this job was already scheduled inside the dedupe
    /// window, or the command channel is full/closed. The periodic fallback
    /// pass still covers every one of them.
    fn schedule_absent_source_reconciliation(
        &self,
        locator: &ClientJobLocator,
        download_id: scryer_domain::download_identity::DownloadId,
        title_id: &str,
    ) {
        let Some(handle) = self.runtime.acquisition.tracked_download_handle.as_ref() else {
            tracing::debug!(
                download_id = %download_id,
                "no tracked download loop; leaving the absent binding to the periodic reconciliation pass"
            );
            return;
        };
        let schedule_key = locator.dedupe_key();
        if !self
            .runtime
            .acquisition
            .download_submission_guards
            .claim_absent_source_reconcile_schedule(&schedule_key)
        {
            tracing::debug!(
                download_id = %download_id,
                client_type = %locator.client_type,
                native_item_id = %locator.item_id,
                "lifecycle reconciliation for this job is already scheduled"
            );
            return;
        }
        if handle.try_reconcile_absent_source(
            locator.clone(),
            download_id,
            Some(title_id.to_string()),
        ) {
            tracing::debug!(
                title_id = %title_id,
                download_id = %download_id,
                client_type = %locator.client_type,
                native_item_id = %locator.item_id,
                "scheduled lifecycle reconciliation for an absent download binding"
            );
        } else {
            self.runtime
                .acquisition
                .download_submission_guards
                .release_absent_source_reconcile_schedule(&schedule_key);
            tracing::debug!(
                download_id = %download_id,
                "tracked download loop is not accepting commands; leaving the absent binding to the periodic reconciliation pass"
            );
        }
    }

    async fn adopt_canonical_download(
        &self,
        intent: &CanonicalDownloadSubmissionIntent,
        request: &DownloadClientAddRequest,
        effective_download_id: scryer_domain::download_identity::DownloadId,
        adopted_grab: DownloadGrabResult,
        stub_adoption: ObservationStubAdoption,
    ) -> AppResult<CanonicalDownloadSubmissionOutcome> {
        let title_id = request.title.id.as_str();
        let Some(existing) = self
            .services
            .workflow
            .download_submissions
            .find_by_canonical_download_id(&effective_download_id)
            .await?
        else {
            return Err(AppError::DownloadSubmitRejected(format!(
                "download client reused canonical identity {effective_download_id}, but its submission could not be loaded"
            )));
        };
        let claimed_stub_seed_goals = match stub_adoption {
            ObservationStubAdoption::ClaimForGrab(seed_goals) if existing.is_observation_stub() => {
                Some(seed_goals)
            }
            _ => None,
        };
        let claims_observation_stub = claimed_stub_seed_goals.is_some();
        if !claims_observation_stub && existing.title_id != title_id {
            return Err(AppError::DownloadSubmitRejected(format!(
                "download client reused canonical identity {effective_download_id} owned by title {}, not {title_id}",
                existing.title_id
            )));
        }

        let accepted_identity =
            accepted_identity_for_grab(intent, request, effective_download_id, &adopted_grab);
        if existing.request_signature.is_none()
            && existing.source_hint.is_none()
            && existing.source_title.is_none()
        {
            let submission =
                submission_for_grab(intent, request, effective_download_id, &adopted_grab);
            let seed_goals = claimed_stub_seed_goals.flatten();
            let disposition = match self
                .services
                .workflow
                .download_submissions
                .record_submission_with_identity(
                    submission.clone(),
                    accepted_identity.clone(),
                    seed_goals.clone(),
                )
                .await
            {
                Ok(disposition) => disposition,
                Err(error) => {
                    self.runtime
                        .acquisition
                        .download_submission_guards
                        .mark_uncertain(
                            title_id,
                            UncertainDownloadSubmissionClaim::accepted(
                                submission,
                                accepted_identity,
                                seed_goals,
                            ),
                        );
                    return Err(AppError::DownloadSubmitAmbiguous(format!(
                        "adopted download submission {effective_download_id} could not be made durable for title {title_id}: {error}"
                    )));
                }
            };
            if let CanonicalDownloadIdentityDisposition::AdoptedExisting { download_id } =
                disposition
                && download_id != effective_download_id
            {
                let Some(rebound) = self
                    .services
                    .workflow
                    .download_submissions
                    .find_by_canonical_download_id(&download_id)
                    .await?
                else {
                    return Err(AppError::DownloadSubmitRejected(format!(
                        "download client reused canonical identity {download_id}, but its submission could not be loaded"
                    )));
                };
                if rebound.title_id != title_id {
                    return Err(AppError::DownloadSubmitRejected(format!(
                        "download client reused canonical identity {download_id} owned by title {}, not {title_id}",
                        rebound.title_id
                    )));
                }
                return Ok(accepted_existing(rebound));
            }
        }
        Ok(CanonicalDownloadSubmissionOutcome::Accepted(
            CanonicalDownloadSubmission {
                grab: adopted_grab,
                // Claiming an observed job is this grab's submission, not a
                // reuse of an earlier Scryer one.
                newly_submitted: claims_observation_stub,
            },
        ))
    }

    pub(crate) async fn submit_canonical_download(
        &self,
        intent: CanonicalDownloadSubmissionIntent,
    ) -> AppResult<CanonicalDownloadSubmissionOutcome> {
        let title_id = intent.request.title.id.clone();
        let _title_guard = self
            .runtime
            .acquisition
            .download_submission_guards
            .acquire_title(&title_id)
            .await;

        let _location_guard = self
            .acquire_location_title_mutation(
                &crate::location::ownership_guard::TITLE_DOWNLOAD_ENTRY,
                &title_id,
            )
            .await?;

        if let Some(claim) = self
            .runtime
            .acquisition
            .download_submission_guards
            .uncertain_claim(&title_id)
        {
            match claim {
                UncertainDownloadSubmissionClaim::Ambiguous {
                    download_id,
                    submission,
                } => {
                    if let Some(submission) = submission.as_ref()
                        && self
                            .services
                            .workflow
                            .download_submissions
                            .record_ambiguous_submission(submission.clone())
                            .await
                            .is_ok()
                    {
                        self.runtime
                            .acquisition
                            .download_submission_guards
                            .clear_uncertain(&title_id);
                    }
                    return Err(AppError::DownloadSubmitAmbiguous(format!(
                        "download submission {download_id} is still uncertain for title {title_id}"
                    )));
                }
                UncertainDownloadSubmissionClaim::Accepted {
                    submission,
                    accepted_identity,
                    seed_goals,
                } => {
                    let disposition = match self
                        .services
                        .workflow
                        .download_submissions
                        .record_submission_with_identity(
                            submission.clone(),
                            accepted_identity.clone(),
                            seed_goals.clone(),
                        )
                        .await
                    {
                        Ok(disposition) => disposition,
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                title_id = %title_id,
                                download_id = %submission.download_id,
                                "accepted download submission is still not durable"
                            );
                            return Err(AppError::DownloadSubmitAmbiguous(format!(
                                "download submission {} is still uncertain for title {title_id}",
                                submission.download_id
                            )));
                        }
                    };
                    self.runtime
                        .acquisition
                        .download_submission_guards
                        .clear_uncertain(&title_id);
                    match disposition {
                        CanonicalDownloadIdentityDisposition::Requested => {
                            if submission_matches_intent(&submission, &intent) {
                                return Ok(accepted_existing(submission));
                            }
                        }
                        CanonicalDownloadIdentityDisposition::AdoptedExisting { download_id } => {
                            let adopted_grab = DownloadGrabResult {
                                job_id: submission.download_client_item_id,
                                client_id: submission.download_client_id,
                                client_type: submission.download_client_type,
                                info_hash: seed_goals.and_then(|goals| goals.info_hash),
                                download_id: Some(download_id),
                                seed_goals: None,
                            };
                            // The recovered claim is an earlier grab, possibly of
                            // a different release than this intent, so the intent
                            // cannot stand in for it on an observed job.
                            return self
                                .adopt_canonical_download(
                                    &intent,
                                    &intent.request,
                                    download_id,
                                    adopted_grab,
                                    ObservationStubAdoption::Refuse,
                                )
                                .await;
                        }
                    }
                }
            }
        }

        let mut state = if let Some(state) = self
            .runtime
            .acquisition
            .download_submission_guards
            .cached_title_state(&title_id)
        {
            state
        } else {
            if !self
                .services
                .workflow
                .download_submissions
                .list_active_unbound_for_title(&title_id)
                .await?
                .is_empty()
            {
                return Err(AppError::DownloadSubmitAmbiguous(format!(
                    "download submission acceptance is unresolved for title {title_id}"
                )));
            }
            let submissions = self
                .services
                .workflow
                .download_submissions
                .list_for_title(&title_id)
                .await?;
            let episodes = self
                .services
                .catalog
                .shows
                .list_episodes_for_title(&title_id)
                .await?;
            let state = CanonicalSubmissionTitleState::new(submissions, episodes);
            self.runtime
                .acquisition
                .download_submission_guards
                .store_title_state(&title_id, state.clone());
            state
        };
        let existing = intent
            .request_signature
            .as_deref()
            .and_then(|signature| {
                state.submissions.iter().find(|submission| {
                    submission.request_signature.as_deref() == Some(signature)
                        && submission.purpose == intent.request.purpose
                        && submission.scope == intent.scope
                })
            })
            .cloned();
        let mut snapshot = if state.submissions.is_empty() {
            None
        } else {
            let guards = &self.runtime.acquisition.download_submission_guards;
            let snapshot_is_authoritative = |snapshot: &DownloadClientSnapshotOutcome| {
                state.submissions.iter().all(|submission| {
                    state
                        .accepted_download_ids
                        .contains(&submission.download_id)
                        || submission_client_state_is_authoritative(snapshot, submission)
                })
            };
            let cached = guards
                .cached_client_snapshot()
                .filter(snapshot_is_authoritative);
            if let Some(snapshot) = cached {
                Some(snapshot)
            } else {
                let _snapshot_guard = guards.acquire_client_snapshot().await;
                let snapshot = if let Some(snapshot) = guards
                    .cached_client_snapshot()
                    .filter(snapshot_is_authoritative)
                {
                    snapshot
                } else {
                    let snapshot = self
                        .services
                        .integrations
                        .download_client
                        .list_snapshot_outcome_excluding_client_types(100, &[])
                        .await
                        .map_err(|error| AppError::DownloadSubmitUnavailable(error.to_string()))?;
                    guards.store_client_snapshot(snapshot.clone());
                    snapshot
                };
                Some(snapshot)
            }
        };

        // A cached positive sighting can protect a claim, but absence from
        // queue + recent history cannot release one. Recheck the exact job
        // before either the same-request retry or conflict admission forgets it.
        // Every client-less submission the guard pass below could attribute.
        // The conflict pass reads it for its authority check: a submission that
        // names no client can never be covered by a client's snapshot authority
        // on its own, so without this an upgraded install refuses every
        // overlapping acquisition outright.
        let mut legacy_client_ids: std::collections::HashMap<
            scryer_domain::download_identity::DownloadId,
            String,
        > = std::collections::HashMap::new();
        if let Some(snapshot) = snapshot.as_mut() {
            let guard_pass_started = std::time::Instant::now();
            let overlapping: Vec<(&DownloadSubmission, ClientJobLocator)> = state
                .submissions
                .iter()
                .filter(|submission| {
                    crate::catalog_workflow::submission_scopes_overlap(
                        &title_id,
                        &submission.scope,
                        &intent.scope,
                        &state.episodes,
                    ) && !state
                        .accepted_download_ids
                        .contains(&submission.download_id)
                })
                .map(|submission| (submission, ClientJobLocator::from_submission(submission)))
                .collect();
            // Only a row whose client binding is still active can reach the
            // deferral below, so the whole title's bindings are resolved in one
            // read and the live client round-trip is spent only on those rows.
            let active_bindings = if overlapping.is_empty() {
                Vec::new()
            } else {
                let native_item_ids: Vec<String> = overlapping
                    .iter()
                    .map(|(_, locator)| locator.item_id.clone())
                    .collect();
                self.services
                    .workflow
                    .download_registry
                    .list_active_bindings_for_native_item_ids(&native_item_ids)
                    .await
                    .map_err(|error| AppError::DownloadSubmitUnavailable(error.to_string()))?
            };
            let overlapping_count = overlapping.len();
            let mut skipped_no_binding = 0usize;
            let mut skipped_settled = 0usize;
            let mut observed = 0usize;
            // Read once per pass, and only if some overlapping submission
            // predates per-client attribution.
            let mut client_configs: Option<Vec<scryer_domain::DownloadClientConfig>> = None;
            for (submission, locator) in overlapping {
                // A submission written before per-client attribution (migration
                // 0179's population) carries no client id, and nothing
                // downstream — the router, the reconciler — can act on such a
                // locator. Attribute it to the single configured client of its
                // type when there is exactly one, so this scope is reconcilable
                // instead of deferred forever.
                let legacy_client_id = if locator.client_id.is_none() {
                    if client_configs.is_none() {
                        client_configs = Some(
                            self.services
                                .integrations
                                .download_client_configs
                                .list(None)
                                .await
                                .map_err(|error| {
                                    AppError::DownloadSubmitUnavailable(error.to_string())
                                })?,
                        );
                    }
                    crate::contracts::resolve_legacy_client_for_type(
                        client_configs.as_deref().unwrap_or_default(),
                        &locator.client_type,
                    )
                } else {
                    None
                };
                if let Some(client_id) = legacy_client_id.as_ref() {
                    legacy_client_ids.insert(submission.download_id, client_id.clone());
                }
                let legacy_attributed = legacy_client_id.is_some();
                let observed_locator = match legacy_client_id.as_deref() {
                    Some(client_id) => locator.attributed_to_client(client_id),
                    None => locator.clone(),
                };
                // A submission whose binding already ended cannot produce the
                // deferral below: the Absent branch only blocks while
                // `find_active_binding_by_locator` still resolves. Checked
                // first so an unbound historical row costs no store reads at
                // all beyond the one bulk binding query above. Leave the
                // snapshot exactly as the client listing reported it so the
                // same-request retry and the conflict pass keep deciding from
                // the same evidence they would have without this fast path.
                let Some(bulk_binding) = active_bindings.iter().find(|binding| {
                    active_binding_matches_locator(binding, &observed_locator, legacy_attributed)
                }) else {
                    skipped_no_binding += 1;
                    continue;
                };
                let durable_state = self
                    .services
                    .workflow
                    .download_submissions
                    .get_identity_tracked_state_for_download(
                        Some(&submission.download_id),
                        &DownloadSubmissionIdentity::default(),
                        Some(&locator),
                    )
                    .await
                    .map_err(|error| AppError::DownloadSubmitUnavailable(error.to_string()))?;
                let imported = matches!(
                    durable_state.as_deref(),
                    Some("imported" | "imported_seeding")
                );
                let cleanup_pending = if imported {
                    self.services
                        .workflow
                        .download_submissions
                        .has_pending_download_cleanup(&submission.download_id)
                        .await
                        .map_err(|error| AppError::DownloadSubmitUnavailable(error.to_string()))?
                } else {
                    false
                };
                if matches!(durable_state.as_deref(), Some("failed" | "ignored"))
                    || (imported && !cleanup_pending)
                {
                    skipped_settled += 1;
                    continue;
                }
                observed += 1;
                match self
                    .services
                    .integrations
                    .download_client
                    .observe_download(&observed_locator, 0)
                    .await
                {
                    Ok(crate::DownloadClientObservation::Present(item)) => {
                        snapshot
                            .items
                            .retain(|cached| !queue_item_matches_submission(cached, submission));
                        snapshot.items.push(*item);
                        if let Some(client_id) = observed_locator.client_id.as_ref() {
                            snapshot.authoritative_client_ids.insert(client_id.clone());
                        }
                    }
                    Ok(crate::DownloadClientObservation::Absent) => {
                        let binding = self
                            .services
                            .workflow
                            .download_registry
                            .find_active_binding_by_locator(&observed_locator)
                            .await
                            .map_err(|error| {
                                AppError::DownloadSubmitUnavailable(error.to_string())
                            })?;
                        if let Some(binding) = binding.as_ref() {
                            // The lifecycle reconciler only visits configured
                            // clients; a binding on a deleted client would
                            // otherwise defer this scope forever.
                            let client_exists = match observed_locator.client_id.as_deref() {
                                Some(client_id) => self
                                    .services
                                    .integrations
                                    .download_client_configs
                                    .get_by_id(client_id)
                                    .await
                                    .map_err(|error| {
                                        AppError::DownloadSubmitUnavailable(error.to_string())
                                    })?
                                    .is_some(),
                                None => true,
                            };
                            if client_exists {
                                // Not a downloader outage: the client answered
                                // and simply no longer lists a job the registry
                                // still binds. Name the blocking download so the
                                // operator settles that instead of hunting a
                                // client problem that is not happening.
                                let deferral = DownloadLifecycleDeferral {
                                    download_id: submission.download_id.to_string(),
                                    client_id: observed_locator.client_id.clone(),
                                    client_type: observed_locator.client_type.clone(),
                                    native_item_id: observed_locator.item_id.clone(),
                                    tracked_state: durable_state
                                        .clone()
                                        .unwrap_or_else(|| "unknown".to_string()),
                                    binding_age_seconds: (Utc::now() - binding.created_at)
                                        .num_seconds()
                                        .max(0),
                                    last_seen_age_seconds: binding.last_seen_at.map(|last_seen| {
                                        (Utc::now() - last_seen).num_seconds().max(0)
                                    }),
                                    source_title: submission.source_title.clone(),
                                    scope: describe_submission_scope(&intent.scope),
                                };
                                tracing::info!(
                                    title_id = %title_id,
                                    download_id = %deferral.download_id,
                                    client_id = ?deferral.client_id,
                                    client_type = %deferral.client_type,
                                    native_item_id = %deferral.native_item_id,
                                    tracked_state = %deferral.tracked_state,
                                    binding_age_seconds = deferral.binding_age_seconds,
                                    last_seen_age_seconds = ?deferral.last_seen_age_seconds,
                                    release = ?deferral.source_title,
                                    scope = %deferral.scope,
                                    "acquisition deferred: lifecycle reconciliation pending"
                                );
                                // The observation that produced this deferral
                                // is exactly the evidence the lifecycle
                                // reconciler needs. Hand it over instead of
                                // waiting for a sweep that may be ten minutes
                                // and a budget window away, so this scope
                                // unblocks on the next attempt rather than
                                // paying for the same discovery again.
                                self.schedule_absent_source_reconciliation(
                                    &observed_locator,
                                    submission.download_id,
                                    &title_id,
                                );
                                return Err(AppError::download_lifecycle_deferred(deferral));
                            }
                            tracing::warn!(
                                download_id = %submission.download_id,
                                client_id = ?observed_locator.client_id,
                                "download client was deleted with an active binding; treating the job as absent"
                            );
                        }
                        snapshot
                            .items
                            .retain(|cached| !queue_item_matches_submission(cached, submission));
                        if let Some(client_id) = observed_locator.client_id.as_ref() {
                            snapshot.authoritative_client_ids.insert(client_id.clone());
                        }
                    }
                    Ok(crate::DownloadClientObservation::Unknown { reason, .. }) => {
                        // A locator with no client id reaches no client: the
                        // router cannot attribute it, and the rule above found
                        // no single configured client of its type to attribute
                        // it to. Stay fail-closed, but report it as the
                        // lifecycle deferral it is so the operator can see
                        // which legacy row is holding the scope instead of
                        // hunting a downloader outage that is not happening.
                        if observed_locator.client_id.is_none() {
                            let deferral = DownloadLifecycleDeferral {
                                download_id: submission.download_id.to_string(),
                                client_id: None,
                                client_type: observed_locator.client_type.clone(),
                                native_item_id: observed_locator.item_id.clone(),
                                tracked_state: durable_state
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                binding_age_seconds: (Utc::now() - bulk_binding.created_at)
                                    .num_seconds()
                                    .max(0),
                                last_seen_age_seconds: bulk_binding
                                    .last_seen_at
                                    .map(|last_seen| (Utc::now() - last_seen).num_seconds().max(0)),
                                source_title: submission.source_title.clone(),
                                scope: describe_submission_scope(&intent.scope),
                            };
                            tracing::info!(
                                title_id = %title_id,
                                download_id = %deferral.download_id,
                                client_type = %deferral.client_type,
                                native_item_id = %deferral.native_item_id,
                                tracked_state = %deferral.tracked_state,
                                binding_age_seconds = deferral.binding_age_seconds,
                                release = ?deferral.source_title,
                                scope = %deferral.scope,
                                reason,
                                "acquisition deferred: a download binding that names no client holds this scope, and its client type has no single configured client to attribute it to"
                            );
                            return Err(AppError::download_lifecycle_deferred(deferral));
                        }
                        return Err(AppError::DownloadSubmitUnavailable(format!(
                            "client state unknown; acquisition deferred for {}: {reason}",
                            submission.download_id
                        )));
                    }
                    Err(error) => {
                        return Err(AppError::DownloadSubmitUnavailable(format!(
                            "client state unknown; acquisition deferred for {}: {error}",
                            submission.download_id
                        )));
                    }
                }
            }
            tracing::debug!(
                title_id = %title_id,
                overlapping = overlapping_count,
                skipped_no_binding,
                skipped_settled,
                observed,
                elapsed_ms = guard_pass_started.elapsed().as_millis() as u64,
                "canonical submission guard finished its client observation pass"
            );
        }

        if let Some(existing) = existing {
            let snapshot = snapshot
                .as_ref()
                .expect("persisted submission has a snapshot");
            if let Some(item) = snapshot
                .items
                .iter()
                .find(|item| queue_item_matches_submission(item, &existing))
            {
                if item.state != DownloadQueueState::Failed {
                    return Ok(accepted_existing(existing));
                }
                if !submission_client_state_is_authoritative(snapshot, &existing) {
                    return Err(AppError::DownloadSubmitUnavailable(format!(
                        "download client state is unavailable for submission {} on title {title_id}",
                        existing.download_id
                    )));
                }
            } else if !submission_client_state_is_authoritative(snapshot, &existing) {
                return Err(AppError::DownloadSubmitUnavailable(format!(
                    "download client state is unavailable for submission {} on title {title_id}",
                    existing.download_id
                )));
            }
            self.services
                .workflow
                .download_submissions
                .delete_by_client_item_id(&ClientJobLocator::from_submission(&existing))
                .await?;
            state.forget(existing.download_id);
            self.runtime
                .acquisition
                .download_submission_guards
                .store_title_state(&title_id, state.clone());
        }

        let conflicts = if intent.request.purpose.is_additional_file() {
            Vec::new()
        } else if let Some(snapshot) = snapshot.as_ref() {
            Self::find_blocking_download_submissions_in_state(
                &intent.request.title,
                &intent.scope,
                &state.submissions,
                snapshot,
                &state.episodes,
                &state.accepted_download_ids,
                &legacy_client_ids,
            )?
        } else {
            Vec::new()
        };
        if !conflicts.is_empty() {
            match intent.conflict_policy {
                SubmissionConflictPolicy::Abort | SubmissionConflictPolicy::Skip => {
                    return Ok(CanonicalDownloadSubmissionOutcome::Conflict(
                        conflicts[0].clone(),
                    ));
                }
                SubmissionConflictPolicy::ReplaceEarly
                    if conflicts.iter().all(|conflict| conflict.replaceable) =>
                {
                    self.replace_blocking_download_submissions(&conflicts)
                        .await?;
                    state.submissions = self
                        .services
                        .workflow
                        .download_submissions
                        .list_for_title(&title_id)
                        .await?;
                    state.accepted_download_ids.clear();
                }
                SubmissionConflictPolicy::ReplaceEarly => {
                    let conflict = conflicts
                        .into_iter()
                        .find(|conflict| !conflict.replaceable)
                        .expect("non-empty conflicts should contain a non-replaceable item");
                    return Ok(CanonicalDownloadSubmissionOutcome::Conflict(conflict));
                }
            }
        }

        let download_id = intent.request.download_id.ok_or_else(|| {
            AppError::Validation("canonical download submission requires a download id".to_string())
        })?;
        let source_kind = intent.request.source_kind;
        let source_hint = normalize_release_attempt_hint(intent.request.source_hint.as_deref());
        let mut request = DownloadClientAddRequest {
            download_id: Some(download_id),
            ..intent.request.clone()
        };
        // Keep the staged file active through submission and every client
        // failover; the request contains only a reference to the lease.
        let _prepared_artifact = self
            .prepare_indexer_artifact_for_submission(&mut request, Some(title_id.clone()))
            .await?;
        let grab = match self
            .services
            .integrations
            .download_client
            .submit_download(&request)
            .await
        {
            Ok(grab) => grab,
            Err(error) => {
                if error.is_download_submit_ambiguous() {
                    let ambiguous = error.ambiguous_download_submission_client().map(
                        |(client_id, client_type)| DownloadSubmission {
                            download_id,
                            title_id: title_id.clone(),
                            facet: request.title.facet.as_str().to_string(),
                            download_client_id: client_id.map(str::to_string),
                            download_client_type: client_type.to_string(),
                            download_client_item_id: String::new(),
                            source_hint: source_hint.clone(),
                            source_provider_id: request.indexer_id.clone(),
                            source_provider_name: intent.source_provider_name.clone(),
                            source_kind,
                            source_title: request.source_title.clone(),
                            info_hash: request.info_hash_hint.clone(),
                            release_size_bytes: intent.release_size_bytes,
                            request_signature: intent.request_signature.clone(),
                            purpose: request.purpose,
                            scope: intent.scope.clone(),
                        },
                    );
                    let persisted = if let Some(ambiguous) = ambiguous.as_ref() {
                        self.services
                            .workflow
                            .download_submissions
                            .record_ambiguous_submission(ambiguous.clone())
                            .await
                            .is_ok()
                    } else {
                        false
                    };
                    if !persisted {
                        self.runtime
                            .acquisition
                            .download_submission_guards
                            .mark_uncertain(
                                &title_id,
                                UncertainDownloadSubmissionClaim::ambiguous(download_id, ambiguous),
                            );
                    }
                }
                return Err(error);
            }
        };

        if grab
            .download_id
            .is_some_and(|returned_download_id| returned_download_id != download_id)
        {
            let ambiguous = DownloadSubmission {
                download_id,
                title_id: title_id.clone(),
                facet: request.title.facet.as_str().to_string(),
                download_client_id: grab.client_id.clone(),
                download_client_type: grab.client_type.clone(),
                download_client_item_id: String::new(),
                source_hint: source_hint.clone(),
                source_provider_id: request.indexer_id.clone(),
                source_provider_name: intent.source_provider_name.clone(),
                source_kind,
                source_title: request.source_title.clone(),
                info_hash: request.info_hash_hint.clone(),
                release_size_bytes: intent.release_size_bytes,
                request_signature: intent.request_signature.clone(),
                purpose: request.purpose,
                scope: intent.scope.clone(),
            };
            if self
                .services
                .workflow
                .download_submissions
                .record_ambiguous_submission(ambiguous.clone())
                .await
                .is_err()
            {
                self.runtime
                    .acquisition
                    .download_submission_guards
                    .mark_uncertain(
                        &title_id,
                        UncertainDownloadSubmissionClaim::ambiguous(download_id, Some(ambiguous)),
                    );
            }
            return Err(AppError::DownloadSubmitAmbiguous(format!(
                "download client returned a different canonical identity for title {title_id}"
            ))
            .with_ambiguous_download_submission_client(
                grab.client_id.clone(),
                grab.client_type.clone(),
            ));
        }

        let accepted_identity = accepted_identity_for_grab(&intent, &request, download_id, &grab);
        let submission = submission_for_grab(&intent, &request, download_id, &grab);
        let seed_goals = grab.seed_goals.clone();
        let identity_disposition = match self
            .services
            .workflow
            .download_submissions
            .record_submission_with_identity(
                submission.clone(),
                accepted_identity.clone(),
                seed_goals.clone(),
            )
            .await
        {
            Ok(disposition) => disposition,
            Err(error) => {
                self.runtime
                    .acquisition
                    .download_submission_guards
                    .mark_uncertain(
                        &title_id,
                        UncertainDownloadSubmissionClaim::accepted(
                            submission,
                            accepted_identity,
                            seed_goals,
                        ),
                    );
                return Err(AppError::DownloadSubmitAmbiguous(format!(
                    "accepted download submission {download_id} could not be made durable for title {title_id}: {error}"
                ))
                .with_ambiguous_download_submission_client(
                    grab.client_id.clone(),
                    grab.client_type.clone(),
                ));
            }
        };
        if let CanonicalDownloadIdentityDisposition::AdoptedExisting {
            download_id: effective_download_id,
        } = identity_disposition
        {
            let adopted_grab = DownloadGrabResult {
                download_id: Some(effective_download_id),
                seed_goals: None,
                ..grab
            };
            let outcome = self
                .adopt_canonical_download(
                    &intent,
                    &request,
                    effective_download_id,
                    adopted_grab,
                    ObservationStubAdoption::ClaimForGrab(seed_goals),
                )
                .await?;
            if let Some(adopted) = self
                .services
                .workflow
                .download_submissions
                .find_by_canonical_download_id(&effective_download_id)
                .await?
            {
                state.remember(adopted);
                self.runtime
                    .acquisition
                    .download_submission_guards
                    .store_title_state(&title_id, state);
            }
            return Ok(outcome);
        }

        state.remember(submission);
        self.runtime
            .acquisition
            .download_submission_guards
            .store_title_state(&title_id, state);

        Ok(CanonicalDownloadSubmissionOutcome::Accepted(
            CanonicalDownloadSubmission {
                grab,
                newly_submitted: true,
            },
        ))
    }

    /// Resolve an indexer-hosted source into the artifact the download-client
    /// router requires; the router no longer fetches indexer URLs itself.
    ///
    /// Rewrites `request` to carry the staged NZB or resolved artifact in
    /// place of the indexer URL, and applies the NZB category gate to buffered
    /// NZB bytes. The returned lease owns the staged file: the caller must hold
    /// it until the download client has accepted the request, across every
    /// client failover. A request that already carries an artifact, has no
    /// source, or runs without a resolver is left untouched.
    pub(crate) async fn prepare_indexer_artifact_for_submission(
        &self,
        request: &mut DownloadClientAddRequest,
        title_id: Option<String>,
    ) -> AppResult<Option<PreparedIndexerArtifact>> {
        if request.resolved_download_artifact.is_some()
            || request.staged_nzb.is_some()
            || !request
                .source_hint
                .as_deref()
                .is_some_and(|source| !source.trim().is_empty())
        {
            return Ok(None);
        }
        let Some(resolver) = self
            .services
            .integrations
            .indexer_artifact_resolver
            .as_ref()
        else {
            return Ok(None);
        };
        let source_url = request.source_hint.clone().expect("checked above");
        let artifact = resolver
            .resolve_artifact(&IndexerArtifactResolutionRequest {
                indexer_id: request.indexer_id.clone(),
                source_url,
                source_kind: request.source_kind,
                info_hash_hint: request.info_hash_hint.clone(),
                title_id,
                search_facet: request
                    .search_facet
                    .clone()
                    .or_else(|| Some(request.title.facet.clone())),
                cancellation: tokio_util::sync::CancellationToken::new(),
            })
            .await?;
        match &artifact {
            PreparedIndexerArtifact::StagedNzb(staged_nzb) => {
                request.source_kind = Some(DownloadSourceKind::NzbFile);
                request.source_hint = None;
                request.staged_nzb = Some(staged_nzb.staged_nzb().clone());
            }
            PreparedIndexerArtifact::Resolved(artifact) => {
                match &artifact {
                    ResolvedDownloadArtifact::Nzb { bytes, .. } => {
                        let head_len = bytes.len().min(NZB_HEAD_PROBE_BYTES);
                        enforce_nzb_category_gate(
                            &bytes[..head_len],
                            request
                                .search_facet
                                .as_ref()
                                .unwrap_or(&request.title.facet),
                        )?;
                        request.source_kind = Some(DownloadSourceKind::NzbFile);
                        request.source_hint = None;
                    }
                    ResolvedDownloadArtifact::Magnet {
                        uri,
                        info_hash_hint,
                    } => {
                        request.source_kind = Some(DownloadSourceKind::MagnetUri);
                        request.source_hint = Some(uri.clone());
                        request.info_hash_hint =
                            info_hash_hint.clone().or(request.info_hash_hint.clone());
                    }
                    ResolvedDownloadArtifact::TorrentFile { info_hash_hint, .. } => {
                        request.source_kind = Some(DownloadSourceKind::TorrentFile);
                        request.source_hint = None;
                        request.info_hash_hint =
                            info_hash_hint.clone().or(request.info_hash_hint.clone());
                    }
                }
                request.resolved_download_artifact = Some(artifact.clone());
            }
        }
        Ok(Some(artifact))
    }
}

// ── Grab submission metrics ────────────────────────────────────────────────
//
// Every title-owned grab funnels through `submit_canonical_download`, and the
// one direct submission (the operator's unlinked grab) reports through
// `record_direct_grab_outcome`, so each outcome is counted once with the same
// label vocabulary. Counting only successes (what the call sites used to do
// inline) made a download client that rejects everything look like "no grabs
// happened" instead of "every grab failed", and let each site invent its own
// `indexer` label.
//
// Label discipline: every value is a `&'static str` from a bounded set, except
// `indexer`, which is a configured indexer name (bounded by the configured
// indexers) with an `unknown` fallback. Titles, URLs, ids and error text never
// reach a label.

const GRAB_SUBMISSIONS_TOTAL: &str = "scryer_grab_submissions_total";
const GRABS_TOTAL: &str = "scryer_grabs_total";
const RSS_SYNC_TOTAL: &str = "scryer_rss_sync_total";
const UNKNOWN_INDEXER: &str = "unknown";

const RESULT_GRABBED: &str = "grabbed";
const RESULT_REUSED: &str = "reused";
const RESULT_CONFLICT: &str = "conflict";
const RESULT_DEFERRED: &str = "deferred";
const RESULT_FAILED: &str = "failed";

/// Which product loop asked for a grab. This is the *trigger*, deliberately
/// separate from the indexer that supplied the release — conflating the two is
/// exactly what the old `indexer="manual"` label did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrabTrigger {
    /// RSS sync grabbed a matched release.
    Rss,
    /// Background acquisition grabbed a searched candidate.
    Auto,
    /// Background acquisition grabbed a season pack.
    SeasonPack,
    /// A parked pending release came due and was grabbed.
    Pending,
    /// An operator or API client queued a specific release.
    Manual,
}

impl GrabTrigger {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Auto => "auto",
            Self::SeasonPack => "season_pack",
            Self::Pending => "pending",
            Self::Manual => "manual",
        }
    }
}

/// Classifies one submission outcome into the fixed `result` label set.
///
/// `deferred` uses the same predicate pair the RSS path uses to choose
/// `ReleaseDownloadAttemptOutcome::Pending` over `Failed`, so the metric and
/// the release-attempt store can never disagree about whether a release was
/// burned.
fn grab_submission_result_label(
    result: &AppResult<CanonicalDownloadSubmissionOutcome>,
) -> &'static str {
    match result {
        Ok(CanonicalDownloadSubmissionOutcome::Accepted(submission)) => {
            if submission.newly_submitted {
                RESULT_GRABBED
            } else {
                RESULT_REUSED
            }
        }
        Ok(CanonicalDownloadSubmissionOutcome::Conflict(_)) => RESULT_CONFLICT,
        Err(err) => grab_error_result_label(err),
    }
}

/// The `result` label for a submission that errored, shared by the canonical
/// and the direct grab paths so the two can never classify the same error
/// differently.
fn grab_error_result_label(err: &AppError) -> &'static str {
    if is_download_submit_unavailable_error(err) || err.is_download_submit_ambiguous() {
        RESULT_DEFERRED
    } else {
        RESULT_FAILED
    }
}

/// Records the outcome of one `submit_canonical_download` call.
///
/// Call this at every grab site immediately after the submission returns and
/// before the outcome is matched, passing the same indexer name the site hands
/// to `record_indexer_grab` — the *configured* indexer name resolved through
/// `AppUseCase::grab_indexer_name`, so the `indexer` label matches the one
/// `scryer_indexer_queries_total` carries. `scryer_grabs_total` is incremented
/// only for a genuinely new submission.
pub(crate) fn record_grab_submission_outcome(
    trigger: GrabTrigger,
    facet: &MediaFacet,
    indexer: Option<&str>,
    result: &AppResult<CanonicalDownloadSubmissionOutcome>,
) {
    record_grab_outcome_labels(
        trigger,
        facet,
        indexer,
        grab_submission_result_label(result),
    );
}

/// Records a grab that went straight to a download client without passing
/// through `submit_canonical_download` — today only the operator's unlinked
/// grab from the Indexers page. A direct submission has no scope to reuse or
/// conflict with, so success is always `grabbed`; a failure is classified by
/// the same predicate the canonical path uses.
pub(crate) fn record_direct_grab_outcome(
    trigger: GrabTrigger,
    facet: &MediaFacet,
    indexer: Option<&str>,
    result: &AppResult<DownloadGrabResult>,
) {
    let result_label = match result {
        Ok(_) => RESULT_GRABBED,
        Err(err) => grab_error_result_label(err),
    };
    record_grab_outcome_labels(trigger, facet, indexer, result_label);
}

fn record_grab_outcome_labels(
    trigger: GrabTrigger,
    facet: &MediaFacet,
    indexer: Option<&str>,
    result_label: &'static str,
) {
    metrics::counter!(
        GRAB_SUBMISSIONS_TOTAL,
        "trigger" => trigger.as_str(),
        "facet" => facet.as_str(),
        "result" => result_label,
    )
    .increment(1);

    if result_label == RESULT_GRABBED {
        let indexer = indexer
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(UNKNOWN_INDEXER)
            .to_string();
        metrics::counter!(
            GRABS_TOTAL,
            "indexer" => indexer,
            "facet" => facet.as_str(),
            "trigger" => trigger.as_str(),
        )
        .increment(1);
    }
}

/// Registers HELP/UNIT metadata for the acquisition metric families this crate
/// emits.
///
/// Crate-public because the binary's metrics setup calls it once at startup, so
/// the scrape surface is self-describing even while a family is still empty.
pub fn describe_acquisition_metrics() {
    metrics::describe_counter!(
        GRAB_SUBMISSIONS_TOTAL,
        "Grab submissions attempted, labelled by trigger (rss, auto, season_pack, pending, manual), media facet, and result (grabbed, reused, conflict, deferred, failed)."
    );
    metrics::describe_counter!(
        GRABS_TOTAL,
        "Releases newly sent to a download client, labelled by the configured name of the indexer that supplied the release (the same value scryer_indexer_queries_total uses), the media facet, and the trigger that asked for the grab. Reused submissions and indexer-file downloads handed to the operator are not grabs."
    );

    metrics::describe_counter!(
        RSS_SYNC_TOTAL,
        "RSS sync cycles that finished, labelled by outcome (completed, or an early exit: no_titles, no_clients)."
    );
    metrics::describe_histogram!(
        "scryer_rss_sync_duration_seconds",
        metrics::Unit::Seconds,
        "Wall-clock duration of one RSS sync cycle, including early exits."
    );
    metrics::describe_counter!(
        "scryer_rss_releases_fetched_total",
        "Releases returned by indexer RSS feeds across all completed RSS sync cycles."
    );
    metrics::describe_counter!(
        "scryer_rss_releases_matched_total",
        "Fetched RSS releases that matched a monitored title or episode."
    );
    metrics::describe_counter!(
        "scryer_rss_releases_grabbed_total",
        "Matched RSS releases that were actually grabbed."
    );

    metrics::describe_counter!(
        "scryer_background_acquisition_title_work_total",
        "Background title-level acquisition units of work, labelled by outcome (completed or failed)."
    );
    metrics::describe_counter!(
        "scryer_background_acquisition_target_work_total",
        "Background target-level acquisition units of work, labelled by outcome (completed or failed)."
    );
    metrics::describe_counter!(
        "scryer_background_acquisition_scan_owned_yields_total",
        "Times background acquisition yielded because a library scan owned the facet it wanted to work on."
    );

    metrics::describe_counter!(
        "scryer_wanted_projection_cache_total",
        "Wanted-projection cache lookups, labelled by result (hit or miss)."
    );
    metrics::describe_histogram!(
        "scryer_wanted_projection_rebuild_duration_seconds",
        metrics::Unit::Seconds,
        "Time taken to rebuild a wanted projection, labelled by the projection kind."
    );
    metrics::describe_gauge!(
        "scryer_wanted_projection_items",
        "Number of rows in the most recently rebuilt wanted projection, labelled by the projection kind."
    );
}

#[cfg(test)]
mod grab_metrics_tests {
    use std::collections::BTreeMap;

    use metrics::with_local_recorder;
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    use super::*;

    /// One recorded counter series: name, sorted labels, value.
    type CounterSeries = (String, BTreeMap<String, String>, u64);

    fn recorded_counters(record: impl FnOnce()) -> Vec<CounterSeries> {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        with_local_recorder(&recorder, record);
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter_map(|(key, _, _, value)| match value {
                DebugValue::Counter(count) => Some((
                    key.key().name().to_string(),
                    key.key()
                        .labels()
                        .map(|label| (label.key().to_string(), label.value().to_string()))
                        .collect::<BTreeMap<String, String>>(),
                    count,
                )),
                _ => None,
            })
            .collect()
    }

    fn series<'a>(counters: &'a [CounterSeries], name: &str) -> Vec<&'a CounterSeries> {
        counters
            .iter()
            .filter(|(series_name, _, _)| series_name == name)
            .collect()
    }

    fn grab_result() -> DownloadGrabResult {
        DownloadGrabResult {
            job_id: "job-1".to_string(),
            client_id: Some("client-1".to_string()),
            client_type: "sabnzbd".to_string(),
            info_hash: None,
            download_id: None,
            seed_goals: None,
        }
    }

    fn accepted(newly_submitted: bool) -> AppResult<CanonicalDownloadSubmissionOutcome> {
        Ok(CanonicalDownloadSubmissionOutcome::Accepted(
            CanonicalDownloadSubmission {
                grab: grab_result(),
                newly_submitted,
            },
        ))
    }

    fn conflict() -> AppResult<CanonicalDownloadSubmissionOutcome> {
        Ok(CanonicalDownloadSubmissionOutcome::Conflict(
            SubmissionScopeConflict {
                title_id: "title-1".to_string(),
                title_name: "Example".to_string(),
                download_client_id: Some("client-1".to_string()),
                download_client_type: "sabnzbd".to_string(),
                download_client_item_id: "job-1".to_string(),
                source_title: None,
                source_kind: None,
                scope: SubmissionScope::Title,
                state: None,
                replaceable: false,
            },
        ))
    }

    fn failure(error: AppError) -> AppResult<CanonicalDownloadSubmissionOutcome> {
        Err(error)
    }

    fn result_label_for(result: &AppResult<CanonicalDownloadSubmissionOutcome>) -> String {
        let counters = recorded_counters(|| {
            record_grab_submission_outcome(
                GrabTrigger::Rss,
                &MediaFacet::Series,
                Some("nzb"),
                result,
            )
        });
        let submissions = series(&counters, GRAB_SUBMISSIONS_TOTAL);
        assert_eq!(
            submissions.len(),
            1,
            "exactly one submission series per call: {counters:?}"
        );
        submissions[0]
            .1
            .get("result")
            .expect("result label present")
            .clone()
    }

    #[test]
    fn every_submission_outcome_maps_to_its_result_label() {
        assert_eq!(result_label_for(&accepted(true)), RESULT_GRABBED);
        assert_eq!(result_label_for(&accepted(false)), RESULT_REUSED);
        assert_eq!(result_label_for(&conflict()), RESULT_CONFLICT);
        // The two deferrable failures: the client was unavailable, and the
        // request may have been accepted with the response lost. Both are
        // retried without burning the release, so neither may read as `failed`.
        assert_eq!(
            result_label_for(&failure(AppError::DownloadSubmitUnavailable(
                "client offline".to_string()
            ))),
            RESULT_DEFERRED
        );
        assert_eq!(
            result_label_for(&failure(AppError::DownloadSubmitFailoverExhausted(
                "every client failed".to_string()
            ))),
            RESULT_DEFERRED
        );
        assert_eq!(
            result_label_for(&failure(AppError::DownloadSubmitAmbiguous(
                "response lost".to_string()
            ))),
            RESULT_DEFERRED
        );
        assert_eq!(
            result_label_for(&failure(AppError::Validation("bad request".to_string()))),
            RESULT_FAILED
        );
        assert_eq!(
            result_label_for(&failure(AppError::DownloadSubmitRejected(
                "client said no".to_string()
            ))),
            RESULT_FAILED
        );
    }

    #[test]
    fn submission_counter_carries_trigger_and_facet() {
        let counters = recorded_counters(|| {
            record_grab_submission_outcome(
                GrabTrigger::SeasonPack,
                &MediaFacet::Anime,
                Some("nzbgeek"),
                &conflict(),
            );
        });

        let submissions = series(&counters, GRAB_SUBMISSIONS_TOTAL);
        assert_eq!(submissions.len(), 1);
        let (_, labels, value) = submissions[0];
        assert_eq!(*value, 1);
        assert_eq!(
            labels.get("trigger").map(String::as_str),
            Some("season_pack")
        );
        assert_eq!(labels.get("facet").map(String::as_str), Some("anime"));
        assert_eq!(labels.get("result").map(String::as_str), Some("conflict"));
    }

    #[test]
    fn grabs_total_is_emitted_only_for_a_new_submission() {
        for result in [
            accepted(false),
            conflict(),
            failure(AppError::DownloadSubmitUnavailable("offline".to_string())),
            failure(AppError::Validation("bad".to_string())),
        ] {
            let counters = recorded_counters(|| {
                record_grab_submission_outcome(
                    GrabTrigger::Auto,
                    &MediaFacet::Movie,
                    Some("nzbgeek"),
                    &result,
                );
            });
            assert!(
                series(&counters, GRABS_TOTAL).is_empty(),
                "non-grabbed outcome must not count as a grab: {counters:?}"
            );
        }

        let counters = recorded_counters(|| {
            record_grab_submission_outcome(
                GrabTrigger::Auto,
                &MediaFacet::Movie,
                Some("nzbgeek"),
                &accepted(true),
            );
        });
        let grabs = series(&counters, GRABS_TOTAL);
        assert_eq!(grabs.len(), 1);
        let (_, labels, value) = grabs[0];
        assert_eq!(*value, 1);
        assert_eq!(labels.get("indexer").map(String::as_str), Some("nzbgeek"));
        assert_eq!(labels.get("facet").map(String::as_str), Some("movie"));
        assert_eq!(labels.get("trigger").map(String::as_str), Some("auto"));
    }

    #[test]
    fn a_direct_grab_is_grabbed_on_success_and_classified_like_a_canonical_failure() {
        let counters = recorded_counters(|| {
            record_direct_grab_outcome(
                GrabTrigger::Manual,
                &MediaFacet::Series,
                Some("nzbgeek"),
                &Ok(grab_result()),
            );
        });
        let submissions = series(&counters, GRAB_SUBMISSIONS_TOTAL);
        assert_eq!(submissions.len(), 1, "{counters:?}");
        assert_eq!(
            submissions[0].1.get("result").map(String::as_str),
            Some(RESULT_GRABBED)
        );
        let grabs = series(&counters, GRABS_TOTAL);
        assert_eq!(grabs.len(), 1, "{counters:?}");
        let (_, labels, value) = grabs[0];
        assert_eq!(*value, 1);
        assert_eq!(labels.get("indexer").map(String::as_str), Some("nzbgeek"));
        assert_eq!(labels.get("facet").map(String::as_str), Some("series"));
        assert_eq!(labels.get("trigger").map(String::as_str), Some("manual"));

        for (error, expected) in [
            (
                AppError::DownloadSubmitUnavailable("offline".to_string()),
                RESULT_DEFERRED,
            ),
            (
                AppError::DownloadSubmitRejected("client said no".to_string()),
                RESULT_FAILED,
            ),
        ] {
            let counters = recorded_counters(|| {
                record_direct_grab_outcome(
                    GrabTrigger::Manual,
                    &MediaFacet::Series,
                    Some("nzbgeek"),
                    &Err(error),
                );
            });
            let submissions = series(&counters, GRAB_SUBMISSIONS_TOTAL);
            assert_eq!(submissions.len(), 1, "{counters:?}");
            assert_eq!(
                submissions[0].1.get("result").map(String::as_str),
                Some(expected)
            );
            assert!(
                series(&counters, GRABS_TOTAL).is_empty(),
                "a failed direct grab must not count as a grab: {counters:?}"
            );
        }
    }

    #[test]
    fn missing_or_blank_indexer_falls_back_to_unknown() {
        for indexer in [None, Some(""), Some("   ")] {
            let counters = recorded_counters(|| {
                record_grab_submission_outcome(
                    GrabTrigger::Manual,
                    &MediaFacet::Movie,
                    indexer,
                    &accepted(true),
                );
            });
            let grabs = series(&counters, GRABS_TOTAL);
            assert_eq!(grabs.len(), 1);
            assert_eq!(
                grabs[0].1.get("indexer").map(String::as_str),
                Some(UNKNOWN_INDEXER),
                "indexer {indexer:?} should fall back to unknown"
            );
        }
    }

    #[test]
    fn trigger_labels_are_unique_snake_case() {
        let triggers = [
            GrabTrigger::Rss,
            GrabTrigger::Auto,
            GrabTrigger::SeasonPack,
            GrabTrigger::Pending,
            GrabTrigger::Manual,
        ];
        let labels: std::collections::BTreeSet<&str> =
            triggers.iter().map(|trigger| trigger.as_str()).collect();
        assert_eq!(
            labels.len(),
            triggers.len(),
            "trigger labels must be unique"
        );
        for label in labels {
            assert!(
                !label.is_empty()
                    && label.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !label.starts_with('_')
                    && !label.ends_with('_'),
                "trigger label {label:?} is not snake_case"
            );
        }
    }
}
