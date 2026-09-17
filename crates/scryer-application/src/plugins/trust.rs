//! Application adapter for the shared artifact verifier.
use super::catalog::RequiredSigner;
use crate::{AppError, AppResult};

fn map_trust_error(error: artifact_trust::Error) -> AppError {
    match error {
        artifact_trust::Error::Validation(message) => AppError::Validation(message),
        artifact_trust::Error::Repository(message) => AppError::Repository(message),
    }
}

pub async fn verify_signed_blob(
    raw: Vec<u8>,
    bundle_raw: Vec<u8>,
    required_signer: RequiredSigner,
) -> AppResult<()> {
    artifact_trust::verify_signed_blob(raw, bundle_raw, required_signer)
        .await
        .map_err(map_trust_error)
}

pub async fn prime_sigstore_trust_roots() -> AppResult<()> {
    scryer_outbound_http::install_default_rustls_provider();
    artifact_trust::prime_sigstore_trust_roots()
        .await
        .map_err(map_trust_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shared_verifier_accepts_signed_plugin_and_preserves_tampering_errors() {
        let raw = include_bytes!("../../test-fixtures/sigstore/fanzub-catalog-v2.1.0.json");
        let bundle =
            include_bytes!("../../test-fixtures/sigstore/fanzub-catalog-v2.1.0.sigstore.json");
        let signer = RequiredSigner {
            github_repository: "scryer-media/scryer-plugins".into(),
            github_workflow: Some(".github/workflows/release-plugin-v3.yml".into()),
            github_ref: Some("refs/tags/plugins-v3/release/1787972354-109745e51603".into()),
        };
        verify_signed_blob(raw.to_vec(), bundle.to_vec(), signer.clone())
            .await
            .unwrap();
        let mut altered = raw.to_vec();
        altered[0] ^= 1;
        assert!(matches!(
            verify_signed_blob(altered, bundle.to_vec(), signer).await,
            Err(AppError::Validation(_))
        ));
    }
}
