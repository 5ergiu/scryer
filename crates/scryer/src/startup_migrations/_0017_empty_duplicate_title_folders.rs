use scryer_application::AppUseCase;

use super::versioning::is_release_before;

pub(crate) const ID: &str = "0017_empty_duplicate_title_folders";

/// Whether this start is an upgrade from a release that could have left the
/// folders behind. A first run, a restart, and an unreadable version are not.
pub(crate) fn applies_to_upgrade_from(previous_version: Option<&str>) -> bool {
    previous_version.is_some_and(|previous| is_release_before(previous, 0, 21, 14))
}

/// Retire automatic cleanup while preserving its migration identity.
/// Path-based removal cannot guarantee that a concurrently replaced directory
/// is the one whose contents and ownership were checked. Preserve the folders.
pub(crate) async fn run(_app: &AppUseCase) {
    tracing::info!("automatic leftover title folder cleanup retired; folders preserved");
}
