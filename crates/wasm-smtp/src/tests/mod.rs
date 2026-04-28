//! Internal test suite for `wasm-smtp`.
//!
//! These tests exercise every layer of the crate that does not require
//! a real network. They live in-tree (rather than under the `tests/`
//! integration-test directory at the crate root) so they can reach
//! `pub(crate)` items and module-private helpers without inflating the
//! public API surface.
//!
//! Module overview:
//!
//! - [`harness`] — `MockTransport` and small shared utilities.
//! - [`protocol_tests`] — reply parsing, command formatting,
//!   dot-stuffing, base64, validators, capability inspection.
//! - [`session_tests`] — `SessionState` transitions.
//! - [`error_tests`] — public error surface.
//! - [`client_tests`] — full SMTP exchange against the mock transport.
//! - [`smtputf8_tests`] — feature-gated SMTPUTF8 (RFC 6531).
//!
//! There is no executor: the mock transport always resolves immediately,
//! so a no-op waker is sufficient to drive the futures.

#![allow(
    // These pedantic lints are useful in production code but produce a lot
    // of noise in test fixtures, where short scripts and explicit byte
    // literals are the norm.
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::missing_panics_doc
)]

mod client_tests;
mod error_tests;
mod harness;
mod protocol_tests;
mod session_tests;

#[cfg(feature = "smtputf8")]
mod smtputf8_tests;
