//! Tokio + rustls `Transport` and `StartTlsCapable` impls.
//!
//! Two transport types are exposed:
//!
//! - [`TokioTlsTransport`]: a TLS-wrapped TCP connection. Use this for
//!   implicit-TLS SMTP submission (port 465) or once a STARTTLS
//!   upgrade has succeeded.
//! - [`TokioPlainTransport`]: a plaintext TCP connection that can
//!   later be upgraded to TLS by [`wasm_smtp::SmtpClient::starttls`]
//!   via the [`StartTlsCapable`] trait. Use this for STARTTLS-on-587.
//!
//! Both implement [`wasm_smtp::Transport`]. `TokioPlainTransport`
//! additionally implements [`StartTlsCapable`].

use std::io;
use std::sync::Arc;

use rustls_pki_types::ServerName;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

use wasm_smtp::{IoError, StartTlsCapable, Transport};

// -- ConnectOptions ---------------------------------------------------------

/// Connection-time options for [`TokioTlsTransport::connect_with`].
///
/// Construct with [`Self::new`] and chain builder-style setters. All
/// fields are optional; defaults (system or webpki trust roots, SNI
/// taken from the connect host argument) work for the common case.
#[derive(Debug, Clone, Default)]
pub struct ConnectOptions {
    /// Override SNI / certificate-name verification target. When `None`,
    /// the SNI is set from the `host` argument of the connect call.
    server_name: Option<String>,
    /// Custom root certificate store. When `None`, the trust anchors
    /// configured by the active cargo feature (`native-roots` or
    /// `webpki-roots`) are used.
    root_store: Option<RootCertStore>,
    /// ALPN protocol identifiers, in preference order. Most SMTP
    /// servers do not advertise ALPN; leave empty unless you know your
    /// server does.
    alpn: Vec<Vec<u8>>,
}

impl ConnectOptions {
    /// Create a new [`ConnectOptions`] with all fields at their
    /// defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the SNI / certificate-name verification target.
    ///
    /// By default the connect helpers use the `host` argument as the
    /// SNI. Override only when the server certificate is issued for
    /// a different name than you connect to (e.g. internal-DNS to
    /// public-name mapping behind a load balancer).
    #[must_use]
    pub fn with_server_name<S: Into<String>>(mut self, name: S) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Replace the trust-anchor set with a custom [`RootCertStore`].
    ///
    /// Use this to validate against an internal / private CA, or to
    /// trust a development-only self-signed certificate. The
    /// resulting `Transport` does **not** also use the default trust
    /// roots; the supplied store is the entire trust set.
    #[must_use]
    pub fn with_root_store(mut self, store: RootCertStore) -> Self {
        self.root_store = Some(store);
        self
    }

    /// Set the ALPN protocols offered during the TLS handshake.
    ///
    /// Most SMTP servers do not advertise ALPN. Leave empty unless
    /// you have a specific reason.
    #[must_use]
    pub fn with_alpn(mut self, protocols: &[&[u8]]) -> Self {
        self.alpn = protocols.iter().map(|p| p.to_vec()).collect();
        self
    }
}

// -- Trust anchor selection -------------------------------------------------

/// Build a [`RootCertStore`] from the trust source selected by feature
/// flags, falling back to a clear compile-time error if neither is on.
//
// The Result return type is uniform across feature combinations even
// though one combination (webpki-roots-only) currently has no failure
// path. `clippy::unnecessary_wraps` is allowed so the signature stays
// stable as feature gates evolve.
#[allow(clippy::unnecessary_wraps)]
fn default_root_store() -> Result<RootCertStore, IoError> {
    let mut store = RootCertStore::empty();

    #[cfg(feature = "native-roots")]
    {
        let certs = rustls_native_certs::load_native_certs()
            .map_err(|_| IoError::new("failed to load native trust store"))?;
        for cert in certs {
            // Per rustls-native-certs convention: malformed certs
            // returned by the OS are skipped here rather than failing
            // the whole build. Successful certs still populate the
            // store.
            let _ = store.add(cert);
        }
        if store.is_empty() {
            return Err(IoError::new(
                "rustls-native-certs returned an empty trust store; the OS \
                 trust store may be missing or unreadable",
            ));
        }
        Ok(store)
    }

    #[cfg(all(feature = "webpki-roots", not(feature = "native-roots")))]
    {
        store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Ok(store)
    }

    #[cfg(not(any(feature = "native-roots", feature = "webpki-roots")))]
    {
        // We can't return Err at compile time, but at this point we
        // know the function will never produce a useful store. Make
        // the failure mode loud at runtime; the README and crate-
        // level doc both flag this as a config error.
        Err(IoError::new(
            "wasm-smtp-tokio was built without a trust-anchor source. \
             Enable the `native-roots` or `webpki-roots` cargo feature.",
        ))
    }
}

