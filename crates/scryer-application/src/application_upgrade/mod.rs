//! In-application upgrades.
//!
//! The upgrade mechanics — manifest validation, installation classification,
//! download, verification, extraction, promotion, rollback, the durable journal
//! and the Windows helper handoff — live in the shared `application-updater`
//! crate. This module owns everything Scryer-specific: its product identity,
//! the job-run orchestration around a pipeline, and the public names the rest of
//! the workspace already imports from here.

mod engine;
pub mod manifest;
mod product;
mod restart;
mod shared;

pub use engine::{
    ApplicationUpgradeJobAccepted, ApplicationUpgradeJobRequest, ApplicationUpgradeJournal,
    ApplicationUpgradeProgress, application_upgrade_helper_update_journal, phases,
};
pub use product::{APPLICATION_UPGRADE_HELPER_PLAN_SCHEMA, SCRYER_PRODUCT};
pub use shared::{map_updater_error, to_updater_error};

pub use application_updater::helper_plan::{
    APPLICATION_UPGRADE_HELPER_WAIT_BUDGET, ApplicationUpgradeHelperMode,
    ApplicationUpgradeHelperOwner, ApplicationUpgradeHelperPlan, ApplicationUpgradeHelperRelaunch,
    ApplicationUpgradeHelperReplacement, MsiHelperJournalTransition, PortableReplacementOperations,
    WRITE_PROBE_PERMISSION_DENIED, WriteProbeOutcome, classify_write_probe_error,
    helper_wait_remaining, helper_write_probe_required, msi_exit_code_transition,
    msi_install_succeeded, open_process_failure_means_exited, path_is_within,
    portable_replacement_operations, portable_replacement_rollback_operations,
    reboot_required_completion_allowed, should_restore_tray_startup,
};
pub use application_updater::installation::{
    EligibilityReason, InstallationAssessment, InstallationEvidence, InstallationKind,
    InstallationOs, ManagementOwner, classify_installation,
};
pub use restart::ApplicationUpgradeRestartHandle;
