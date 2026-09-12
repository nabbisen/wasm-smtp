//! Low-level WASI TCP stream that wraps `wasi:sockets/tcp` into a simple
//! byte-oriented read/write interface consumed by `WasiTlsTransport`.

use crate::error::WasiSmtpError;
use wasi::io::poll::poll;
use wasi::io::streams::{InputStream, OutputStream, StreamError};
use wasi::sockets::instance_network::instance_network;
use wasi::sockets::network::IpAddressFamily;
use wasi::sockets::network::{IpAddress, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress};
use wasi::sockets::tcp::{ShutdownType, TcpSocket};
use wasi::sockets::tcp_create_socket::create_tcp_socket;

/// Maximum payload accepted by `blocking-write-and-flush` in WASI 0.2.
const MAX_BLOCKING_WRITE: usize = 4096;

/// A connected, plaintext WASI TCP stream.
///
/// **Field order is load-bearing.** In WASI 0.2 the input and output
/// streams are child resources of the socket, and dropping a parent
/// resource while a child is still alive traps the guest. Rust drops
/// struct fields in declaration order, so the streams must be declared
/// before the socket. Reordering these fields reintroduces a trap that
/// only shows up on a real host.
pub(crate) struct WasiStream {
    reader: InputStream,
    writer: OutputStream,
    socket: TcpSocket,
}

impl WasiStream {
    /// Connect to `addr:port` over WASI TCP.
    pub(crate) fn connect(addr: IpAddress, port: u16) -> Result<Self, WasiSmtpError> {
        let family = match addr {
            IpAddress::Ipv4(_) => IpAddressFamily::Ipv4,
            IpAddress::Ipv6(_) => IpAddressFamily::Ipv6,
        };

        let socket = create_tcp_socket(family)
            .map_err(|e| WasiSmtpError::new(format!("create_tcp_socket failed: {e:?}")))?;

        let network = instance_network();
        let remote_addr = match addr {
            IpAddress::Ipv4(a) => IpSocketAddress::Ipv4(Ipv4SocketAddress { port, address: a }),
            IpAddress::Ipv6(a) => IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port,
                address: a,
                flow_info: 0,
                scope_id: 0,
            }),
        };

        // Initiate the non-blocking connect.
        socket
            .start_connect(&network, remote_addr)
            .map_err(|e| WasiSmtpError::new(format!("start_connect failed: {e:?}")))?;

        // Poll until connected.
        {
            let p = socket.subscribe();
            poll(&[&p]);
        }

        let (reader, writer) = socket
            .finish_connect()
            .map_err(|e| WasiSmtpError::new(format!("finish_connect failed: {e:?}")))?;

        Ok(Self {
            reader,
            writer,
            socket,
        })
    }

    /// Read up to `buf.len()` bytes.
    ///
    /// Returns the number of bytes read, or 0 on EOF — and 0 means EOF and
    /// nothing else, because the core reads it as "peer closed" (RFC 005).
    ///
    /// Two things conspire against that contract:
    ///
    /// - The non-blocking `read` returns an empty list whenever no bytes
    ///   are ready yet, which is indistinguishable here from a closed peer.
    ///   `blocking-read` is the right primitive: it returns once at least
    ///   one byte is available or the stream closes.
    /// - `blocking-read` nevertheless *can* return an empty list. Observed
    ///   under wasmtime 27 on every reply after the first, which is why the
    ///   empty case blocks again rather than reporting EOF. Reporting it as
    ///   EOF is the defect this crate shipped with (RFC 025 defect 2), and
    ///   it looks exactly like a server that hangs up mid-session.
    ///
    /// The loop has no iteration cap on purpose: a blocking read with no
    /// data and no close is a stalled peer, which is the caller's timeout
    /// to impose, not this layer's. It does wait on the stream's pollable
    /// before retrying, which is what turns a host that returns empty
    /// *without* having blocked into a wait rather than a spin burning CPU
    /// budget on host calls until the next byte lands.
    pub(crate) fn read(&mut self, buf: &mut [u8]) -> Result<usize, WasiSmtpError> {
        loop {
            match self.reader.blocking_read(buf.len() as u64) {
                // Not ready yet despite the name. Wait for readiness, then
                // block again.
                Ok(bytes) if bytes.is_empty() => {
                    let pollable = self.reader.subscribe();
                    poll(&[&pollable]);
                    continue;
                }
                Ok(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    return Ok(n);
                }
                // The one clean-EOF signal the interface gives us.
                Err(StreamError::Closed) => return Ok(0),
                Err(StreamError::LastOperationFailed(e)) => {
                    return Err(WasiSmtpError::new(format!(
                        "stream read failed: {}",
                        e.to_debug_string()
                    )));
                }
            }
        }
    }

    /// Write all bytes in `buf` to the stream.
    ///
    /// `blocking-write-and-flush` accepts at most 4096 bytes per call in
    /// WASI 0.2 and returns once they have been handed to the peer, so the
    /// buffer is chunked and no hand-rolled check-write/poll loop is
    /// needed.
    pub(crate) fn write_all(&mut self, buf: &[u8]) -> Result<(), WasiSmtpError> {
        for chunk in buf.chunks(MAX_BLOCKING_WRITE) {
            self.writer.blocking_write_and_flush(chunk).map_err(|e| {
                WasiSmtpError::new(match e {
                    StreamError::Closed => "stream write failed: stream closed".to_owned(),
                    StreamError::LastOperationFailed(e) => {
                        format!("stream write failed: {:?}", e.to_debug_string())
                    }
                })
            })?;
        }
        Ok(())
    }

    /// Flush the write buffer.
    ///
    /// A no-op in practice: `write_all` uses `blocking-write-and-flush`, so
    /// nothing is left buffered by the time this is called. Kept because the
    /// `Transport` contract exposes `flush` and the TLS layer calls it.
    pub(crate) fn flush(&mut self) -> Result<(), WasiSmtpError> {
        Ok(())
    }

    /// Close the connection.
    pub(crate) fn close(&mut self) -> Result<(), WasiSmtpError> {
        self.socket
            .shutdown(ShutdownType::Both)
            .map_err(|e| WasiSmtpError::new(format!("shutdown failed: {e:?}")))?;
        Ok(())
    }
}
