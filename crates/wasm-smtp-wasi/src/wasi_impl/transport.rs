//! [`WasiTlsTransport`] — the main `Transport` implementation for WASI.
//!
//! Wraps a WASI TCP stream with rustls for TLS. The STARTTLS upgrade path
//! uses the same struct: the inner stream is upgraded from plaintext to
//! TLS-secured when `StartTlsCapable::upgrade_to_tls` is called.

use std::io::{Read, Write};
use std::sync::Arc;

use rustls::{ClientConnection, StreamOwned};
use rustls_pki_types::ServerName;

use wasm_smtp::IoError;
use wasm_smtp::{StartTlsCapable, Transport};

use super::dns;
use super::stream::WasiStream;
use crate::error::WasiSmtpError;
use crate::tls::{ConnectOptions, make_tls_config, server_name};

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

enum Inner {
    /// Plaintext WASI stream (used before STARTTLS upgrade).
    Plain(WasiStream),
    /// rustls-wrapped TLS stream.
    Tls(StreamOwned<ClientConnection, WasiStreamIo>),
}

/// Newtype that implements `std::io::Read + Write` over `WasiStream`,
/// enabling `rustls::StreamOwned` to wrap it.
struct WasiStreamIo(WasiStream);

impl Read for WasiStreamIo {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0
            .read(buf)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

impl Write for WasiStreamIo {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .write_all(buf)
            .map(|_| buf.len())
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0
            .flush()
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// WasiTlsTransport
// ---------------------------------------------------------------------------

/// WASI TCP transport with rustls TLS, implementing [`Transport`] and
/// [`StartTlsCapable`].
///
/// Created by the connection helpers in `wasm_smtp_wasi` (`connect_smtps`,
/// `connect_smtp_starttls`, etc.). Do not construct directly.
pub struct WasiTlsTransport {
    inner: Inner,
    /// SNI hostname used for the TLS upgrade (stored for STARTTLS path).
    sni: ServerName<'static>,
    /// TLS config (stored for STARTTLS path where TLS is deferred).
    tls_config: Option<Arc<rustls::ClientConfig>>,
}

impl WasiTlsTransport {
    /// Connect with Implicit TLS: DNS + TCP + TLS handshake.
    pub(crate) async fn connect_implicit_tls(
        host: &str,
        port: u16,
        opts: ConnectOptions,
    ) -> Result<Self, WasiSmtpError> {
        let stream = tcp_connect(host, port)?;
        let sni = server_name(opts.server_name.as_deref().unwrap_or(host))?;
        let tls_config = make_tls_config(&opts)?;
        let tls_stream = tls_handshake(stream, sni.clone(), Arc::clone(&tls_config))?;
        Ok(Self {
            inner: Inner::Tls(tls_stream),
            sni,
            tls_config: None, // Not needed; already connected.
        })
    }

    /// Connect as plaintext (for STARTTLS). TLS is deferred to
    /// `upgrade_to_tls`.
    pub(crate) async fn connect_plain(
        host: &str,
        port: u16,
    ) -> Result<Self, WasiSmtpError> {
        let opts = ConnectOptions::default();
        let stream = tcp_connect(host, port)?;
        let sni = server_name(host)?;
        let tls_config = make_tls_config(&opts)?;
        Ok(Self {
            inner: Inner::Plain(stream),
            sni,
            tls_config: Some(tls_config),
        })
    }
}

// ---------------------------------------------------------------------------
// Transport impl
// ---------------------------------------------------------------------------

impl Transport for WasiTlsTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match &mut self.inner {
            Inner::Plain(s) => s.read(buf).map_err(|e| IoError::new(e.to_string())),
            Inner::Tls(s) => {
                s.read(buf).map_err(|e| IoError::new(e.to_string()))
            }
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        match &mut self.inner {
            Inner::Plain(s) => s.write_all(buf).map_err(|e| IoError::new(e.to_string())),
            Inner::Tls(s) => {
                s.write_all(buf).map_err(|e| IoError::new(e.to_string()))?;
                Ok(())
            }
        }
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        match &mut self.inner {
            Inner::Plain(s) => s.flush().map_err(|e| IoError::new(e.to_string())),
            Inner::Tls(s) => {
                s.flush().map_err(|e| IoError::new(e.to_string()))?;
                Ok(())
            }
        }
    }

    async fn close(&mut self) -> Result<(), IoError> {
        match &mut self.inner {
            Inner::Plain(s) => s.close().map_err(|e| IoError::new(e.to_string())),
            Inner::Tls(s) => {
                // Flush and send TLS close_notify.
                s.flush().ok();
                s.conn.send_close_notify();
                // Best-effort write the close_notify bytes.
                let mut buf = Vec::new();
                s.conn.write_tls(&mut buf).ok();
                s.sock.0.write_all(&buf).ok();
                s.sock.0.close().map_err(|e| IoError::new(e.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// StartTlsCapable impl
// ---------------------------------------------------------------------------

impl StartTlsCapable for WasiTlsTransport {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError> {
        let tls_config = self
            .tls_config
            .take()
            .ok_or_else(|| IoError::new("upgrade_to_tls called on an already-TLS transport"))?;

        // Extract the plain stream.
        let plain = match std::mem::replace(&mut self.inner, Inner::Plain(dummy_stream())) {
            Inner::Plain(s) => s,
            Inner::Tls(_) => {
                return Err(IoError::new(
                    "upgrade_to_tls called on a transport that is already TLS",
                ))
            }
        };

        let tls_stream =
            tls_handshake(plain, self.sni.clone(), tls_config).map_err(|e| IoError::new(e.to_string()))?;
        self.inner = Inner::Tls(tls_stream);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn tcp_connect(host: &str, port: u16) -> Result<WasiStream, WasiSmtpError> {
    let addrs = dns::resolve(host)?;
    let mut last_err = WasiSmtpError::new(format!("no addresses resolved for '{host}'"));
    for addr in addrs {
        match WasiStream::connect(addr, port) {
            Ok(s) => return Ok(s),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn tls_handshake(
    stream: WasiStream,
    sni: ServerName<'static>,
    config: Arc<rustls::ClientConfig>,
) -> Result<StreamOwned<ClientConnection, WasiStreamIo>, WasiSmtpError> {
    let conn = ClientConnection::new(config, sni)
        .map_err(|e| WasiSmtpError::new(format!("TLS ClientConnection::new failed: {e}")))?;
    let mut tls = StreamOwned::new(conn, WasiStreamIo(stream));
    // Drive the handshake.
    tls.flush()
        .map_err(|e| WasiSmtpError::new(format!("TLS handshake failed: {e}")))?;
    Ok(tls)
}

/// Construct a placeholder `WasiStream` for `mem::replace`. Never used.
#[cold]
fn dummy_stream() -> WasiStream {
    panic!("dummy_stream must never be used for I/O")
}
