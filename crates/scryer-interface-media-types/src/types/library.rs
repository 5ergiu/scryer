use super::{
    DownloadClientRoutingEntryInput, DownloadClientRoutingEntryPayload, DownloadSourceKindValue,
    FillerPolicyValue, ImportDecisionValue, ImportModeValue, ImportSkipReasonValue,
    ImportStatusValue, ImportTypeValue, IndexerRoutingEntryInput, IndexerRoutingEntryPayload,
    JobRunPayload, Long, ManualImportCandidateMappingInput, MediaFacetValue, MonitorSelectionInput,
    MonitorTypeValue, PendingImportStatusValue, RecapPolicyValue, ReleaseNumberingValue,
    RenameCollisionPolicyValue, RenameMissingMetadataPolicyValue, ScoringPersonaValue,
};
use async_graphql::{Enum, ID, InputObject, MaybeUndefined, OneofObject, SimpleObject};
use chrono::{DateTime, Utc};

#[derive(SimpleObject, Clone)]
/// Root-folder path and default marker.
pub struct RootFolderPayload {
    /// Filesystem path.
    pub path: String,
    /// Whether this is the default root folder.
    pub is_default: bool,
}

#[derive(SimpleObject, Clone)]
/// Library root-folder identity, path, and default marker.
pub struct LibraryRootPayload {
    /// Root-folder ID.
    pub id: ID,
    /// Filesystem path.
    pub path: String,
    /// Whether this is the default root folder.
    pub is_default: bool,
}

#[derive(SimpleObject, Clone)]
/// Effective library settings together with nullable per-library overrides.
pub struct LibrarySettingsPayload {
    /// Ordered release-search languages whose localized titles are also queried; empty searches only the primary name.
    pub search_languages: Vec<String>,
    /// Library override for required audio language codes; `original` resolves per title.
    pub required_audio_languages_override: Option<Vec<String>>,
    /// Effective configured requirements after inheritance; `original` remains unchanged.
    pub required_audio_languages: Vec<String>,
    /// Library metadata-language override; null means inherit the global default.
    pub metadata_language_override: Option<String>,
    /// Effective metadata language after inheritance.
    pub metadata_language: String,
    /// Library override for season folders; null means inherit the facet setting.
    pub use_season_folders_override: Option<bool>,
    /// Effective season-folder setting after inheritance.
    pub use_season_folders: bool,
    /// Library override quality-profile ID; null means inherit.
    pub quality_profile_id_override: Option<ID>,
    /// Effective quality-profile ID.
    pub quality_profile_id: ID,
    /// Library override request quality-profile IDs; null means inherit.
    pub request_quality_profile_ids_override: Option<Vec<ID>>,
    /// Effective request quality-profile IDs; empty means no additional profiles.
    pub request_quality_profile_ids: Vec<ID>,
    /// Effective default request quality-profile ID.
    pub request_quality_profile_default_id: ID,
    /// Library override scoring persona; null means inherit.
    pub scoring_persona_override: Option<ScoringPersonaValue>,
    /// Effective scoring persona.
    pub scoring_persona: ScoringPersonaValue,
    /// Library override filler policy; null means inherit.
    pub filler_policy_override: Option<FillerPolicyValue>,
    /// Effective filler policy, or null when unset.
    pub filler_policy: Option<FillerPolicyValue>,
    /// Library override recap policy; null means inherit.
    pub recap_policy_override: Option<RecapPolicyValue>,
    /// Effective recap policy, or null when unset.
    pub recap_policy: Option<RecapPolicyValue>,
    /// Library override for monitoring specials; null means inherit.
    pub monitor_specials_override: Option<bool>,
    /// Effective specials monitoring setting, or null when unset.
    pub monitor_specials: Option<bool>,
    /// Library override for inter-season movies; null means inherit.
    pub inter_season_movies_override: Option<bool>,
    /// Effective inter-season movie setting, or null when unset.
    pub inter_season_movies: Option<bool>,
    /// Library override for filler movies; null means inherit.
    pub monitor_filler_movies_override: Option<bool>,
    /// Effective filler-movie monitoring setting, or null when unset.
    pub monitor_filler_movies: Option<bool>,
    /// Library override for NFO writing; null means inherit.
    pub nfo_write_on_import_override: Option<bool>,
    /// Effective NFO-on-import setting.
    pub nfo_write_on_import: bool,
    /// Library override for Plex match writing; null means inherit.
    pub plexmatch_write_on_import_override: Option<bool>,
    /// Effective Plex match-on-import setting, or null when unset.
    pub plexmatch_write_on_import: Option<bool>,
    /// Library override import mode; null means inherit.
    pub import_mode_override: Option<ImportModeValue>,
    /// Effective import mode.
    pub import_mode: ImportModeValue,
    /// Library override for Linux permission updates; null means inherit.
    pub set_permissions_linux_override: Option<bool>,
    /// Effective Linux permission-update setting.
    pub set_permissions_linux: bool,
    /// Library override file chmod mode; null means inherit.
    pub file_chmod_override: Option<String>,
    /// Effective file chmod mode, or null when unset.
    pub file_chmod: Option<String>,
    /// Library override folder chmod mode; null means inherit.
    pub folder_chmod_override: Option<String>,
    /// Effective folder chmod mode, or null when unset.
    pub folder_chmod: Option<String>,
    /// Library override chown group; null means inherit.
    pub chown_group_override: Option<String>,
    /// Effective chown group, or null when unset.
    pub chown_group: Option<String>,
    /// Library override indexer routing entries; null means inherit.
    pub indexer_routing_override: Option<Vec<IndexerRoutingEntryPayload>>,
    /// Library override download-client routing entries; null means inherit.
    pub download_client_routing_override: Option<Vec<DownloadClientRoutingEntryPayload>>,
}

