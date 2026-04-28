//! Transport abstraction for SMTP I/O.
//!
//! The SMTP state machine in this crate is environment-independent. Adapter
//! crates (e.g. `wasm-smtp-cloudflare`) connect a runtime-native socket to
//! the state machine by implementing [`Transport`].
//!
//! ## Contract
//!
//! Implementations wrap a connected, byte-oriented stream. The trait
//! intentionally exposes only the minimum surface needed by the SMTP state
//! machine:
//!
//! - [`read`](Transport::read) returns the number of bytes filled into the
//!   buffer, or `Ok(0)` to signal that the peer cleanly closed the connection.
//! - [`write_all`](Transport::write_all) must perform short-write retries
//!   internally and only return after every byte has been accepted, or after
//!   a fatal error.
//! - [`close`](Transport::close) releases the connection. The transport must
//!   not be used for further I/O once `close` has returned.
//!
//! Errors of any kind originating below SMTP must be converted to
//! [`IoError`] at this boundary, which keeps adapter-specific types out of
//! the core public API.
//!
//! ## TLS
//!
//! Two TLS models are supported, selected by the transport:
//!
//! - **Implicit TLS** (port 465): the transport is already TLS-secured before
//!   the SMTP state machine sees it. Plain [`Transport`] is sufficient.
//! - **STARTTLS** (port 587 / 25): the transport is initially plaintext and
//!   is upgraded to TLS in-place after the `STARTTLS` SMTP command. Such
//!   transports must additionally implement [`StartTlsCapable`].
//!
//! The TLS handshake itself, in either model, is the transport
//! implementation's responsibility.

use crate::error::IoError;

/// Async byte-oriented transport contract used by [`crate::SmtpClient`].
///
/// See the [module-level documentation](self) for the contract.
#[allow(async_fn_in_trait)]
// Single-threaded WASM runtimes (the primary target) do not need a `Send`
// bound on the returned futures. Adapter crates that target multi-threaded
// runtimes can wrap their transport in a type that adds a `Send` bound at
// the call site.
pub trait Transport {
    /// Read up to `buf.len()` bytes into `buf`.
    ///
    /// Returns the number of bytes filled. `Ok(0)` signals that the peer
    /// closed the connection cleanly (EOF). Implementations must not return
    /// `Ok(0)` for any other reason, because the SMTP state machine treats
    /// `Ok(0)` as a graceful close.
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Write the entire buffer.
    ///
    /// Implementations must perform short-write retries internally and only
    /// return after every byte has been accepted by the underlying stream, or
    /// after a fatal error.
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError>;

    /// Close the transport.
    ///
    /// After this call returns (whether `Ok` or `Err`), the transport must
    /// not be used for further I/O. Calling `close` is independent of the
    /// SMTP `QUIT` command: `QUIT` is an SMTP-level shutdown, `close` is a
    /// transport-level shutdown.
    async fn close(&mut self) -> Result<(), IoError>;
}

/// Marker for a [`Transport`] that can be upgraded to TLS in-place after
/// connection.
///
/// This is what enables the SMTP `STARTTLS` flow (RFC 3207). The plaintext
/// SMTP greeting and the initial `EHLO` are exchanged in cleartext; the
/// client then issues `STARTTLS`, awaits a `220` reply, and asks the
/// transport to upgrade. From that point on the byte stream is TLS-secured
/// and the SMTP state machine continues as if it had always been so (with
/// a second `EHLO` per RFC 3207 §4.2).
///
/// Transports that are connected with Implicit TLS (port 465) need not
/// implement this trait — they are already secure at construction time.
#[allow(async_fn_in_trait)]
pub trait StartTlsCapable: Transport {
    /// Upgrade the byte stream to TLS in-place.
    ///
    /// On success, all subsequent [`Transport::read`] and
    /// [`Transport::write_all`] calls operate on a TLS-secured stream. On
    /// failure, the transport must be considered unusable: the caller will
    /// transition to [`crate::SessionState::Closed`] and call
    /// [`Transport::close`].
    ///
    /// Implementations may perform the TLS handshake synchronously or
    /// lazily on the next read/write; both are acceptable provided that
    /// any handshake error eventually surfaces as an [`IoError`] in
    /// subsequent reads or writes.
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError>;
}
