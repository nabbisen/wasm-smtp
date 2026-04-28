//! [`CloudflareTransport`] — the [`Transport`] implementation that
//! wraps a Cloudflare Workers [`worker::Socket`].
//!
//! The adapter is intentionally thin. The bulk of the byte-pushing is
//! done by the `tokio::io` traits already implemented on
//! `worker::Socket`; this module only translates between those traits
//! and the [`Transport`] contract from `wasm-smtp-core`, and converts
//! Workers-side errors into [`IoError`]'s string-based representation.
//!
//! The two free helpers `read_async_io` and `write_all_async_io`
//! are factored out from the trait impl so that they can be unit-tested
//! on host targets with `tokio_test::io::Builder` (see `tests.rs`).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use wasm_smtp_core::{IoError, Transport};
use worker::Socket;

/// SMTP transport backed by a Cloudflare Workers TCP socket.
///
/// Construct via [`crate::connect_implicit_tls`]
/// or [`crate::connect_smtps`].
///
/// The wrapped `worker::Socket` is opened with
/// `SecureTransport::On`, so the byte stream presented to
/// `wasm-smtp-core` is already TLS-secured.
pub struct CloudflareTransport {
    socket: Socket,
}

impl CloudflareTransport {
    /// Wrap a pre-connected [`Socket`] as a [`Transport`].
    ///
    /// This constructor does not perform the connect; use
    /// [`crate::connect_implicit_tls`] for the standard Implicit-TLS
    /// path. It is exposed mainly so that tests and advanced callers
    /// can supply a `Socket` they constructed themselves (e.g. with
    /// non-default `SocketOptions`).
    #[must_use]
    pub fn from_socket(socket: Socket) -> Self {
        Self { socket }
    }

    /// Consume the transport and return the inner `worker::Socket`.
    ///
    /// Useful when the caller has finished with SMTP and wants to
    /// observe the socket's `closed()` future or its `opened()` info.
    #[must_use]
    pub fn into_inner(self) -> Socket {
        self.socket
    }
}

impl core::fmt::Debug for CloudflareTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CloudflareTransport")
            .finish_non_exhaustive()
    }
}

impl Transport for CloudflareTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        read_async_io(&mut self.socket, buf).await
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        write_all_async_io(&mut self.socket, buf).await
    }

    async fn close(&mut self) -> Result<(), IoError> {
        // `worker::Socket::close` shuts down both halves cleanly. This
        // is preferable to `AsyncWriteExt::shutdown`, which only closes
        // the writable side.
        self.socket
            .close()
            .await
            .map_err(|e| IoError::new(format!("socket close failed: {e}")))
    }
}

// -----------------------------------------------------------------------------
// Internal helpers (unit-testable with tokio_test mocks)
// -----------------------------------------------------------------------------

/// Read up to `buf.len()` bytes from any `tokio::io::AsyncRead` stream
/// into [`IoError`]-flavored result.
///
/// `Ok(0)` propagates the `AsyncRead` convention for "peer closed
/// cleanly", which `wasm-smtp-core` interprets as
/// [`ProtocolError::UnexpectedClose`] when a reply is mid-assembly.
///
/// [`ProtocolError::UnexpectedClose`]: wasm_smtp_core::ProtocolError::UnexpectedClose
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