#[derive(SimpleObject, Clone)]
/// Download-client option used by paged queue and history filters.
pub struct DownloadClientFilterOptionPayload {
    /// Download-client ID.
    pub client_id: ID,
    /// Download-client name.
    pub client_name: String,
    /// Download-client provider type.
    pub client_type: String,
}

#[derive(SimpleObject, Clone)]
/// Result of one import operation, including decision and source/destination paths.
pub struct ImportResultPayload {
    /// Import record ID.
    pub import_id: ID,
    /// Final import decision.
    pub decision: ImportDecisionValue,
    /// Skip reason when the decision skipped the import, or null otherwise.
    pub skip_reason: Option<ImportSkipReasonValue>,
    /// Imported title ID, or null when no title was bound.
    pub title_id: Option<ID>,
    /// Source path examined by the import.
    pub source_path: String,
    /// Destination path written by the import, or null when no destination was written.
    pub dest_path: Option<String>,
    /// Error message, or null when the operation completed without an error.
    pub error_message: Option<String>,
}

#[derive(SimpleObject, Clone)]
/// Persisted import record with lifecycle status, decision, IDs, and UTC timestamps.
pub struct ImportRecordPayload {
    /// Import record ID.
    pub id: ID,
    /// External system that supplied the import.
    pub source_system: String,
    /// Source-system reference identifying the download or item.
    pub source_ref: String,
    /// Source title, or null when unavailable.
    pub source_title: Option<String>,
    /// Media facet, or null when not yet classified.
    pub facet: Option<MediaFacetValue>,
    /// Import operation type.
    pub import_type: ImportTypeValue,
    /// Current import lifecycle status.
    pub status: ImportStatusValue,
    /// Error message, or null when no error is recorded.
    pub error_message: Option<String>,
    /// Import decision, or null before a decision is made.
    pub decision: Option<ImportDecisionValue>,
    /// Skip reason, or null when not skipped.
    pub skip_reason: Option<ImportSkipReasonValue>,
    /// Bound title ID, or null when no title was selected.
    pub title_id: Option<ID>,
    /// Source path, or null when not recorded.
    pub source_path: Option<String>,
    /// Destination path, or null when no destination was produced.
    pub dest_path: Option<String>,
    /// UTC start time, or null before processing begins.
    pub started_at: Option<DateTime<Utc>>,
    /// UTC completion time, or null while incomplete.
    pub finished_at: Option<DateTime<Utc>>,
    /// UTC record creation time.
    pub created_at: DateTime<Utc>,
}

#[derive(InputObject)]
/// Requests retrying one import record, optionally supplying an archive password.
pub struct RetryImportInput {
    /// Import record ID to retry.
    pub import_id: ID,
    /// Optional password for an encrypted source archive.
    pub password: Option<String>,
}

#[derive(InputObject)]
/// Requests releasing the sources a completed import is holding.
pub struct ReleaseHeldImportSourcesInput {
    /// Completed import record ID whose held sources are released.
    pub import_id: ID,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(rename_items = "SCREAMING_SNAKE_CASE")]
/// Why a completed import kept its download's sources.
pub enum HeldImportSourcesReasonValue {
    /// Subtitle discovery or delivery is still pending.
    SubtitlesPending,
    /// The import finished, but removing its replaced sources did not.
    SourceCleanupIncomplete,
    /// The hold has no recorded reason.
    Unknown,
    /// Archive extraction failed; the unextracted archives were never imported.
    ArchiveExtractionFailed,
}