/// Build the rustls `ClientConfig` for a connect call.
fn build_client_config(opts: &ConnectOptions) -> Result<Arc<ClientConfig>, IoError> {
    let root_store = match &opts.root_store {
        Some(s) => s.clone(),
        None => default_root_store()?,
    };

    let mut config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    if !opts.alpn.is_empty() {
        config.alpn_protocols.clone_from(&opts.alpn);
    }

    Ok(Arc::new(config))
}

fn map_io_err(_e: &io::Error, context: &'static str) -> IoError {
    // wasm_smtp::IoError requires a 'static str for `new`. Until we
    // can attach a Box<dyn Error> source (Phase 12 candidate), we
    // discard the underlying io::Error detail and surface a fixed
    // context string. The original error is reachable to the
    // adapter author at debug time via the same call site.
    IoError::new(context)
}

// -- TokioPlainTransport (for STARTTLS) -------------------------------------

/// Plaintext TCP transport, primed to be upgraded to TLS via
/// [`StartTlsCapable`].
///
/// Use this for STARTTLS submission (port 587). Construct with
/// [`Self::connect`], hand it to [`wasm_smtp::SmtpClient::connect_starttls`],
/// and `wasm-smtp` will drive the upgrade.
///
/// For implicit-TLS submission (port 465), use [`TokioTlsTransport`]
/// instead — it does the TLS handshake at construction time.
pub struct TokioPlainTransport {
    /// `Some` while the connection is plaintext; taken (left as `None`)
    /// once `upgrade_to_tls` consumes the TCP stream and replaces it
    /// with a TLS-wrapped stream. The TLS-wrapped stream lives in
    /// `tls`; this field tracks the pre-upgrade phase.
    plain: Option<TcpStream>,
    /// `Some` after a successful `upgrade_to_tls`. Reads/writes
    /// flow through here once present.
    tls: Option<TlsStream<TcpStream>>,
    /// SNI / certificate-name target for the eventual STARTTLS upgrade.
    /// Captured at connect time so the upgrade can be self-contained.
    server_name: String,
    /// Connection options retained so the upgrade can rebuild the
    /// rustls config consistently with the caller's intent.
    opts: ConnectOptions,
}

impl TokioPlainTransport {
    /// Open a plaintext TCP connection. The transport is suitable for
    /// passing to [`wasm_smtp::SmtpClient::connect_starttls`].
    ///
    /// `server_name` is the name to verify the server certificate
    /// against once the STARTTLS upgrade happens; usually equal to
    /// `host`.
    pub async fn connect(host: &str, port: u16, server_name: &str) -> Result<Self, IoError> {
        Self::connect_with(host, port, server_name, ConnectOptions::new()).await
    }

    /// Open a plaintext TCP connection with custom [`ConnectOptions`].
    ///
    /// The options are remembered and used by the eventual
    /// [`StartTlsCapable::upgrade_to_tls`] call. They have no effect
    /// before then — the plaintext leg is just a TCP socket.
    pub async fn connect_with(
        host: &str,
        port: u16,
        server_name: &str,
        opts: ConnectOptions,
    ) -> Result<Self, IoError> {
        let stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| map_io_err(&e, "TCP connect failed"))?;
        Ok(Self {
            plain: Some(stream),
            tls: None,
            server_name: server_name.to_string(),
            opts,
        })
    }

    fn effective_server_name(&self) -> &str {
        self.opts
            .server_name
            .as_deref()
            .unwrap_or(&self.server_name)
    }
}

