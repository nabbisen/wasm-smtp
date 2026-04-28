//! [`CloudflareTransport`] — the [`Transport`] implementation that
//! wraps a Cloudflare Workers [`worker::Socket`].
//!
//! The adapter is intentionally thin. The bulk of the byte-pushing is
//! done by the `tokio::io` traits already implemented on
//! `worker::Socket`; this module only translates between those traits
//! and the [`Transport`] contract from `wasm-smtp`, and converts
//! Workers-side errors into [`IoError`]'s string-based representation.
//!
//! The two free helpers `read_async_io` and `write_all_async_io`
//! are factored out from the trait impl so that they can be unit-tested
//! on host targets with `tokio_test::io::Builder` (see `tests.rs`).
//!
//! ## STARTTLS support
//!
//! [`CloudflareTransport`] implements [`StartTlsCapable`]. The
//! underlying `worker::Socket` must have been opened with
//! `SecureTransport::StartTls` for the upgrade to succeed; use
//! [`crate::connect_starttls`] (not [`crate::connect_implicit_tls`])
//! when building a transport intended for the STARTTLS flow.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use wasm_smtp::{IoError, StartTlsCapable, Transport};
use worker::Socket;

/// SMTP transport backed by a Cloudflare Workers TCP socket.
///
/// Construct via [`crate::connect_implicit_tls`] (Implicit TLS on
/// port 465) or [`crate::connect_starttls`] (plaintext socket
/// configured for in-place TLS upgrade), or via
/// [`crate::connect_smtps`] / [`crate::connect_smtp_starttls`] for
/// the full SMTP greeting.
///
/// The wrapped `worker::Socket` lives behind an `Option` so that
/// [`StartTlsCapable::upgrade_to_tls`] can `take()` it, call
/// `Socket::start_tls()` (which consumes `self`), and put the
/// upgraded socket back. After a successful upgrade the same
/// `CloudflareTransport` instance keeps working — the read/write
/// surface is unchanged.
pub struct CloudflareTransport {
    /// Always `Some` between construction and `close()`. Briefly
    /// `None` only while `upgrade_to_tls()` is exchanging the inner
    /// socket; if a panic occurred mid-upgrade the next read/write
    /// would return a clean `IoError`.
    socket: Option<Socket>,
}

impl CloudflareTransport {
    /// Wrap a pre-connected [`Socket`] as a [`Transport`].
    ///
    /// This constructor does not perform the connect; use
    /// [`crate::connect_implicit_tls`] for the Implicit-TLS path or
    /// [`crate::connect_starttls`] for the STARTTLS path. It is
    /// exposed mainly so that tests and advanced callers can supply a
    /// `Socket` they constructed themselves (e.g. with non-default
    /// `SocketOptions`).
    #[must_use]
    pub fn from_socket(socket: Socket) -> Self {
        Self {
            socket: Some(socket),
        }
    }

    /// Consume the transport and return the inner `worker::Socket`.
    ///
    /// Returns `None` if the transport's inner socket was already
    /// taken (e.g. by a panic during a STARTTLS upgrade); under
    /// normal usage this is always `Some`.
    #[must_use]
    pub fn into_inner(self) -> Option<Socket> {
        self.socket
    }

    /// Borrow the inner socket, returning a clean [`IoError`] if it
    /// is missing (which would only happen after a panic during
    /// `upgrade_to_tls`). Used by every I/O method to keep the
    /// `Option` plumbing out of the per-method bodies.
    fn socket_mut(&mut self) -> Result<&mut Socket, IoError> {
        self.socket
            .as_mut()
            .ok_or_else(|| IoError::new("transport socket is missing (interrupted upgrade?)"))
    }
}

impl core::fmt::Debug for CloudflareTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CloudflareTransport")
            .field("has_socket", &self.socket.is_some())
            .finish_non_exhaustive()
    }
}

impl Transport for CloudflareTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let socket = self.socket_mut()?;
        read_async_io(socket, buf).await
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let socket = self.socket_mut()?;
        write_all_async_io(socket, buf).await
    }

    async fn close(&mut self) -> Result<(), IoError> {
        // `worker::Socket::close` shuts down both halves cleanly. This
        // is preferable to `AsyncWriteExt::shutdown`, which only closes
        // the writable side.
        let socket = self.socket_mut()?;
        socket
            .close()
            .await
            .map_err(|e| IoError::new(format!("socket close failed: {e}")))
    }
}

impl StartTlsCapable for CloudflareTransport {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError> {
        // `Socket::start_tls(self) -> Socket` consumes the original
        // socket and synchronously returns a new one that performs the
        // TLS handshake lazily on the next read/write. To keep our
        // `&mut self` API, we briefly take the inner `Option` and put
        // the upgraded socket back.
        //
        // The original socket must have been opened with
        // `SecureTransport::StartTls` (see `crate::connect_starttls`).
        // If it was opened with `SecureTransport::Off` the upgrade
        // will surface a runtime error on the next read/write, since
        // `worker::Socket::start_tls` does not validate the option.
        let socket = self
            .socket
            .take()
            .ok_or_else(|| IoError::new("transport socket is missing"))?;
        let upgraded = socket.start_tls();
        self.socket = Some(upgraded);
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Internal helpers (unit-testable with tokio_test mocks)
// -----------------------------------------------------------------------------

/// Read up to `buf.len()` bytes from any `tokio::io::AsyncRead` stream
/// into [`IoError`]-flavored result.
///
/// `Ok(0)` propagates the `AsyncRead` convention for "peer closed
/// cleanly", which `wasm-smtp` interprets as
/// [`ProtocolError::UnexpectedClose`] when a reply is mid-assembly.
///
/// [`ProtocolError::UnexpectedClose`]: wasm_smtp::ProtocolError::UnexpectedClose
pub(crate) async fn read_async_io<S>(stream: &mut S, buf: &mut [u8]) -> Result<usize, IoError>
where
    S: AsyncRead + Unpin,
{
    AsyncReadExt::read(stream, buf)
        .await
        .map_err(|e| IoError::new(format!("read failed: {e}")))
}

/// Write the entire buffer to any `tokio::io::AsyncWrite` stream,
/// returning [`IoError`] on failure.
///
/// `AsyncWriteExt::write_all` already loops internally until every
/// byte has been accepted, so the `Transport::write_all` contract
/// (no short writes) is upheld for free.
pub(crate) async fn write_all_async_io<S>(stream: &mut S, buf: &[u8]) -> Result<(), IoError>
where
    S: AsyncWrite + Unpin,
{
    AsyncWriteExt::write_all(stream, buf)
        .await
        .map_err(|e| IoError::new(format!("write failed: {e}")))
}
