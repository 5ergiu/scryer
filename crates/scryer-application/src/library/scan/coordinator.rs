use crate::domain_events::new_library_scan_domain_event;
use crate::{
    AppResult, AppUseCase, Id, LibraryScanMode, LibraryScanSession, LibraryScanSummary,
    library_scan_progress::reduce_library_scan_projection_event,
};
use scryer_domain::{
    DomainEventFilter, DomainEventPayload, DomainEventType, LibraryScanCanceledEventData,
    LibraryScanCompletedEventData, LibraryScanDeltaRecordedEventData, LibraryScanFailedEventData,
    LibraryScanProgressedEventData, LibraryScanStartedEventData, LibraryScanSummaryEventData,
    MediaFacet, NewDomainEvent,
};
use tracing::{debug, trace, warn};

const LIBRARY_SCAN_TRACKER_EVENT_TYPES: &[DomainEventType] = &[
    DomainEventType::LibraryScanStarted,
    DomainEventType::LibraryScanTitleDiscovered,
    DomainEventType::LibraryScanDeltaRecorded,
    DomainEventType::LibraryScanProgressed,
    DomainEventType::LibraryScanCompleted,
    DomainEventType::LibraryScanCanceled,
    DomainEventType::LibraryScanFailed,
];

#[derive(Clone)]
pub(crate) struct LibraryScanCoordinator {
    app: AppUseCase,
    session_id: String,
    facet: Option<MediaFacet>,
}

impl LibraryScanCoordinator {
    pub(crate) async fn start_for_library(
        app: AppUseCase,
        facet: MediaFacet,
        library_id: Option<String>,
        mode: LibraryScanMode,
        session_id_override: Option<String>,
    ) -> AppResult<(Self, LibraryScanSession)> {
        let session_id = session_id_override.unwrap_or_else(|| Id::new().0);
        let session = app
            .runtime
            .library
            .library_scan_tracker
            .start_session_with_id_for_library(session_id, facet.clone(), library_id, mode)
            .await?;
        let coordinator = Self::with_facet(app, session.session_id.clone(), facet);
        coordinator.publish_started(&session).await;
        Ok((coordinator, session))
    }

    pub(crate) fn new(app: AppUseCase, session_id: impl Into<String>) -> Self {
        Self {
            app,
            session_id: session_id.into(),
            facet: None,
        }
    }

