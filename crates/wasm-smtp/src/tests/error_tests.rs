//! Tests for the public error surface in `error.rs`.

use crate::error::{AuthError, InvalidInputError, IoError, ProtocolError, SmtpError, SmtpOp};
use std::error::Error;

#[test]
fn smtp_error_display_protocol_includes_code_and_message() {
    let e = SmtpError::Protocol(ProtocolError::UnexpectedCode {
        during: SmtpOp::MailFrom,
        expected_class: 2,
        actual: 451,
        enhanced: None,
        message: "temporary local problem".into(),
    });
    let s = format!("{e}");
    assert!(s.contains("451"), "should include actual code: {s}");
    assert!(
        s.contains("temporary local problem"),
        "should include server text: {s}"
    );
    // Phase 4: the operation context should be visible to operators
    // reading logs.
    assert!(
        s.contains("MAIL FROM"),
        "should mention the SMTP operation in progress: {s}"
    );
}

#[test]
fn smtp_op_display_uses_wire_keyword() {
    // Quick coverage: every op variant should produce a non-empty
    // string in Display, matching the SMTP wire keyword where there
    // is one.
    for (op, expected) in [
        (SmtpOp::Greeting, "greeting"),
        (SmtpOp::Ehlo, "EHLO"),
        (SmtpOp::StartTls, "STARTTLS"),
        (SmtpOp::AuthPlain, "AUTH PLAIN"),
        (SmtpOp::AuthLogin, "AUTH LOGIN"),
        (SmtpOp::AuthXOAuth2, "AUTH XOAUTH2"),
        (SmtpOp::MailFrom, "MAIL FROM"),
        (SmtpOp::RcptTo, "RCPT TO"),
        (SmtpOp::Data, "DATA"),
        (SmtpOp::Quit, "QUIT"),
    ] {
        assert_eq!(format!("{op}"), expected);
        assert_eq!(op.as_str(), expected);
    }
}

#[test]
fn auth_rejected_carries_server_code_and_text() {
    let e = SmtpError::Auth(AuthError::Rejected {
        code: 535,
        enhanced: None,
        message: "5.7.8 invalid".into(),
    });
    let s = format!("{e}");
    assert!(s.contains("535"));
    assert!(s.contains("5.7.8 invalid"));
}

#[test]
fn invalid_input_takes_only_static_strings() {
    // The constructor signature is `&'static str`, so it is a
    // compile-time guarantee that runtime user input cannot be
    // embedded into the error message.
    let e = InvalidInputError::new("test reason");
    assert_eq!(e.reason(), "test reason");
    assert_eq!(format!("{e}"), "test reason");
}

#[test]
fn from_conversions_wrap_in_correct_variant() {
    let e: SmtpError = IoError::new("transport gone").into();
    assert!(matches!(e, SmtpError::Io(_)));
    let e: SmtpError = ProtocolError::UnexpectedClose.into();
    assert!(matches!(e, SmtpError::Protocol(_)));
    let e: SmtpError = AuthError::UnsupportedMechanism.into();
    assert!(matches!(e, SmtpError::Auth(_)));
    let e: SmtpError = InvalidInputError::new("x").into();
    assert!(matches!(e, SmtpError::InvalidInput(_)));
}

#[test]
fn smtp_error_source_chains_to_inner_variant() {
    let e: SmtpError = IoError::new("inner").into();
    let src = e.source().expect("should have source");
    assert!(format!("{src}").contains("inner"));
}
