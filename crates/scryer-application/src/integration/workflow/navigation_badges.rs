/// How long the badge facts may stand between refreshes.
///
/// The web polls `navigationBadgeCounts` every 30 seconds, so counting on the
/// same cadence answers each poll with facts at most one poll old. The half
/// that actually moves while a download runs — the import attention count — is
/// refreshed as soon as the queue snapshot changes rather than waiting for the
/// interval.
pub const NAVIGATION_BADGE_FACTS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

impl AppUseCase {
    /// Count the badge facts and publish them for the request path to read.
    ///
    /// Every read here is unfiltered: the counts are kept per library so the
    /// per-actor permission filter can still be applied in memory, without a
    /// per-actor query.
    pub async fn refresh_navigation_badge_facts(&self) -> AppResult<()> {
        let candidate_library_ids = self.permission_candidate_library_ids(None).await?;
        let pending_imports = self.pending_import_counts_by_library().await?;
        let media_requests = self
            .pending_media_request_counts_by_library(&candidate_library_ids)
            .await?;
        let (_, model) = self.current_download_queue_read_model().await?;
        // The same two filters `count_download_import_items` applies, in the
        // same order; only the permission filter is deferred to the request.
        let import_attention = model
            .items
            .iter()
            .filter(|item| is_history_download_state(&item.state))
            .filter(|item| matches_download_import_filter(item, DownloadImportFilter::Attention))
            .map(|item| {
                item.title_id
                    .as_deref()
                    .and_then(|title_id| model.title_library_ids.get(title_id).cloned())
            })
            .collect::<Vec<_>>();
        let plugins = self.build_available_plugins().await?;

        let facts = crate::services::NavigationBadgeFacts {
            candidate_library_ids,
            pending_imports,
            media_requests,
            import_attention,
            plugin_update_count: plugins
                .iter()
                .filter(|plugin| plugin.update_available)
                .count() as i64,
            plugin_blocked_count: plugins
                .iter()
                .filter(|plugin| plugin.blocked_reason.is_some())
                .count() as i64,
        };
        self.runtime
            .integrations
            .navigation_badge_facts
            .current
            .write()
            .await
            .replace(std::sync::Arc::new(facts));
        Ok(())
    }

    /// The published facts.
    ///
    /// Only a request that arrives before the refresh loop's first pass finds
    /// nothing published; that one counts them itself, single-flighted through
    /// the build lock the way the download-queue read model is. Afterwards the
    /// request path never reads a store for these numbers, however stale they
    /// are — a poll must not pay for the archive.
    async fn navigation_badge_facts(
        &self,
    ) -> AppResult<std::sync::Arc<crate::services::NavigationBadgeFacts>> {
        let cache = &self.runtime.integrations.navigation_badge_facts;
        if let Some(facts) = cache.current.read().await.clone() {
            return Ok(facts);
        }
        let _build_guard = cache.build_lock.lock().await;
        if let Some(facts) = cache.current.read().await.clone() {
            return Ok(facts);
        }
        self.refresh_navigation_badge_facts().await?;
        Ok(cache
            .current
            .read()
            .await
            .clone()
            .unwrap_or_else(|| {
                std::sync::Arc::new(crate::services::NavigationBadgeFacts::default())
            }))
    }

    /// The navigation badge counts this actor may see.
    pub async fn navigation_badge_counts(
        &self,
        actor: &User,
    ) -> AppResult<crate::types::NavigationBadgeCounts> {
        let facts = self.navigation_badge_facts().await?;
        let authorization = self.authorization_for_actor(actor).await?;
        let libraries_for = |permission: scryer_domain::LibraryPermission| {
            facts
                .candidate_library_ids
                .iter()
                .filter(|library_id| {
                    crate::authorization::effective_library_permission(
                        &authorization,
                        library_id,
                        permission,
                    )
                })
                .cloned()
                .collect::<HashSet<_>>()
        };
        let resolvable = libraries_for(scryer_domain::LibraryPermission::ResolveImports);
        let manageable = libraries_for(scryer_domain::LibraryPermission::ManageTitles);
        let can_view_operational_history =
            authorization.has_app_permission(scryer_domain::AppPermission::ManageSystemSettings);

        let mut pending_imports = PendingImportCounts::default();
        if !resolvable.is_empty() {
            for (library_id, counts) in &facts.pending_imports {
                if resolvable.contains(library_id) {
                    pending_imports.movie += counts.movie;
                    pending_imports.series += counts.series;
                    pending_imports.anime += counts.anime;
                }
            }
        }

        let mut pending_media_requests = MediaRequestCounts::default();
        if !manageable.is_empty() {
            for (library_id, counts) in &facts.media_requests {
                if manageable.contains(library_id) {
                    pending_media_requests.movie += counts.movie;
                    pending_media_requests.series += counts.series;
                    pending_media_requests.anime += counts.anime;
                }
            }
        }

        let activity_import_count = if resolvable.is_empty() {
            0
        } else {
            facts
                .import_attention
                .iter()
                .filter(|library_id| match library_id {
                    // A row with no title an operator can be scoped by is
                    // operational history, which only a system administrator
                    // sees — the same rule the history collector applies.
                    None => can_view_operational_history,
                    Some(library_id) => resolvable.contains(library_id),
                })
                .count() as i64
        };

        Ok(crate::types::NavigationBadgeCounts {
            pending_imports,
            pending_media_requests,
            activity_import_count,
            plugin_update_count: if can_view_operational_history {
                facts.plugin_update_count
            } else {
                0
            },
            plugin_blocked_count: if can_view_operational_history {
                facts.plugin_blocked_count
            } else {
                0
            },
        })
    }
}

/// Keep the navigation badge facts current, off every request path.
///
/// Wakes on whichever comes first: a new download-queue snapshot, which is what
/// moves the import attention count, or the refresh interval, which bounds how
/// stale the durable facts can be.
pub async fn start_navigation_badge_facts_refresh(
    app: AppUseCase,
    token: tokio_util::sync::CancellationToken,
) {
    let mut queue_sync = app.runtime.acquisition.download_queue_snapshot.subscribe();
    loop {
        if let Err(error) = app.refresh_navigation_badge_facts().await {
            tracing::warn!("navigation badge facts refresh failed: {error}");
        }
        tokio::select! {
            _ = token.cancelled() => return,
            _ = tokio::time::sleep(NAVIGATION_BADGE_FACTS_REFRESH_INTERVAL) => {}
            changed = queue_sync.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}
