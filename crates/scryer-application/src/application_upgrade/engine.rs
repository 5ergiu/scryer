//! Job-run orchestration for in-application upgrades.
//!
//! The upgrade mechanics live in the shared `application-updater` crate. What
//! remains here is everything that belongs to Scryer: the durable job run, its
//! domain events and progress records, the restart handle, and the thin
//! adapters that bind the shared core to Scryer's product identity and error
//! type.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
use application_updater::helper_plan::ApplicationUpgradeHelperPlan;
use application_updater::helper_plan::reboot_required_completion_allowed;
use application_updater::pipeline::{
    DownloadProgress, PortablePromotionFailure, PortableUpgradePaths, ProgressFuture,
    UPGRADE_BUNDLE_MAX_BYTES, rename_path,
};
#[cfg(windows)]
use application_updater::windows_handoff::{WindowsUpgradeHandoff, WindowsUpgradeHandoffInput};

use crate::application_upgrade::InstallationKind;
use crate::application_upgrade::manifest::{
    UPGRADE_MANIFEST_MAX_BYTES, UpgradeArtifact, UpgradeManifest,
    parse_and_validate_upgrade_manifest,
};
use crate::application_upgrade::product::{JOURNAL_SCHEMA, SCRYER_PRODUCT};
use crate::application_upgrade::shared::{map_updater_error, to_updater_error};
use crate::domain_events::DomainEventActor;
use crate::{
    AppError, AppResult, AppUseCase, JobKey, JobRun, JobRunRecord, JobRunStatus, JobTriggerSource,
    SCRYER_VERSION, filesystem_space_raw,
};
use scryer_domain::{
    DomainEventPayload, Id, JobRunCompletedEventData, JobRunFailedEventData,
    JobRunStartedEventData, User,
};

/// Stable progress phase names consumed by the application-upgrade UI.
pub use application_updater::phases;

/// The crash-safe handoff between applying an upgrade and validating the next boot.
pub use application_updater::journal::ApplicationUpgradeJournal;

/// Progress persisted in `workflow_operations.progress_json` for an application upgrade.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationUpgradeProgress {
    pub status: String,
    pub phase: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub target_version: String,
    pub target_tag: String,
    pub error: Option<String>,
}

/// Internal start request assembled by the GraphQL mutation after installation assessment.
#[derive(Clone, Debug)]
pub struct ApplicationUpgradeJobRequest {
    pub expected_tag: String,
    pub expected_version: String,
    pub installation_kind: InstallationKind,
    /// Tests and nonstandard executable hosts may provide the startup evidence path directly.
    pub executable_path: Option<PathBuf>,
    /// Whether the desktop tray owns and supervises this backend process.
    pub tray_supervised: bool,
}

/// Accepted durable job run returned once the asynchronous engine is registered.
#[derive(Clone, Debug)]
pub struct ApplicationUpgradeJobAccepted {
    pub job_run: JobRun,
}

/// The host-owned free-space admission check, injectable so tests can drive the
/// insufficient-space path without filling a filesystem.
type UpgradeSpaceCheck = fn(&Path, u64) -> AppResult<()>;
/// The rename primitive, injectable so tests can drive promotion and rollback
/// failures.
type UpgradeRename = fn(&Path, &Path) -> std::io::Result<()>;

struct UpgradePipelineDependencies<'a> {
    client: &'a reqwest::Client,
    artifact_url_override: Option<&'a str>,
    ensure_available_space: UpgradeSpaceCheck,
    #[cfg_attr(windows, allow(dead_code))]
    rename: UpgradeRename,
}

impl ApplicationUpgradeProgress {
    fn checking(request: &ApplicationUpgradeJobRequest) -> Self {
        Self {
            status: JobRunStatus::Running.as_str().to_string(),
            phase: phases::CHECKING.to_string(),
            downloaded_bytes: 0,
            total_bytes: 0,
            target_version: request.expected_version.clone(),
            target_tag: request.expected_tag.clone(),
            error: None,
        }
    }
}

impl AppUseCase {
    /// Validate the signed-update notice and begin the single-flight application-upgrade job.
    pub async fn start_application_upgrade_job(
        &self,
        actor: &User,
        request: ApplicationUpgradeJobRequest,
    ) -> AppResult<ApplicationUpgradeJobAccepted> {
        if request.expected_tag.trim().is_empty() {
            return Err(AppError::Validation(
                "expectedTag must not be empty".to_string(),
            ));
        }
        if request.expected_version.trim().is_empty() {
            return Err(AppError::Validation(
                "expectedVersion must not be empty".to_string(),
            ));
        }

        let notice = self.smg_scryer_update_notice().await?.ok_or_else(|| {
            AppError::Validation("no application update notice is available".to_string())
        })?;
        if !notice.available {
            return Err(AppError::Validation(
                "the application update notice is not available".to_string(),
            ));
        }
        if notice.latest_tag != request.expected_tag {
            return Err(AppError::Validation(
                "expectedTag does not match the current application update notice".to_string(),
            ));
        }
        if notice.latest_version != request.expected_version {
            return Err(AppError::Validation(
                "expectedVersion does not match the current application update notice".to_string(),
            ));
        }

        let expected_version = Version::parse(&request.expected_version).map_err(|error| {
            AppError::Validation(format!("expectedVersion must be valid semver: {error}"))
        })?;
        let running_version = Version::parse(SCRYER_VERSION).map_err(|error| {
            AppError::Repository(format!("running application version is invalid: {error}"))
        })?;
        if expected_version <= running_version {
            return Err(AppError::Validation(
                "expectedVersion must be strictly newer than the running version".to_string(),
            ));
        }

        if !matches!(
            request.installation_kind,
            InstallationKind::Portable | InstallationKind::DirectMsi
        ) {
            return Err(AppError::Validation(
                "application upgrade installation is not eligible".to_string(),
            ));
        }

        let maintenance_guard = self.try_acquire_system_maintenance()?;
        if self
            .runtime
            .jobs
            .job_run_tracker
            .has_active_job(JobKey::ApplicationUpgrade)
            .await
        {
            return Err(AppError::Validation(
                "an application upgrade job is already running".to_string(),
            ));
        }

        let now = chrono::Utc::now();
        let mut run = JobRunRecord {
            id: Id::new().0,
            job_key: JobKey::ApplicationUpgrade,
            operation_type: format!(
                "application_upgrade:{SCRYER_VERSION}->{}",
                request.expected_version
            ),
            status: JobRunStatus::Running,
            trigger_source: JobTriggerSource::Manual,
            actor_user_id: Some(actor.id.clone()),
            progress_json: serde_json::to_string(&ApplicationUpgradeProgress::checking(&request))
                .ok(),
            summary_json: None,
            summary_text: None,
            error_text: None,
            started_at: now,
            completed_at: None,
            created_at: now,
            updated_at: now,
        };
        run = self.services.events.job_runs.create_job_run(&run).await?;
        let job_run = JobRun::from_record(&run, None);
        self.runtime
            .jobs
            .job_run_tracker
            .upsert_active_run(job_run.clone())
            .await;

        let actor_event = DomainEventActor::from(actor);
        let _ = self
            .append_domain_event(crate::domain_events::new_job_run_domain_event(
                actor_event.clone(),
                run.id.clone(),
                DomainEventPayload::JobRunStarted(JobRunStartedEventData {
                    run_id: run.id.clone(),
                    job_key: run.job_key.as_str().to_string(),
                    operation_type: run.operation_type.clone(),
                    trigger_source: run.trigger_source.as_str().to_string(),
                }),
            ))
            .await;

        let app = self.clone();
        tokio::spawn(async move {
            app.run_application_upgrade_job(run, actor_event, request, maintenance_guard)
                .await;
        });

        Ok(ApplicationUpgradeJobAccepted { job_run })
    }

