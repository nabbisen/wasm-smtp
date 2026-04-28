//! # wasm-smtp
//!
//! Environment-independent SMTP client core for WASM and other constrained
//! runtimes.
//!
//! This crate implements the SMTP state machine, response parsing, command
//! formatting, dot-stuffing, and error classification. It is intentionally
//! free of any runtime-specific socket code: applications must provide a
//! [`Transport`] implementation that wraps the runtime-native socket type.
//! Adapter crates such as `wasm-smtp-cloudflare` provide ready-made
//! transports.
//!
//! ## Scope
//!
//! - **Implicit TLS** (port 465) is the standard connection model. The TLS
//!   handshake itself is the [`Transport`] implementation's responsibility;
//!   the core operates on an already-secure byte stream. STARTTLS is
//!   intentionally out of scope for the initial release.
//! - **MIME composition is out of scope.** Callers pass a fully-formed,
//!   CRLF-normalized message body string to [`SmtpClient::send_mail`].
//! - **Bulk delivery, retry queues, and rate limiting are out of scope.** They
//!   belong in the calling application.
//!
//! ## Example
//!
//! ```ignore
//! use wasm_smtp::{SmtpClient, Transport};
//!
//! async fn send<T: Transport>(transport: T) -> Result<(), wasm_smtp::SmtpError> {
//!     let mut client = SmtpClient::connect(transport, "client.example.com").await?;
//!     // login() picks the best mechanism the server advertised:
//!     // PLAIN if available, falling back to LOGIN. For explicit
//!     // control, use login_with(AuthMechanism::Plain, …).
//!     client.login("user@example.com", "secret").await?;
//!     client.send_mail(
//!         "user@example.com",
//!         &["recipient@example.org"],
//!         "From: user@example.com\r\n\
//!          To: recipient@example.org\r\n\
//!          Subject: Hello\r\n\
//!          \r\n\
//!          Body text.\r\n",
//!     ).await?;
//!     client.quit().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Acceptable use
//!
//! See `TERMS_OF_USE.md` at the repository root. This crate must not be used
//! to deliver unsolicited bulk mail, to impersonate other senders, or to
//! deliver mail that violates the operating policy of any SMTP server.

pub mod client;
pub mod error;
pub mod protocol;
pub mod session;
pub mod transport;

pub use client::SmtpClient;
pub use error::{AuthError, InvalidInputError, IoError, ProtocolError, SmtpError, SmtpOp};
pub use protocol::{AuthMechanism, EnhancedStatus};
pub use session::SessionState;
pub use transport::{StartTlsCapable, Transport};

#[cfg(test)]
mod tests;
