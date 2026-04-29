//! Internal test suite for `wasm-smtp-tokio`.
//!
//! These tests cover the parts of the adapter that can be exercised
//! without a real TLS-speaking SMTP server: input validation, error
//! paths, the structure of `ConnectOptions`, and the lifecycle
//! invariants of `TokioPlainTransport`'s pre/post-upgrade state.
//!
//! End-to-end tests against a real submission server (Gmail,
//! Postmark, Mailpit, etc.) live outside `cargo test` and are run by
//! the consumer of this crate during their integration cycle.

#![allow(clippy::missing_panics_doc, clippy::too_many_lines)]

mod connect_options_tests;
mod error_path_tests;
