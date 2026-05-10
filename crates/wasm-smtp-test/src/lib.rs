//! # wasm-smtp-test
//!
//! Test transport and fixtures for [`wasm-smtp`].
//!
//! This crate provides [`MockTransport`]: a synchronous, scripted SMTP
//! transport that can be driven by `#[test]` without an async executor.
//! It is intended for testing code that uses `wasm-smtp` — both in-tree
//! tests inside `wasm-smtp` itself and downstream crates that build on top
//! of it.
//!
//! **This crate is for development and testing only.** It must never be
//! used in production code. There is no `dangerous_configuration`-style
//! escape hatch — `MockTransport` simply does not do network I/O at all.
//!
//! ## Quick start
//!
//! ```rust
//! use wasm_smtp_test::{MockTransport, block_on};
//! use wasm_smtp::SmtpClient;
//!
//! let greeting = b"220 test.example.com ESMTP\r\n";
//! let ehlo_ok  = b"250-test.example.com\r\n250-AUTH PLAIN LOGIN\r\n250 OK\r\n";
//! let auth_ok  = b"235 2.7.0 Authentication successful\r\n";
//! let mail_ok  = b"250 2.1.0 OK\r\n";
//! let rcpt_ok  = b"250 2.1.5 OK\r\n";
//! let data_go  = b"354 Start mail input\r\n";
//! let data_ok  = b"250 2.0.0 OK: queued\r\n";
//! let quit_ok  = b"221 2.0.0 Bye\r\n";
//!
//! let (transport, written, _closed) = MockTransport::new(&[
//!     greeting, ehlo_ok, auth_ok, mail_ok, rcpt_ok, data_go, data_ok, quit_ok,
//! ]);
//!
//! let result = block_on(async {
//!     let mut client = SmtpClient::connect(transport, "client.example.com").await?;
//!     client.login("user", "pass").await?;
//!     client.send_mail(
//!         "user@example.com",
//!         &["to@example.com"],
//!         "Subject: hi\r\n\r\nhello\r\n",
//!     ).await?;
//!     client.quit().await
//! });
//! assert!(result.is_ok(), "send failed: {result:?}");
//!
//! let sent = String::from_utf8(written.borrow().clone()).unwrap();
//! assert!(sent.contains("EHLO client.example.com\r\n"));
//! assert!(sent.contains("MAIL FROM:<user@example.com>\r\n"));
//! ```

mod transport;

pub use transport::{MockHandles, MockStartTlsHandles, MockTransport, UpgradeBehavior};

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

/// Drive a future to completion using a no-op waker.
///
/// This works correctly only for futures that always resolve synchronously —
/// i.e. futures that never return `Poll::Pending`. [`MockTransport`] satisfies
/// this property: its `read` and `write_all` implementations are synchronous
/// byte-buffer operations that always return `Poll::Ready`.
///
/// # Panics
///
/// Panics if the future returns `Poll::Pending`. This indicates a bug in
/// the test — either the future performs real async I/O, or the mock
/// transport ran out of scripted responses before the session completed.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!(
            "block_on: future returned Pending — \
             the MockTransport is probably out of scripted responses, \
             or the future performs real async I/O"
        ),
    }
}

/// Concatenate several byte slices into one `Vec<u8>`.
///
/// Useful for assembling a scripted server reply that must be delivered
/// in a single chunk (e.g. a multi-line EHLO response):
///
/// ```rust
/// use wasm_smtp_test::flatten;
///
/// let ehlo = flatten(&[
///     b"250-mail.example.com\r\n",
///     b"250-AUTH PLAIN LOGIN\r\n",
///     b"250 OK\r\n",
/// ]);
/// assert_eq!(&ehlo[..4], b"250-");
/// ```
#[must_use]
pub fn flatten(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}
