//! WASI-specific socket implementation.
//!
//! This module is compiled **only** when targeting `wasm32-wasip2` (or any
//! other `wasm32` architecture). On native hosts the connection helpers are
//! absent; tests run through `MockTransport` instead.
//!
//! ## Architecture
//!
//! ```text
//! SmtpClient
//!     └─ WasiTlsTransport (Transport + StartTlsCapable)
//!           ├─ [TLS path]    rustls::ClientConnection + WasiStream
//!           └─ [plain path]  WasiStream (STARTTLS before upgrade)
//!
//! WasiStream
//!     ├─ wasi:sockets/tcp::TcpSocket (connected)
//!     ├─ wasi:io/streams::InputStream  (read side)
//!     └─ wasi:io/streams::OutputStream (write side)
//! ```
//!
//! DNS resolution uses `wasi:sockets/ip-name-lookup`. The adapter tries all
//! returned addresses in order and returns the first successful connection.

mod dns;
mod stream;
mod transport;

pub use transport::WasiTlsTransport;