impl Transport for TokioPlainTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if let Some(s) = self.tls.as_mut() {
            s.read(buf).await.map_err(|e| map_io_err(&e, "read failed"))
        } else if let Some(s) = self.plain.as_mut() {
            s.read(buf).await.map_err(|e| map_io_err(&e, "read failed"))
        } else {
            Err(IoError::new("transport in invalid state: no live stream"))
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if let Some(s) = self.tls.as_mut() {
            s.write_all(buf)
                .await
                .map_err(|e| map_io_err(&e, "write failed"))
        } else if let Some(s) = self.plain.as_mut() {
            s.write_all(buf)
                .await
                .map_err(|e| map_io_err(&e, "write failed"))
        } else {
            Err(IoError::new("transport in invalid state: no live stream"))
        }
    }

    async fn close(&mut self) -> Result<(), IoError> {
        if let Some(mut s) = self.tls.take() {
            // shutdown writes the TLS close_notify; ignore errors at
            // close time per the trait contract (the SMTP QUIT has
            // already been sent by the core).
            let _ = s.shutdown().await;
        }
        if let Some(mut s) = self.plain.take() {
            let _ = s.shutdown().await;
        }
        Ok(())
    }
}

impl StartTlsCapable for TokioPlainTransport {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError> {
        let plain = self
            .plain
            .take()
            .ok_or_else(|| IoError::new("upgrade_to_tls called twice or after close"))?;

        let config = build_client_config(&self.opts)?;
        let connector = TlsConnector::from(config);
        let server_name = ServerName::try_from(self.effective_server_name().to_owned())
            .map_err(|_| IoError::new("invalid server name for SNI"))?;

        let tls = connector
            .connect(server_name, plain)
            .await
            .map_err(|e| map_io_err(&e, "TLS handshake failed"))?;

        self.tls = Some(tls);
        Ok(())
    }
}

// -- TokioTlsTransport (implicit TLS) ---------------------------------------

/// TLS-wrapped TCP transport for implicit-TLS submission (port 465).
///
/// Construct with [`Self::connect_implicit_tls`] for the common case,
/// or [`Self::connect_with`] when you need custom trust anchors,
/// alternate SNI, or ALPN.
pub struct TokioTlsTransport {
    tls: Option<TlsStream<TcpStream>>,
}

impl TokioTlsTransport {
    /// Connect to the SMTP server with implicit TLS.
    ///
    /// The TCP handshake and the TLS handshake both complete before
    /// this returns. `server_name` is the SNI / certificate hostname
    /// to verify against; it is usually equal to `host`.
    ///
    /// Equivalent to [`Self::connect_with`] with default
    /// [`ConnectOptions`] and the supplied SNI.
    pub async fn connect_implicit_tls(
        host: &str,
        port: u16,
        server_name: &str,
    ) -> Result<Self, IoError> {
        Self::connect_with(
            host,
            port,
            ConnectOptions::new().with_server_name(server_name),
        )
        .await
    }

    /// Connect with custom [`ConnectOptions`].
    ///
    /// If `opts.server_name` is `None`, `host` is used as the SNI.
    pub async fn connect_with(
        host: &str,
        port: u16,
        opts: ConnectOptions,
    ) -> Result<Self, IoError> {
        let tcp = TcpStream::connect((host, port))
            .await
            .map_err(|e| map_io_err(&e, "TCP connect failed"))?;

        let config = build_client_config(&opts)?;
        let connector = TlsConnector::from(config);
        let sni_string = opts.server_name.unwrap_or_else(|| host.to_string());
        let server_name = ServerName::try_from(sni_string)
            .map_err(|_| IoError::new("invalid server name for SNI"))?;

        let tls = connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| map_io_err(&e, "TLS handshake failed"))?;

        Ok(Self { tls: Some(tls) })
    }
}

impl Transport for TokioTlsTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self.tls.as_mut() {
            Some(s) => s.read(buf).await.map_err(|e| map_io_err(&e, "read failed")),
            None => Err(IoError::new("transport already closed")),
        }
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        match self.tls.as_mut() {
            Some(s) => s
                .write_all(buf)
                .await
                .map_err(|e| map_io_err(&e, "write failed")),
            None => Err(IoError::new("transport already closed")),
        }
    }

    async fn close(&mut self) -> Result<(), IoError> {
        if let Some(mut s) = self.tls.take() {
            let _ = s.shutdown().await;
        }
        Ok(())
    }
}
