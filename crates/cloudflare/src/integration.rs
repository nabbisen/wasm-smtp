//! High-level integration helpers that compose the Cloudflare
//! transport with `wasm-smtp-core`.
//!
//! Most callers should use [`connect_smtps`]. It performs the entire
//! "open a TLS socket, run the SMTP greeting and EHLO" sequence and
//! returns an [`SmtpClient`] that is ready for `login` or
//! `send_mail`.

use wasm_smtp_core::{SmtpClient, SmtpError};

use crate::adapter::CloudflareTransport;
use crate::socket::connect_implicit_tls;

/// Open a TLS-secured connection to `host:port` and run the SMTP
/// greeting and `EHLO` handshake.
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
