//! Cloudflare Workers socket factory.
//!
//! Two factory functions are exposed, one per TLS model:
//!
//! - [`connect_implicit_tls`]: opens a TCP connection with TLS already
//!   enabled (`SecureTransport::On`). Use this on port 465.
//! - [`connect_starttls`]: opens a TCP connection in plaintext but
//!   configured for in-place upgrade (`SecureTransport::StartTls`).
//!   Use this on ports 587 / 25 in combination with
//!   [`crate::SmtpClient::starttls`].
//!
//! In both cases the runtime performs the TLS handshake; the
//! [`CloudflareTransport`] presented to `wasm-smtp` operates on
//! the resulting (already- or eventually-) secure stream.

use wasm_smtp::IoError;
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

/// Open a plaintext TCP connection to `host:port`, configured for an
/// in-place TLS upgrade later via STARTTLS.
///
/// The returned [`CloudflareTransport`] starts in plaintext and can be
/// promoted to TLS by calling
/// [`wasm_smtp::StartTlsCapable::upgrade_to_tls`] (or, more
/// commonly, by letting [`crate::SmtpClient::connect_starttls`] /
/// [`crate::SmtpClient::starttls`] do it for you).
///
/// Uses `SecureTransport::StartTls`. Note that the runtime requires
/// this option to have been set at connect time for `start_tls()` to
/// be valid — there is no way to upgrade a socket that was opened
/// with `SecureTransport::Off`.
///
/// # Errors
///
/// As with [`connect_implicit_tls`], errors during the TCP connect
/// surface as [`IoError`].
pub async fn connect_starttls(host: &str, port: u16) -> Result<CloudflareTransport, IoError> {
    let socket = Socket::builder()
        .secure_transport(SecureTransport::StartTls)
        .connect(host.to_string(), port)
        .map_err(|e| IoError::new(format!("connect to {host}:{port} failed: {e}")))?;

    // `opened()` awaits the TCP connect (no TLS yet under StartTls).
    socket
        .opened()
        .await
        .map_err(|e| IoError::new(format!("TCP connect to {host}:{port} failed: {e}")))?;

    Ok(CloudflareTransport::from_socket(socket))
}
