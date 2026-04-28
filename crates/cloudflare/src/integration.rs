//! High-level integration helpers that compose the Cloudflare
//! transport with `wasm-smtp-core`.
//!
//! Two entry points are provided, one per TLS model:
//!
//! - [`connect_smtps`] uses Implicit TLS on (typically) port 465.
//! - [`connect_smtp_starttls`] uses STARTTLS on (typically) port 587.
//!
//! Both return an [`SmtpClient`] in
//! [`wasm_smtp_core::SessionState::Authentication`], ready for
//! `login` or `send_mail`.

use wasm_smtp_core::{SmtpClient, SmtpError};

use crate::adapter::CloudflareTransport;
use crate::socket::{connect_implicit_tls, connect_starttls};

/// Open a TLS-secured connection to `host:port` (Implicit TLS) and
/// run the SMTP greeting and `EHLO` handshake.
///
/// `ehlo_domain` is the FQDN or address literal that identifies this
/// client to the server in the `EHLO` line. It is typically the
/// hostname of the Worker (or the Worker's environment) — the same
/// value that appears in the `Host` header of an HTTP `fetch` from
/// the same Worker.
///
/// On success the returned [`SmtpClient`] is in a state where the
/// caller may issue [`SmtpClient::login`] or [`SmtpClient::send_mail`].
///
/// # Errors
///
/// Any failure from [`connect_implicit_tls`] is wrapped in
/// [`SmtpError::Io`]. Any failure from `SmtpClient::connect` (which
/// includes a non-2xx greeting and any malformed `EHLO` reply) is
/// returned directly.
pub async fn connect_smtps(
    host: &str,
    port: u16,
    ehlo_domain: &str,
) -> Result<SmtpClient<CloudflareTransport>, SmtpError> {
    let transport = connect_implicit_tls(host, port).await?;
    SmtpClient::connect(transport, ehlo_domain).await
}

/// Open a plaintext connection to `host:port`, run the SMTP greeting
/// and `EHLO`, issue `STARTTLS`, upgrade the transport to TLS, and
/// re-issue `EHLO` per RFC 3207 §4.2.
///
/// This is the convenience entry point for the STARTTLS submission
/// flow (typically port 587). On success the returned client is in
/// [`wasm_smtp_core::SessionState::Authentication`] just like the
/// Implicit-TLS path — the caller proceeds with
/// [`SmtpClient::login`] or [`SmtpClient::send_mail`] without needing
/// to observe the upgrade.
///
/// # Errors
///
/// - [`SmtpError::Io`] if the TCP connect or transport upgrade fails.
/// - [`SmtpError::Protocol`] with `ProtocolError::ExtensionUnavailable`
///   if the server did not advertise `STARTTLS`.
/// - [`SmtpError::Protocol`] with `ProtocolError::UnexpectedCode`
///   (`during: SmtpOp::StartTls`) if the server rejected the command.
/// - Other `SmtpError` variants for greeting / EHLO failures.
pub async fn connect_smtp_starttls(
    host: &str,
    port: u16,
    ehlo_domain: &str,
) -> Result<SmtpClient<CloudflareTransport>, SmtpError> {
    let transport = connect_starttls(host, port).await?;
    SmtpClient::connect_starttls(transport, ehlo_domain).await
}
