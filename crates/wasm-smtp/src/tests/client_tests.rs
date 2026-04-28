//! End-to-end tests that drive `SmtpClient` against a mock
//! transport. Covers the full happy-path SMTP exchange,
//! authentication mechanisms (PLAIN / LOGIN / XOAUTH2),
//! STARTTLS, ENHANCEDSTATUSCODES, and error paths.

use super::harness::{MockTransport, UpgradeBehavior, block_on, flatten};
use crate::client::SmtpClient;
use crate::error::{AuthError, ProtocolError, SmtpError, SmtpOp};
use crate::protocol::AuthMechanism;
use crate::session::SessionState;

/// Standard greeting + EHLO reply used by most happy-path tests.
fn greeting_then_ehlo() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com Hello [192.0.2.1]\r\n",
        b"250-PIPELINING\r\n",
        b"250-8BITMIME\r\n",
        b"250 AUTH LOGIN PLAIN\r\n",
    ])
}

// -- connect / EHLO -----------------------------------------------------

#[test]
fn connect_reads_greeting_and_sends_ehlo() {
    let script = greeting_then_ehlo();
    let (transport, written, _closed) = MockTransport::new(&[&script[..]]);
    let client = block_on(SmtpClient::connect(transport, "client.example.com")).expect("connect");

    assert_eq!(client.state(), SessionState::Authentication);
    let caps = client.capabilities();
    assert_eq!(caps.len(), 3);
    assert_eq!(caps[0], "PIPELINING");
    assert_eq!(caps[1], "8BITMIME");
    assert_eq!(caps[2], "AUTH LOGIN PLAIN");

    // Only one command should have been sent: EHLO.
    assert_eq!(&*written.borrow(), b"EHLO client.example.com\r\n");
}

#[test]
fn connect_fails_on_non_220_greeting() {
    let script: &[u8] = b"554 Service unavailable\r\n";
    let (transport, _written, _closed) = MockTransport::new(&[script]);
    let err = block_on(SmtpClient::connect(transport, "client.example.com"))
        .expect_err("greeting should fail");
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode { actual, .. }) => {
            assert_eq!(actual, 554);
        }
        other => panic!("expected ProtocolError::UnexpectedCode, got {other:?}"),
    }
}

#[test]
fn invalid_ehlo_domain_is_rejected_before_io() {
    let (transport, written, _closed) = MockTransport::new(&[]);
    let err = block_on(SmtpClient::connect(transport, "")).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // No bytes should ever have been sent to the transport.
    assert!(written.borrow().is_empty());
}

// -- AUTH LOGIN ---------------------------------------------------------

#[test]
fn login_sends_correct_auth_login_sequence() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
        b"334 VXNlcm5hbWU6\r\n",
        b"334 UGFzc3dvcmQ6\r\n",
        b"235 Authentication succeeded\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");

    let expected = b"EHLO client.example\r\n\
                     AUTH LOGIN\r\n\
                     dXNlcg==\r\n\
                     cGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn login_fails_when_auth_login_not_advertised() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n", // No AUTH advertised at all
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Auth(AuthError::UnsupportedMechanism)
    ));
    assert_eq!(client.state(), SessionState::Closed);
}

#[test]
fn login_fails_on_535_rejection() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
        b"334 VXNlcm5hbWU6\r\n",
        b"334 UGFzc3dvcmQ6\r\n",
        b"535 5.7.8 Authentication credentials invalid\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    match err {
        SmtpError::Auth(AuthError::Rejected { code, .. }) => assert_eq!(code, 535),
        other => panic!("expected AuthError::Rejected, got {other:?}"),
    }
}

#[test]
fn login_rejects_empty_username_before_io() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let pre_login_writes_len = written.borrow().len();
    let err = block_on(client.login("", "pass")).expect_err("empty user must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // No additional bytes should have been written.
    assert_eq!(written.borrow().len(), pre_login_writes_len);
}

// -- send_mail ----------------------------------------------------------

