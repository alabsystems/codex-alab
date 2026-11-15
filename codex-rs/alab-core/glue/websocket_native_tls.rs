// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

//! Native-only roots matching the locked WebSocket transport's default Rustls connector.

use codex_utils_rustls_provider::ensure_rustls_crypto_provider;
use rustls::ClientConfig;
use rustls::RootCertStore;
use std::io;
use std::sync::Arc;

/// Preserve tokio-tungstenite 7447c427's `rustls-tls-native-roots` default when
/// the caller performs TLS before handing the stream to its WebSocket adapter.
/// Native loading retains its `SSL_CERT_FILE` and `SSL_CERT_DIR` behavior; this
/// path does not add WebPKI roots or consult `CODEX_CA_CERTIFICATE`.
pub fn build_native_only_websocket_tls_config() -> Result<Arc<ClientConfig>, io::Error> {
    let rustls_native_certs::CertificateResult { certs, errors, .. } =
        rustls_native_certs::load_native_certs();
    if !errors.is_empty() {
        tracing::warn!(
            native_root_error_count = errors.len(),
            "encountered errors while loading native WebSocket root certificates"
        );
    }
    // Check the loaded certificate list, as the pinned connector does. A
    // nonempty list whose certificates are all ignored must not acquire a new
    // early failure here; Rustls retains the original handshake verdict.
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "no native root CA certificates found (error count: {})",
                errors.len()
            ),
        ));
    }
    let native_root_count = certs.len();
    let mut roots = RootCertStore::empty();
    let (native_roots_added, native_roots_ignored) = roots.add_parsable_certificates(certs);
    tracing::debug!(
        native_root_count,
        native_roots_added,
        native_roots_ignored,
        "loaded native WebSocket root certificates"
    );
    ensure_rustls_crypto_provider();
    Ok(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}
