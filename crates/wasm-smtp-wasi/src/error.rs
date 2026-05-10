//! Error types for the WASI adapter layer.

use core::fmt;
use wasm_smtp::IoError;

/// An error originating in the WASI socket or DNS layer.
///
/// Converted to [`IoError`] before surfacing through `wasm-smtp`'s public API.
#[derive(Debug)]
pub struct WasiSmtpError {
    message: String,
}

impl WasiSmtpError {
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for WasiSmtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WasiSmtpError {}

impl From<WasiSmtpError> for IoError {
    fn from(e: WasiSmtpError) -> Self {
        IoError::with_source(e.message.clone(), e)
    }
}