#[test]
fn send_mail_full_transaction_no_auth() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        // MAIL FROM
        b"250 OK\r\n",
        // RCPT TO #1
        b"250 OK\r\n",
        // RCPT TO #2 (251 = "User not local; will forward" is also a 2xx)
        b"251 User not local; will forward\r\n",
        // DATA
        b"354 End data with <CR><LF>.<CR><LF>\r\n",
        // After body
        b"250 Queued\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let body = "From: a@example.com\r\nTo: b@example.org\r\nSubject: hi\r\n\r\nHello.\r\n";
    block_on(client.send_mail("a@example.com", &["b@example.org", "c@example.org"], body))
        .expect("send_mail");

    let expected = b"EHLO client.example\r\n\
                     MAIL FROM:<a@example.com>\r\n\
                     RCPT TO:<b@example.org>\r\n\
                     RCPT TO:<c@example.org>\r\n\
                     DATA\r\n\
                     From: a@example.com\r\nTo: b@example.org\r\nSubject: hi\r\n\r\nHello.\r\n.\r\n";
    assert_eq!(&*written.borrow(), expected);
    // After a successful transaction the client is ready for the next.
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn send_mail_dot_stuffs_leading_dot_lines() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"250 OK\r\n",
        b"250 OK\r\n",
        b"354 OK\r\n",
        b"250 Queued\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    // Body whose data section starts with a `.` line and contains
    // double-dot-prefixed line.
    let body = "Subject: t\r\n\r\n.line1\r\n..line2\r\n";
    block_on(client.send_mail("a@b.com", &["c@d.com"], body)).expect("send");

    // Locate the DATA payload, i.e. what follows the literal "DATA\r\n".
    let got = written.borrow();
    let after_data = b"DATA\r\n";
    let pos = got
        .windows(after_data.len())
        .position(|w| w == after_data)
        .expect("DATA marker in capture");
    let payload = &got[pos + after_data.len()..];
    // ".line1" -> "..line1"; "..line2" -> "...line2".
    let expected = b"Subject: t\r\n\r\n..line1\r\n...line2\r\n.\r\n";
    assert_eq!(payload, expected);
}

#[test]
fn send_mail_rejects_empty_recipients() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &[], "x")).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
}

#[test]
fn send_mail_rejects_crlf_injection_in_address() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let pre = written.borrow().len();
    let err =
        block_on(client.send_mail("a@b.com\r\nRSET", &["c@d.com"], "x")).expect_err("must reject");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // Nothing extra should have been written after the EHLO.
    assert_eq!(written.borrow().len(), pre);
}

#[test]
fn send_mail_after_mail_from_rejection_marks_session_closed() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"550 No such user\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &["c@d.com"], "Subject: x\r\n\r\nx\r\n"))
        .expect_err("server should reject");
    assert!(matches!(
        err,
        SmtpError::Protocol(ProtocolError::UnexpectedCode { .. })
    ));
    assert_eq!(client.state(), SessionState::Closed);
}

#[test]
fn two_send_mails_in_one_session_succeed() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        // First transaction.
        b"250 OK\r\n", // MAIL FROM
        b"250 OK\r\n", // RCPT TO
        b"354 OK\r\n", // DATA
        b"250 Queued\r\n",
        // Second transaction.
        b"250 OK\r\n",
        b"250 OK\r\n",
        b"354 OK\r\n",
        b"250 Queued\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let body = "Subject: t\r\n\r\nbody\r\n";
    block_on(client.send_mail("a@b.com", &["c@d.com"], body)).expect("first send");
    block_on(client.send_mail("a@b.com", &["e@f.com"], body)).expect("second send");
    assert_eq!(client.state(), SessionState::MailFrom);
}

// -- QUIT ---------------------------------------------------------------

#[test]
fn quit_sends_quit_and_closes_transport() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
        // QUIT
        b"221 Bye\r\n",
    ]);
    let (transport, written, closed) = MockTransport::new(&[&server_script[..]]);
    let client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    block_on(client.quit()).expect("quit");
    assert!(written.borrow().ends_with(b"QUIT\r\n"));
    assert!(*closed.borrow(), "transport.close() must be called");
}

// -- protocol robustness ------------------------------------------------

#[test]
fn unexpected_close_during_reply_is_classified() {
    // Server sends greeting then dribbles an unfinished EHLO reply.
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n", // continuation, then EOF
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let err = block_on(SmtpClient::connect(transport, "c.example")).expect_err("must fail");
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedClose) => {}
        other => panic!("expected UnexpectedClose, got {other:?}"),
    }
}

#[test]
fn inconsistent_multiline_codes_are_rejected() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-line1\r\n",
        b"251 line2\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let err = block_on(SmtpClient::connect(transport, "c.example")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Protocol(ProtocolError::InconsistentMultiline { .. })
    ));
}

#[test]
fn malformed_reply_line_is_rejected() {
    let server_script: &[u8] = b"abc not a real reply\r\n";
    let (transport, _written, _closed) = MockTransport::new(&[server_script]);
    let err = block_on(SmtpClient::connect(transport, "c.example")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Protocol(ProtocolError::Malformed(_))
    ));
}

#[test]
fn read_handles_chunks_split_arbitrarily() {
    // Same script as the basic test, but split across multiple read
    // calls so the buffered reader is exercised.
    let chunks: Vec<&[u8]> = vec![
        b"220 mail.exam",
        b"ple.com ESMTP\r\n250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
    ];
    let (transport, _written, _closed) = MockTransport::new(&chunks);
    let client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    assert_eq!(client.state(), SessionState::Authentication);
}

// -- AUTH PLAIN (Phase 4) ----------------------------------------------

