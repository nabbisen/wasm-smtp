//! Cloudflare Workers socket factory.
//!
//! The single function exported here, [`connect_implicit_tls`], opens
//! a TLS-secured TCP connection to an SMTP submission endpoint and
//! returns a ready-to-use [`CloudflareTransport`].
//!
//! "Implicit TLS" means the runtime performs the TLS handshake before
//! any application bytes flow. This is the model used by the SMTP
//! Submissions port (typically 465). STARTTLS, in which a plaintext
//! SMTP greeting is exchanged before TLS is negotiated, is not
//! supported by this crate.

use wasm_smtp_core::IoError;
use worker::{SecureTransport, Socket};

use crate::adapter::CloudflareTransport;

/// Open a TLS-secured TCP connection to `host:port` and return it as
/// a [`CloudflareTransport`].
///
/// Uses `SecureTransport::On`, so the runtime negotiates TLS before
/// any byte is delivered to the SMTP state machine. The function does
/// not return until the connection has been established (i.e. it
/// awaits `Socket::opened`); on failure, an [`IoError`] is returned
/// without leaking Workers-side error types into the public API.
///
/// # Errors
///
/// - The underlying `connect` call rejected the request (typically:
///   the destination host:port is not reachable from the Worker, or
///   Workers' outbound-connection allowlist forbids it).
/// - The TLS handshake failed (typically: the server presented an
///   invalid certificate, or the certificate chain could not be
///   validated by the runtime).
pub async fn connect_implicit_tls(host: &str, port: u16) -> Result<CloudflareTransport, IoError> {
    let socket = Socket::builder()
        .secure_transport(SecureTransport::On)
        .connect(host.to_string(), port)
        .map_err(|e| IoError::new(format!("connect to {host}:{port} failed: {e}")))?;

    socket
        .opened()
        .await
        .map_err(|e| IoError::new(format!("TLS handshake to {host}:{port} failed: {e}")))?;

    Ok(CloudflareTransport::from_socket(socket))
}