    /// Return the current tracked run and the newest persisted run for the upgrade status query.
    pub async fn application_upgrade_job_runs(
        &self,
    ) -> AppResult<(Option<JobRun>, Option<JobRun>)> {
        let active = self
            .runtime
            .jobs
            .job_run_tracker
            .active_run_for_job(JobKey::ApplicationUpgrade)
            .await;
        let latest = self
            .services
            .events
            .job_runs
            .list_job_runs(Some(JobKey::ApplicationUpgrade), 1)
            .await?
            .into_iter()
            .next()
            .map(|record| JobRun::from_record(&record, None));
        Ok((active, latest))
    }

    async fn run_application_upgrade_job(
        &self,
        mut run: JobRunRecord,
        actor: DomainEventActor,
        request: ApplicationUpgradeJobRequest,
        _maintenance_guard: tokio::sync::OwnedMutexGuard<()>,
    ) {
        let result = self.execute_application_upgrade(&mut run, &request).await;
        if let Err(error) = result {
            self.cleanup_application_upgrade_staging();
            if let Err(finish_error) = self
                .finish_application_upgrade_failure(&mut run, actor, error.to_string())
                .await
            {
                tracing::error!(error = %finish_error, run_id = %run.id, "failed to finish application upgrade job");
            }
        }
    }

