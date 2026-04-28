//! Tests for the `smtputf8` cargo feature (RFC 6531).
//!
//! Compiled in only when the feature is enabled; the parent
//! `tests/mod.rs` gates the `mod smtputf8_tests;` declaration
//! accordingly.

use super::harness::{MockTransport, block_on, flatten};
use crate::client::SmtpClient;
use crate::error::{ProtocolError, SmtpError, SmtpOp};
use crate::protocol::{ehlo_advertises_smtputf8, format_mail_from_smtputf8, validate_address_utf8};
use crate::session::SessionState;

// -- ehlo_advertises_smtputf8 -----------------------------------------

#[test]
fn ehlo_advertises_smtputf8_finds_listed_extension() {
    let lines: Vec<String> = vec!["PIPELINING".into(), "SMTPUTF8".into()];
    assert!(ehlo_advertises_smtputf8(&lines));
}

#[test]
fn ehlo_advertises_smtputf8_is_case_insensitive() {
    let lines: Vec<String> = vec!["smtputf8".into()];
    assert!(ehlo_advertises_smtputf8(&lines));
}

#[test]
fn ehlo_advertises_smtputf8_returns_false_when_absent() {
    let lines: Vec<String> = vec!["PIPELINING".into(), "AUTH PLAIN".into()];
    assert!(!ehlo_advertises_smtputf8(&lines));
}

#[test]
fn ehlo_advertises_smtputf8_does_not_match_substrings() {
    let lines: Vec<String> = vec!["SMTPUTF8X".into()];
    assert!(!ehlo_advertises_smtputf8(&lines));
}

// -- validate_address_utf8 --------------------------------------------

#[test]
fn validate_address_utf8_accepts_ascii() {
    // Anything the strict ASCII validator accepts must also pass here.
    assert!(validate_address_utf8("user@example.com").is_ok());
    assert!(validate_address_utf8("a.b+c@d.example").is_ok());
}

/// Phase 9 / M-4: UTF-8 length limits also apply to `validate_address_utf8`.
/// Japanese characters are 3 octets each in UTF-8, so 100 of them
/// produce a 300-octet local-part which is past every limit.
#[test]
fn validate_address_utf8_rejects_overly_long_japanese_local_part() {
    let long_local: String = "\u{4E2D}".repeat(100);
    let addr = format!("{long_local}@example.jp");
    assert!(addr.len() > 254);
    assert!(validate_address_utf8(&addr).is_err());
}

#[test]
fn validate_address_utf8_rejects_overly_long_total() {
    // 64-byte ASCII local-part (at the limit) + '@' + 191-byte ASCII
    // domain = 256 octets, just over the 254 cap.
    let local = "a".repeat(64);
    let domain = format!("{}.example", "x".repeat(183)); // 183 + 8 = 191
    let addr = format!("{local}@{domain}");
    assert_eq!(addr.len(), 256);
    assert!(validate_address_utf8(&addr).is_err());
}

#[test]
fn validate_address_utf8_accepts_japanese_local_part() {
    assert!(validate_address_utf8("\u{9001}\u{4FE1}@example.jp").is_ok());
}

#[test]
fn validate_address_utf8_accepts_idn_domain() {
    // U-label domain (Japanese ".jp" 例え.jp).
    assert!(validate_address_utf8("user@\u{4F8B}\u{3048}.jp").is_ok());
}

#[test]
fn validate_address_utf8_accepts_combined_local_and_domain() {
    assert!(validate_address_utf8("\u{9001}\u{4FE1}@\u{4F8B}\u{3048}.jp").is_ok());
}

#[test]
fn validate_address_utf8_rejects_empty() {
    assert!(validate_address_utf8("").is_err());
}

#[test]
fn validate_address_utf8_rejects_crlf() {
    assert!(validate_address_utf8("a\r@b.com").is_err());
    assert!(validate_address_utf8("a\n@b.com").is_err());
}

#[test]
fn validate_address_utf8_rejects_nul() {
    assert!(validate_address_utf8("a\0b@c.com").is_err());
}

#[test]
fn validate_address_utf8_rejects_angle_brackets() {
    assert!(validate_address_utf8("<a@b.com>").is_err());
    assert!(validate_address_utf8("a@b<c.com").is_err());
}

#[test]
fn validate_address_utf8_rejects_ascii_whitespace() {
    assert!(validate_address_utf8("a b@c.com").is_err());
    assert!(validate_address_utf8("a\tb@c.com").is_err());
}

#[test]
fn validate_address_utf8_accepts_ideographic_space() {
    // U+3000 is whitespace by Unicode category but valid in some
    // local parts and not a SMTP framing concern.
    assert!(validate_address_utf8("a\u{3000}b@c.com").is_ok());
}

#[test]
fn validate_address_utf8_rejects_ascii_control_chars() {
    // ASCII DEL (0x7F).
    assert!(validate_address_utf8("a\u{007F}b@c.com").is_err());
    // Bell (0x07).
    assert!(validate_address_utf8("a\u{0007}b@c.com").is_err());
}

