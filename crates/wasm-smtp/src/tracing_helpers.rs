//! Internal `tracing` helpers.
//!
//! When the `tracing` cargo feature is enabled, these forward to
//! the corresponding `tracing` macros. When the feature is
//! disabled, they compile to no-ops, so call sites can use them
//! unconditionally without scattering `#[cfg]` attributes through
//! the SMTP state machine.
//!
//! All events are emitted with `target = "wasm_smtp"` so callers
//! can filter on this single name in their `tracing-subscriber`
//! configuration.
//!
//! ## What is logged
//!
//! See the `tracing` cargo feature documentation in `Cargo.toml`.
//! In short:
//!
//! - `trace!`: individual wire frames (request and response text).
//! - `debug!`: major SMTP transitions and envelope values.
//! - `warn!`: recoverable failures.
//! - `error!`: unrecoverable failures that close the session.
//!
//! ## What is NOT logged
//!
//! Passwords, OAuth tokens, AUTH challenge bytes, SCRAM nonces,
//! message bodies. The call sites in `client.rs` carry this rule;
//! this module just provides the macros.

#![allow(unused_macros)]

/// Emit a `trace!`-level event with `target = "wasm_smtp"`.
macro_rules! smtp_trace {
    ($($arg:tt)+) => {
        #[cfg(feature = "tracing")]
        ::tracing::trace!(target: "wasm_smtp", $($arg)+);
    };
}

/// Emit a `debug!`-level event with `target = "wasm_smtp"`.
macro_rules! smtp_debug {
    ($($arg:tt)+) => {
        #[cfg(feature = "tracing")]
        ::tracing::debug!(target: "wasm_smtp", $($arg)+);
    };
}

/// Emit a `warn!`-level event with `target = "wasm_smtp"`.
macro_rules! smtp_warn {
    ($($arg:tt)+) => {
        #[cfg(feature = "tracing")]
        ::tracing::warn!(target: "wasm_smtp", $($arg)+);
    };
}

/// Emit an `error!`-level event with `target = "wasm_smtp"`.
macro_rules! smtp_error {
    ($($arg:tt)+) => {
        #[cfg(feature = "tracing")]
        ::tracing::error!(target: "wasm_smtp", $($arg)+);
    };
}

pub(crate) use {smtp_debug, smtp_error, smtp_trace, smtp_warn};
