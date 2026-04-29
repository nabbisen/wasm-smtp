//! Tests for the `mail-builder` cargo feature.
//!
//! Compiled in only when the feature is enabled. Verifies that
//! `SmtpClient::send_message` accepts a `mail_builder::MessageBuilder`
//! and produces the same wire output as the equivalent manual
//! `send_mail` call.

use super::harness::{MockTransport, block_on, flatten};
use crate::client::SmtpClient;
use mail_builder::MessageBuilder;

/// A `MessageBuilder` and the equivalent manual body should produce
/// identical wire bytes when both are submitted through `send_message`
/// and `send_mail` respectively.
#[test]
fn send_message_produces_same_wire_as_manual_send_mail() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        // MAIL FROM
        b"250 OK\r\n",
        // RCPT TO
        b"250 OK\r\n",
        // DATA
        b"354 End data with <CR><LF>.<CR><LF>\r\n",
        // After body
        b"250 Queued\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");

    let message = MessageBuilder::new()
        .from(("Notifications", "notify@example.com"))
        .to(("Alice", "alice@example.org"))
        .subject("Status update")
        .text_body("Hello.\r\n");

    block_on(client.send_message("notify@example.com", &["alice@example.org"], message))
        .expect("send_message");

    // Verify the wire output starts with the EHLO + envelope + DATA
    // dance we expect, then ends with the dot-stuffed body terminator.
    let written = written.borrow();
    let written_str = std::str::from_utf8(&written).expect("UTF-8 wire bytes");

    assert!(
        written_str.starts_with("EHLO client.example\r\nMAIL FROM:<notify@example.com>\r\n"),
        "envelope sender wrong: {written_str:?}",
    );
    assert!(
        written_str.contains("RCPT TO:<alice@example.org>\r\n"),
        "envelope recipient wrong: {written_str:?}",
    );
    assert!(
        written_str.contains("DATA\r\n"),
        "DATA verb missing: {written_str:?}",
    );
    // The mail-builder serialized headers should be in the body.
    assert!(
        written_str.contains("From:"),
        "From header missing: {written_str:?}",
    );
    assert!(
        written_str.contains("Subject: Status update"),
        "Subject header missing: {written_str:?}",
    );
    // Body must end with the SMTP terminator.
    assert!(
        written_str.ends_with("\r\n.\r\n"),
        "DATA terminator missing or wrong: {written_str:?}",
    );
}

/// Empty recipient list must be rejected before any I/O — same
/// invariant as `send_mail`. The mail-builder layer does not relax it.
#[test]
fn send_message_rejects_empty_recipient_list() {
    use crate::error::SmtpError;

    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let pre_writes = written.borrow().len();

    let message = MessageBuilder::new()
        .from("notify@example.com")
        .to("alice@example.org")
        .subject("test")
        .text_body("body");

    let err = block_on(client.send_message("notify@example.com", &[], message))
        .expect_err("empty recipients must fail");

    assert!(
        matches!(err, SmtpError::InvalidInput(_)),
        "expected InvalidInput, got {err:?}",
    );
    // Should have failed before sending MAIL FROM.
    assert_eq!(written.borrow().len(), pre_writes);
}

/// Envelope-vs-header separation: caller can submit a message whose
/// envelope `to` list differs from the message's `To:` header (the
/// classic Bcc pattern). The crate does not police this.
#[test]
fn send_message_envelope_can_differ_from_headers() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"250 OK\r\n", // MAIL FROM
        b"250 OK\r\n", // RCPT TO #1
        b"250 OK\r\n", // RCPT TO #2 (the bcc)
        b"354 End data with <CR><LF>.<CR><LF>\r\n",
        b"250 Queued\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");

    let message = MessageBuilder::new()
        .from("notify@example.com")
        .to("alice@example.org") // visible in headers
        .subject("Update")
        .text_body("Hi.");

    // Envelope includes a recipient that isn't in the headers.
    block_on(client.send_message(
        "notify@example.com",
        &["alice@example.org", "auditor@example.com"],
        message,
    ))
    .expect("send_message with bcc-style envelope");

    let written = written.borrow();
    let written_str = std::str::from_utf8(&written).expect("UTF-8");
    assert!(written_str.contains("RCPT TO:<alice@example.org>"));
    assert!(written_str.contains("RCPT TO:<auditor@example.com>"));
}
