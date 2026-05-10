//! Tests for the `AuditSink` / `SmtpAuditEvent` hook (RFC 012).

use std::sync::{Arc, Mutex};

use super::harness::{MockTransport, block_on, flatten};
use crate::audit::{AuditSink, NoopAuditSink, SmtpAuditEvent, VecAuditSink};
use crate::client::{SmtpClient, SmtpClientOptions};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn full_script() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 Authentication successful\r\n",
        b"250 2.1.0 OK\r\n",
        b"250 2.1.5 OK\r\n",
        b"354 Start mail input\r\n",
        b"250 2.0.0 OK: queued\r\n",
        b"221 2.0.0 Bye\r\n",
    ])
}

fn run_full_session(opts: SmtpClientOptions) {
    let script = full_script();
    let (transport, _written, _closed) = MockTransport::new(&[&script]);
    let mut client =
        block_on(SmtpClient::connect_with(transport, "client.example.com", opts)).expect("connect");
    block_on(client.login("user@example.com", "pass")).expect("login");
    block_on(client.send_mail(
        "from@example.com",
        &["to@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect("send");
    block_on(client.quit()).expect("quit");
}

// ---------------------------------------------------------------------------
// NoopAuditSink — compiles and doesn't panic
// ---------------------------------------------------------------------------

#[test]
fn noop_sink_does_not_panic() {
    let opts = SmtpClientOptions::new().with_audit(Box::new(NoopAuditSink));
    run_full_session(opts);
}

// ---------------------------------------------------------------------------
// VecAuditSink — captures the full event sequence
// ---------------------------------------------------------------------------

#[test]
fn full_session_emits_expected_event_sequence() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    let events = sink.events();

    // Verify count and rough sequence.
    assert!(!events.is_empty(), "must have captured events");

    // Check milestone presence in order.
    let connected_pos = events.iter().position(|e| e.starts_with("Connected"));
    let greeting_pos = events.iter().position(|e| e.starts_with("GreetingReceived"));
    let ehlo_pos = events.iter().position(|e| e.starts_with("EhloCompleted"));
    let auth_pos = events.iter().position(|e| e.starts_with("AuthCompleted"));
    let mail_pos = events.iter().position(|e| e.starts_with("MailFromAccepted"));
    let rcpt_pos = events.iter().position(|e| e.starts_with("RecipientAccepted"));
    let msg_pos = events.iter().position(|e| e.starts_with("MessageAccepted"));
    let quit_pos = events.iter().position(|e| e.starts_with("QuitCompleted"));

    assert!(connected_pos.is_some(), "Connected must be emitted");
    assert!(greeting_pos.is_some(), "GreetingReceived must be emitted");
    assert!(ehlo_pos.is_some(), "EhloCompleted must be emitted");
    assert!(auth_pos.is_some(), "AuthCompleted must be emitted");
    assert!(mail_pos.is_some(), "MailFromAccepted must be emitted");
    assert!(rcpt_pos.is_some(), "RecipientAccepted must be emitted");
    assert!(msg_pos.is_some(), "MessageAccepted must be emitted");
    assert!(quit_pos.is_some(), "QuitCompleted must be emitted");

    // Ordering checks.
    assert!(connected_pos < greeting_pos, "Connected before GreetingReceived");
    assert!(greeting_pos < ehlo_pos, "GreetingReceived before EhloCompleted");
    assert!(ehlo_pos < auth_pos, "EhloCompleted before AuthCompleted");
    assert!(auth_pos < mail_pos, "AuthCompleted before MailFromAccepted");
    assert!(mail_pos < rcpt_pos, "MailFromAccepted before RecipientAccepted");
    assert!(rcpt_pos < msg_pos, "RecipientAccepted before MessageAccepted");
    assert!(msg_pos < quit_pos, "MessageAccepted before QuitCompleted");
}

#[test]
fn greeting_received_carries_reply_code() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    let greeting_event = sink
        .events()
        .into_iter()
        .find(|e| e.starts_with("GreetingReceived"))
        .expect("GreetingReceived must be emitted");
    assert!(
        greeting_event.contains("220"),
        "GreetingReceived must carry code 220, got: {greeting_event}"
    );
}

#[test]
fn auth_completed_carries_mechanism_name() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    let auth_event = sink
        .events()
        .into_iter()
        .find(|e| e.starts_with("AuthCompleted"))
        .expect("AuthCompleted must be emitted");
    // Server advertised PLAIN and LOGIN; default selection should pick PLAIN.
    assert!(
        auth_event.contains("PLAIN") || auth_event.contains("LOGIN"),
        "AuthCompleted must carry mechanism name, got: {auth_event}"
    );
}

