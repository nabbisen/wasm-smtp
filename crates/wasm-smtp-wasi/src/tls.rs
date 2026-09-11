//! TLS configuration for the WASI adapter.
//!
//! [`ConnectOptions`] controls certificate validation, SNI, and ALPN.
//! The actual TLS implementation uses [rustls]; on `wasm32-wasip2` it wraps
//! WASI byte streams; on native hosts it is used only for testing.

use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls_pki_types::ServerName;
use std::sync::Arc;

use crate::error::WasiSmtpError;

/// Options controlling TLS behaviour for [`connect_smtps`][crate::connect_smtps]
/// and [`connect_smtps_with`][crate::connect_smtps_with].
///
/// Construct with [`Self::default`] for the common case (system or bundled
/// trust roots, SNI from the connect host). Use the builder methods to
/// override.
#[derive(Clone, Default)]
pub struct ConnectOptions {
    /// Override the SNI hostname. When `None`, the `host` argument of the
    /// connect call is used.
    pub(crate) server_name: Option<String>,
    /// Custom root certificate store. When `None`, the default trust anchors
    /// from the active cargo feature (`webpki-roots` or `native-roots`) are
    /// used.
    pub(crate) root_store: Option<RootCertStore>,
    /// ALPN protocols in preference order. Leave empty for standard SMTP
    /// servers (they do not advertise ALPN).
    pub(crate) alpn: Vec<Vec<u8>>,
}

impl ConnectOptions {
    /// Override the SNI / certificate-name verification target.
    ///
    /// By default the `host` argument of the connect call is used as the SNI.
    /// Override only when the server certificate is issued for a different
    /// hostname (e.g. behind an internal load balancer with a different DNS
    /// name than the public hostname).
    #[must_use]
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = Some(name.into());
        self
    }

    /// Replace the trust-anchor set with a custom [`RootCertStore`].
    ///
    /// The supplied store is the **entire** trust set — the default anchors
    /// are not also included. Use this to validate against a private CA or a
    /// development self-signed certificate.
    #[must_use]
    pub fn with_root_store(mut self, store: RootCertStore) -> Self {
        self.root_store = Some(store);
        self
    }

    /// Set ALPN protocols offered during the TLS handshake.
    #[must_use]
    pub fn with_alpn(mut self, protocols: &[&[u8]]) -> Self {
        self.alpn = protocols.iter().map(|p| p.to_vec()).collect();
        self
    }
}

// ---------------------------------------------------------------------------
// Build a rustls ClientConfig from ConnectOptions
// ---------------------------------------------------------------------------

/// Construct a [`ClientConfig`] from [`ConnectOptions`].
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn make_tls_config(opts: &ConnectOptions) -> Result<Arc<ClientConfig>, WasiSmtpError> {
    let root_store = if let Some(store) = opts.root_store.clone() {
        store
    } else {
        default_root_store()?
    };

    let mut config = ClientConfig::builder_with_protocol_versions(rustls::DEFAULT_VERSIONS)
        .with_root_certificates(root_store)
        .with_no_client_auth();

    if !opts.alpn.is_empty() {
        config.alpn_protocols.clone_from(&opts.alpn);
    }

    Ok(Arc::new(config))
}

/// Build the default trust-anchor set from the active cargo feature.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn default_root_store() -> Result<RootCertStore, WasiSmtpError> {
    let mut store = RootCertStore::empty();

    #[cfg(feature = "webpki-roots")]
    {
        store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        return Ok(store);
    }

    #[cfg(feature = "native-roots")]
    {
        // rustls-native-certs 0.8 returns a `CertificateResult` struct
        // carrying both the certs that decoded and any per-source errors,
        // leaving the partial-failure policy to the caller. We adopt the
        // same policy as `wasm-smtp-tokio`: keep every cert that decoded,
        // fail only if the resulting trust store is empty.
        let result = rustls_native_certs::load_native_certs();
        for cert in result.certs {
            let _ = store.add(cert);
        }
        if store.is_empty() {
            return Err(WasiSmtpError::new(if result.errors.is_empty() {
                "rustls-native-certs returned an empty trust store; the OS \
                 trust store may be missing or unreadable"
            } else {
                "rustls-native-certs returned an empty trust store; the OS \
                 trust store may be missing or unreadable, and one or more \
                 sources reported errors"
            }));
        }
        return Ok(store);
    }

    // Neither feature is enabled — this is a configuration error.
    #[allow(unreachable_code)]
    Err(WasiSmtpError::new(
        "no trust-anchor source configured: enable the `webpki-roots` \
         or `native-roots` cargo feature",
    ))
}

/// Build a rustls [`ServerName`] from a hostname string.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn server_name(host: &str) -> Result<ServerName<'static>, WasiSmtpError> {
    ServerName::try_from(host.to_owned())
        .map_err(|e| WasiSmtpError::new(format!("invalid server name '{host}': {e}")))
}
