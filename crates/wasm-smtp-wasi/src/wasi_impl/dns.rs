//! DNS resolution via `wasi:sockets/ip-name-lookup`.
//!
//! WASI 0.2 provides `wasi:sockets/ip-name-lookup` as the standard name
//! resolution interface. This module wraps that interface, polling until
//! a result is available.

use crate::error::WasiSmtpError;
use wasi::sockets::ip_name_lookup::{IpAddress, resolve_addresses};
use wasi::sockets::network::{IpAddressFamily, Network};
use wasi::sockets::instance_network::instance_network;
use wasi::io::poll::poll;

/// Resolve `host` to a list of IP addresses.
///
/// Returns all addresses returned by the resolver. The caller should try
/// them in order; `connect_first` wraps this into a single connection attempt.
pub(crate) fn resolve(host: &str) -> Result<Vec<IpAddress>, WasiSmtpError> {
    let network: Network = instance_network();
    let stream = resolve_addresses(&network, host)
        .map_err(|e| WasiSmtpError::new(format!("DNS resolve failed for '{host}': {e:?}")))?;

    let mut addresses = Vec::new();
    loop {
        // Poll until data is ready.
        let pollable = stream.subscribe();
        poll(&[&pollable]);

        match stream.resolve_next_address() {
            Ok(Some(addr)) => addresses.push(addr),
            Ok(None) => break,
            Err(e) => {
                return Err(WasiSmtpError::new(format!(
                    "DNS resolve_next_address failed for '{host}': {e:?}"
                )));
            }
        }
    }

    if addresses.is_empty() {
        return Err(WasiSmtpError::new(format!("DNS: no addresses found for '{host}'")));
    }
    Ok(addresses)
}