    async fn execute_application_upgrade(
        &self,
        run: &mut JobRunRecord,
        request: &ApplicationUpgradeJobRequest,
    ) -> AppResult<()> {
        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress::checking(request),
        )
        .await?;
        let client = application_upgrade_http_client()?;
        let manifest_url =
            release_asset_url(&request.expected_tag, "scryer-upgrade-manifest.json")?;
        let bundle_url = release_asset_url(
            &request.expected_tag,
            "scryer-upgrade-manifest.json.sigstore.json",
        )?;
        let manifest_raw = fetch_capped_bytes(
            &client,
            manifest_url.as_str(),
            UPGRADE_MANIFEST_MAX_BYTES,
            "upgrade manifest",
        )
        .await?;
        let bundle_raw = fetch_capped_bytes(
            &client,
            bundle_url.as_str(),
            UPGRADE_BUNDLE_MAX_BYTES,
            "upgrade manifest signature bundle",
        )
        .await?;
        verify_upgrade_manifest_signature(manifest_raw.clone(), bundle_raw, &request.expected_tag)
            .await?;
        let manifest = parse_and_validate_upgrade_manifest(&manifest_raw)?;
        self.run_upgrade_pipeline(run, request, &manifest, &client, None)
            .await
    }

    async fn run_upgrade_pipeline(
        &self,
        run: &mut JobRunRecord,
        request: &ApplicationUpgradeJobRequest,
        manifest: &UpgradeManifest,
        client: &reqwest::Client,
        artifact_url_override: Option<&str>,
    ) -> AppResult<()> {
        self.run_upgrade_pipeline_with_dependencies(
            run,
            request,
            manifest,
            UpgradePipelineDependencies {
                client,
                artifact_url_override,
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await
    }

    async fn run_upgrade_pipeline_with_dependencies(
        &self,
        run: &mut JobRunRecord,
        request: &ApplicationUpgradeJobRequest,
        manifest: &UpgradeManifest,
        dependencies: UpgradePipelineDependencies<'_>,
    ) -> AppResult<()> {
        if manifest.tag != request.expected_tag {
            return Err(AppError::Validation(
                "upgrade manifest tag does not match expectedTag".to_string(),
            ));
        }
        if manifest.version != request.expected_version {
            return Err(AppError::Validation(
                "upgrade manifest version does not match expectedVersion".to_string(),
            ));
        }
        let artifact = select_artifact(manifest, request.installation_kind)?.clone();

        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress {
                phase: phases::DOWNLOADING.to_string(),
                total_bytes: artifact.size,
                ..ApplicationUpgradeProgress::checking(request)
            },
        )
        .await?;
        let staging_dir = self.application_upgrade_staging_dir();
        recreate_staging_dir(&staging_dir)?;
        (dependencies.ensure_available_space)(&staging_dir, staging_space_requirement(&artifact))?;
        let download_path = staging_dir.join("artifact");
        download_artifact(
            self,
            run,
            request,
            dependencies.client,
            &artifact,
            dependencies.artifact_url_override,
            &download_path,
        )
        .await?;

        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress {
                phase: phases::VERIFYING.to_string(),
                downloaded_bytes: artifact.size,
                total_bytes: artifact.size,
                ..ApplicationUpgradeProgress::checking(request)
            },
        )
        .await?;
        verify_artifact_hash(&download_path, &artifact)?;
        validate_archive_members(&download_path, &artifact)?;

        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress {
                phase: phases::STAGING.to_string(),
                downloaded_bytes: artifact.size,
                total_bytes: artifact.size,
                ..ApplicationUpgradeProgress::checking(request)
            },
        )
        .await?;
        let extracted_dir = staging_dir.join("extracted");
        extract_archive(&download_path, &artifact, &extracted_dir)?;

        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress {
                phase: phases::APPLYING.to_string(),
                downloaded_bytes: artifact.size,
                total_bytes: artifact.size,
                ..ApplicationUpgradeProgress::checking(request)
            },
        )
        .await?;
        #[cfg(windows)]
        {
            return self
                .handoff_windows_upgrade(run, request, &artifact, &extracted_dir, &download_path)
                .await;
        }

        #[cfg(not(windows))]
        {
            let paths = portable_upgrade_paths(request, SCRYER_VERSION)?;
            let journal_path = self.application_upgrade_journal_path();
            let journal = ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: request.expected_version.clone(),
                expected_tag: request.expected_tag.clone(),
                executable_path: paths.executable_path.clone(),
                backup_path: paths.backup_path.clone(),
                backup_paths: vec![paths.backup_path.clone()],
                phase: phases::RESTARTING.to_string(),
                helper_error: None,
                written_at: Some(Utc::now()),
            };
            // The journal has to be durable before the binary moves: a crash in
            // between must never leave a promoted executable that the next boot
            // has no record of.
            write_journal(&journal_path, &journal)?;
            if let Err(failure) = apply_portable_upgrade(
                &extracted_dir,
                &artifact,
                &paths,
                &request.expected_version,
                dependencies.ensure_available_space,
                dependencies.rename,
            ) {
                let (error, restored) = failure.into_parts();
                let error = map_updater_error(error);
                if restored {
                    if let Err(cleanup_error) = remove_file_if_exists(&journal_path) {
                        tracing::warn!(
                            error = %cleanup_error,
                            "failed to remove the application upgrade journal after a restored promotion failure"
                        );
                    }
                } else {
                    tracing::error!(
                        journal_path = %journal_path.display(),
                        backup_path = %paths.backup_path.display(),
                        "portable application upgrade could not restore the previous executable; preserving recovery journal"
                    );
                }
                return Err(error);
            }

            if let Err(error) = self
                .update_application_upgrade_progress(
                    run,
                    ApplicationUpgradeProgress {
                        phase: phases::RESTARTING.to_string(),
                        downloaded_bytes: artifact.size,
                        total_bytes: artifact.size,
                        ..ApplicationUpgradeProgress::checking(request)
                    },
                )
                .await
            {
                return Err(roll_back_portable_promotion(
                    &paths,
                    &journal_path,
                    dependencies.rename,
                    error,
                ));
            }
            let restart = match self.application_upgrade_restart_handle() {
                Ok(restart) => restart,
                Err(error) => {
                    return Err(roll_back_portable_promotion(
                        &paths,
                        &journal_path,
                        dependencies.rename,
                        error,
                    ));
                }
            };
            restart.schedule_restart();
            Ok(())
        }
    }

    fn application_upgrade_restart_handle(
        &self,
    ) -> AppResult<crate::application_upgrade::ApplicationUpgradeRestartHandle> {
        self.runtime
            .jobs
            .application_upgrade_restart
            .read()
            .ok()
            .and_then(|handle| handle.clone())
            .ok_or_else(|| {
                AppError::Repository(
                    "application upgrade restart controller is not configured".to_string(),
                )
            })
    }

    #[cfg(windows)]
    async fn handoff_windows_upgrade(
        &self,
        run: &mut JobRunRecord,
        request: &ApplicationUpgradeJobRequest,
        artifact: &UpgradeArtifact,
        extracted_dir: &Path,
        msi_path: &Path,
    ) -> AppResult<()> {
        let executable_path = request
            .executable_path
            .clone()
            .or_else(|| std::env::current_exe().ok())
            .ok_or_else(|| {
                AppError::Repository("failed to resolve the running executable path".to_string())
            })?;
        let install_dir = executable_path.parent().map(PathBuf::from).ok_or_else(|| {
            AppError::Validation("running executable has no parent directory".to_string())
        })?;
        let direct_relaunch_args = std::env::args_os()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let direct_relaunch_cwd = std::env::current_dir().unwrap_or_else(|_| install_dir.clone());
        let handoff = build_windows_upgrade_handoff(WindowsUpgradeHandoffInput {
            run_id: &run.id,
            expected_version: &request.expected_version,
            expected_tag: &request.expected_tag,
            installation_kind: request.installation_kind,
            tray_supervised: request.tray_supervised,
            executable_path: &executable_path,
            install_dir: &install_dir,
            backend_process_id: std::process::id(),
            artifact: Some(artifact),
            extracted_dir: Some(extracted_dir),
            msi_path: Some(msi_path),
            journal_path: self.application_upgrade_journal_path(),
            direct_relaunch_args: &direct_relaunch_args,
            direct_relaunch_cwd: &direct_relaunch_cwd,
            current_version: SCRYER_VERSION,
            written_at: Utc::now(),
        })?;
        if let Some(existing_backup) = handoff
            .journal
            .backup_paths
            .iter()
            .find(|path| path.exists())
        {
            return Err(AppError::Validation(format!(
                "refusing to overwrite existing application backup '{}'",
                existing_backup.display()
            )));
        }
        write_journal(&handoff.plan.journal_path, &handoff.journal)?;
        self.update_application_upgrade_progress(
            run,
            ApplicationUpgradeProgress {
                phase: handoff.progress_phase.to_string(),
                downloaded_bytes: artifact.size,
                total_bytes: artifact.size,
                ..ApplicationUpgradeProgress::checking(request)
            },
        )
        .await?;
        let helper_dir = self.application_upgrade_helper_dir();
        let plan_path = helper_dir.join("plan.json");
        let helper_path = helper_dir.join("scryer-upgrade-helper.exe");
        write_helper_plan(&plan_path, &handoff.plan)?;
        copy_and_spawn_windows_upgrade_helper(&helper_path, &plan_path)?;
        self.application_upgrade_restart_handle()?.schedule_exit();
        Ok(())
    }

    async fn update_application_upgrade_progress(
        &self,
        run: &mut JobRunRecord,
        progress: ApplicationUpgradeProgress,
    ) -> AppResult<()> {
        run.progress_json = serde_json::to_string(&progress).ok();
        run.updated_at = chrono::Utc::now();
        let updated = self.services.events.job_runs.update_job_run(run).await?;
        *run = updated.clone();
        self.runtime
            .jobs
            .job_run_tracker
            .upsert_active_run(JobRun::from_record(&updated, None))
            .await;
        Ok(())
    }

    async fn finish_application_upgrade_failure(
        &self,
        run: &mut JobRunRecord,
        actor: DomainEventActor,
        error_text: String,
    ) -> AppResult<()> {
        let mut progress = run
            .progress_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<ApplicationUpgradeProgress>(raw).ok())
            .unwrap_or(ApplicationUpgradeProgress {
                status: JobRunStatus::Running.as_str().to_string(),
                phase: phases::CHECKING.to_string(),
                downloaded_bytes: 0,
                total_bytes: 0,
                target_version: String::new(),
                target_tag: String::new(),
                error: None,
            });
        progress.status = JobRunStatus::Failed.as_str().to_string();
        progress.error = Some(error_text.clone());
        let now = chrono::Utc::now();
        run.status = JobRunStatus::Failed;
        run.progress_json = serde_json::to_string(&progress).ok();
        run.summary_text = Some("Application upgrade failed".to_string());
        run.error_text = Some(error_text.clone());
        run.completed_at = Some(now);
        run.updated_at = now;
        let updated = self.services.events.job_runs.update_job_run(run).await?;
        *run = updated.clone();
        self.runtime
            .jobs
            .job_run_tracker
            .upsert_active_run(JobRun::from_record(&updated, None))
            .await;
        let _ = self
            .append_domain_event(crate::domain_events::new_job_run_domain_event(
                actor,
                updated.id.clone(),
                DomainEventPayload::JobRunFailed(JobRunFailedEventData {
                    run_id: updated.id.clone(),
                    job_key: updated.job_key.as_str().to_string(),
                    error_text: Some(error_text),
                }),
            ))
            .await;
        Ok(())
    }

    /// Finalize the journal created before a restart and return runs that must
    /// remain running because an operating-system reboot is still required.
    pub async fn finalize_application_upgrade_journal(&self) -> AppResult<Vec<String>> {
        self.finalize_application_upgrade_journal_with_boot_time(None)
            .await
    }

    /// Finalize an upgrade journal with an injectable operating-system boot time.
    /// Windows hosts supply this from `GetTickCount64`; tests inject a fixed value.
    pub async fn finalize_application_upgrade_journal_with_boot_time(
        &self,
        boot_time: Option<SystemTime>,
    ) -> AppResult<Vec<String>> {
        let journal_path = self.application_upgrade_journal_path();
        let Some(journal) = load_journal(&journal_path)? else {
            return Ok(Vec::new());
        };
        if journal.schema != JOURNAL_SCHEMA {
            return Err(AppError::Validation(format!(
                "unsupported application upgrade journal schema '{}'",
                journal.schema
            )));
        }
        let current_executable = std::env::current_exe().map_err(|error| {
            AppError::Repository(format!("failed to resolve running executable: {error}"))
        })?;
        let expected_version_booted = SCRYER_VERSION == journal.expected_version;
        // Startup evidence records the canonical executable path, so the journal
        // comparison has to canonicalize too; otherwise a Homebrew or symlinked
        // layout looks like a boot of the wrong binary.
        let expected_executable_booted =
            canonical_path(&current_executable) == canonical_path(&journal.executable_path);
        if journal.phase == phases::REBOOT_REQUIRED {
            if reboot_required_completion_allowed(
                journal.written_at,
                boot_time.map(DateTime::<Utc>::from),
                expected_version_booted,
                expected_executable_booted,
            ) {
                self.complete_journal_application_upgrade(&journal, &journal_path)
                    .await?;
                return Ok(Vec::new());
            }
            // The run stays Running until the operator reboots. Rehydrate the
            // in-memory tracker so single-flight admission still sees it and a
            // second upgrade cannot start behind the pending one. A tracker
            // failure must not swallow the exclusion, or startup reconciliation
            // would fail the very run it is meant to preserve.
            if let Err(error) = self
                .rehydrate_application_upgrade_active_run(&journal.run_id)
                .await
            {
                tracing::warn!(
                    error = %error,
                    run_id = %journal.run_id,
                    "failed to re-register the application upgrade run awaiting a reboot"
                );
            }
            return Ok(vec![journal.run_id]);
        }
        if let Some(error) = journal.helper_error.clone() {
            self.finish_journal_application_upgrade(
                &journal,
                JobRunStatus::Failed,
                None,
                Some(error),
            )
            .await?;
            remove_file_if_exists(&journal_path)?;
            remove_dir_if_exists(&self.application_upgrade_staging_dir())?;
            remove_dir_if_exists(&self.application_upgrade_helper_dir())?;
            return Ok(Vec::new());
        }
        if journal.phase != phases::RESTARTING {
            return Err(AppError::Validation(format!(
                "unsupported application upgrade journal phase '{}'",
                journal.phase
            )));
        }

        if expected_version_booted && expected_executable_booted {
            self.complete_journal_application_upgrade(&journal, &journal_path)
                .await?;
            return Ok(Vec::new());
        }

        self.finish_journal_application_upgrade(
            &journal,
            JobRunStatus::Failed,
            None,
            Some("upgrade did not boot the expected version; backups preserved".to_string()),
        )
        .await?;
        Ok(Vec::new())
    }

    /// Put a still-running upgrade run back into the in-memory job tracker.
    ///
    /// The tracker is rebuilt from scratch on every start, so a run that
    /// survives a restart (an upgrade waiting for an operating-system reboot)
    /// is invisible to `has_active_job` until it is re-registered here.
    async fn rehydrate_application_upgrade_active_run(&self, run_id: &str) -> AppResult<()> {
        let Some(record) = self.services.events.job_runs.get_job_run(run_id).await? else {
            return Ok(());
        };
        if record.status.is_terminal() {
            return Ok(());
        }
        self.runtime
            .jobs
            .job_run_tracker
            .upsert_active_run(JobRun::from_record(&record, None))
            .await;
        Ok(())
    }

    async fn complete_journal_application_upgrade(
        &self,
        journal: &ApplicationUpgradeJournal,
        journal_path: &Path,
    ) -> AppResult<()> {
        let old_version = self
            .services
            .events
            .job_runs
            .get_job_run(&journal.run_id)
            .await?
            .and_then(|run| {
                run.operation_type
                    .split_once(':')
                    .and_then(|(_, versions)| versions.split_once("->"))
                    .map(|(old, _)| old.to_string())
            })
            .unwrap_or_else(|| "previous version".to_string());
        self.finish_journal_application_upgrade(
            journal,
            JobRunStatus::Completed,
            Some(format!(
                "Upgraded application from {old_version} to {}",
                journal.expected_version
            )),
            None,
        )
        .await?;
        remove_file_if_exists(&journal.backup_path)?;
        for backup_path in &journal.backup_paths {
            remove_file_if_exists(backup_path)?;
        }
        remove_file_if_exists(journal_path)?;
        remove_dir_if_exists(&self.application_upgrade_staging_dir())?;
        remove_dir_if_exists(&self.application_upgrade_helper_dir())?;
        Ok(())
    }

    async fn finish_journal_application_upgrade(
        &self,
        journal: &ApplicationUpgradeJournal,
        status: JobRunStatus,
        summary_text: Option<String>,
        error_text: Option<String>,
    ) -> AppResult<()> {
        let mut run = self
            .services
            .events
            .job_runs
            .get_job_run(&journal.run_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("application upgrade run {}", journal.run_id))
            })?;
        // A run that already reached a terminal status was finalized by the
        // pipeline itself; re-finalizing would rewrite its outcome. Recovery
        // files are still cleaned up by the caller.
        if run.status.is_terminal() {
            tracing::info!(
                run_id = %run.id,
                status = %run.status.as_str(),
                "skipping application upgrade journal finalization for an already finished run"
            );
            return Ok(());
        }
        let mut progress = run
            .progress_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<ApplicationUpgradeProgress>(raw).ok())
            .unwrap_or(ApplicationUpgradeProgress {
                status: JobRunStatus::Running.as_str().to_string(),
                phase: phases::RESTARTING.to_string(),
                downloaded_bytes: 0,
                total_bytes: 0,
                target_version: journal.expected_version.clone(),
                target_tag: journal.expected_tag.clone(),
                error: None,
            });
        progress.status = status.as_str().to_string();
        progress.error = error_text.clone();
        let now = chrono::Utc::now();
        run.status = status;
        run.progress_json = serde_json::to_string(&progress).ok();
        run.summary_text = summary_text.clone();
        run.error_text = error_text.clone();
        run.completed_at = Some(now);
        run.updated_at = now;
        let updated = self.services.events.job_runs.update_job_run(&run).await?;
        self.runtime
            .jobs
            .job_run_tracker
            .upsert_active_run(JobRun::from_record(&updated, None))
            .await;
        let payload = match status {
            JobRunStatus::Completed => {
                DomainEventPayload::JobRunCompleted(JobRunCompletedEventData {
                    run_id: updated.id.clone(),
                    job_key: updated.job_key.as_str().to_string(),
                    summary_text,
                })
            }
            JobRunStatus::Failed => DomainEventPayload::JobRunFailed(JobRunFailedEventData {
                run_id: updated.id.clone(),
                job_key: updated.job_key.as_str().to_string(),
                error_text,
            }),
            _ => unreachable!("journal finalization only writes terminal statuses"),
        };
        let _ = self
            .append_domain_event(crate::domain_events::new_job_run_domain_event(
                DomainEventActor::system(),
                updated.id.clone(),
                payload,
            ))
            .await;
        Ok(())
    }

    fn application_upgrade_root_dir(&self) -> PathBuf {
        self.runtime
            .environment
            .config_dir
            .as_ref()
            .join("application-upgrade")
    }

    fn application_upgrade_staging_dir(&self) -> PathBuf {
        self.application_upgrade_root_dir().join("staging")
    }

    fn application_upgrade_helper_dir(&self) -> PathBuf {
        self.application_upgrade_root_dir().join("helper")
    }

    fn application_upgrade_journal_path(&self) -> PathBuf {
        self.application_upgrade_root_dir().join("journal.json")
    }

    fn cleanup_application_upgrade_staging(&self) {
        if let Err(error) = remove_dir_if_exists(&self.application_upgrade_staging_dir()) {
            tracing::warn!(error = %error, "failed to clean application upgrade staging directory");
        }
    }
}

