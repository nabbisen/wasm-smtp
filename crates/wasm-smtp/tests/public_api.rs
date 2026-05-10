//! Integration tests for `wasm-smtp`.
//!
//! These tests exercise only the public API — [`SmtpClient`],
//! [`SmtpClientOptions`], and the public error types — using a
//! self-contained `TestTransport` defined below. This keeps the test
//! free of any dependency on `wasm-smtp-test`, which would create a
//! circular dependency (`wasm-smtp-test` → `wasm-smtp` → `wasm-smtp-test`).
//!
//! For low-level protocol and internal state-machine tests, see the unit
//! tests in `crates/wasm-smtp/src/tests/`.

use wasm_smtp::{
    AuthMechanism, SendOutcome, SessionState, SmtpClient, SmtpClientOptions, SmtpError, PolicyError,
};
use wasm_smtp::audit::VecAuditSink;
use wasm_smtp::policy::SendPolicy;
use wasm_smtp::{IoError, Transport};
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

// ── Minimal self-contained test transport ─────────────────────────────────

/// (transport, written-bytes handle, closed flag)
type Handles = (TestTransport, Rc<RefCell<Vec<u8>>>, Rc<RefCell<bool>>);

struct TestTransport {
    incoming: VecDeque<Vec<u8>>,
    written:  Rc<RefCell<Vec<u8>>>,
    closed:   Rc<RefCell<bool>>,
}

impl TestTransport {
    fn new(chunks: &[&[u8]]) -> Handles {
        let written = Rc::new(RefCell::new(Vec::new()));
        let closed  = Rc::new(RefCell::new(false));
        let incoming = chunks.iter().map(|c| c.to_vec()).collect();
        (
            Self { incoming, written: Rc::clone(&written), closed: Rc::clone(&closed) },
            written,
            closed,
        )
    }
}

impl Transport for TestTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let Some(chunk) = self.incoming.front_mut() else { return Ok(0); };
        let n = buf.len().min(chunk.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        chunk.drain(..n);
        if chunk.is_empty() { self.incoming.pop_front(); }
        Ok(n)
    }
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.written.borrow_mut().extend_from_slice(buf);
        Ok(())
    }
    async fn close(&mut self) -> Result<(), IoError> {
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("test future returned Pending"),
    }
}

fn server(parts: &[&[u8]]) -> Vec<u8> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

// ── Test exchanges ────────────────────────────────────────────────────────

fn standard_exchange() -> Vec<u8> {
    server(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n",
        b"250 2.1.5 OK\r\n",
        b"354 Start mail input\r\n",
        b"250 2.0.0 OK: queued as QUEUE001\r\n",
        b"221 2.0.0 Bye\r\n",
    ])
}

const BODY: &str =
    "From: from@example.com\r\nTo: to@example.com\r\nSubject: hi\r\n\r\nHello.\r\n";

// ── Tests: basic lifecycle ────────────────────────────────────────────────

#[test]
fn connect_login_send_quit() {
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    let outcome: SendOutcome = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        assert_eq!(c.state(), SessionState::Authentication);
        c.login("user@example.com", "secret").await.unwrap();
        assert_eq!(c.state(), SessionState::MailFrom);
        let o = c.send_mail("from@example.com", &["to@example.com"], BODY).await.unwrap();
        c.quit().await.unwrap();
        o
    });
    assert_eq!(outcome.code, 250);
    assert_eq!(outcome.queue_id.as_deref(), Some("QUEUE001"));
}

#[test]
fn connect_exposes_capabilities() {
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    block_on(async {
        let c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        let caps = c.capabilities();
        assert!(!caps.is_empty(), "capabilities must be populated after connect");
        assert!(caps.iter().any(|c| c.contains("AUTH")));
    });
}

// ── Tests: SmtpClientOptions ──────────────────────────────────────────────

#[test]
fn options_builder_debug() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    let _ = format!("{opts:?}");
}

#[test]
fn audit_sink_receives_events() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect_with(transport, "client.example.com", opts).await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"], BODY).await.unwrap();
        c.quit().await.unwrap();
    });
    let events = sink.events();
    for expected in &["EhloCompleted", "AuthCompleted", "MailFromAccepted",
                      "RecipientAccepted", "MessageAccepted", "QuitCompleted"] {
        assert!(events.iter().any(|e| e.starts_with(expected)), "missing: {expected}");
    }
}

#[test]
fn send_policy_vetoes_sender() {
    struct RejectAll;
    impl SendPolicy for RejectAll {
        fn check_sender(&self, _: &str) -> Result<(), PolicyError> {
            Err(PolicyError::new("sender not allowed"))
        }
        fn check_recipients(&self, _: &[&str]) -> Result<(), PolicyError> { Ok(()) }
        fn check_message_size(&self, _: usize) -> Result<(), PolicyError> { Ok(()) }
    }
    let opts = SmtpClientOptions::new().with_policy(Box::new(RejectAll));
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    let err = block_on(async {
        let mut c = SmtpClient::connect_with(transport, "client.example.com", opts).await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"], BODY).await
    }).expect_err("policy should reject");
    assert!(matches!(err, SmtpError::Policy(_)), "expected Policy error, got {err:?}");
}

// ── Tests: authentication ─────────────────────────────────────────────────

#[test]
fn login_with_plain_explicit() {
    let exchange = server(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    let (transport, written, _) = TestTransport::new(&[&exchange]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login_with(AuthMechanism::Plain, "user@example.com", "secret").await.unwrap();
        c.quit().await.unwrap();
    });
    let wire = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(wire.contains("AUTH PLAIN "), "AUTH PLAIN must be sent");
}

// ── Tests: error surfaces ─────────────────────────────────────────────────

#[test]
fn send_mail_empty_recipients_returns_invalid_input() {
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    let err = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &[], BODY).await
    }).expect_err("empty recipients should fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)), "expected InvalidInput, got {err:?}");
}

#[test]
fn quit_consumes_self() {
    // Verifies the type-system guarantee: quit() consumes self.
    let (transport, _, _) = TestTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"], BODY).await.unwrap();
        c.quit().await.unwrap();
        // c is consumed; any further call would be a compile error.
    });
}

// ── Tests: multiple sends on one connection ───────────────────────────────

#[test]
fn two_messages_on_one_connection() {
    let exchange = server(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n", b"250 2.1.5 OK\r\n", b"354 Start\r\n",
        b"250 2.0.0 OK: queued as MSGID01\r\n",
        b"250 2.1.0 OK\r\n", b"250 2.1.5 OK\r\n", b"354 Start\r\n",
        b"250 2.0.0 OK: queued as MSGID02\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    let (transport, _, _) = TestTransport::new(&[&exchange]);
    let (o1, o2) = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        let o1 = c.send_mail("a@e.com", &["b@e.com"], BODY).await.unwrap();
        let o2 = c.send_mail("a@e.com", &["b@e.com"], BODY).await.unwrap();
        c.quit().await.unwrap();
        (o1, o2)
    });
    assert_eq!(o1.queue_id.as_deref(), Some("MSGID01"));
    assert_eq!(o2.queue_id.as_deref(), Some("MSGID02"));
}