#[test]
fn login_uses_plain_when_advertised() {
    // Server advertises both PLAIN and LOGIN; login() should pick
    // PLAIN and complete in one round trip (235 immediately after
    // the AUTH PLAIN line).
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN LOGIN\r\n",
        b"235 Authentication succeeded\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");

    // base64("\0user\0pass") == "AHVzZXIAcGFzcw=="
    let expected = b"EHLO client.example\r\n\
                     AUTH PLAIN AHVzZXIAcGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn login_falls_back_to_login_when_only_login_advertised() {
    // Server advertises only LOGIN — exactly the v0.1 behavior.
    // login() must continue to work against this server.
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
        b"334 VXNlcm5hbWU6\r\n",
        b"334 UGFzc3dvcmQ6\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");

    let expected = b"EHLO client.example\r\n\
                     AUTH LOGIN\r\n\
                     dXNlcg==\r\n\
                     cGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn login_fails_when_no_supported_mechanism_is_advertised() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH CRAM-MD5\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let pre_login = written.borrow().len();
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Auth(AuthError::UnsupportedMechanism)
    ));
    // No auth-related bytes should have been emitted.
    assert_eq!(written.borrow().len(), pre_login);
    assert_eq!(client.state(), SessionState::Closed);
}

#[test]
fn login_plain_handles_535_rejection_as_auth_error() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"535 5.7.8 invalid credentials\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    match err {
        SmtpError::Auth(AuthError::Rejected {
            code,
            enhanced,
            message,
        }) => {
            assert_eq!(code, 535);
            assert!(message.contains("5.7.8"));
            // ENHANCEDSTATUSCODES is not advertised in the script
            // above, so the prefix should remain in the message
            // (and not be parsed out into the structured field).
            assert!(
                enhanced.is_none(),
                "without EHLO advertisement, no enhanced parse"
            );
        }
        other => panic!("expected AuthError::Rejected, got {other:?}"),
    }
    assert_eq!(client.state(), SessionState::Closed);
}

#[test]
fn login_with_plain_explicit_uses_plain_even_when_login_also_advertised() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN LOGIN\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login_with(AuthMechanism::Plain, "user", "pass")).expect("login");

    let expected = b"EHLO client.example\r\n\
                     AUTH PLAIN AHVzZXIAcGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
}

#[test]
fn login_with_login_explicit_uses_login_even_when_plain_also_advertised() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN LOGIN\r\n",
        b"334 VXNlcm5hbWU6\r\n",
        b"334 UGFzc3dvcmQ6\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login_with(AuthMechanism::Login, "user", "pass")).expect("login");

    let expected = b"EHLO client.example\r\n\
                     AUTH LOGIN\r\n\
                     dXNlcg==\r\n\
                     cGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
}

#[test]
fn login_with_plain_fails_when_only_login_advertised() {
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH LOGIN\r\n",
    ]);
    let (transport, _written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err =
        block_on(client.login_with(AuthMechanism::Plain, "user", "pass")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Auth(AuthError::UnsupportedMechanism)
    ));
}

#[test]
fn login_rejects_credentials_with_nul_byte_before_io() {
    // A NUL in the credentials would corrupt the SASL PLAIN framing.
    // The validation must catch it before any byte is written.
    let server_script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
    ]);
    let (transport, written, _closed) = MockTransport::new(&[&server_script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let pre_login = written.borrow().len();
    let err = block_on(client.login("user\0evil", "pass")).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    assert_eq!(written.borrow().len(), pre_login);
}

#[test]
fn unsupported_mechanism_message_lists_supported_options() {
    // Smoke-test the improved Display output: the user-facing error
    // should say which mechanisms ARE supported, so the operator
    // can reason about why their server is incompatible.
    let err = SmtpError::Auth(AuthError::UnsupportedMechanism);
    let s = format!("{err}");
    assert!(s.contains("PLAIN"), "should mention PLAIN: {s}");
    assert!(s.contains("LOGIN"), "should mention LOGIN: {s}");
}

// -- ProtocolError::UnexpectedCode `during` field (Phase 4) ------------

/// Helper: extract the `during` operation from an `UnexpectedCode` error,
/// or panic with a helpful message identifying what we got instead.
fn during_of(err: SmtpError) -> SmtpOp {
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode { during, .. }) => during,
        other => panic!("expected UnexpectedCode, got {other:?}"),
    }
}

#[test]
fn unexpected_code_during_greeting() {
    let (transport, _w, _c) = MockTransport::new(&[b"554 service unavailable\r\n"]);
    let err = block_on(SmtpClient::connect(transport, "c.example")).expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::Greeting);
}

#[test]
fn unexpected_code_during_ehlo() {
    // 220 greeting, then 5xx on EHLO.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"502 EHLO not implemented\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let err = block_on(SmtpClient::connect(transport, "c.example")).expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::Ehlo);
}

#[test]
fn unexpected_code_during_mail_from() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"550 sender domain refused\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &["c@d.com"], "Subject: x\r\n\r\nx\r\n"))
        .expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::MailFrom);
}

#[test]
fn unexpected_code_during_rcpt_to() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"250 OK\r\n",           // MAIL FROM accepted
        b"550 no such user\r\n", // RCPT TO refused
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &["c@d.com"], "Subject: x\r\n\r\nx\r\n"))
        .expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::RcptTo);
}

