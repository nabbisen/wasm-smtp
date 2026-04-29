//! Tokio + rustls transport adapter for [`wasm-smtp`].
//!
//! `wasm-smtp` is runtime-agnostic: it drives the SMTP state machine
//! and parses replies, but knows nothing about sockets or TLS. This
//! crate provides the concrete `Transport` implementation that most
//! tokio-based servers — axum, actix, warp, hyper, plain tokio — need
//! to connect to a real SMTP submission endpoint.
//!
//! # Quick start (implicit TLS, port 465)
//!
//! ```no_run
//! use wasm_smtp::SmtpClient;
//! use wasm_smtp_tokio::TokioTlsTransport;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let transport = TokioTlsTransport::connect_implicit_tls(
//!     "smtp.example.com",
//!     465,
//!     "smtp.example.com", // SNI / certificate hostname
//! ).await?;
//!
//! let mut client = SmtpClient::connect(transport, "client.example.com").await?;
//! client.login("user@example.com", "secret").await?;
//! client.send_mail(
//!     "user@example.com",
//!     &["recipient@example.org"],
//!     "Subject: hi\r\n\r\nhello\r\n",
//! ).await?;
//! client.quit().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # STARTTLS (port 587)
//!
//! For STARTTLS-on-587 endpoints, connect plaintext first and let
//! `wasm-smtp` drive the STARTTLS upgrade:
//!
//! ```no_run
//! use wasm_smtp::SmtpClient;
//! use wasm_smtp_tokio::TokioPlainTransport;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let transport = TokioPlainTransport::connect(
//!     "smtp.example.com",
//!     587,
//!     "smtp.example.com",
//! ).await?;
//!
//! // `connect_starttls` runs the EHLO + STARTTLS dance and asks the
//! // transport to upgrade to TLS in place.
//! let mut client = SmtpClient::connect_starttls(transport, "client.example.com").await?;
//! client.login("user@example.com", "secret").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Trust anchors
//!
//! Two cargo features control where the trust roots come from:
//!
//! - `native-roots` (default): use the system trust store via
//!   [`rustls-native-certs`]. Best for desktop / server deployments
//!   that already manage CA trust through the OS.
//! - `webpki-roots`: use the bundled Mozilla root set via
//!   [`webpki-roots`]. Best for minimal containers, distroless images,
//!   or any environment without a system CA store.
//!
//! Pick exactly one. They are mutually exclusive at the API level —
//! the connect helpers will fail to compile if neither is enabled.
//!
//! # Custom configuration
//!
//! For more control (custom root stores, ALPN, alternate SNI), use
//! [`TokioTlsTransport::connect_with`] together with [`ConnectOptions`]:
//!
//! ```no_run
//! use wasm_smtp_tokio::{TokioTlsTransport, ConnectOptions};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let opts = ConnectOptions::new()
//!     .with_server_name("alt-name.example.com");
//!
//! let transport = TokioTlsTransport::connect_with(
//!     "smtp.example.com",
//!     465,
//!     opts,
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Security
//!
//! Certificate validation is **on by default and cannot be disabled
//! through the public API**. There is intentionally no
//! `dangerous_configuration`-style escape hatch on this crate. To
//! talk to a server with a self-signed certificate (a development
//! mail catcher, for instance), construct a custom root store
//! containing the test CA and pass it through
//! [`ConnectOptions::with_root_store`].
//!
//! [`wasm-smtp`]: https://docs.rs/wasm-smtp
//! [`rustls-native-certs`]: https://docs.rs/rustls-native-certs
//! [`webpki-roots`]: https://docs.rs/webpki-roots

mod transport;

#[cfg(test)]
mod tests;

pub use transport::{ConnectOptions, TokioPlainTransport, TokioTlsTransport};
