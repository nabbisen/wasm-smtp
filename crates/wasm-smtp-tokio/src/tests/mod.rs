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

/// Install a process-level rustls crypto provider for tests that build a
/// `ClientConfig`.
///
/// `ClientConfig::builder()` derives the provider from rustls's enabled
/// features, which is unambiguous when this crate is built on its own.
/// Under `cargo test --workspace`, however, cargo unifies rustls features
/// across the workspace — `wasm-smtp-wasi` enables `ring` while this crate
/// defaults to `aws-lc-rs` — so rustls has both compiled in and the
/// automatic choice panics. Installing one explicitly settles it. The
/// result is ignored because another test may have installed it first.
pub(crate) fn install_test_crypto_provider() {
    #[cfg(feature = "aws-lc-rs")]
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    #[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
}