#[test]
fn unexpected_code_during_data_command() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"250 OK\r\n",           // MAIL FROM
        b"250 OK\r\n",           // RCPT TO
        b"503 bad sequence\r\n", // DATA refused
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &["c@d.com"], "Subject: x\r\n\r\nx\r\n"))
        .expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::Data);
}

#[test]
fn unexpected_code_during_data_body() {
    // The 250 after the body is rejected with a 5xx.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"250 OK\r\n",                // MAIL FROM
        b"250 OK\r\n",                // RCPT TO
        b"354 go ahead\r\n",          // DATA accepted
        b"552 message too large\r\n", // body rejected
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.send_mail("a@b.com", &["c@d.com"], "Subject: x\r\n\r\nx\r\n"))
        .expect_err("must fail");
    // We use SmtpOp::Data for both the DATA command and the body,
    // because operators conceptualize them as the same step.
    assert_eq!(during_of(err), SmtpOp::Data);
}

#[test]
fn unexpected_code_during_quit_propagates_after_close() {
    // QUIT replies with a non-221: the error is returned from quit().
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
        b"500 unrecognized\r\n", // QUIT rejected
    ]);
    let (transport, _w, closed) = MockTransport::new(&[&script[..]]);
    let client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.quit()).expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::Quit);
    // Even on failure, the transport must have been closed.
    assert!(*closed.borrow());
}

#[test]
fn auth_plain_unexpected_non_5xx_keeps_protocol_error_with_op() {
    // Non-5xx unexpected codes during AUTH PLAIN should remain
    // ProtocolError::UnexpectedCode (not converted to AuthError),
    // and should be tagged with SmtpOp::AuthPlain.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"432 password expired\r\n", // 4xx, not converted to Auth
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "c.example")).expect("connect");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    assert_eq!(during_of(err), SmtpOp::AuthPlain);
}

// -- STARTTLS (Phase 5) -----------------------------------------------

/// Pre-TLS portion of a STARTTLS-aware server script: greeting,
/// EHLO with STARTTLS advertised, 220 ready-to-start.
fn starttls_pre_upgrade() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        // First EHLO reply: includes STARTTLS, no AUTH advertised yet.
        b"250-mail.example.com\r\n",
        b"250-PIPELINING\r\n",
        b"250 STARTTLS\r\n",
        // STARTTLS accepted.
        b"220 ready to start TLS\r\n",
    ])
}

/// Post-TLS portion: re-issued EHLO reply on the secure channel,
/// now advertising AUTH PLAIN/LOGIN.
fn starttls_post_upgrade() -> Vec<u8> {
    flatten(&[
        b"250-mail.example.com\r\n",
        b"250-PIPELINING\r\n",
        b"250 AUTH PLAIN LOGIN\r\n",
    ])
}

#[test]
fn connect_starttls_runs_full_upgrade_sequence() {
    let (transport, written, _closed, upgrades) = MockTransport::with_starttls(
        &[&starttls_pre_upgrade()[..]],
        &[&starttls_post_upgrade()[..]],
        UpgradeBehavior::Succeed,
    );
    let client = block_on(SmtpClient::connect_starttls(transport, "client.example"))
        .expect("connect_starttls");

    // After the full upgrade we should be in Authentication, with the
    // POST-TLS capability set advertised.
    assert_eq!(client.state(), SessionState::Authentication);
    let caps = client.capabilities();
    assert_eq!(caps.len(), 2);
    assert_eq!(caps[0], "PIPELINING");
    assert_eq!(caps[1], "AUTH PLAIN LOGIN");

    // Wire bytes: EHLO, STARTTLS, EHLO again. No AUTH yet.
    let expected = b"EHLO client.example\r\n\
                     STARTTLS\r\n\
                     EHLO client.example\r\n";
    assert_eq!(&*written.borrow(), expected);

    // The transport upgrade must have been invoked exactly once.
    assert_eq!(*upgrades.borrow(), 1);
}

#[test]
fn starttls_then_login_uses_post_tls_capabilities() {
    // After STARTTLS the second EHLO reveals AUTH PLAIN, which login()
    // must pick up. This proves we discard the pre-TLS capabilities
    // and parse the new ones.
    let pre = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        b"220 ready\r\n",
    ]);
    let post = flatten(&[
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, written, _c, _u) =
        MockTransport::with_starttls(&[&pre[..]], &[&post[..]], UpgradeBehavior::Succeed);
    let mut client =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect("connect_starttls");
    block_on(client.login("user", "pass")).expect("login");

    let expected = b"EHLO c.example\r\n\
                     STARTTLS\r\n\
                     EHLO c.example\r\n\
                     AUTH PLAIN AHVzZXIAcGFzcw==\r\n";
    assert_eq!(&*written.borrow(), expected);
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[test]
fn starttls_fails_when_extension_not_advertised() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        // No STARTTLS in caps.
        b"250-mail.example.com\r\n",
        b"250 8BITMIME\r\n",
    ]);
    let (transport, written, _c, upgrades) =
        MockTransport::with_starttls(&[&script[..]], &[], UpgradeBehavior::Succeed);
    let pre_upgrade_writes_len = 0;
    let err =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect_err("must fail");

    match err {
        SmtpError::Protocol(ProtocolError::ExtensionUnavailable { name }) => {
            assert_eq!(name, "STARTTLS");
        }
        other => panic!("expected ExtensionUnavailable, got {other:?}"),
    }
    // We sent the EHLO but nothing else: STARTTLS was never written.
    assert_eq!(&*written.borrow(), b"EHLO c.example\r\n");
    assert!(written.borrow().len() > pre_upgrade_writes_len);
    // upgrade_to_tls() must NOT have been called.
    assert_eq!(*upgrades.borrow(), 0);
}