// ---------------------------------------------------------------------------
// Adapters onto the shared application-upgrade core.
//
// Each of these binds one shared operation to Scryer's product identity and
// maps the shared error onto `AppError`. The messages, checks and ordering are
// the shared core's, which are the ones this module used before the extraction.
// ---------------------------------------------------------------------------

fn application_upgrade_http_client() -> AppResult<reqwest::Client> {
    application_updater::pipeline::application_upgrade_http_client(&SCRYER_PRODUCT)
        .map_err(map_updater_error)
}

async fn verify_upgrade_manifest_signature(
    manifest_raw: Vec<u8>,
    bundle_raw: Vec<u8>,
    release_tag: &str,
) -> AppResult<()> {
    application_updater::pipeline::verify_upgrade_manifest_signature(
        &SCRYER_PRODUCT,
        manifest_raw,
        bundle_raw,
        release_tag,
    )
    .await
    .map_err(map_updater_error)
}

fn release_asset_url(tag: &str, filename: &str) -> AppResult<url::Url> {
    application_updater::pipeline::release_asset_url(&SCRYER_PRODUCT, tag, filename)
        .map_err(map_updater_error)
}

async fn fetch_capped_bytes(
    client: &reqwest::Client,
    url: &str,
    cap: u64,
    label: &str,
) -> AppResult<Vec<u8>> {
    application_updater::pipeline::fetch_capped_bytes(client, url, cap, label)
        .await
        .map_err(map_updater_error)
}

fn select_artifact(
    manifest: &UpgradeManifest,
    installation_kind: InstallationKind,
) -> AppResult<&UpgradeArtifact> {
    application_updater::pipeline::select_artifact(manifest, installation_kind)
        .map_err(map_updater_error)
}

