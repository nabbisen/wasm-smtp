//! Tokio + rustls transport adapter for [`wasm-smtp`].
//!
//! `wasm-smtp` is runtime-agnostic: it drives the SMTP state machine
//! and parses replies, but knows nothing about sockets or TLS. This
//! crate provides the concrete `Transport` implementation that most
//! tokio-based servers — axum, actix, warp, hyper, plain tokio — need
//! to connect to a real SMTP submission endpoint.
//!
//! # Cargo features
//!
//! Two pairs of mutually-exclusive features select the TLS stack:
//!
//! | Trust source     | Pick one                                |
//! |------------------|-----------------------------------------|
//! | `native-roots`   | OS trust store via `rustls-native-certs` (default) |
//! | `webpki-roots`   | Bundled Mozilla root set                |
//!
//! | Crypto provider  | Pick one                                |
//! |------------------|-----------------------------------------|
//! | `aws-lc-rs`      | BoringSSL-derived, default, FIPS paths  |
//! | `ring`           | Traditional rustls provider, faster to compile |
//!
//! Picking both members of a pair, or neither, is a configuration
//! error: this crate fails to compile in either case (see the
//! `compile_error!` block below) so the misconfiguration is caught at
//! `cargo build` time rather than in production.
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

// ---- compile-time feature validation -------------------------------------
//
// `cargo` does not support mutually-exclusive features natively. We use
// `compile_error!` to surface misconfigurations as a build error rather
// than letting them slip through and produce a runtime panic from rustls's
// `CryptoProvider::install_default()`.

#[cfg(all(feature = "native-roots", feature = "webpki-roots"))]
compile_error!(
    "wasm-smtp-tokio: the `native-roots` and `webpki-roots` features are \
     mutually exclusive. Pick exactly one. To use webpki-roots, set \
     `default-features = false, features = [\"webpki-roots\", \"aws-lc-rs\"]` \
     (or substitute `ring` for `aws-lc-rs`)."
);

#[cfg(not(any(feature = "native-roots", feature = "webpki-roots")))]
compile_error!(
    "wasm-smtp-tokio: a trust-anchor source is required. Enable exactly \
     one of `native-roots` (default) or `webpki-roots`."
);

#[cfg(all(feature = "aws-lc-rs", feature = "ring"))]
compile_error!(
    "wasm-smtp-tokio: the `aws-lc-rs` and `ring` features are mutually \
     exclusive. Pick exactly one. The default is `aws-lc-rs`; set \
     `default-features = false` and add `ring` (plus a trust-anchor \
     feature) to switch."
);

#[cfg(not(any(feature = "aws-lc-rs", feature = "ring")))]
compile_error!(
    "wasm-smtp-tokio: a rustls crypto provider is required. Enable \
     exactly one of `aws-lc-rs` (default) or `ring`."
);

mod transport;

#[cfg(test)]
mod tests;

pub use transport::{ConnectOptions, TokioPlainTransport, TokioTlsTransport};
