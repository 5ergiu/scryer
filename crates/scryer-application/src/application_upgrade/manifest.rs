//! Signed release-manifest validation for in-application upgrades.
//!
//! The schema and its validation live in the shared `application-updater`
//! crate; this module binds them to Scryer's product identity and maps the
//! shared error type onto [`AppError`] with the same messages as before.

use crate::application_upgrade::product::SCRYER_PRODUCT;
use crate::application_upgrade::shared::map_updater_error;
use crate::{AppResult, plugins::catalog::RequiredSigner};

pub use application_updater::manifest::{
    UPGRADE_MANIFEST_MAX_BYTES, UpgradeArchitecture, UpgradeArchive, UpgradeArtifact,
    UpgradeArtifactMember, UpgradeChannel, UpgradeManifest, UpgradePlatform,
};

pub use crate::application_upgrade::product::UPGRADE_MANIFEST_SCHEMA_VERSION;

/// Parses and validates a signed upgrade manifest payload.
pub fn parse_and_validate_upgrade_manifest(raw: &[u8]) -> AppResult<UpgradeManifest> {
    application_updater::manifest::parse_and_validate_upgrade_manifest(&SCRYER_PRODUCT, raw)
        .map_err(map_updater_error)
}

/// The Sigstore identity required of Scryer's release workflow for a tag.
pub fn scryer_release_required_signer(release_tag: &str) -> RequiredSigner {
    SCRYER_PRODUCT.release_required_signer(release_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_golden_fixture() {
        let raw = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../api/upgrade/manifest.v1.example.json"
        ));
        parse_and_validate_upgrade_manifest(raw).expect("golden fixture is valid");
    }

    #[test]
    fn release_signer_is_pinned_to_the_release_workflow() {
        let signer = scryer_release_required_signer("scryer-v0.19.4");
        assert_eq!(signer.github_repository, "scryer-media/scryer");
        assert_eq!(
            signer.github_workflow.as_deref(),
            Some(".github/workflows/scryer.yml")
        );
        assert_eq!(
            signer.github_ref.as_deref(),
            Some("refs/tags/scryer-v0.19.4")
        );
    }
}