/// Reports download progress into the durable job run.
struct JobRunDownloadProgress<'a> {
    app: &'a AppUseCase,
    run: &'a mut JobRunRecord,
    request: &'a ApplicationUpgradeJobRequest,
}

impl DownloadProgress for JobRunDownloadProgress<'_> {
    fn report(&mut self, downloaded_bytes: u64, total_bytes: u64) -> ProgressFuture<'_> {
        Box::pin(async move {
            self.app
                .update_application_upgrade_progress(
                    self.run,
                    ApplicationUpgradeProgress {
                        phase: phases::DOWNLOADING.to_string(),
                        downloaded_bytes,
                        total_bytes,
                        ..ApplicationUpgradeProgress::checking(self.request)
                    },
                )
                .await
                .map_err(to_updater_error)
        })
    }
}

async fn download_artifact(
    app: &AppUseCase,
    run: &mut JobRunRecord,
    request: &ApplicationUpgradeJobRequest,
    client: &reqwest::Client,
    artifact: &UpgradeArtifact,
    artifact_url_override: Option<&str>,
    destination: &Path,
) -> AppResult<()> {
    let mut progress = JobRunDownloadProgress { app, run, request };
    application_updater::pipeline::download_artifact(
        client,
        artifact,
        artifact_url_override,
        destination,
        &mut progress,
    )
    .await
    .map_err(map_updater_error)
}

fn verify_artifact_hash(path: &Path, artifact: &UpgradeArtifact) -> AppResult<()> {
    application_updater::pipeline::verify_artifact_hash(path, artifact).map_err(map_updater_error)
}

fn validate_archive_members(path: &Path, artifact: &UpgradeArtifact) -> AppResult<()> {
    application_updater::pipeline::validate_archive_members(path, artifact)
        .map_err(map_updater_error)
}

fn extract_archive(path: &Path, artifact: &UpgradeArtifact, destination: &Path) -> AppResult<()> {
    application_updater::pipeline::extract_archive(path, artifact, destination)
        .map_err(map_updater_error)
}

#[cfg_attr(windows, allow(dead_code))]
fn portable_upgrade_paths(
    request: &ApplicationUpgradeJobRequest,
    current_version: &str,
) -> AppResult<PortableUpgradePaths> {
    application_updater::pipeline::portable_upgrade_paths(
        request.installation_kind,
        request.executable_path.as_deref(),
        current_version,
    )
    .map_err(map_updater_error)
}

#[cfg_attr(windows, allow(dead_code))]
fn apply_portable_upgrade(
    extracted_dir: &Path,
    artifact: &UpgradeArtifact,
    paths: &PortableUpgradePaths,
    expected_version: &str,
    ensure_available_space: UpgradeSpaceCheck,
    rename: UpgradeRename,
) -> Result<(), PortablePromotionFailure> {
    application_updater::pipeline::apply_portable_upgrade(
        &SCRYER_PRODUCT,
        extracted_dir,
        artifact,
        paths,
        expected_version,
        |path, required_bytes| {
            ensure_available_space(path, required_bytes).map_err(to_updater_error)
        },
        rename,
    )
}

#[cfg(not(windows))]
fn roll_back_portable_promotion(
    paths: &PortableUpgradePaths,
    journal_path: &Path,
    rename: UpgradeRename,
    error: AppError,
) -> AppError {
    map_updater_error(application_updater::pipeline::roll_back_portable_promotion(
        paths,
        journal_path,
        rename,
        to_updater_error(error),
    ))
}

#[cfg(windows)]
fn build_windows_upgrade_handoff(
    input: WindowsUpgradeHandoffInput<'_>,
) -> AppResult<WindowsUpgradeHandoff> {
    application_updater::windows_handoff::build_windows_upgrade_handoff(&SCRYER_PRODUCT, input)
        .map_err(map_updater_error)
}

#[cfg(windows)]
fn write_helper_plan(path: &Path, plan: &ApplicationUpgradeHelperPlan) -> AppResult<()> {
    application_updater::windows_handoff::write_helper_plan(path, plan).map_err(map_updater_error)
}

#[cfg(windows)]
fn copy_and_spawn_windows_upgrade_helper(helper_path: &Path, plan_path: &Path) -> AppResult<()> {
    application_updater::windows_handoff::copy_and_spawn_windows_upgrade_helper(
        helper_path,
        plan_path,
    )
    .map_err(map_updater_error)
}

fn recreate_staging_dir(path: &Path) -> AppResult<()> {
    application_updater::pipeline::recreate_staging_dir(path).map_err(map_updater_error)
}

fn staging_space_requirement(artifact: &UpgradeArtifact) -> u64 {
    application_updater::pipeline::staging_space_requirement(artifact)
}

fn ensure_available_space(path: &Path, required_bytes: u64) -> AppResult<()> {
    let space = filesystem_space_raw(path).map_err(|error| {
        AppError::Repository(format!(
            "failed to inspect upgrade filesystem space: {error}"
        ))
    })?;
    if space.available_bytes < required_bytes {
        return Err(AppError::Validation(format!(
            "insufficient free space for application upgrade: need {required_bytes} bytes, have {} bytes",
            space.available_bytes
        )));
    }
    Ok(())
}

fn write_journal(path: &Path, journal: &ApplicationUpgradeJournal) -> AppResult<()> {
    application_updater::journal::write_journal(path, journal).map_err(map_updater_error)
}

/// Resolve a path through symlinks, falling back to the path as given.
fn canonical_path(path: &Path) -> PathBuf {
    application_updater::pipeline::canonical_path(path)
}

fn load_journal(path: &Path) -> AppResult<Option<ApplicationUpgradeJournal>> {
    application_updater::journal::load_journal(path).map_err(map_updater_error)
}

/// Persist a terminal status observed by the temporary upgrade helper.
pub fn application_upgrade_helper_update_journal(
    path: &Path,
    phase: &str,
    helper_error: Option<String>,
) -> AppResult<()> {
    application_updater::journal::application_upgrade_helper_update_journal(
        path,
        phase,
        helper_error,
    )
    .map_err(map_updater_error)
}

fn remove_file_if_exists(path: &Path) -> AppResult<()> {
    application_updater::journal::remove_file_if_exists(path).map_err(map_updater_error)
}

