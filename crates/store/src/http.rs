//! The one HTTP client: rustls (ring) + the OS certificate verifier (security §Network).

use std::sync::Arc;
use std::time::Duration;

use rustls_platform_verifier::BuilderVerifierExt;

use crate::{ErrorClass, StoreError};

/// HTTPS only, certificates checked by the OS verifier, 60 s per request.
///
/// # Errors
/// `Permanent` if TLS or the client can't be set up.
pub fn http_client() -> Result<reqwest::Client, StoreError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(BuilderVerifierExt::with_platform_verifier)
        .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))?
        .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("leemusync/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| StoreError::new(ErrorClass::Permanent, &e.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds() {
        assert!(super::http_client().is_ok());
    }
}