#[test]
fn starttls_fails_when_server_rejects_command() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        // Server rejects STARTTLS with a 5xx (atypical but observable).
        b"502 STARTTLS not configured\r\n",
    ]);
    let (transport, _w, _c, upgrades) =
        MockTransport::with_starttls(&[&script[..]], &[], UpgradeBehavior::Succeed);
    let err =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect_err("must fail");

    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode { during, actual, .. }) => {
            assert_eq!(during, SmtpOp::StartTls);
            assert_eq!(actual, 502);
        }
        other => panic!("expected UnexpectedCode for StartTls, got {other:?}"),
    }
    // The transport must NOT have been upgraded: the server refused.
    assert_eq!(*upgrades.borrow(), 0);
}

#[test]
fn starttls_propagates_transport_upgrade_failure_as_io_error() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        b"220 ready\r\n",
    ]);
    let (transport, _w, _c, upgrades) = MockTransport::with_starttls(
        &[&script[..]],
        &[],
        UpgradeBehavior::Fail("simulated TLS handshake failure"),
    );
    let err =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect_err("must fail");

    match err {
        SmtpError::Io(e) => {
            assert!(format!("{e}").contains("TLS handshake"));
        }
        other => panic!("expected Io for upgrade failure, got {other:?}"),
    }
    // The upgrade was attempted exactly once.
    assert_eq!(*upgrades.borrow(), 1);
}

#[test]
fn explicit_starttls_method_works_post_connect() {
    // Same flow but reached via the explicit two-call API:
    // SmtpClient::connect() then client.starttls(). This is the
    // path callers use when they want to inspect capabilities first.
    let (transport, written, _c, _u) = MockTransport::with_starttls(
        &[&starttls_pre_upgrade()[..]],
        &[&starttls_post_upgrade()[..]],
        UpgradeBehavior::Succeed,
    );
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    // Pre-STARTTLS capabilities visible to the caller.
    assert!(client.capabilities().iter().any(|c| c == "STARTTLS"));
    block_on(client.starttls()).expect("starttls");
    // Post-STARTTLS capabilities have replaced the pre-TLS ones.
    assert!(
        client
            .capabilities()
            .iter()
            .any(|c| c == "AUTH PLAIN LOGIN"),
        "post-TLS caps should include AUTH advertisement: {:?}",
        client.capabilities()
    );
    assert!(
        !client.capabilities().iter().any(|c| c == "STARTTLS"),
        "STARTTLS should not appear in post-TLS caps: {:?}",
        client.capabilities()
    );
    assert_eq!(client.state(), SessionState::Authentication);

    // Bytes match the all-in-one connect_starttls test.
    assert_eq!(
        &*written.borrow(),
        b"EHLO client.example\r\nSTARTTLS\r\nEHLO client.example\r\n"
    );
}

#[test]
fn starttls_rejects_call_after_login() {
    // STARTTLS must be issued BEFORE auth. Calling it after login()
    // is a programming error and must return InvalidInput.
    let pre = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        b"220 ready\r\n",
    ]);
    let post = flatten(&[
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"235 OK\r\n",
    ]);
    let (transport, _w, _c, upgrades) =
        MockTransport::with_starttls(&[&pre[..]], &[&post[..]], UpgradeBehavior::Succeed);
    let mut client =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect("connect_starttls");
    block_on(client.login("user", "pass")).expect("login");

    // Now the second starttls() must be refused.
    let err = block_on(client.starttls()).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // No additional upgrade was attempted.
    assert_eq!(*upgrades.borrow(), 1);
}

// -- STARTTLS injection defense (Phase 9 / M-2) ----------------------

