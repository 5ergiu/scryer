use super::*;
use crate::contracts::{
    ClientJobLocator, DownloadClientBindingRecord, DownloadRecord, ObservationResolution,
    ObservedClientJob, TerminalDownloadHistoryRow,
};
use crate::escalation_backoff::DownloadClientStatus as DownloadClientBackoffStatus;
use crate::location::model::{
    FileVerificationRecord, LocationOperation, LocationOperationCounters, LocationOperationState,
    LocationReasonCode, TitleCheckpoint,
};
use crate::location::ownership_guard::{OwnedEntity, OwnershipConflict};
use crate::types::{
    ApiKeyRecord, CollectionMediaSizeSummary, EpisodeMediaAvailability, EpisodeMediaSizeSummary,
    IndexerSearchPlanCapability, IndexerSearchPlanRequest, IndexerSearchPlanSummary,
    IndexerSearchStrategyEventSink, LoginVerificationChallengeRecord,
    OAuthClientRegistrationRecord, PendingReleaseObservation, PendingReleaseRole,
    TitleCatalogFilterCounts,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use scryer_domain::download_identity::DownloadId;
use scryer_domain::{
    AnimeNumberingBridge, CanonicalMediaTag, ImportTransferPhase, ImportType, IndexerCapsSnapshot,
    MaintenanceRuleRevision, MaintenanceRuleSet, MonitorSelection, PersistedPluginWasmPayload,
    title_catalog_name_tie_key, title_catalog_sort_key_for_title,
};
use scryer_plugin_sdk::{
    ArchivePluginFormat, ArchivePluginProcessRequest, ArchivePluginProcessResponse,
    SubtitleSyncAlignResponse, SubtitleSyncAudioCodec, SubtitleSyncMediaMetadataSnapshot,
    SubtitleSyncOptions,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub const NOTIFICATION_REQUEST_SCHEMA_VERSION: u32 = 1;

/// Fallback for repositories that have no tag registry behind them (the null
/// repository and the non-SQL test doubles). Reads answer empty; writes say so
/// rather than pretending to succeed.
fn title_tag_registry_unsupported() -> AppError {
    AppError::Repository("this repository does not store the title tag registry".to_string())
}

/// Admission gate for observed indexer dispatches during orderly shutdown.
/// The transport holds an admission while it invokes its dispatch observer;
/// closing the gate makes later attempts fail before HTTP can start.
#[derive(Clone)]
pub struct IndexerDispatchGate {
    state: Arc<Mutex<IndexerDispatchGateState>>,
    idle: Arc<Notify>,
}

impl Default for IndexerDispatchGate {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(IndexerDispatchGateState::default())),
            idle: Arc::new(Notify::new()),
        }
    }
}

#[derive(Default)]
struct IndexerDispatchGateState {
    closed: bool,
    admitted: u32,
}

impl IndexerDispatchGate {
    pub fn admit(&self) -> Option<IndexerDispatchAdmission> {
        let mut state = self.state.lock().ok()?;
        if state.closed {
            return None;
        }
        state.admitted = state.admitted.saturating_add(1);
        Some(IndexerDispatchAdmission { gate: self.clone() })
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
        }
    }

    /// Wait until every dispatch admitted before close has completed its
    /// observer/send boundary. No later dispatch can be admitted.
    pub async fn wait_for_idle(&self) {
        loop {
            let notified = self.idle.notified();
            let idle = self
                .state
                .lock()
                .map(|state| state.admitted == 0)
                .unwrap_or(true);
            if idle {
                return;
            }
            notified.await;
        }
    }
}

/// A successful gate admission. Its lifetime documents that the dispatch
/// observer and transport send belong to the pre-shutdown producer set.
pub struct IndexerDispatchAdmission {
    gate: IndexerDispatchGate,
}

impl Drop for IndexerDispatchAdmission {
    fn drop(&mut self) {
        if let Ok(mut state) = self.gate.state.lock() {
            state.admitted = state.admitted.saturating_sub(1);
            if state.admitted == 0 {
                self.gate.idle.notify_waiters();
            }
        }
    }
}

/// A field-level title option patch. `None` preserves stored state, `Some(None)`
/// clears an override, and `Some(Some(_))` applies an explicit override.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleOptionsPatch {
    pub search_languages: Option<Option<Vec<String>>>,
    pub search_aliases: Option<Option<Vec<String>>>,