    pub(crate) fn with_facet(
        app: AppUseCase,
        session_id: impl Into<String>,
        facet: MediaFacet,
    ) -> Self {
        Self {
            app,
            session_id: session_id.into(),
            facet: Some(facet),
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) async fn publish_started(&self, session: &LibraryScanSession) {
        let _ = self
            .app
            .append_domain_event(new_library_scan_domain_event(
                None,
                session.session_id.clone(),
                session.facet.clone(),
                DomainEventPayload::LibraryScanStarted(LibraryScanStartedEventData {
                    session_id: session.session_id.clone(),
                    library_id: session.library_id.clone(),
                    mode: session.mode.as_str().to_string(),
                }),
            ))
            .await;
    }

    pub(crate) async fn publish_progress(&self) {
        if let Some(event) = self.prepare_progress_event().await {
            let _ = self.app.append_domain_event(event).await;
        }
    }

    /// Builds the coalesced progress event `publish_progress` would append,
    /// without appending it, so a caller that just recorded deltas can put
    /// every event of one flush into a single transaction.
    async fn prepare_progress_event(&self) -> Option<NewDomainEvent> {
        let Some(session) = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .get_session(self.session_id())
            .await
        else {
            trace!(
                session_id = %self.session_id,
                "library scan coordinator publish_progress skipped for inactive session"
            );
            return None;
        };

        if session.is_ready_to_complete() {
            trace!(
                session_id = %self.session_id,
                "library scan coordinator publish_progress deferred to completion path"
            );
            return None;
        }

        Some(coalesced_library_scan_state_event(&session))
    }

    /// Records `delta` and publishes the coalesced progress state, appending
    /// both events in one transaction. Same events, same contents, same order
    /// as `record_delta` followed by `publish_progress`; only the commit
    /// boundary between them is removed.
    pub(crate) async fn record_delta_and_publish_progress(
        &self,
        delta: LibraryScanDeltaRecordedEventData,
    ) {
        self.record_deltas_and_publish_progress(vec![delta]).await;
    }

    /// Batch form of [`Self::record_delta_and_publish_progress`]. Deltas are
    /// applied to the tracker in the given order, exactly as the equivalent
    /// sequence of `record_delta` calls would.
    pub(crate) async fn record_deltas_and_publish_progress(
        &self,
        deltas: Vec<LibraryScanDeltaRecordedEventData>,
    ) {
        let mut events = Vec::with_capacity(deltas.len() + 1);
        for delta in deltas {
            if let Some(event) = self.prepare_delta_event(delta).await {
                events.push(event);
            }
        }
        if let Some(event) = self.prepare_progress_event().await {
            events.push(event);
        }
        if events.is_empty() {
            return;
        }
        let _ = self.app.append_domain_events(events).await;
    }

    pub(crate) async fn register_discovery_batch(
        &self,
        discovered_count: usize,
        track_file_total: bool,
    ) {
        if discovered_count == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.found_titles_delta = discovered_count as i64;
        if track_file_total {
            delta.file_total_delta = discovered_count as i64;
        }
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_metadata_total_known(&self) {
        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.metadata_total_known = Some(true);
        self.record_delta(delta).await;
    }

    pub(crate) async fn add_metadata_total(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.metadata_total_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn add_file_total(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_total_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_file_total_known(&self) {
        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_total_known = Some(true);
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_metadata_completed(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.metadata_completed_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_metadata_failed(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.metadata_failed_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_file_completed(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_completed_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_file_failed(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_failed_delta = additional as i64;
        self.record_delta(delta).await;
    }

    /// Records the per-file completion/failure deltas of one flush and the
    /// coalesced progress state in a single transaction. Equivalent to
    /// `mark_file_completed` then `mark_file_failed` then `publish_progress`.
    pub(crate) async fn mark_file_progress_and_publish(&self, completed: usize, failed: usize) {
        let mut deltas = Vec::with_capacity(2);
        if completed > 0 {
            let mut delta = empty_scan_delta(self.session_id.clone());
            delta.file_completed_delta = completed as i64;
            deltas.push(delta);
        }
        if failed > 0 {
            let mut delta = empty_scan_delta(self.session_id.clone());
            delta.file_failed_delta = failed as i64;
            deltas.push(delta);
        }
        self.record_deltas_and_publish_progress(deltas).await;
    }

    /// `mark_file_failed` followed by `publish_progress`, in one transaction.
    pub(crate) async fn mark_file_failed_and_publish(&self, additional: usize) {
        if additional == 0 {
            self.publish_progress().await;
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_failed_delta = additional as i64;
        self.record_delta_and_publish_progress(delta).await;
    }

    /// `add_file_total` followed by `publish_progress`, in one transaction.
    pub(crate) async fn add_file_total_and_publish(&self, additional: usize) {
        if additional == 0 {
            self.publish_progress().await;
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_total_delta = additional as i64;
        self.record_delta_and_publish_progress(delta).await;
    }

    /// `mark_file_total_known` followed by `publish_progress`, in one transaction.
    pub(crate) async fn mark_file_total_known_and_publish(&self) {
        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.file_total_known = Some(true);
        self.record_delta_and_publish_progress(delta).await;
    }

    /// `mark_metadata_completed` followed by `publish_progress`, in one transaction.
    pub(crate) async fn mark_metadata_completed_and_publish(&self, additional: usize) {
        if additional == 0 {
            self.publish_progress().await;
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.metadata_completed_delta = additional as i64;
        self.record_delta_and_publish_progress(delta).await;
    }

    /// `mark_title_match_completed` followed by `publish_progress`, in one transaction.
    pub(crate) async fn mark_title_match_completed_and_publish(&self, additional: usize) {
        if additional == 0 {
            self.publish_progress().await;
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.title_match_completed_delta = additional as i64;
        self.record_delta_and_publish_progress(delta).await;
    }

    pub(crate) async fn mark_discovery_complete(&self, track_file_total: bool) {
        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.title_match_total_known = Some(true);
        if track_file_total {
            delta.file_total_known = Some(true);
        }
        self.record_delta(delta).await;
    }

    pub(crate) async fn mark_title_match_completed(&self, additional: usize) {
        if additional == 0 {
            return;
        }

        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.title_match_completed_delta = additional as i64;
        self.record_delta(delta).await;
    }

    pub(crate) async fn set_summary(&self, summary: LibraryScanSummary) {
        let mut delta = empty_scan_delta(self.session_id.clone());
        delta.summary = Some(library_scan_summary_event_data(&summary));
        self.record_delta(delta).await;
    }

    pub(crate) async fn maybe_complete(&self) {
        let Some(session) = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .complete_if_finished(self.session_id())
            .await
        else {
            trace!(
                session_id = %self.session_id,
                "library scan coordinator maybe_complete found no terminal session"
            );
            return;
        };

        self.app
            .clear_library_scan_cancellation_token(self.session_id())
            .await;
        publish_coalesced_library_scan_state(&self.app, &session).await;
    }

    pub(crate) async fn fail(&self) {
        let failed_session = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .fail_session(self.session_id())
            .await;
        self.app
            .clear_library_scan_cancellation_token(self.session_id())
            .await;
        let facet = match failed_session.as_ref().map(|session| session.facet.clone()) {
            Some(facet) => facet,
            None => {
                let Some(facet) = self.resolve_facet().await else {
                    warn!(session_id = %self.session_id, "failed to resolve facet for library scan failure event");
                    return;
                };
                facet
            }
        };

        let _ = self
            .app
            .append_domain_event(self.scan_event(
                facet,
                DomainEventPayload::LibraryScanFailed(LibraryScanFailedEventData {
                    session_id: self.session_id.clone(),
                    error_message: "library scan failed".to_string(),
                }),
            ))
            .await;
    }

    pub(crate) async fn cancel(&self) {
        let canceled_session = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .cancel_session(self.session_id())
            .await;
        self.app
            .clear_library_scan_cancellation_token(self.session_id())
            .await;
        let Some(session) = canceled_session else {
            trace!(
                session_id = %self.session_id,
                "library scan coordinator cancel skipped for inactive session"
            );
            return;
        };

        let _ = self
            .app
            .append_domain_event(self.scan_event(
                session.facet.clone(),
                DomainEventPayload::LibraryScanCanceled(library_scan_canceled_event_data(&session)),
            ))
            .await;
    }

    fn scan_event(&self, facet: MediaFacet, payload: DomainEventPayload) -> NewDomainEvent {
        new_library_scan_domain_event(None, self.session_id.clone(), facet, payload)
    }

    async fn record_delta(&self, delta: LibraryScanDeltaRecordedEventData) {
        if let Some(event) = self.prepare_delta_event(delta).await {
            let _ = self.app.append_domain_event(event).await;
        }
    }

    /// Applies `delta` to the tracker and builds the delta event `record_delta`
    /// would append, without appending it.
    async fn prepare_delta_event(
        &self,
        delta: LibraryScanDeltaRecordedEventData,
    ) -> Option<NewDomainEvent> {
        if !delta_has_effect(&delta) {
            return None;
        }

        let Some(snapshot) = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .apply_delta(self.session_id(), &delta)
            .await
        else {
            warn!(session_id = %self.session_id, "ignored library scan delta for inactive session");
            return None;
        };

        debug!(
            session_id = %self.session_id,
            facet = %snapshot.facet.as_str(),
            found_titles_total = ?delta.found_titles_total,
            found_titles_delta = delta.found_titles_delta,
            title_match_completed_delta = delta.title_match_completed_delta,
            title_match_failed_delta = delta.title_match_failed_delta,
            title_match_total_known = ?delta.title_match_total_known,
            metadata_total_delta = delta.metadata_total_delta,
            metadata_completed_delta = delta.metadata_completed_delta,
            metadata_failed_delta = delta.metadata_failed_delta,
            metadata_total_known = ?delta.metadata_total_known,
            file_total_delta = delta.file_total_delta,
            file_completed_delta = delta.file_completed_delta,
            file_failed_delta = delta.file_failed_delta,
            file_total_known = ?delta.file_total_known,
            summary_present = delta.summary.is_some(),
            summary_is_delta = delta.summary_is_delta,
            "library scan coordinator recording delta"
        );

        Some(self.scan_event(
            snapshot.facet,
            DomainEventPayload::LibraryScanDeltaRecorded(delta),
        ))
    }

    async fn resolve_facet(&self) -> Option<MediaFacet> {
        if let Some(facet) = self.facet.clone() {
            return Some(facet);
        }

        if let Some(session) = self
            .app
            .runtime
            .library
            .library_scan_tracker
            .get_session(self.session_id())
            .await
        {
            return Some(session.facet);
        }

        load_library_scan_session_facet(&self.app, self.session_id()).await
    }
}

/// A resumable fold of the stored library-scan events for one session.
///
/// [`load_projected_library_scan_session`] is a one-shot fold from sequence 0.
/// A caller that polls the same session in a loop keeps one of these instead
/// and folds only the events appended since its previous pass: the projection
/// is a left fold over a prefix-extending event sequence, so folding
/// `[0..a]` then `[a..b]` lands on exactly the state a fresh fold over
/// `[0..b]` produces, and the returned snapshot is the same value.
///
/// Replaying from 0 on every pass is what made a running scan quadratic: each
/// coalesced progress publish woke the waiter, and the waiter re-read every
/// scan event the run had emitted so far.
pub(crate) struct LibraryScanProjectionReplay {
    session_id: String,
    after_sequence: i64,
    sessions: std::collections::HashMap<String, LibraryScanSession>,
    last_snapshot: Option<LibraryScanSession>,
}

impl LibraryScanProjectionReplay {
    pub(crate) fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            after_sequence: 0,
            sessions: std::collections::HashMap::new(),
            last_snapshot: None,
        }
    }

    /// Fold every event appended since the last call and return the session
    /// snapshot as of now, or `None` while nothing has projected one yet.
    pub(crate) async fn advance(
        &mut self,
        app: &AppUseCase,
    ) -> AppResult<Option<LibraryScanSession>> {
        loop {
            let batch = app
                .services
                .events
                .domain_events
                .list(&DomainEventFilter {
                    event_types: Some(LIBRARY_SCAN_TRACKER_EVENT_TYPES.to_vec()),
                    // Every library-scan event is written to the stream named
                    // by its own session id (`new_library_scan_domain_event`),
                    // so this is the same scoping the `library_scan_event_session_id`
                    // check below applies - just done by the database instead
                    // of after reading every other session's events off disk.
                    stream_id: Some(self.session_id.clone()),
                    after_sequence: Some(self.after_sequence),
                    limit: 500,
                    ..DomainEventFilter::default()
                })
                .await?;
            if batch.is_empty() {
                break;
            }

            self.after_sequence = batch
                .last()
                .map(|event| event.sequence)
                .unwrap_or(self.after_sequence);
            let count = batch.len();
            for event in batch {
                if library_scan_event_session_id(&event.payload) == Some(self.session_id.as_str()) {
                    self.last_snapshot =
                        reduce_library_scan_projection_event(&mut self.sessions, &event);
                }
            }
            if count < 500 {
                break;
            }
        }

        Ok(self.last_snapshot.clone())
    }
}

pub(crate) async fn load_projected_library_scan_session(
    app: &AppUseCase,
    session_id: &str,
) -> AppResult<Option<LibraryScanSession>> {
    LibraryScanProjectionReplay::new(session_id)
        .advance(app)
        .await
}

async fn publish_coalesced_library_scan_state(app: &AppUseCase, session: &LibraryScanSession) {
    let _ = app
        .append_domain_event(coalesced_library_scan_state_event(session))
        .await;
}

/// Builds the coalesced progress/completion event for `session`, including the
/// diagnostic logging the publishing path emits, without appending it.
fn coalesced_library_scan_state_event(session: &LibraryScanSession) -> NewDomainEvent {
    let ready_to_complete = session.is_ready_to_complete();
    if ready_to_complete {
        debug!(
            session_id = %session.session_id,
            facet = %session.facet.as_str(),
            status = %session.status.as_str(),
            found_titles = session.found_titles,
            title_match_total = session.title_match_progress.total,
            title_match_completed = session.title_match_progress.completed,
            title_match_failed = session.title_match_progress.failed,
            metadata_total = session.metadata_progress.total,
            metadata_completed = session.metadata_progress.completed,
            metadata_failed = session.metadata_progress.failed,
            file_total = session.file_progress.total,
            file_completed = session.file_progress.completed,
            file_failed = session.file_progress.failed,
            summary_present = session.summary.is_some(),
            emitted_event_type = "library_scan_completed",
            "library scan coordinator publishing coalesced state"
        );
    } else {
        trace!(
            session_id = %session.session_id,
            facet = %session.facet.as_str(),
            status = %session.status.as_str(),
            found_titles = session.found_titles,
            title_match_total_known = session.title_match_total_known,
            title_match_total = session.title_match_progress.total,
            title_match_completed = session.title_match_progress.completed,
            title_match_failed = session.title_match_progress.failed,
            metadata_total_known = session.metadata_total_known,
            metadata_total = session.metadata_progress.total,
            metadata_completed = session.metadata_progress.completed,
            metadata_failed = session.metadata_progress.failed,
            file_total_known = session.file_total_known,
            file_total = session.file_progress.total,
            file_completed = session.file_progress.completed,
            file_failed = session.file_progress.failed,
            summary_present = session.summary.is_some(),
            ready_to_complete,
            emitted_event_type = "library_scan_progressed",
            "library scan coordinator publishing coalesced state"
        );
    }

    let payload = if ready_to_complete {
        DomainEventPayload::LibraryScanCompleted(library_scan_completed_event_data(session))
    } else {
        DomainEventPayload::LibraryScanProgressed(library_scan_progressed_event_data(session))
    };

    new_library_scan_domain_event(
        None,
        session.session_id.clone(),
        session.facet.clone(),
        payload,
    )
}

async fn load_library_scan_session_facet(app: &AppUseCase, session_id: &str) -> Option<MediaFacet> {
    let mut after_sequence = 0i64;

    loop {
        let batch = app
            .services
            .events
            .domain_events
            .list(&DomainEventFilter {
                event_types: Some(LIBRARY_SCAN_TRACKER_EVENT_TYPES.to_vec()),
                stream_id: Some(session_id.to_string()),
                after_sequence: Some(after_sequence),
                limit: 500,
                ..DomainEventFilter::default()
            })
            .await
            .ok()?;
        if batch.is_empty() {
            return None;
        }

        after_sequence = batch
            .last()
            .map(|event| event.sequence)
            .unwrap_or(after_sequence);

        for event in batch {
            if library_scan_event_session_id(&event.payload) == Some(session_id)
                && let Some(facet) = event.facet
            {
                return Some(facet);
            }
        }
    }
}

fn empty_scan_delta(session_id: String) -> LibraryScanDeltaRecordedEventData {
    LibraryScanDeltaRecordedEventData {
        session_id,
        found_titles_total: None,
        found_titles_delta: 0,
        title_match_completed_delta: 0,
        title_match_failed_delta: 0,
        title_match_total_known: None,
        metadata_total_delta: 0,
        metadata_completed_delta: 0,
        metadata_failed_delta: 0,
        metadata_total_known: None,
        file_total_delta: 0,
        file_completed_delta: 0,
        file_failed_delta: 0,
        file_total_known: None,
        summary: None,
        summary_is_delta: false,
    }
}

fn delta_has_effect(delta: &LibraryScanDeltaRecordedEventData) -> bool {
    delta.found_titles_total.is_some()
        || delta.found_titles_delta != 0
        || delta.title_match_completed_delta != 0
        || delta.title_match_failed_delta != 0
        || delta.title_match_total_known.is_some()
        || delta.metadata_total_delta != 0
        || delta.metadata_completed_delta != 0
        || delta.metadata_failed_delta != 0
        || delta.metadata_total_known.is_some()
        || delta.file_total_delta != 0
        || delta.file_completed_delta != 0
        || delta.file_failed_delta != 0
        || delta.file_total_known.is_some()
        || delta.summary.is_some()
}

fn library_scan_progressed_event_data(
    session: &LibraryScanSession,
) -> LibraryScanProgressedEventData {
    LibraryScanProgressedEventData {
        session_id: session.session_id.clone(),
        status: session.status.as_str().to_string(),
        found_titles: session.found_titles as i64,
        title_match_completed: session.title_match_progress.completed as i64,
        title_match_total_known: session.title_match_total_known,
        titles_completed: session.metadata_progress.completed as i64,
        titles_total: session
            .metadata_total_known
            .then_some(session.metadata_progress.total as i64),
        files_completed: session.file_progress.completed as i64,
        files_total: session
            .file_total_known
            .then_some(session.file_progress.total as i64),
        warning_message: session.warning_message.clone(),
    }
}

fn library_scan_completed_event_data(
    session: &LibraryScanSession,
) -> LibraryScanCompletedEventData {
    LibraryScanCompletedEventData {
        session_id: session.session_id.clone(),
        status: session.completion_status().as_str().to_string(),
        found_titles: session.found_titles as i64,
        title_match_completed: session.title_match_progress.completed as i64,
        title_match_total_known: true,
        titles_completed: session.metadata_progress.completed as i64,
        titles_total: Some(session.metadata_progress.total as i64),
        files_completed: session.file_progress.completed as i64,
        files_total: Some(session.file_progress.total as i64),
        summary: session
            .summary
            .as_ref()
            .map(library_scan_summary_event_data),
        warning_message: session.warning_message.clone(),
    }
}

fn library_scan_canceled_event_data(session: &LibraryScanSession) -> LibraryScanCanceledEventData {
    LibraryScanCanceledEventData {
        session_id: session.session_id.clone(),
        status: session.status.as_str().to_string(),
        found_titles: session.found_titles as i64,
        title_match_completed: session.title_match_progress.completed as i64,
        title_match_total_known: session.title_match_total_known,
        titles_completed: session.metadata_progress.completed as i64,
        titles_total: Some(session.metadata_progress.total as i64),
        files_completed: session.file_progress.completed as i64,
        files_total: Some(session.file_progress.total as i64),
        summary: session
            .summary
            .as_ref()
            .map(library_scan_summary_event_data),
    }
}

fn library_scan_event_session_id(payload: &DomainEventPayload) -> Option<&str> {
    match payload {
        DomainEventPayload::LibraryScanStarted(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanTitleDiscovered(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanDeltaRecorded(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanProgressed(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanCompleted(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanCanceled(data) => Some(&data.session_id),
        DomainEventPayload::LibraryScanFailed(data) => Some(&data.session_id),
        _ => None,
    }
}

fn library_scan_summary_event_data(summary: &LibraryScanSummary) -> LibraryScanSummaryEventData {
    LibraryScanSummaryEventData {
        scanned: summary.scanned as i64,
        matched: summary.matched as i64,
        imported: summary.imported as i64,
        skipped: summary.skipped as i64,
        unmatched: summary.unmatched as i64,
    }
}