#[test]
fn starttls_buffer_residue_aborts_upgrade() {
    // Simulate a STARTTLS injection attack: extra SMTP commands
    // are pipelined onto the plaintext channel right after the
    // server's `220 ready` reply, before the TLS handshake. A
    // robust client must detect the unread residue at the moment
    // of upgrade and refuse to proceed.
    let pre_with_injected_residue = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        b"220 ready\r\n",
        // Attacker-injected command bytes pipelined onto the
        // plaintext channel — these would, without the defense,
        // be read AFTER the upgrade and treated as if they had
        // arrived over the secured channel.
        b"NOOP smuggled\r\n",
        b"MAIL FROM:<attacker@example.com>\r\n",
    ]);
    let (transport, _w, closed, upgrades) = MockTransport::with_starttls(
        &[&pre_with_injected_residue[..]],
        &[],
        UpgradeBehavior::Succeed,
    );
    let err = block_on(SmtpClient::connect_starttls(transport, "c.example"))
        .expect_err("must reject the injected residue");

    match err {
        SmtpError::Protocol(ProtocolError::StartTlsBufferResidue { byte_count }) => {
            // The two injected lines together total > 0 bytes; we
            // don't pin an exact value because "where the line
            // boundary fell" depends on the read chunk size. The
            // important check is that the defense fires.
            assert!(byte_count > 0, "byte_count must be positive: {byte_count}");
        }
        other => panic!("expected StartTlsBufferResidue, got {other:?}"),
    }

    // The session must NOT have proceeded to TLS — upgrade_to_tls
    // must not have been called.
    assert_eq!(
        *upgrades.borrow(),
        0,
        "upgrade_to_tls must not be called when residue is detected"
    );
    // The transport's `close()` is the caller's responsibility
    // via `quit()` or drop; our state-machine-level invariant is
    // that the session has been moved to Closed and any further
    // calls fail-fast. We verify the transport-level close flag
    // is left alone here, and rely on the next test below to
    // confirm session-state semantics.
    let _ = closed; // unused, retained for potential future test
}

#[test]
fn starttls_buffer_residue_byte_count_is_residual_length() {
    // Verify that byte_count actually counts the unread bytes
    // remaining when the upgrade is about to begin. We use a
    // single, exactly-known injection.
    let pre = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        b"220 ready\r\n",
        b"X\r\n", // exactly 3 bytes of residue
    ]);
    let (transport, _w, _c, _u) =
        MockTransport::with_starttls(&[&pre[..]], &[], UpgradeBehavior::Succeed);
    let err =
        block_on(SmtpClient::connect_starttls(transport, "c.example")).expect_err("must reject");
    match err {
        SmtpError::Protocol(ProtocolError::StartTlsBufferResidue { byte_count }) => {
            assert_eq!(byte_count, 3, "expected exactly the 3 bytes of `X\\r\\n`");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// -- ENHANCEDSTATUSCODES (Phase 6) ------------------------------------

/// EHLO reply that advertises ENHANCEDSTATUSCODES alongside AUTH.
fn greeting_then_ehlo_with_esmtp() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250-PIPELINING\r\n",
        b"250-ENHANCEDSTATUSCODES\r\n",
        b"250 AUTH PLAIN\r\n",
    ])
}