#[test]
fn validate_address_utf8_rejects_c1_control_chars() {
    // U+0080-U+009F are C1 controls.
    assert!(validate_address_utf8("a\u{0085}b@c.com").is_err());
    assert!(validate_address_utf8("a\u{0095}b@c.com").is_err());
}

// -- format_mail_from_smtputf8 ----------------------------------------

#[test]
fn format_mail_from_smtputf8_appends_parameter() {
    let bytes = format_mail_from_smtputf8("user@example.com");
    assert_eq!(bytes, b"MAIL FROM:<user@example.com> SMTPUTF8\r\n");
}

#[test]
fn format_mail_from_smtputf8_carries_utf8_address() {
    let bytes = format_mail_from_smtputf8("\u{9001}\u{4FE1}@example.jp");
    // The bytes must be exact UTF-8 of the input.
    let mut expected: Vec<u8> = Vec::new();
    expected.extend_from_slice(b"MAIL FROM:<");
    expected.extend_from_slice("\u{9001}\u{4FE1}@example.jp".as_bytes());
    expected.extend_from_slice(b"> SMTPUTF8\r\n");
    assert_eq!(bytes, expected);
}

// -- send_mail_smtputf8 E2E -------------------------------------------

/// Greeting + EHLO advertising both AUTH PLAIN and SMTPUTF8.
fn greeting_then_ehlo_with_smtputf8() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250-PIPELINING\r\n",
        b"250-SMTPUTF8\r\n",
        b"250 AUTH PLAIN\r\n",
    ])
}

#[test]
fn send_mail_smtputf8_full_flow_with_japanese_addresses() {
    let script = flatten(&[
        &greeting_then_ehlo_with_smtputf8()[..],
        b"235 OK\r\n",       // AUTH PLAIN
        b"250 OK\r\n",       // MAIL FROM
        b"250 OK\r\n",       // RCPT TO
        b"354 go ahead\r\n", // DATA
        b"250 Queued\r\n",   // body accepted
    ]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    block_on(client.send_mail_smtputf8(
        "\u{9001}\u{4FE1}@example.jp",
        &["\u{53D7}\u{4FE1}@\u{4F8B}\u{3048}.jp"],
        "Subject: hi\r\n\r\nbody\r\n",
    ))
    .expect("send_mail_smtputf8");

    // The wire bytes should include: EHLO, AUTH PLAIN, MAIL FROM with
    // SMTPUTF8 parameter, RCPT TO without parameter, DATA, body, .
    let bytes = written.borrow();
    let s = std::str::from_utf8(&bytes).expect("bytes are valid UTF-8");
    assert!(s.contains("MAIL FROM:<\u{9001}\u{4FE1}@example.jp> SMTPUTF8\r\n"));
    assert!(s.contains("RCPT TO:<\u{53D7}\u{4FE1}@\u{4F8B}\u{3048}.jp>\r\n"));
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn send_mail_smtputf8_fails_when_extension_not_advertised() {
    // EHLO does not advertise SMTPUTF8.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let after_login = written.borrow().len();

    let err = block_on(client.send_mail_smtputf8(
        "\u{9001}\u{4FE1}@example.jp",
        &["\u{53D7}\u{4FE1}@example.jp"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");

    match err {
        SmtpError::Protocol(ProtocolError::ExtensionUnavailable { name }) => {
            assert_eq!(name, "SMTPUTF8");
        }
        other => panic!("expected ExtensionUnavailable, got {other:?}"),
    }
    // No bytes were written for MAIL FROM: the failure happened
    // before any transport I/O.
    assert_eq!(written.borrow().len(), after_login);
    assert_eq!(client.state(), SessionState::Closed);
}

#[test]
fn send_mail_smtputf8_validates_addresses_before_io() {
    let script = flatten(&[&greeting_then_ehlo_with_smtputf8()[..], b"235 OK\r\n"]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let after_login = written.borrow().len();

    // CR in the address must be caught locally.
    let err = block_on(client.send_mail_smtputf8(
        "u\rser@example.com",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // Nothing was written for the failed send.
    assert_eq!(written.borrow().len(), after_login);
    // Session remains usable for retry.
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn send_mail_smtputf8_rejects_server_error_during_mail_from() {
    let script = flatten(&[
        &greeting_then_ehlo_with_smtputf8()[..],
        b"235 OK\r\n",                    // AUTH PLAIN
        b"550 sender domain refused\r\n", // MAIL FROM rejected
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let err = block_on(client.send_mail_smtputf8(
        "\u{9001}\u{4FE1}@example.jp",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode { during, actual, .. }) => {
            assert_eq!(during, SmtpOp::MailFrom);
            assert_eq!(actual, 550);
        }
        other => panic!("expected UnexpectedCode for MailFrom: {other:?}"),
    }
}

#[test]
fn ascii_send_mail_unchanged_when_smtputf8_feature_enabled() {
    // Even with the feature on, the default `send_mail` continues
    // to use the strict ASCII validator. A UTF-8 address must be
    // refused by the default API.
    let script = flatten(&[&greeting_then_ehlo_with_smtputf8()[..], b"235 OK\r\n"]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let err = block_on(client.send_mail(
        "\u{9001}\u{4FE1}@example.jp",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
}
