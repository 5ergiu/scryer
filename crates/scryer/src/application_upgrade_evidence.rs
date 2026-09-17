//! Startup evidence for the in-application upgrade classifier.
//!
//! The observation and the classification both live in the shared
//! `application-updater` crate; this binding supplies Scryer's product identity
//! (its environment markers, registry key and write-probe prefix).

use scryer_application::application_upgrade::{InstallationAssessment, SCRYER_PRODUCT};

pub fn collect_installation_assessment() -> InstallationAssessment {
    application_updater::evidence::collect_installation_assessment(&SCRYER_PRODUCT)
}
