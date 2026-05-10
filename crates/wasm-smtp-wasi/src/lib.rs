//! # wasm-smtp-wasi
//!
//! WASI sockets adapter for [`wasm-smtp`], targeting `wasm32-wasip2`
//! (WASI 0.2 Component Model) runtimes such as wasmtime and WAMR.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use wasm_smtp_wasi::connect_smtps;
//!
//! # async fn run() -> Result<(), wasm_smtp::SmtpError> {
//! // Implicit TLS on port 465 (recommended).
//! let mut client = connect_smtps(
//!     "smtp.example.com",
//!     465,
//!     "client.example.com",
//! ).await?;
//!
//! client.login("user@example.com", "secret").await?;
//! client.send_mail(
//!     "user@example.com",
//!     &["recipient@example.org"],
//!     "Subject: hello\r\n\r\nBody.\r\n",
//! ).await?;
//! client.quit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## TLS
//!
//! TLS is handled by [rustls] on top of the WASI byte streams. Certificate
//! validation is enforced and cannot be disabled through the public API.
//! Trust anchors come from the Mozilla root CA set (`webpki-roots` feature,
//! default) or from the OS at runtime (`native-roots` feature, not useful on
//! most WASI runtimes).
//!
//! ## STARTTLS
//!
//! Port 587 STARTTLS is supported via [`connect_smtp_starttls`].
//! The TLS upgrade is performed by rustls after the SMTP `STARTTLS`
//! handshake.
//!
//! ## Build target
//!
//! This crate is designed for `wasm32-wasip2`. Building for other targets
//! is only useful for running unit tests; the connection helpers will
//! return a compile-time error on non-WASM targets in release builds.
//!
//! ```text
//! cargo build --target wasm32-wasip2 -p wasm-smtp-wasi
//! ```
//!
//! To run tests on native (without a WASI runtime):
//!
//! ```text
//! cargo test -p wasm-smtp-wasi
//! ```
//!
//! ## Feature flags
//!
//! | Flag | Default | Description |
//! |---|---|---|
//! | `webpki-roots` | ✅ | Bundle Mozilla root CA set |
//! | `native-roots` | ❌ | Use OS trust store (falls back to webpki-roots on WASI) |
//! | `plaintext-only` | ❌ | **Test / proxy-offload only.** No TLS. |
//!
//! ## Limitations
//!
//! - `wasm32-wasip2` target stdlib must be installed.
//! - No connection pooling (one transport = one TCP connection).
//! - No built-in retry or timeout logic.
//!
//! [rustls]: https://docs.rs/rustls

mod error;
mod tls;

#[cfg(target_arch = "wasm32")]
mod wasi_impl;

mod tests;

// Public re-exports.
pub use error::WasiSmtpError;
pub use tls::ConnectOptions;

#[cfg(target_arch = "wasm32")]
use wasm_smtp::SmtpClient;

#[cfg(target_arch = "wasm32")]
use wasm_smtp::SmtpError;

#[cfg(target_arch = "wasm32")]
use wasi_impl::WasiTlsTransport;

// ---------------------------------------------------------------------------
// Public connect helpers
// ---------------------------------------------------------------------------

/// Connect with Implicit TLS (port 465) and complete the SMTP greeting
/// and `EHLO` exchange.
///
/// Returns a ready-to-use [`SmtpClient`] after the greeting and `EHLO`.
/// Call [`SmtpClient::login`] next.
///
/// # Errors
///
/// - [`SmtpError::Io`] on DNS lookup failure, TCP connect failure, or TLS
///   handshake failure.
/// - [`SmtpError::Protocol`] if the server greeting or `EHLO` response is
///   unexpected.
#[cfg(target_arch = "wasm32")]
pub async fn connect_smtps(
    host: &str,
    port: u16,
    ehlo_domain: &str,
) -> Result<SmtpClient<WasiTlsTransport>, SmtpError> {
    let transport = WasiTlsTransport::connect_implicit_tls(host, port, ConnectOptions::default())
        .await
        .map_err(|e| SmtpError::Io(e.into()))?;
    SmtpClient::connect(transport, ehlo_domain).await
}

/// Connect with Implicit TLS using custom [`ConnectOptions`].
#[cfg(target_arch = "wasm32")]
pub async fn connect_smtps_with(
    host: &str,
    port: u16,
    ehlo_domain: &str,
    options: ConnectOptions,
) -> Result<SmtpClient<WasiTlsTransport>, SmtpError> {
    let transport = WasiTlsTransport::connect_implicit_tls(host, port, options)
        .await
        .map_err(|e| SmtpError::Io(e.into()))?;
    SmtpClient::connect(transport, ehlo_domain).await
}

/// Connect with STARTTLS (port 587): plaintext TCP → `STARTTLS` upgrade.
///
/// Drives the SMTP greeting, `EHLO`, and `STARTTLS` upgrade. Returns a
/// ready-to-use [`SmtpClient`] after the post-TLS `EHLO`.
#[cfg(target_arch = "wasm32")]
pub async fn connect_smtp_starttls(
    host: &str,
    port: u16,
    ehlo_domain: &str,
) -> Result<SmtpClient<WasiTlsTransport>, SmtpError> {
    let transport = WasiTlsTransport::connect_plain(host, port)
        .await
        .map_err(|e| SmtpError::Io(e.into()))?;
    SmtpClient::connect_starttls(transport, ehlo_domain).await
}

