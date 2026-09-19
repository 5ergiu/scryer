//! The temporary Windows upgrade helper.
//!
//! The helper logic lives in the shared `application-updater` crate; this
//! binding supplies Scryer's product identity and its per-user startup
//! registration, which an MSI major upgrade can drop and the helper restores.

use std::path::Path;

use application_updater::helper::TrayStartup;
use scryer_application::application_upgrade::SCRYER_PRODUCT;

/// Scryer's per-user "start at login" registration.
struct ScryerTrayStartup;

impl TrayStartup for ScryerTrayStartup {
    fn startup_enabled(&self) -> Result<bool, String> {
        #[cfg(windows)]
        {
            crate::windows_startup::startup_enabled()
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    fn register_startup(&self, tray_path: &Path) -> Result<(), String> {
        #[cfg(windows)]
        {
            crate::windows_startup::register_startup(tray_path)
        }
        #[cfg(not(windows))]
        {
            let _ = tray_path;
            Ok(())
        }
    }
}

pub fn maybe_run_upgrade_helper() -> Result<bool, String> {
    application_updater::helper::maybe_run_upgrade_helper(&SCRYER_PRODUCT, &ScryerTrayStartup)
}