#[test]
fn message_accepted_carries_reply_code() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    let msg_event = sink
        .events()
        .into_iter()
        .find(|e| e.starts_with("MessageAccepted"))
        .expect("MessageAccepted must be emitted");
    assert!(
        msg_event.contains("250"),
        "MessageAccepted must carry code 250, got: {msg_event}"
    );
}

// ---------------------------------------------------------------------------
// Multiple recipients — one RecipientAccepted per RCPT TO
// ---------------------------------------------------------------------------

#[test]
fn multiple_recipients_emit_one_event_each() {
    let two_rcpt_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n",
        b"250 2.1.5 OK\r\n",  // first RCPT TO
        b"250 2.1.5 OK\r\n",  // second RCPT TO
        b"354 Start mail input\r\n",
        b"250 2.0.0 OK\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    let (transport, _written, _closed) = MockTransport::new(&[&two_rcpt_script]);
    let mut client =
        block_on(SmtpClient::connect_with(transport, "client.example.com", opts)).expect("connect");
    block_on(client.login("u", "p")).expect("login");
    block_on(client.send_mail(
        "from@example.com",
        &["a@example.com", "b@example.com"],
        "Subject: hi\r\n\r\nbody\r\n",
    ))
    .expect("send");
    block_on(client.quit()).expect("quit");

    let accepted_count = sink
        .events()
        .iter()
        .filter(|e| e.starts_with("RecipientAccepted"))
        .count();
    assert_eq!(accepted_count, 2, "must emit one RecipientAccepted per recipient");
}

// ---------------------------------------------------------------------------
// Events contain no credentials or message body
// ---------------------------------------------------------------------------

#[test]
fn audit_events_never_contain_credentials() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    for event in sink.events() {
        assert!(
            !event.contains("pass"),
            "audit event must not contain password: {event}"
        );
        assert!(
            !event.contains("user@example.com"),
            "audit event must not contain username: {event}"
        );
    }
}

#[test]
fn audit_events_never_contain_message_body() {
    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));
    run_full_session(opts);

    for event in sink.events() {
        assert!(
            !event.contains("body"),
            "audit event must not contain message body: {event}"
        );
    }
}

// ---------------------------------------------------------------------------
// Custom AuditSink — counter implementation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CounterSink {
    messages: Arc<Mutex<u32>>,
}

impl AuditSink for CounterSink {
    fn on_event(&self, event: &SmtpAuditEvent<'_>) {
        if let SmtpAuditEvent::MessageAccepted { .. } = event {
            *self.messages.lock().unwrap() += 1;
        }
    }
}

#[test]
fn custom_sink_counts_messages_accepted() {
    let counter = Arc::new(Mutex::new(0u32));
    let sink = CounterSink {
        messages: Arc::clone(&counter),
    };
    let opts = SmtpClientOptions::new().with_audit(Box::new(sink));
    run_full_session(opts);
    assert_eq!(*counter.lock().unwrap(), 1, "one message was accepted");
}

// ---------------------------------------------------------------------------
// VecAuditSink is empty before use
// ---------------------------------------------------------------------------

#[test]
fn vec_sink_is_empty_before_session() {
    let sink = VecAuditSink::default();
    assert!(sink.is_empty());
    assert_eq!(sink.len(), 0);
}
