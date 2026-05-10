//! Low-level WASI TCP stream that wraps `wasi:sockets/tcp` into a simple
//! byte-oriented read/write interface consumed by `WasiTlsTransport`.

use crate::error::WasiSmtpError;
use wasi::io::streams::{InputStream, OutputStream, StreamError};
use wasi::io::poll::poll;
use wasi::sockets::tcp::{ShutdownType, TcpSocket};
use wasi::sockets::network::{IpAddress, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress};
use wasi::sockets::tcp_create_socket::create_tcp_socket;
use wasi::sockets::network::IpAddressFamily;
use wasi::sockets::instance_network::instance_network;

/// A connected, plaintext WASI TCP stream.
pub(crate) struct WasiStream {
    socket: TcpSocket,
    reader: InputStream,
    writer: OutputStream,
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
            IpAddress::Ipv4(a) => IpSocketAddress::Ipv4(Ipv4SocketAddress {
                port,
                address: a,
            }),
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

        Ok(Self { socket, reader, writer })
    }

    /// Read up to `buf.len()` bytes.
    ///
    /// Returns the number of bytes read, or 0 on EOF.
    pub(crate) fn read(&mut self, buf: &mut [u8]) -> Result<usize, WasiSmtpError> {
        loop {
            match self.reader.read(buf.len() as u64) {
                Ok(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    return Ok(n);
                }
                Err(StreamError::Closed) => return Ok(0),
                Err(StreamError::LastOperationFailed(e)) => {
                    // Would-block: poll and retry.
                    let p = self.reader.subscribe();
                    poll(&[&p]);
                    // Try once more after polling; propagate on second failure.
                    match self.reader.read(buf.len() as u64) {
                        Ok(bytes) => {
                            let n = bytes.len().min(buf.len());
                            buf[..n].copy_from_slice(&bytes[..n]);
                            return Ok(n);
                        }
                        Err(StreamError::Closed) => return Ok(0),
                        Err(_) => return Err(WasiSmtpError::new(
                            format!("stream read failed: {e:?}")
                        )),
                    }
                }
            }
        }
    }

    /// Write all bytes in `buf` to the stream.
    pub(crate) fn write_all(&mut self, buf: &[u8]) -> Result<(), WasiSmtpError> {
        let mut written = 0;
        while written < buf.len() {
            // Check how many bytes the writer can accept.
            let capacity = self.writer.check_write()
                .map_err(|e| WasiSmtpError::new(format!("check_write failed: {e:?}")))?;

            if capacity == 0 {
                let p = self.writer.subscribe();
                poll(&[&p]);
                continue;
            }

            let chunk_len = (buf.len() - written).min(capacity as usize);
            self.writer
                .write(&buf[written..written + chunk_len])
                .map_err(|e| WasiSmtpError::new(format!("write failed: {e:?}")))?;
            written += chunk_len;
        }
        Ok(())
    }

    /// Flush the write buffer.
    pub(crate) fn flush(&mut self) -> Result<(), WasiSmtpError> {
        self.writer
            .flush()
            .map_err(|e| WasiSmtpError::new(format!("flush failed: {e:?}")))?;
        // Poll until flush completes.
        let p = self.writer.subscribe();
        poll(&[&p]);
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
