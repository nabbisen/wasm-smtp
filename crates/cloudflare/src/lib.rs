//! # wasm-smtp-cloudflare
//!
//! Cloudflare Workers socket adapter for [`wasm-smtp-core`].
//!
//! This crate is a thin bridge between the Cloudflare Workers runtime's
//! TCP socket API (`worker::Socket`) and the
//! [`wasm-smtp-core::Transport`] / [`wasm-smtp-core::StartTlsCapable`]
//! contracts. It does not implement SMTP itself; everything
//! protocol-shaped lives in `wasm-smtp-core`.
//!
//! ## Scope
//!
//! - Open a TCP connection from a Worker using `worker::connect()`.
//! - Configure either Implicit TLS (`SecureTransport::On`, port 465)
//!   or STARTTLS (`SecureTransport::StartTls`, port 587).
//! - Wrap the resulting `worker::Socket` so it implements both
//!   `wasm-smtp-core::Transport` and (for STARTTLS)
//!   `wasm-smtp-core::StartTlsCapable`.
//! - Translate Workers-side I/O failures into
//!   [`wasm-smtp-core::IoError`] strings.
//!
//! ## Out of scope
//!
//! - SMTP state, command formatting, response parsing, dot-stuffing —
//!   these belong in `wasm-smtp-core`.
//! - MIME composition or attachment building — supply a fully-formed
//!   RFC 5322 message as the body.
//!
//! ## Quick start (Implicit TLS, port 465)
//!
//! ```ignore
//! use wasm_smtp_cloudflare::connect_smtps;
//!
//! # async fn handler() -> Result<(), wasm_smtp_core::SmtpError> {
//! let mut client = connect_smtps("smtp.example.com", 465, "client.example.com").await?;
//! client.login("user@example.com", "secret").await?;
//! client.send_mail(
//!     "user@example.com",
//!     &["recipient@example.org"],
//!     "From: user@example.com\r\n\
//!      To: recipient@example.org\r\n\
//!      Subject: Hello\r\n\
//!      \r\n\
//!      Body text.\r\n",
//! ).await?;
//! client.quit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Quick start (STARTTLS, port 587)
//!
//! ```ignore
//! use wasm_smtp_cloudflare::connect_smtp_starttls;
//!
//! # async fn handler() -> Result<(), wasm_smtp_core::SmtpError> {
//! let mut client =
//!     connect_smtp_starttls("smtp.example.com", 587, "client.example.com").await?;
//! client.login("user@example.com", "secret").await?;
//! // ... same as above
//! # Ok(())
//! # }
//! ```
//!
//! ## Targets
//!
//! Production code only runs on `wasm32-unknown-unknown` inside the
//! Cloudflare Workers runtime. The crate compiles on host targets so
//! that `cargo check` can validate types and so that the conversion
//! helpers in [`adapter`] can be unit-tested against `tokio-test` mocks
//! — but at runtime, `worker::Socket` requires the Workers runtime.
//!
//! [`wasm-smtp-core`]: https://docs.rs/wasm-smtp-core
//! [`wasm-smtp-core::Transport`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/trait.Transport.html
//! [`wasm-smtp-core::StartTlsCapable`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/trait.StartTlsCapable.html
//! [`wasm-smtp-core::IoError`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.IoError.html

pub mod adapter;
pub mod integration;
pub mod socket;

pub use adapter::CloudflareTransport;
pub use integration::{connect_smtp_starttls, connect_smtps};
pub use socket::{connect_implicit_tls, connect_starttls};

/// Re-export of the core `Transport` trait so that callers depending on
/// this crate do not need a direct dependency on `wasm-smtp-core` for
/// the most common use.
pub use wasm_smtp_core::Transport;

/// Re-export of `StartTlsCapable` for callers that want to call
/// `upgrade_to_tls` directly.
pub use wasm_smtp_core::StartTlsCapable;

/// Re-export of `SmtpClient` for convenience: `connect_smtps` returns
/// `SmtpClient<CloudflareTransport>`.
pub use wasm_smtp_core::SmtpClient;

/// Re-export of `SmtpError` for convenience.
pub use wasm_smtp_core::SmtpError;

#[cfg(test)]
mod tests;