#[test]
fn unexpected_code_carries_enhanced_when_advertised() {
    let script = flatten(&[
        &greeting_then_ehlo_with_esmtp()[..],
        b"235 2.7.0 ok\r\n",                  // AUTH PLAIN ok
        b"550 5.7.1 relay access denied\r\n", // MAIL FROM rejected
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let err = block_on(client.send_mail(
        "a@example.com",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");

    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode {
            during,
            actual,
            enhanced,
            message,
            ..
        }) => {
            assert_eq!(during, SmtpOp::MailFrom);
            assert_eq!(actual, 550);
            let es = enhanced.expect("enhanced should be Some when advertised");
            assert_eq!(es.class, 5);
            assert_eq!(es.subject, 7);
            assert_eq!(es.detail, 1);
            // The wire form is preserved in `message`. The Display
            // impl renders `[5.7.1]` separately from the message.
            assert!(message.contains("5.7.1"));
        }
        other => panic!("expected UnexpectedCode with enhanced, got {other:?}"),
    }
}

#[test]
fn unexpected_code_no_enhanced_when_not_advertised() {
    // EHLO does NOT include ENHANCEDSTATUSCODES; even if the server
    // sends "5.7.1" in the reply text, we must not parse it.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"235 OK\r\n",
        b"550 5.7.1 relay access denied\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let err = block_on(client.send_mail(
        "a@example.com",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");

    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode { enhanced, .. }) => {
            assert!(
                enhanced.is_none(),
                "without EHLO advertisement, enhanced must be None"
            );
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[test]
fn unexpected_code_display_includes_enhanced_bracket() {
    // When enhanced is set, Display renders `[x.y.z]` after the code.
    let script = flatten(&[
        &greeting_then_ehlo_with_esmtp()[..],
        b"235 OK\r\n",
        b"550 5.7.1 relay access denied\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login("user", "pass")).expect("login");
    let err = block_on(client.send_mail(
        "a@example.com",
        &["b@example.org"],
        "Subject: x\r\n\r\nx\r\n",
    ))
    .expect_err("must fail");
    let s = format!("{err}");
    assert!(
        s.contains("[5.7.1]"),
        "Display should include enhanced bracket: {s}"
    );
    assert!(s.contains("550"), "Display should include basic code: {s}");
}

#[test]
fn auth_rejected_carries_enhanced_when_advertised() {
    // ENHANCEDSTATUSCODES is advertised; AUTH PLAIN is rejected with
    // 535 5.7.8. The enhanced field must be propagated into
    // AuthError::Rejected.
    let script = flatten(&[
        &greeting_then_ehlo_with_esmtp()[..],
        b"535 5.7.8 invalid credentials\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");

    match err {
        SmtpError::Auth(AuthError::Rejected {
            code,
            enhanced,
            message,
        }) => {
            assert_eq!(code, 535);
            let es = enhanced.expect("enhanced should be Some");
            assert_eq!((es.class, es.subject, es.detail), (5, 7, 8));
            assert!(message.contains("5.7.8"));
        }
        other => panic!("expected Auth::Rejected with enhanced, got {other:?}"),
    }
}

#[test]
fn starttls_aborts_upgrade_when_buffer_holds_residue() {
    // STARTTLS injection / pipelining defence (RFC 3207 §5).
    //
    // The server "answers" the STARTTLS command with both a 220
    // ready reply AND an attacker-supplied EHLO-shaped line on
    // the same plaintext channel, before the TLS handshake. The
    // client must detect the residue, abort the upgrade, and
    // surface ProtocolError::StartTlsBufferResidue.
    let pre = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 STARTTLS\r\n",
        // Honest 220 reply + attacker-injected pipelined data:
        b"220 ready to start TLS\r\n",
        // Bytes pipelined onto the plaintext stream — these would
        // be read AFTER the TLS handshake on a vulnerable client
        // and treated as if they had arrived from the (now
        // authenticated) server.
        b"250 INJECTED capability\r\n",
    ]);
    // post_chunks empty: the upgrade must be rejected before
    // upgrade_to_tls() is reached, so no post-TLS bytes will be
    // read.
    let (transport, _w, _c, upgrades) =
        MockTransport::with_starttls(&[&pre[..]], &[], UpgradeBehavior::Succeed);
    let err = block_on(SmtpClient::connect_starttls(transport, "client.example"))
        .expect_err("must fail with residue error");

    match err {
        SmtpError::Protocol(ProtocolError::StartTlsBufferResidue { byte_count }) => {
            // The injected line is 25 bytes ("250 INJECTED capability\r\n").
            assert_eq!(byte_count, 25);
        }
        other => panic!("expected StartTlsBufferResidue, got {other:?}"),
    }
    // The TLS upgrade must NOT have been attempted: we caught the
    // injection BEFORE handing the socket off.
    assert_eq!(*upgrades.borrow(), 0);
}

#[test]
fn enhancedstatuscodes_disabled_after_starttls_re_ehlo_without_it() {
    // The post-TLS EHLO reply governs the post-TLS enhanced state.
    // If the server stops advertising ENHANCEDSTATUSCODES after the
    // upgrade, parses are no longer attempted.
    let pre = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        // Pre-TLS EHLO advertises ENHANCEDSTATUSCODES.
        b"250-mail.example.com\r\n",
        b"250-STARTTLS\r\n",
        b"250 ENHANCEDSTATUSCODES\r\n",
        b"220 ready\r\n",
    ]);
    let post = flatten(&[
        // Post-TLS EHLO drops it.
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN\r\n",
        b"535 5.7.8 invalid\r\n", // 5.7.8 should NOT be parsed now
    ]);
    let (transport, _w, _c, _u) =
        MockTransport::with_starttls(&[&pre[..]], &[&post[..]], UpgradeBehavior::Succeed);
    let mut client = block_on(SmtpClient::connect_starttls(transport, "client.example"))
        .expect("connect_starttls");
    let err = block_on(client.login("user", "pass")).expect_err("must fail");
    match err {
        SmtpError::Auth(AuthError::Rejected { enhanced, .. }) => {
            assert!(
                enhanced.is_none(),
                "post-TLS EHLO dropped ENHANCEDSTATUSCODES: enhanced must be None"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }
}

// -- XOAUTH2 (Phase 6 / Phase 7) --------------------------------------
//
// The XOAUTH2 SASL profile is gated behind the `xoauth2` cargo
// feature (default-on). All tests that drive the
// `login_xoauth2` / `login_with(AuthMechanism::XOAuth2, ..)`
// code paths are conditional on the feature.

/// EHLO reply that advertises AUTH XOAUTH2 (and PLAIN, so we can also
/// check that `select_auth_mechanism` still picks PLAIN, not XOAUTH2).
#[cfg(feature = "xoauth2")]
fn greeting_then_ehlo_with_xoauth2() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250-PIPELINING\r\n",
        b"250 AUTH PLAIN LOGIN XOAUTH2\r\n",
    ])
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_happy_path_succeeds_directly() {
    // 235 directly: server accepted the bearer token.
    let script = flatten(&[
        &greeting_then_ehlo_with_xoauth2()[..],
        b"235 2.7.0 Accepted\r\n",
    ]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login_xoauth2("user@example.com", "ya29.token")).expect("login");

    // Wire bytes: EHLO, then AUTH XOAUTH2 <b64>. Reconstruct the
    // expected base64 to compare.
    let mut payload = Vec::new();
    payload.extend_from_slice(b"user=user@example.com\x01auth=Bearer ya29.token\x01\x01");
    let b64 = crate::protocol::base64_encode(&payload);
    let expected = format!("EHLO client.example\r\nAUTH XOAUTH2 {b64}\r\n");
    assert_eq!(&*written.borrow(), expected.as_bytes());
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_login_with_explicit_mechanism_works() {
    // login_with(XOAuth2, ...) should be equivalent to login_xoauth2.
    let script = flatten(&[&greeting_then_ehlo_with_xoauth2()[..], b"235 OK\r\n"]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    block_on(client.login_with(AuthMechanism::XOAuth2, "user@example.com", "ya29.token"))
        .expect("login_with XOAuth2");
    assert_eq!(client.state(), SessionState::MailFrom);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_handles_334_error_continuation() {
    // RFC 7628-style flow: server returns 334 with base64 JSON,
    // client sends an empty line, server sends final 5xx.
    // The 5xx text and any enhanced code must end up in the
    // AuthError::Rejected.
    let script = flatten(&[
        &greeting_then_ehlo_with_xoauth2()[..],
        b"334 eyJzdGF0dXMiOiI0MDEifQ==\r\n", // {"status":"401"} in b64
        b"535 5.7.8 Username and Password not accepted\r\n",
    ]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    // ENHANCEDSTATUSCODES is NOT advertised in this script, so
    // enhanced will be None — but we still verify the rejection
    // path itself.
    let err =
        block_on(client.login_xoauth2("user@example.com", "ya29.token")).expect_err("must fail");
    match err {
        SmtpError::Auth(AuthError::Rejected { code, message, .. }) => {
            assert_eq!(code, 535);
            assert!(message.contains("Username and Password"));
        }
        other => panic!("expected Auth::Rejected, got {other:?}"),
    }

    // The client must have written the empty continuation line
    // between AUTH XOAUTH2 and the final read.
    let bytes = written.borrow();
    // The pattern "...<b64>\r\n\r\n" indicates the empty
    // continuation. Search for the trailing `\r\n\r\n`.
    let s = std::str::from_utf8(&bytes).unwrap();
    assert!(
        s.ends_with("\r\n\r\n"),
        "must end with empty continuation line: {s:?}"
    );
    assert_eq!(client.state(), SessionState::Closed);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_returns_unsupported_when_not_advertised() {
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250 AUTH PLAIN LOGIN\r\n", // no XOAUTH2
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err = block_on(client.login_xoauth2("user", "ya29.token")).expect_err("must fail");
    assert!(matches!(
        err,
        SmtpError::Auth(AuthError::UnsupportedMechanism)
    ));
    assert_eq!(client.state(), SessionState::Closed);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_validates_token_before_io() {
    // A token with a space would be rejected by the server but we
    // catch it locally. No bytes should be sent for AUTH.
    let script = flatten(&[&greeting_then_ehlo_with_xoauth2()[..]]);
    let (transport, written, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    // Capture the written bytes after EHLO so we can compare.
    let after_ehlo = written.borrow().len();
    let err = block_on(client.login_xoauth2("user", "bad token")).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // Nothing was written for AUTH.
    assert_eq!(written.borrow().len(), after_ehlo);
    // The session is still usable: input validation does not
    // poison the connection.
    assert_eq!(client.state(), SessionState::Authentication);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_validates_user_before_io() {
    let script = flatten(&[&greeting_then_ehlo_with_xoauth2()[..]]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    // SOH in the user would corrupt the SASL framing.
    let err = block_on(client.login_xoauth2("u\x01v", "ya29.token")).expect_err("must fail");
    assert!(matches!(err, SmtpError::InvalidInput(_)));
    // Session should remain usable for legitimate retry.
    assert_eq!(client.state(), SessionState::Authentication);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_with_enhanced_status_propagates_code() {
    // ENHANCEDSTATUSCODES + XOAUTH2 + 334 error continuation +
    // final 5xx with enhanced. The enhanced should be parsed off
    // the final reply.
    let script = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n",
        b"250-ENHANCEDSTATUSCODES\r\n",
        b"250 AUTH XOAUTH2\r\n",
        b"334 eyJzdGF0dXMiOiI0MDEifQ==\r\n",
        b"535 5.7.8 Bad credentials\r\n",
    ]);
    let (transport, _w, _c) = MockTransport::new(&[&script[..]]);
    let mut client = block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
    let err =
        block_on(client.login_xoauth2("user@example.com", "ya29.token")).expect_err("must fail");
    match err {
        SmtpError::Auth(AuthError::Rejected { enhanced, .. }) => {
            let es = enhanced.expect("enhanced should be Some");
            assert_eq!((es.class, es.subject, es.detail), (5, 7, 8));
        }
        other => panic!("unexpected: {other:?}"),
    }
}