impl From<scryer_application::HeldSourcesReason> for HeldImportSourcesReasonValue {
    fn from(value: scryer_application::HeldSourcesReason) -> Self {
        use scryer_application::HeldSourcesReason as Reason;
        match value {
            Reason::SubtitlesPending => Self::SubtitlesPending,
            Reason::SourceCleanupIncomplete => Self::SourceCleanupIncomplete,
            Reason::Unknown => Self::Unknown,
            Reason::ArchiveExtractionFailed => Self::ArchiveExtractionFailed,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(rename_items = "SCREAMING_SNAKE_CASE")]
/// What the download client's removal policy does with the download after release.
pub enum HeldDownloadClientPolicyValue {
    /// The client removes the completed download.
    Removes,
    /// The client removes the completed download once its seeding requirements are met.
    RemovesAfterSeeding,
    /// The policy keeps the download, or no configured client can act on it.
    Keeps,
    /// The download's title could not be resolved, so the policy is unknown.
    Unknown,
}

impl From<scryer_application::HeldDownloadClientPolicy> for HeldDownloadClientPolicyValue {
    fn from(value: scryer_application::HeldDownloadClientPolicy) -> Self {
        use scryer_application::HeldDownloadClientPolicy as Policy;
        match value {
            Policy::Removes => Self::Removes,
            Policy::RemovesAfterSeeding => Self::RemovesAfterSeeding,
            Policy::Keeps => Self::Keeps,
            Policy::Unknown => Self::Unknown,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(rename_items = "SCREAMING_SNAKE_CASE")]
/// What releasing held sources did to the tracked download.
pub enum HeldImportSourcesSettlementValue {
    /// Verification proved the download complete; it is imported and the client's policy applies.
    Imported,
    /// Verification did not prove the download complete; it returned to import and nothing was cleaned up.
    AwaitingImport,
    /// The download was not blocked on the hold; its own workflow decides.
    Unchanged,
    /// The download is no longer tracked; it is settled when next observed.
    Untracked,
    /// The holds were released but the tracked download could not be reached; releasing again resumes.
    NotSettled,
    /// A held import may hold content never extracted or imported, and nothing proved every expected file imported; not marked imported, no retry scheduled and nothing cleaned up.
    Unproven,
}

impl From<scryer_application::HeldSourcesSettlement> for HeldImportSourcesSettlementValue {
    fn from(value: scryer_application::HeldSourcesSettlement) -> Self {
        use scryer_application::HeldSourcesSettlement as Settlement;
        match value {
            Settlement::Imported => Self::Imported,
            Settlement::AwaitingImport => Self::AwaitingImport,
            Settlement::Unchanged => Self::Unchanged,
            Settlement::Untracked => Self::Untracked,
            Settlement::NotSettled => Self::NotSettled,
            Settlement::Unproven => Self::Unproven,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(rename_items = "SCREAMING_SNAKE_CASE")]
/// Why a released import's extraction workspace was preserved.
pub enum HeldWorkspacePreservedValue {
    /// The workspace's ownership marker could not be authenticated.
    NotOwned,
    /// It contains a symlink, special file, unreadable entry, or too many entries.
    Unsafe,
    /// A video in it is not recorded, by its path, as imported by the import that owns it.
    HoldsUnimportedVideo,
    /// An unfinished import or open manual-import selection may still need it.
    InUse,
    /// What was imported from it could not be established.
    Unverified,
    /// Removal was attempted and the workspace remains.
    RemovalFailed,
    /// The download was not proven imported, so nothing of it was removed.
    DownloadNotImported,
}

impl From<scryer_application::HeldWorkspacePreserved> for HeldWorkspacePreservedValue {
    fn from(value: scryer_application::HeldWorkspacePreserved) -> Self {
        use scryer_application::HeldWorkspacePreserved as Preserved;
        match value {
            Preserved::NotOwned => Self::NotOwned,
            Preserved::Unsafe => Self::Unsafe,
            Preserved::HoldsUnimportedVideo => Self::HoldsUnimportedVideo,
            Preserved::InUse => Self::InUse,
            Preserved::Unverified => Self::Unverified,
            Preserved::RemovalFailed => Self::RemovalFailed,
            Preserved::DownloadNotImported => Self::DownloadNotImported,
        }
    }
}

#[derive(SimpleObject, Clone)]
/// Outcome of releasing a download's held sources.
pub struct HeldImportSourcesReleasedPayload {
    /// Import record ID the release was requested through.
    pub import_id: ID,
    /// Every import of the download whose hold was released.
    pub released_import_ids: Vec<ID>,
    /// What the release did to the tracked download.
    pub settlement: HeldImportSourcesSettlementValue,
    /// The download client's removal policy that now applies.
    pub client_policy: HeldDownloadClientPolicyValue,
    /// Whether every identified extraction workspace was removed and none could have been missed.
    pub workspace_removed: bool,
    /// Number of extraction workspaces removed.
    pub workspaces_removed: i32,
    /// Distinct reasons extraction workspaces were preserved; preserved workspaces are never removed later by inference.
    pub preserved_workspace_reasons: Vec<HeldWorkspacePreservedValue>,
    /// Whether a released import's workspaces could not be searched for in full; any not found are preserved.
    pub workspace_lookup_incomplete: bool,
}

impl From<scryer_application::HeldSourcesRelease> for HeldImportSourcesReleasedPayload {
    fn from(value: scryer_application::HeldSourcesRelease) -> Self {
        Self {
            workspace_removed: value.workspace_removed(),
            workspaces_removed: i32::try_from(value.workspaces_removed).unwrap_or(i32::MAX),
            import_id: value.import_id.into(),
            released_import_ids: value
                .released_import_ids
                .into_iter()
                .map(Into::into)
                .collect(),
            settlement: value.settlement.into(),
            client_policy: value.client_policy.into(),
            preserved_workspace_reasons: value
                .preserved_workspaces
                .into_iter()
                .map(Into::into)
                .collect(),
            workspace_lookup_incomplete: value.workspace_lookup_incomplete,
        }
    }
}

#[derive(SimpleObject, Clone)]
/// Held sources of a download the viewer may release.
pub struct HeldImportSourcesPayload {
    /// Completed import record ID the release is requested through.
    pub import_id: ID,
    /// Every import of the download the release covers.
    pub import_ids: Vec<ID>,
    /// Distinct titles those imports belong to.
    pub title_names: Vec<String>,
    /// The most severe reason the sources are held.
    pub reason: HeldImportSourcesReasonValue,
    /// What the download client's removal policy does with the download after release.
    pub client_policy: HeldDownloadClientPolicyValue,
}

impl From<scryer_application::HeldSourcesReleaseOffer> for HeldImportSourcesPayload {
    fn from(value: scryer_application::HeldSourcesReleaseOffer) -> Self {
        Self {
            import_id: value.import_id.into(),
            import_ids: value.import_ids.into_iter().map(Into::into).collect(),
            title_names: value.title_names,
            reason: value.reason.into(),
            client_policy: value.client_policy.into(),
        }
    }
}

#[derive(InputObject)]
/// Identifies a tracked download to ignore without deleting it from the download client.
pub struct IgnoreTrackedDownloadInput {
    /// Download-client ID, or null when the provider identity is sufficient.
    pub client_id: Option<ID>,
    /// Download-client provider type.
    pub client_type: String,
    /// Provider-issued download item ID.
    pub download_client_item_id: String,
}

#[derive(InputObject)]
/// Marks a tracked download failed and optionally prevents reacquisition.
pub struct MarkTrackedDownloadFailedInput {
    /// Download-client ID, or null when the provider identity is sufficient.
    pub client_id: Option<ID>,
    /// Download-client provider type.
    pub client_type: String,
    /// Provider-issued download item ID.
    pub download_client_item_id: String,
    /// When true, suppress reacquisition after marking failure.
    pub skip_reacquire: Option<bool>,
}

#[derive(OneofObject, Clone)]
/// Union input selecting the acquisition scope of a tracked download.
pub enum QueueDownloadScopeInput {
    /// One target episode ID.
    Episode(ID),
    /// Multiple target episode IDs.
    EpisodeSet(Vec<ID>),
    /// One series-movie link ID.
    SeriesMovie(ID),
    /// One collection ID.
    Collection(ID),
    /// Whole-title marker; the boolean must indicate the title scope.
    Title(bool),
}

#[derive(InputObject)]
/// Assigns a tracked download to a title and an explicit acquisition scope.
pub struct AssignTrackedDownloadTitleInput {
    /// Download-client ID, or null when the provider identity is sufficient.
    pub client_id: Option<ID>,
    /// Download-client provider type.
    pub client_type: String,
    /// Provider-issued download item ID.
    pub download_client_item_id: String,
    /// Target title ID.
    pub title_id: ID,
    /// Target scope within the title.
    pub scope: QueueDownloadScopeInput,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[graphql(rename_items = "SCREAMING_SNAKE_CASE")]
/// Lifecycle of title hydration while a tracked download is being associated.
pub enum AddTitleHydrationStateValue {
    /// Hydration has been requested but is not complete.
    Pending,
    /// Hydration completed successfully.
    Complete,
    /// No hydration was needed.
    NotRequired,
}

#[derive(InputObject)]
/// Requests scanning one library and optionally supplies an import warmup session.
pub struct ScanLibraryInput {
    /// Target library ID.
    pub library_id: ID,
    /// Optional import warmup session ID to use during the scan.
    pub import_warmup_session_id: Option<ID>,
}

#[derive(SimpleObject, Clone)]
/// Counts produced by a library scan.
pub struct LibraryScanSummaryPayload {
    /// Number of files or records scanned.
    pub scanned: i32,
    /// Number matched to known titles.
    pub matched: i32,
    /// Number imported successfully.
    pub imported: i32,
    /// Number skipped by scan or import policy.
    pub skipped: i32,
    /// Number left unmatched.
    pub unmatched: i32,
}

#[derive(SimpleObject, Clone)]
/// Pending-import counts grouped by media facet.
pub struct PendingImportCountsPayload {
    /// Pending movie count.
    pub movie: i32,
    /// Pending series count.
    pub series: i32,
    /// Pending anime count.
    pub anime: i32,
}

#[derive(SimpleObject, Clone)]
/// Title-history event counts for one dashboard activity window.
pub struct ActivityWindowCountsPayload {
    /// Releases grabbed during the window.
    pub grabbed: i32,
    /// Existing files replaced by a better release during the window.
    pub upgraded: i32,
    /// Imports that completed during the window.
    pub imported: i32,
    /// Imports rejected as failed during the window; skipped imports are excluded.
    pub import_failed: i32,
    /// Downloads that failed during the window.
    pub download_failed: i32,
}

#[derive(SimpleObject, Clone)]
/// A trailing activity window together with the window immediately before it,
/// so callers can render each count with its period-over-period delta.
pub struct DashboardActivityStatsPayload {
    /// Counts for the trailing window ending at the time of the request.
    pub current: ActivityWindowCountsPayload,
    /// Counts for the equally long window immediately before the current one.
    pub previous: ActivityWindowCountsPayload,
}

#[derive(SimpleObject, Clone)]
/// Filesystem usage of the volume backing one library root folder.
pub struct StorageRootUsagePayload {
    /// Configured root-folder path as stored on the library.
    pub path: String,
    /// ID of the library that owns this root folder.
    pub library_id: ID,
    /// Name of the library that owns this root folder.
    pub library_name: String,
    /// Media facet of the library that owns this root folder.
    pub facet: MediaFacetValue,
    /// Bytes in use on the backing filesystem; null when it cannot be inspected.
    pub used_bytes: Option<Long>,
    /// Total bytes on the backing filesystem; null when it cannot be inspected.
    pub total_bytes: Option<Long>,
}

#[derive(SimpleObject, Clone)]
/// Media-request counts grouped by media facet.
pub struct MediaRequestCountsPayload {
    /// Movie request count.
    pub movie: i32,
    /// Series request count.
    pub series: i32,
    /// Anime request count.
    pub anime: i32,
}

#[derive(SimpleObject, Clone)]
/// Authorization-filtered counts used for application navigation indicators.
pub struct NavigationBadgeCountsPayload {
    /// Pending imports visible to the caller.
    pub pending_import_counts: PendingImportCountsPayload,
    /// Pending media requests visible to the caller.
    pub pending_media_request_counts: MediaRequestCountsPayload,
    /// Count of imports awaiting operator attention and visible to the caller.
    pub activity_import_count: i32,
    /// Count of available plugin updates visible to the caller.
    pub plugin_update_count: i32,
    /// Count of installed plugins blocked on this version of Scryer and visible to the caller.
    pub plugin_blocked_count: i32,
}

#[derive(SimpleObject, Clone)]
/// One metadata search attempt made while resolving a pending import.
pub struct PendingImportSearchAttemptPayload {
    /// Search query submitted.
    pub query: String,
    /// Number of metadata results returned.
    pub result_count: i32,
    /// Top result titles retained for diagnostics.
    pub top_results: Vec<String>,
    /// Human-readable attempt summary.
    pub summary: String,
}

/// Coarse bucket for why a pending import is awaiting resolution.
///
/// The free-text `reason` field remains the authoritative scanner code; this
/// enum is the stable grouping the dashboard filters on, so a new scanner code
/// surfaces as `OTHER` instead of breaking clients.
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum PendingImportReasonClassValue {
    /// Metadata lookup returned no candidates to choose from.
    Unmatched,
    /// Metadata lookup returned candidates but none could be accepted automatically.
    Ambiguous,
    /// The file's media metadata could not be read, so its quality is unknown.
    QualityUnknown,
    /// Any other scanner reason, including parse and folder-ownership problems.
    Other,
}

#[derive(SimpleObject, Clone)]
/// One unmatched library item awaiting import resolution.
pub struct PendingImportItemPayload {
    /// Pending-import item ID.
    pub id: ID,
    /// Library ID containing the item.
    pub library_id: ID,
    /// Media facet inferred for the item.
    pub facet: MediaFacetValue,
    /// Current pending-import status.
    pub status: PendingImportStatusValue,
    /// Bound title ID, or null before resolution.
    pub title_id: Option<ID>,
    /// Bound title name, or null before resolution.
    pub title_name: Option<String>,
    /// Bound title slug, or null before resolution.
    pub title_slug: Option<String>,
    /// Folder currently owned by the bound title, or null before resolution.
    pub title_folder_path: Option<String>,
    /// Display name derived from the unmatched item.
    pub display_name: String,
    /// Full source path.
    pub path: String,
    /// Containing folder path, or null when not applicable.
    pub folder_path: Option<String>,
    /// Metadata query suggested for resolution.
    pub query: String,
    /// Parsed year hint, or null when unavailable.
    pub year_hint: Option<i32>,
    /// Explanation for the pending state.
    pub reason: String,
    /// Coarse bucket for `reason`, stable across scanner reason-code changes.
    pub reason_class: PendingImportReasonClassValue,
    /// Metadata search attempts made for this item.
    pub search_attempts: Vec<PendingImportSearchAttemptPayload>,
    /// Size of the pending file; null for folder items and unreadable files.
    pub size_bytes: Option<Long>,
    /// When the scanner first recorded this item.
    pub created_at: DateTime<Utc>,
}

#[derive(SimpleObject, Clone)]
/// Offset-paginated pending-import connection.
pub struct PendingImportConnectionPayload {
    /// Items in the requested page; empty means no items matched.
    pub items: Vec<PendingImportItemPayload>,
    /// Total matching items before pagination.
    pub total_count: i32,
    /// Whether more matching items exist after this page.
    pub has_more: bool,
}

#[derive(InputObject)]
/// Resolves one pending import to a title and requested title metadata.
pub struct ResolvePendingImportInput {
    /// Pending-import item ID.
    pub pending_import_id: ID,
    /// Title metadata to associate with the item.
    pub title: AddTitleInput,
    /// Attach the item to a title that already carries this identity in the
    /// same library and facet instead of rejecting the request. Defaults to
    /// false, which keeps the "title already exists in this library" error.
    #[graphql(default = false)]
    pub attach_to_existing_title: bool,
}

#[derive(SimpleObject, Clone)]
/// Parsed file details and suggested episode bindings for a pending import.
pub struct PendingImportBindingFilePreviewPayload {
    /// Full file path.
    pub file_path: String,
    /// File name component.
    pub file_name: String,
    /// File size in bytes.
    pub size_bytes: Long,
    /// Parsed season number, or null when unavailable.
    pub parsed_season: Option<i32>,
    /// Parsed episode numbers; empty means none were detected.
    pub parsed_episodes: Vec<i32>,
    /// Parsed absolute episode numbers; empty means none were detected.
    pub parsed_absolute_numbers: Vec<i32>,
    /// Suggested episode IDs; empty means no suggestions were found.
    pub suggested_episode_ids: Vec<ID>,
}

#[derive(InputObject)]
/// Binds one pending import to an optional collection and episode IDs.
pub struct BindPendingImportInput {
    /// Pending-import item ID.
    pub pending_import_id: ID,
    /// Collection ID, or null when binding episodes directly.
    pub collection_id: Option<ID>,
    /// Episode IDs to bind; empty means no explicit episode list.
    pub episode_ids: Vec<ID>,
}

#[derive(SimpleObject, Clone)]
/// Result of ignoring one pending import.
pub struct IgnorePendingImportPayload {
    /// Pending-import item ID.
    pub id: ID,
    /// Status after the ignore operation.
    pub status: PendingImportStatusValue,
}

#[derive(SimpleObject, Clone)]
/// Result of requesting cancellation for an acquisition search.
pub struct CancelAcquisitionSearchPayload {
    /// Acquisition-search job ID.
    pub id: ID,
    /// True when cancellation was accepted; false when the search was already terminal.
    pub accepted: bool,
}

#[derive(SimpleObject, Clone)]
/// Result of requesting cancellation for a library scan.
pub struct CancelLibraryScanPayload {
    /// Library-scan session ID.
    pub session_id: ID,
    /// True when cancellation was accepted; false when the scan was already terminal.
    pub accepted: bool,
}

#[derive(SimpleObject, Clone)]
/// Read-only media deletion preview with confirmation and sample-path details.
pub struct DeletePreviewPayload {
    /// Stable preview fingerprint required to apply the same plan.
    pub fingerprint: String,
    /// Total files selected by the preview.
    pub total_file_count: i32,
    /// Selected media-file count.
    pub media_count: i32,
    /// Selected subtitle-file count.
    pub subtitle_count: i32,
    /// Selected image-file count.
    pub image_count: i32,
    /// Selected other-file count.
    pub other_count: i32,
    /// Selected directory count.
    pub directory_count: i32,
    /// Whether typed confirmation is required before applying the plan.
    pub requires_typed_confirmation: bool,
    /// Required confirmation text, or null when typed confirmation is not required.
    pub typed_confirmation_prompt: Option<String>,
    /// Human-readable preview target label.
    pub target_label: String,
    /// Sample paths from the selected files and directories.
    pub sample_paths: Vec<String>,
}

#[derive(SimpleObject, Clone)]
/// Per-title result within a multi-title deletion preview.
pub struct DeleteTitlePreviewResultPayload {
    /// Target title ID.
    pub title_id: ID,
    /// Deletion preview, or null when preview generation failed.
    pub preview: Option<DeletePreviewPayload>,
    /// Error message, or null when preview generation succeeded.
    pub error: Option<String>,
}

#[derive(SimpleObject, Clone)]
/// Combined deletion preview for multiple titles.
pub struct DeleteTitlesPreviewPayload {
    /// Aggregate preview across all successful title targets.
    pub preview: DeletePreviewPayload,
    /// Per-title results.
    pub items: Vec<DeleteTitlePreviewResultPayload>,
    /// Number of title targets that failed preview generation.
    pub failed_count: i32,
}

#[derive(SimpleObject, Clone)]
/// Per-file result within a multi-episode media-file deletion preview.
pub struct DeleteEpisodeFilePreviewResultPayload {
    /// Media-file identity that would be deleted.
    pub file_id: ID,
    /// First requested episode the media file is linked to.
    pub episode_id: ID,
    /// Every requested episode the media file is linked to. A multi-episode
    /// file is deleted once but covers each of these episodes.
    pub episode_ids: Vec<ID>,
    /// Deletion preview for this file, or null when preview generation failed.
    pub preview: Option<DeletePreviewPayload>,
    /// Error message, or null when preview generation succeeded.
    pub error: Option<String>,
}

#[derive(SimpleObject, Clone)]
/// Combined deletion preview for the media files of several episodes of one title.
pub struct DeleteEpisodeFilesPreviewPayload {
    /// Aggregate preview across all resolved media files.
    pub preview: DeletePreviewPayload,
    /// Per-file results, ordered by media-file identity.
    pub items: Vec<DeleteEpisodeFilePreviewResultPayload>,
    /// Number of media files resolved from the requested episodes.
    pub file_count: i32,
    /// Number of media files that failed preview generation.
    pub failed_count: i32,
}

#[derive(SimpleObject, Clone)]
/// Accepted media-file IDs and background job information for a multi-episode
/// media-file deletion request.
pub struct DeleteEpisodeFilesPayload {
    /// Background job run tracking the deletion work.
    pub job_run: JobRunPayload,
    /// Media-file identities accepted for processing.
    pub accepted_file_ids: Vec<ID>,
}

#[derive(SimpleObject, Clone)]
/// Accepted title IDs and background job information for a deletion request.
pub struct DeleteTitlesPayload {
    /// Background job run tracking the deletion work.
    pub job_run: JobRunPayload,
    /// Title IDs accepted for processing.
    pub accepted_title_ids: Vec<ID>,
}

#[derive(SimpleObject, Clone)]
/// One media rename plan item with current and proposed paths and decision details.
pub struct MediaRenamePlanItemPayload {
    /// Collection ID, or null when not part of a collection.
    pub collection_id: Option<ID>,
    /// Series-movie link IDs associated with the item.
    pub series_movie_link_ids: Vec<ID>,
    /// Current filesystem path.
    pub current_path: String,
    /// Proposed filesystem path, or null when no rename is possible.
    pub proposed_path: Option<String>,
    /// Normalized filename, or null when metadata is insufficient.
    pub normalized_filename: Option<String>,
    /// Whether the proposed path collides with another path.
    pub collision: bool,
    /// Machine-readable reason for the plan decision.
    pub reason_code: String,
    /// Planned write action.
    pub write_action: String,
    /// Source file size in bytes, or null when unavailable.
    pub source_size_bytes: Option<Long>,
    /// Source modification time as Unix milliseconds, or null when unavailable.
    pub source_mtime_unix_ms: Option<Long>,
}

#[derive(SimpleObject, Clone)]
/// Read-only media rename plan and stable fingerprint for later application.
pub struct MediaRenamePlanPayload {
    /// Media facet covered by the plan.
    pub facet: MediaFacetValue,
    /// Target title ID, or null when the plan covers the whole facet.
    pub title_id: Option<ID>,
    /// Rename template used to generate proposed paths.
    pub template: String,
    /// Collision policy used by the plan.
    pub collision_policy: RenameCollisionPolicyValue,
    /// Missing-metadata policy used by the plan.
    pub missing_metadata_policy: RenameMissingMetadataPolicyValue,
    /// Stable fingerprint required to apply this exact plan.
    pub fingerprint: String,
    /// Total plan-item count.
    pub total: i32,
    /// Number of items eligible for rename.
    pub renamable: i32,
    /// Number of items already matching the desired path.
    pub noop: i32,
    /// Number of collision items.
    pub conflicts: i32,
    /// Number of items with planning errors.
    pub errors: i32,
    /// Number of titles with nothing to move whose recorded folder is not a
    /// title folder; applying the plan records the planned folder for them.
    pub folder_repairs: i32,
    /// Plan items in deterministic service order.
    pub items: Vec<MediaRenamePlanItemPayload>,
}

#[derive(SimpleObject, Clone)]
/// Result for one applied media rename plan item.
pub struct MediaRenameApplyItemPayload {
    /// Collection ID, or null when not part of a collection.
    pub collection_id: Option<ID>,
    /// Series-movie link IDs associated with the item.
    pub series_movie_link_ids: Vec<ID>,
    /// Path before the apply operation.
    pub current_path: String,
    /// Planned path, or null when no rename was proposed.
    pub proposed_path: Option<String>,
    /// Final path after application, or null when not moved.
    pub final_path: Option<String>,
    /// Write action attempted.
    pub write_action: String,
    /// Apply status.
    pub status: String,
    /// Machine-readable reason for the outcome.
    pub reason_code: String,
    /// Error message, or null when the item succeeded or was skipped without error.
    pub error_message: Option<String>,
}

#[derive(SimpleObject, Clone)]
/// Aggregate result of applying a previously generated rename plan.
pub struct MediaRenameApplyPayload {
    /// Fingerprint of the plan that was applied.
    pub plan_fingerprint: String,
    /// Total plan-item count.
    pub total: i32,
    /// Number of items applied.
    pub applied: i32,
    /// Number of items skipped.
    pub skipped: i32,
    /// Number of items that failed.
    pub failed: i32,
    /// Per-item apply results.
    pub items: Vec<MediaRenameApplyItemPayload>,
}

#[derive(InputObject, Clone)]
/// An external identifier supplied with a title or media request.
pub struct ExternalIdInput {
    /// Provider namespace for the identifier, such as TVDB or IMDb.
    pub source: String,
    /// Entity kind the identifier names at that source, such as `movie`,
    /// `series`, `anime` or `title`. Omit it when the kind is unknown: the
    /// identifier then matches a stored id of any kind.
    pub kind: Option<String>,
    /// Provider-issued identifier value.
    pub value: String,
}

impl ExternalIdInput {
    /// Convert to the domain id, preserving the caller-supplied entity kind.
    ///
    /// Dropping `kind` here would hand the title store a wildcard id: a movie
    /// id whose source/value already exists on a series would resolve to that
    /// series instead of creating a distinct title, and the input would read
    /// back with a null kind.
    pub fn into_domain(self) -> scryer_domain::ExternalId {
        match self.kind {
            Some(kind) => scryer_domain::ExternalId::with_kind(self.source, kind, self.value),
            None => scryer_domain::ExternalId::new(self.source, self.value),
        }
    }
}

#[derive(InputObject, Clone)]
/// Optional title settings used when creating or updating a title.
pub struct TitleOptionsInput {
    /// Search-language override: omitted preserves, null inherits the library, empty searches only the primary name and custom aliases.
    pub search_languages: MaybeUndefined<Vec<String>>,
    /// User-owned search aliases: omitted preserves, null or empty clears. Metadata refresh preserves these aliases.
    pub search_aliases: MaybeUndefined<Vec<String>>,
