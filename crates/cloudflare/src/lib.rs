//! # wasm-smtp-cloudflare
//!
//! Cloudflare Workers socket adapter for [`wasm-smtp`].
//!
//! This crate is a thin bridge between the Cloudflare Workers runtime's
//! TCP socket API (`worker::Socket`) and the
//! [`wasm-smtp::Transport`] / [`wasm-smtp::StartTlsCapable`]
//! contracts. It does not implement SMTP itself; everything
//! protocol-shaped lives in `wasm-smtp`.
//!
//! ## Scope
//!
//! - Open a TCP connection from a Worker using `worker::connect()`.
//! - Configure either Implicit TLS (`SecureTransport::On`, port 465)
//!   or STARTTLS (`SecureTransport::StartTls`, port 587).
//! - Wrap the resulting `worker::Socket` so it implements both
//!   `wasm-smtp::Transport` and (for STARTTLS)
//!   `wasm-smtp::StartTlsCapable`.
//! - Translate Workers-side I/O failures into
//!   [`wasm-smtp::IoError`] strings.
//!
//! ## Out of scope
//!
//! - SMTP state, command formatting, response parsing, dot-stuffing —
//!   these belong in `wasm-smtp`.
//! - MIME composition or attachment building — supply a fully-formed
//!   RFC 5322 message as the body.
//!
//! ## Quick start (Implicit TLS, port 465)
//!
//! ```ignore
//! use wasm_smtp_cloudflare::connect_smtps;
//!
//! # async fn handler() -> Result<(), wasm_smtp::SmtpError> {
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
//! # async fn handler() -> Result<(), wasm_smtp::SmtpError> {
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
//! [`wasm-smtp`]: https://docs.rs/wasm-smtp
//! [`wasm-smtp::Transport`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/trait.Transport.html
//! [`wasm-smtp::StartTlsCapable`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/trait.StartTlsCapable.html
//! [`wasm-smtp::IoError`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/struct.IoError.html

pub mod adapter;
pub mod integration;
pub mod socket;

pub use adapter::CloudflareTransport;
pub use integration::{connect_smtp_starttls, connect_smtps};
pub use socket::{connect_implicit_tls, connect_starttls};

/// Re-export of the core `Transport` trait so that callers depending on
/// this crate do not need a direct dependency on `wasm-smtp` for
/// the most common use.
pub use wasm_smtp::Transport;

/// Re-export of `StartTlsCapable` for callers that want to call
/// `upgrade_to_tls` directly.
pub use wasm_smtp::StartTlsCapable;

/// Re-export of `SmtpClient` for convenience: `connect_smtps` returns
/// `SmtpClient<CloudflareTransport>`.
pub use wasm_smtp::SmtpClient;

/// Re-export of `SmtpError` for convenience.
pub use wasm_smtp::SmtpError;

#[cfg(test)]
mod tests;
