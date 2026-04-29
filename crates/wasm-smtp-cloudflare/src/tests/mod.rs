//! Internal test suite for `wasm-smtp-cloudflare`.
//!
//! These tests run on the host (`cargo test`); they do **not**
//! exercise `worker::Socket`, which requires the Cloudflare Workers
//! runtime. Instead, they cover the byte-pushing helpers
//! [`crate::adapter::read_async_io`] and
//! [`crate::adapter::write_all_async_io`] using
//! `tokio_test::io::Builder` as a stand-in for the real socket.
//!
//! Coverage for the full SMTP exchange — greeting, `EHLO`, `AUTH
//! LOGIN`, `MAIL FROM`/`RCPT TO`/`DATA`/`QUIT` — lives in
//! `wasm-smtp`'s in-tree mock-driven integration tests. The
//! adapter does not duplicate that work; it only verifies that the
//! `tokio::io` ↔ `wasm-smtp::Transport` translation is correct.
//!
//! End-to-end tests against a real submission server require a
//! Cloudflare Workers runtime (`wrangler dev`) and are not run by
//! `cargo test`.

#![allow(
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]

mod e2e_via_tokio_mock;
mod io_tests;