fn remove_dir_if_exists(path: &Path) -> AppResult<()> {
    application_updater::journal::remove_dir_if_exists(path).map_err(map_updater_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::JobRunRepository;
    #[cfg(unix)]
    use crate::application_upgrade::ApplicationUpgradeRestartHandle;
    #[cfg(unix)]
    use crate::application_upgrade::InstallationKind;
    #[cfg(unix)]
    use crate::application_upgrade::manifest::UPGRADE_MANIFEST_SCHEMA_VERSION;
    #[cfg(unix)]
    use crate::application_upgrade::manifest::{
        UpgradeArchitecture, UpgradeArchive, UpgradeArtifactMember, UpgradeChannel, UpgradePlatform,
    };
    #[cfg(unix)]
    use application_updater::pipeline::UPGRADE_STAGING_RESERVE_BYTES;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::sync::Arc;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicBool, Ordering};
    #[cfg(unix)]
    use std::time::Duration;
    #[cfg(unix)]
    use wiremock::matchers::{method, path};
    #[cfg(unix)]
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[cfg(unix)]
    fn test_request(executable_path: PathBuf) -> ApplicationUpgradeJobRequest {
        ApplicationUpgradeJobRequest {
            expected_tag: "v99.0.0".to_string(),
            expected_version: "99.0.0".to_string(),
            installation_kind: InstallationKind::Portable,
            executable_path: Some(executable_path),
            tray_supervised: false,
        }
    }

    #[cfg(unix)]
    fn test_run(request: &ApplicationUpgradeJobRequest) -> JobRunRecord {
        let now = chrono::Utc::now();
        JobRunRecord {
            id: Id::new().0,
            job_key: JobKey::ApplicationUpgrade,
            operation_type: format!(
                "application_upgrade:{SCRYER_VERSION}->{}",
                request.expected_version
            ),
            status: JobRunStatus::Running,
            trigger_source: JobTriggerSource::Manual,
            actor_user_id: None,
            progress_json: serde_json::to_string(&ApplicationUpgradeProgress::checking(request))
                .ok(),
            summary_json: None,
            summary_text: None,
            error_text: None,
            started_at: now,
            completed_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[cfg(unix)]
    fn runtime_platform() -> UpgradePlatform {
        match std::env::consts::OS {
            "linux" => UpgradePlatform::Linux,
            "macos" => UpgradePlatform::Darwin,
            os => panic!("unsupported unix upgrade test platform {os}"),
        }
    }

    #[cfg(unix)]
    fn runtime_architecture() -> UpgradeArchitecture {
        match std::env::consts::ARCH {
            "x86_64" => UpgradeArchitecture::X86_64,
            "aarch64" => UpgradeArchitecture::Arm64,
            arch => panic!("unsupported upgrade test architecture {arch}"),
        }
    }

    #[cfg(unix)]
    fn tar_gz(members: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (path, bytes, mode) in members {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).expect("set archive path");
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            archive
                .append(&header, *bytes)
                .expect("append archive member");
        }
        archive
            .into_inner()
            .expect("finish archive")
            .finish()
            .expect("finish gzip")
    }

    #[cfg(unix)]
    fn portable_manifest(bytes: &[u8], members: Vec<UpgradeArtifactMember>) -> UpgradeManifest {
        UpgradeManifest {
            schema: UPGRADE_MANIFEST_SCHEMA_VERSION.to_string(),
            tag: "v99.0.0".to_string(),
            version: "99.0.0".to_string(),
            artifacts: vec![UpgradeArtifact {
                platform: runtime_platform(),
                arch: runtime_architecture(),
                channel: UpgradeChannel::Portable,
                asset_name: "scryer.tar.gz".to_string(),
                url:
                    "https://github.com/scryer-media/scryer/releases/download/v99.0.0/scryer.tar.gz"
                        .to_string(),
                size: bytes.len() as u64,
                blake3: blake3::hash(bytes).to_hex().to_string(),
                archive: UpgradeArchive::TarGz,
                members,
            }],
        }
    }

    #[cfg(unix)]
    fn executable_member(bytes: &[u8]) -> UpgradeArtifactMember {
        UpgradeArtifactMember {
            path: "scryer".to_string(),
            size: bytes.len() as u64,
            executable: true,
        }
    }

    #[cfg(unix)]
    async fn artifact_server(body: Vec<u8>) -> (MockServer, String) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/artifact"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        let url = format!("{}/artifact", server.uri());
        (server, url)
    }

    #[cfg(unix)]
    fn test_http_client() -> reqwest::Client {
        scryer_outbound_http::install_default_rustls_provider();
        reqwest::Client::new()
    }

    #[cfg(unix)]
    async fn run_pipeline_and_finish_failure(
        app: &AppUseCase,
        run: &mut JobRunRecord,
        request: &ApplicationUpgradeJobRequest,
        manifest: &UpgradeManifest,
        dependencies: UpgradePipelineDependencies<'_>,
    ) -> AppError {
        let error = app
            .run_upgrade_pipeline_with_dependencies(run, request, manifest, dependencies)
            .await
            .expect_err("pipeline should fail");
        app.cleanup_application_upgrade_staging();
        app.finish_application_upgrade_failure(run, DomainEventActor::system(), error.to_string())
            .await
            .expect("persist failed application upgrade run");
        error
    }

    #[cfg(unix)]
    static REQUESTED_STAGING_BYTES: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);

    #[cfg(unix)]
    fn injected_insufficient_space(_path: &Path, required_bytes: u64) -> AppResult<()> {
        REQUESTED_STAGING_BYTES.store(required_bytes, Ordering::SeqCst);
        Err(AppError::Validation(
            "insufficient free space for application upgrade: injected test limit".to_string(),
        ))
    }

    #[cfg(unix)]
    fn fail_replacement_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        if from
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".scryer-upgrade-new-"))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected replacement rename failure",
            ));
        }
        fs::rename(from, to)
    }

    #[cfg(unix)]
    fn fail_replacement_and_rollback_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        let name = from.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with(".scryer-upgrade-new-") || name.contains(".pre-upgrade-") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected replacement and rollback rename failure",
            ));
        }
        fs::rename(from, to)
    }

    #[cfg(unix)]
    fn fail_post_promotion_rollback_rename(from: &Path, to: &Path) -> std::io::Result<()> {
        if from
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains(".pre-upgrade-"))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected post-promotion rollback rename failure",
            ));
        }
        fs::rename(from, to)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_happy_path_replaces_portable_executable_and_writes_restart_journal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let restarted = Arc::new(AtomicBool::new(false));
        let restart_observed = Arc::clone(&restarted);
        app.set_application_upgrade_restart_handle(ApplicationUpgradeRestartHandle::new(
            move || {
                restart_observed.store(true, Ordering::SeqCst);
            },
        ));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        app.run_upgrade_pipeline_with_dependencies(
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await
        .expect("portable upgrade pipeline succeeds");

        assert_eq!(
            fs::read(&executable_path).expect("replacement executable"),
            new_binary
        );
        let backup_path = PathBuf::from(format!(
            "{}.pre-upgrade-{SCRYER_VERSION}",
            executable_path.display()
        ));
        assert_eq!(
            fs::read(&backup_path).expect("backup executable"),
            b"old executable"
        );
        let journal = load_journal(&app.application_upgrade_journal_path())
            .expect("load journal")
            .expect("journal exists");
        assert_eq!(journal.phase, phases::RESTARTING);
        assert_eq!(journal.executable_path, executable_path);
        assert_eq!(journal.backup_path, backup_path);
        let progress: ApplicationUpgradeProgress =
            serde_json::from_str(run.progress_json.as_deref().expect("running progress"))
                .expect("decode progress");
        assert_eq!(run.status, JobRunStatus::Running);
        assert_eq!(progress.status, JobRunStatus::Running.as_str());
        assert_eq!(progress.phase, phases::RESTARTING);
        assert!(restarted.load(Ordering::SeqCst));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_blake3_mismatch_fails_and_cleans_staging_without_touching_executable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let mut manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        manifest.artifacts[0].blake3 = "0".repeat(64);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await;

        assert!(error.to_string().contains("BLAKE3 hash does not match"));
        assert_eq!(run.status, JobRunStatus::Failed);
        assert!(!app.application_upgrade_staging_dir().exists());
        assert_eq!(
            fs::read(&executable_path).expect("original executable"),
            b"old executable"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_oversize_response_fails_with_manifest_size_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let mut manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        manifest.artifacts[0].size = manifest.artifacts[0].size.saturating_sub(1);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await;

        assert!(error.to_string().contains("exceeds the manifest size"));
        assert_eq!(run.status, JobRunStatus::Failed);
        assert!(!app.application_upgrade_staging_dir().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_archive_member_mismatch_fails_before_apply() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[
            ("scryer", new_binary, 0o755),
            ("unexpected.txt", b"extra member", 0o644),
        ]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await;

        assert!(error.to_string().contains("members do not exactly match"));
        assert_eq!(run.status, JobRunStatus::Failed);
        assert_eq!(
            fs::read(&executable_path).expect("original executable"),
            b"old executable"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_insufficient_space_uses_injected_space_check() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path);
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: None,
                ensure_available_space: injected_insufficient_space,
                rename: rename_path,
            },
        )
        .await;

        assert!(
            error
                .to_string()
                .contains("insufficient free space for application upgrade")
        );
        assert_eq!(run.status, JobRunStatus::Failed);
        assert!(!app.application_upgrade_staging_dir().exists());
        // Staging admission must budget for the decompressed members as well as
        // the compressed artifact.
        let artifact = &manifest.artifacts[0];
        assert_eq!(
            REQUESTED_STAGING_BYTES.load(Ordering::SeqCst),
            artifact.size + new_binary.len() as u64 + UPGRADE_STAGING_RESERVE_BYTES
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_apply_rename_failure_restores_original_executable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: fail_replacement_rename,
            },
        )
        .await;

        let backup_path = PathBuf::from(format!(
            "{}.pre-upgrade-{SCRYER_VERSION}",
            executable_path.display()
        ));
        assert!(
            error
                .to_string()
                .contains("failed to replace application executable")
        );
        assert_eq!(run.status, JobRunStatus::Failed);
        assert_eq!(
            fs::read(&executable_path).expect("rolled-back executable"),
            b"old executable"
        );
        assert!(!backup_path.exists(), "backup should have been rolled back");
        assert!(
            !app.application_upgrade_journal_path().exists(),
            "a failed promotion must not leave its journal behind"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_preserves_the_journal_when_apply_rollback_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: fail_replacement_and_rollback_rename,
            },
        )
        .await;

        let backup_path = PathBuf::from(format!(
            "{}.pre-upgrade-{SCRYER_VERSION}",
            executable_path.display()
        ));
        assert!(
            error
                .to_string()
                .contains("failed to restore the previous executable")
        );
        assert_eq!(run.status, JobRunStatus::Failed);
        assert!(
            !executable_path.exists(),
            "failed restoration leaves no live executable"
        );
        assert_eq!(
            fs::read(&backup_path).expect("preserved backup"),
            b"old executable"
        );
        assert!(
            app.application_upgrade_journal_path().exists(),
            "a failed restoration must retain the recovery journal"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_rolls_the_promotion_back_when_a_post_promotion_step_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        // No restart handle is configured, so the step after promotion fails.
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: rename_path,
            },
        )
        .await;

        let message = error.to_string();
        assert!(
            message.contains("restart controller is not configured"),
            "error should name the original failure: {message}"
        );
        assert!(
            message.contains("the previous executable was restored"),
            "error should name the rollback outcome: {message}"
        );
        assert_eq!(run.status, JobRunStatus::Failed);
        assert_eq!(
            fs::read(&executable_path).expect("rolled-back executable"),
            b"old executable"
        );
        let backup_path = PathBuf::from(format!(
            "{}.pre-upgrade-{SCRYER_VERSION}",
            executable_path.display()
        ));
        assert!(!backup_path.exists(), "backup should have been rolled back");
        assert!(
            !app.application_upgrade_journal_path().exists(),
            "a rolled-back promotion must leave no journal"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipeline_retains_recovery_state_when_post_promotion_rollback_fails() {
        let temp = tempfile::tempdir().expect("tempdir");
        let executable_path = temp.path().join("bin/scryer");
        fs::create_dir_all(executable_path.parent().expect("executable parent"))
            .expect("create executable directory");
        fs::write(&executable_path, b"old executable").expect("write old executable");
        let new_binary = b"new executable";
        let archive = tar_gz(&[("scryer", new_binary, 0o755)]);
        let manifest = portable_manifest(&archive, vec![executable_member(new_binary)]);
        let (_server, artifact_url) = artifact_server(archive).await;
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(executable_path.clone());
        let mut run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let client = test_http_client();

        let error = run_pipeline_and_finish_failure(
            &app,
            &mut run,
            &request,
            &manifest,
            UpgradePipelineDependencies {
                client: &client,
                artifact_url_override: Some(&artifact_url),
                ensure_available_space,
                rename: fail_post_promotion_rollback_rename,
            },
        )
        .await;

        let backup_path = PathBuf::from(format!(
            "{}.pre-upgrade-{SCRYER_VERSION}",
            executable_path.display()
        ));
        assert!(
            error
                .to_string()
                .contains("the recovery journal was retained")
        );
        assert_eq!(run.status, JobRunStatus::Failed);
        assert_eq!(
            fs::read(&executable_path).expect("promoted executable"),
            new_binary
        );
        assert_eq!(
            fs::read(&backup_path).expect("preserved backup"),
            b"old executable"
        );
        assert!(
            app.application_upgrade_journal_path().exists(),
            "a failed rollback must retain the recovery journal"
        );
    }

    #[cfg(feature = "runtime-plugin-trust")]
    #[tokio::test]
    async fn real_signed_upgrade_manifest_requires_its_exact_release_tag() {
        let manifest =
            include_bytes!("../../test-fixtures/sigstore/scryer-upgrade-manifest-v0.19.3.json");
        let bundle = include_bytes!(
            "../../test-fixtures/sigstore/scryer-upgrade-manifest-v0.19.3.sigstore.json"
        );

        verify_upgrade_manifest_signature(manifest.to_vec(), bundle.to_vec(), "scryer-v0.19.3")
            .await
            .expect("the real signed Scryer release must verify for its own tag");

        let error =
            verify_upgrade_manifest_signature(manifest.to_vec(), bundle.to_vec(), "scryer-v0.19.4")
                .await
                .expect_err("a valid Scryer release signature must not verify for another tag");
        assert!(error.to_string().contains("workflow identity mismatch"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_completes_matching_boot_and_cleans_recovery_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let executable_path = std::env::current_exe().expect("current executable");
        let request = test_request(executable_path.clone());
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        let staging_file = app.application_upgrade_staging_dir().join("artifact");
        fs::create_dir_all(staging_file.parent().expect("staging parent")).expect("create staging");
        fs::write(&staging_file, b"staged artifact").expect("write staging");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: SCRYER_VERSION.to_string(),
                expected_tag: request.expected_tag.clone(),
                executable_path,
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone()],
                phase: phases::RESTARTING.to_string(),
                helper_error: None,
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        assert!(
            app.finalize_application_upgrade_journal()
                .await
                .expect("finalize journal")
                .is_empty()
        );

        let finalized = job_runs
            .get_job_run(&run.id)
            .await
            .expect("load finalized run")
            .expect("run exists");
        assert_eq!(finalized.status, JobRunStatus::Completed);
        assert!(!backup_path.exists());
        assert!(!app.application_upgrade_journal_path().exists());
        assert!(!app.application_upgrade_staging_dir().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_records_helper_error_and_preserves_backup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(temp.path().join("bin/scryer"));
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: request.expected_version.clone(),
                expected_tag: request.expected_tag.clone(),
                executable_path: request.executable_path.clone().expect("executable path"),
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone()],
                phase: phases::RESTARTING.to_string(),
                helper_error: Some("elevation helper failed".to_string()),
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        app.finalize_application_upgrade_journal()
            .await
            .expect("finalize helper failure");

        let finalized = job_runs
            .get_job_run(&run.id)
            .await
            .expect("load finalized run")
            .expect("run exists");
        assert_eq!(finalized.status, JobRunStatus::Failed);
        assert_eq!(
            finalized.error_text.as_deref(),
            Some("elevation helper failed")
        );
        assert!(backup_path.exists());
        assert!(!app.application_upgrade_journal_path().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_preserves_files_when_boot_version_mismatches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(temp.path().join("bin/scryer"));
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: "0.0.0".to_string(),
                expected_tag: request.expected_tag.clone(),
                executable_path: request.executable_path.clone().expect("executable path"),
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone()],
                phase: phases::RESTARTING.to_string(),
                helper_error: None,
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        app.finalize_application_upgrade_journal()
            .await
            .expect("finalize mismatch");

        let finalized = job_runs
            .get_job_run(&run.id)
            .await
            .expect("load finalized run")
            .expect("run exists");
        assert_eq!(finalized.status, JobRunStatus::Failed);
        assert!(
            finalized
                .error_text
                .as_deref()
                .is_some_and(|error| error.contains("backups preserved"))
        );
        assert!(backup_path.exists());
        assert!(app.application_upgrade_journal_path().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_leaves_reboot_required_run_untouched() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(temp.path().join("bin/scryer"));
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: request.expected_version.clone(),
                expected_tag: request.expected_tag.clone(),
                executable_path: request.executable_path.clone().expect("executable path"),
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone()],
                phase: phases::REBOOT_REQUIRED.to_string(),
                helper_error: None,
                written_at: None,
            },
        )
        .expect("write journal");

        assert_eq!(
            app.finalize_application_upgrade_journal()
                .await
                .expect("finalize reboot journal"),
            vec![run.id.clone()]
        );

        let unchanged = job_runs
            .get_job_run(&run.id)
            .await
            .expect("load run")
            .expect("run exists");
        assert_eq!(unchanged.status, JobRunStatus::Running);
        assert!(backup_path.exists());
        assert!(app.application_upgrade_journal_path().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_completes_reboot_required_after_a_new_boot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let executable_path = std::env::current_exe().expect("current executable");
        let request = test_request(executable_path.clone());
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        let tray_backup_path = temp.path().join("scryer-tray.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        fs::write(&tray_backup_path, b"tray backup").expect("write tray backup");
        let helper_file = app.application_upgrade_helper_dir().join("plan.json");
        fs::create_dir_all(helper_file.parent().expect("helper parent")).expect("create helper");
        fs::write(&helper_file, b"helper plan").expect("write helper plan");
        let staging_file = app.application_upgrade_staging_dir().join("artifact");
        fs::create_dir_all(staging_file.parent().expect("staging parent")).expect("create staging");
        fs::write(&staging_file, b"staged artifact").expect("write staging");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: SCRYER_VERSION.to_string(),
                expected_tag: request.expected_tag.clone(),
                executable_path,
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone(), tray_backup_path.clone()],
                phase: phases::REBOOT_REQUIRED.to_string(),
                helper_error: None,
                written_at: Some(Utc::now() - chrono::Duration::seconds(5)),
            },
        )
        .expect("write journal");

        assert!(
            app.finalize_application_upgrade_journal_with_boot_time(Some(SystemTime::now()))
                .await
                .expect("finalize reboot journal")
                .is_empty()
        );
        assert_eq!(
            job_runs
                .get_job_run(&run.id)
                .await
                .expect("load run")
                .expect("run exists")
                .status,
            JobRunStatus::Completed
        );
        assert!(!backup_path.exists());
        assert!(!tray_backup_path.exists());
        assert!(!app.application_upgrade_journal_path().exists());
        assert!(!app.application_upgrade_staging_dir().exists());
        assert!(!app.application_upgrade_helper_dir().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_excludes_reboot_required_before_a_new_boot() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let executable_path = std::env::current_exe().expect("current executable");
        let request = test_request(executable_path.clone());
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: SCRYER_VERSION.to_string(),
                expected_tag: request.expected_tag.clone(),
                executable_path,
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path.clone()],
                phase: phases::REBOOT_REQUIRED.to_string(),
                helper_error: None,
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        assert_eq!(
            app.finalize_application_upgrade_journal_with_boot_time(Some(
                SystemTime::now() - Duration::from_secs(60)
            ))
            .await
            .expect("finalize reboot journal"),
            vec![run.id.clone()]
        );
        assert_eq!(
            job_runs
                .get_job_run(&run.id)
                .await
                .expect("load run")
                .expect("run exists")
                .status,
            JobRunStatus::Running
        );
        assert!(backup_path.exists());
        assert!(app.application_upgrade_journal_path().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reboot_required_run_stays_single_flight_after_a_restart() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let executable_path = std::env::current_exe().expect("current executable");
        let request = test_request(executable_path.clone());
        let run = test_run(&request);
        job_runs.seed(run.clone()).await;
        let backup_path = temp.path().join("scryer.pre-upgrade");
        fs::write(&backup_path, b"backup").expect("write backup");
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: request.expected_version.clone(),
                expected_tag: request.expected_tag.clone(),
                executable_path,
                backup_path: backup_path.clone(),
                backup_paths: vec![backup_path],
                phase: phases::REBOOT_REQUIRED.to_string(),
                helper_error: None,
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        assert!(
            !app.runtime
                .jobs
                .job_run_tracker
                .has_active_job(JobKey::ApplicationUpgrade)
                .await,
            "a fresh process starts with an empty job tracker"
        );
        assert_eq!(
            app.finalize_application_upgrade_journal()
                .await
                .expect("finalize reboot journal"),
            vec![run.id.clone()]
        );
        assert!(
            app.runtime
                .jobs
                .job_run_tracker
                .has_active_job(JobKey::ApplicationUpgrade)
                .await,
            "the pending reboot run must be tracked as active again"
        );
        assert_eq!(
            app.runtime
                .jobs
                .job_run_tracker
                .active_run_for_job(JobKey::ApplicationUpgrade)
                .await
                .map(|tracked| tracked.id),
            Some(run.id.clone())
        );

        app.upsert_system_setting_json(
            "smg.scryer_update_notice",
            &crate::SmgScryerUpdateNotice {
                available: true,
                current_version: SCRYER_VERSION.to_string(),
                latest_version: request.expected_version.clone(),
                latest_tag: request.expected_tag.clone(),
                release_url: None,
                published_at: None,
                checked_at: Utc::now().to_rfc3339(),
            },
            None,
        )
        .await
        .expect("seed update notice");

        let error = app
            .start_application_upgrade_job(&actor, test_request(temp.path().join("bin/scryer")))
            .await
            .expect_err("a second upgrade must be refused while one awaits reboot");
        assert!(
            error.to_string().contains("already running"),
            "unexpected error: {error}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn journal_finalization_does_not_rewrite_an_already_finished_run() {
        let temp = tempfile::tempdir().expect("tempdir");
        let (app, _actor, job_runs) =
            crate::lib_tests::bootstrap_application_upgrade(temp.path().join("data"));
        let request = test_request(temp.path().join("bin/scryer"));
        let mut run = test_run(&request);
        run.status = JobRunStatus::Completed;
        run.summary_text = Some("Application upgrade completed".to_string());
        run.completed_at = Some(Utc::now());
        job_runs.seed(run.clone()).await;
        write_journal(
            &app.application_upgrade_journal_path(),
            &ApplicationUpgradeJournal {
                schema: JOURNAL_SCHEMA.to_string(),
                run_id: run.id.clone(),
                expected_version: request.expected_version.clone(),
                expected_tag: request.expected_tag.clone(),
                executable_path: request.executable_path.clone().expect("executable path"),
                backup_path: temp.path().join("scryer.pre-upgrade"),
                backup_paths: Vec::new(),
                phase: phases::RESTARTING.to_string(),
                helper_error: Some("elevation helper failed".to_string()),
                written_at: Some(Utc::now()),
            },
        )
        .expect("write journal");

        app.finalize_application_upgrade_journal()
            .await
            .expect("finalize journal for a finished run");

        let unchanged = job_runs
            .get_job_run(&run.id)
            .await
            .expect("load run")
            .expect("run exists");
        assert_eq!(unchanged.status, JobRunStatus::Completed);
        assert_eq!(unchanged.error_text, None);
        assert_eq!(
            unchanged.summary_text.as_deref(),
            Some("Application upgrade completed")
        );
        assert!(
            !app.application_upgrade_journal_path().exists(),
            "recovery files are still cleaned up"
        );
    }
}
