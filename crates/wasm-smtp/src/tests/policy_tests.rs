//! Tests for the `SendPolicy` hook (RFC 011).

use super::harness::{MockTransport, block_on, flatten};
use crate::client::{SmtpClient, SmtpClientOptions};
use crate::error::{PolicyError, SmtpError};
use crate::policy::{BoundedPolicy, DefaultPolicy, SendPolicy};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn full_exchange() -> Vec<u8> {
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

fn connect_and_login(
    opts: SmtpClientOptions,
) -> (SmtpClient<MockTransport>, std::rc::Rc<std::cell::RefCell<Vec<u8>>>) {
    let script = full_exchange();
    let (transport, written, _closed) = MockTransport::new(&[&script]);
    let mut client = block_on(SmtpClient::connect_with(transport, "client.example.com", opts))
        .expect("connect");
    block_on(client.login("user@example.com", "pass")).expect("login");
    (client, written)
}

// ---------------------------------------------------------------------------
// DefaultPolicy — allow everything
// ---------------------------------------------------------------------------

#[test]
fn default_policy_allows_any_send() {
    let (mut client, _written) = connect_and_login(SmtpClientOptions::default());
    block_on(client.send_mail(
        "from@example.com",
        &["to@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect("send_mail with default policy should succeed");
}

// ---------------------------------------------------------------------------
// Custom policy — sender rejection
// ---------------------------------------------------------------------------

struct BlockAllSenders;
impl SendPolicy for BlockAllSenders {
    fn check_sender(&self, _from: &str) -> Result<(), PolicyError> {
        Err(PolicyError::new("all senders blocked"))
    }
    fn check_recipients(&self, _to: &[&str]) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_message_size(&self, _bytes: usize) -> Result<(), PolicyError> {
        Ok(())
    }
}

#[test]
fn policy_rejection_on_sender_returns_policy_error() {
    let opts = SmtpClientOptions::new().with_policy(Box::new(BlockAllSenders));
    let (mut client, written) = connect_and_login(opts);

    let err = block_on(client.send_mail(
        "from@example.com",
        &["to@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect_err("policy should reject");

    assert!(matches!(err, SmtpError::Policy(_)), "expected Policy error, got {err:?}");
    if let SmtpError::Policy(e) = &err {
        assert_eq!(e.message(), "all senders blocked");
    }

    // No MAIL FROM should have been sent.
    let sent = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(!sent.contains("MAIL FROM"), "MAIL FROM must not be sent after policy rejection");
}

// ---------------------------------------------------------------------------
// Custom policy — recipient rejection
// ---------------------------------------------------------------------------

struct BlockRecipient(&'static str);
impl SendPolicy for BlockRecipient {
    fn check_sender(&self, _from: &str) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_recipients(&self, recipients: &[&str]) -> Result<(), PolicyError> {
        if recipients.contains(&self.0) {
            Err(PolicyError::new(format!("recipient {} is blocked", self.0)))
        } else {
            Ok(())
        }
    }
    fn check_message_size(&self, _bytes: usize) -> Result<(), PolicyError> {
        Ok(())
    }
}

#[test]
fn policy_rejection_on_recipient_stops_before_mail_from() {
    let opts =
        SmtpClientOptions::new().with_policy(Box::new(BlockRecipient("blocked@example.com")));
    let (mut client, written) = connect_and_login(opts);

    let err = block_on(client.send_mail(
        "from@example.com",
        &["blocked@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect_err("policy should reject");

    assert!(matches!(err, SmtpError::Policy(_)));
    let sent = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(!sent.contains("MAIL FROM"), "MAIL FROM must not be sent");
    assert!(!sent.contains("RCPT TO"), "RCPT TO must not be sent");
}

// ---------------------------------------------------------------------------
// BoundedPolicy — max_recipients
// ---------------------------------------------------------------------------

#[test]
fn bounded_policy_rejects_too_many_recipients() {
    let policy = BoundedPolicy::new().max_recipients(2);
    let opts = SmtpClientOptions::new().with_policy(Box::new(policy));
    let (mut client, _written) = connect_and_login(opts);

    let err = block_on(client.send_mail(
        "from@example.com",
        &["a@example.com", "b@example.com", "c@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect_err("bounded policy should reject 3 recipients when max=2");

    assert!(matches!(err, SmtpError::Policy(_)));
}

#[test]
fn bounded_policy_allows_within_limit() {
    let policy = BoundedPolicy::new().max_recipients(5);
    let opts = SmtpClientOptions::new().with_policy(Box::new(policy));
    let (mut client, _written) = connect_and_login(opts);

    block_on(client.send_mail(
        "from@example.com",
        &["to@example.com"],
        "Subject: test\r\n\r\nbody\r\n",
    ))
    .expect("within limit should succeed");
}

// ---------------------------------------------------------------------------
// BoundedPolicy — max_message_bytes
// ---------------------------------------------------------------------------

#[test]
fn bounded_policy_rejects_oversized_body() {
    let policy = BoundedPolicy::new().max_message_bytes(10);
    let opts = SmtpClientOptions::new().with_policy(Box::new(policy));
    let (mut client, written) = connect_and_login(opts);

    let body = "Subject: test\r\n\r\n".to_string() + &"X".repeat(100);
    let err =
        block_on(client.send_mail("from@example.com", &["to@example.com"], &body)).expect_err(
            "body exceeds max_message_bytes",
        );

    assert!(matches!(err, SmtpError::Policy(_)));
    let sent = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(!sent.contains("DATA"), "DATA must not be sent when body is too large");
}

// ---------------------------------------------------------------------------
// PolicyError does not include credentials
// ---------------------------------------------------------------------------

#[test]
fn policy_error_debug_contains_no_credentials() {
    let e = PolicyError::new("sender domain not allowed");
    let debug = format!("{e:?}");
    // The message is part of Debug but no credentials are embedded here;
    // this test guards against future refactors that might attach a 'from'
    // address or other PII to the error struct.
    assert!(!debug.contains("password"), "debug must not contain password");
    assert!(!debug.contains("secret"), "debug must not contain secret");
}

// ---------------------------------------------------------------------------
// DefaultPolicy unit tests
// ---------------------------------------------------------------------------

#[test]
fn default_policy_passes_all_checks() {
    let p = DefaultPolicy;
    assert!(p.check_sender("any@example.com").is_ok());
    assert!(p
        .check_recipients(&["a@example.com", "b@example.com"])
        .is_ok());
    assert!(p.check_message_size(usize::MAX).is_ok());
}
