//! SP1 probes: shared instance, TLS with the platform verifier.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls_platform_verifier::BuilderVerifierExt;

/// Returns `<version> #<n>`, where `n` counts calls through both bindings.
#[must_use]
#[flutter_rust_bridge::frb(sync)]
pub fn core_version() -> String {
    format!("{} #{}", env!("CARGO_PKG_VERSION"), crate::count_call())
}

/// Opens a TLS connection to `host:443` with the OS certificate verifier,
/// sends `HEAD /`, and returns the HTTP status line.
///
/// # Errors
/// DNS, TCP, TLS or certificate failures, as text.
#[allow(clippy::needless_pass_by_value)] // FRB passes owned strings
pub fn https_status(host: String) -> Result<String, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(BuilderVerifierExt::with_platform_verifier)
        .map_err(|e| e.to_string())?
        .with_no_client_auth();
    let name = ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
    let mut conn =
        rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
    let mut sock = TcpStream::connect((host.as_str(), 443)).map_err(|e| e.to_string())?;
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    write!(
        tls,
        "HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    let mut buf = [0u8; 256];
    let n = tls.read(&mut buf).map_err(|e| e.to_string())?;
    let head = String::from_utf8_lossy(&buf[..n]);
    Ok(head.lines().next().unwrap_or_default().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_bindings_share_one_counter() {
        let a = core_version();
        let b = crate::native::engine_ping();
        let c = core_version();
        let n = |s: &str| -> u64 { s.rsplit('#').next().unwrap().parse().unwrap() };
        assert!(n(&b) > n(&a));
        assert!(n(&c) > n(&b));
    }

    // Network probes for SP1: `cargo test -p leemusync-bridge -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn https_accepts_a_valid_certificate() {
        assert!(
            https_status("www.rust-lang.org".into())
                .unwrap()
                .starts_with("HTTP/1.1")
        );
    }

    #[test]
    #[ignore = "needs the network"]
    fn https_rejects_bad_certificates() {
        for host in [
            "expired.badssl.com",
            "self-signed.badssl.com",
            "wrong.host.badssl.com",
        ] {
            let err = https_status(host.into()).unwrap_err();
            assert!(err.contains("certificate"), "{host}: {err}");
        }
    }
}
