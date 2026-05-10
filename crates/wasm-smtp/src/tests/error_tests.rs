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

// -- IoError source chain (Phase 12) --------------------------------------

#[test]
fn io_error_new_has_no_source() {
    let e = IoError::new("simple message");
    assert_eq!(e.message(), "simple message");
    assert_eq!(format!("{e}"), "simple message");
    assert!(e.source().is_none(), "new() must not synthesize a source");
}

#[test]
fn io_error_with_source_preserves_inner() {
    use std::io;

    let inner = io::Error::new(io::ErrorKind::ConnectionRefused, "no listener at port 1");
    let outer = IoError::with_source("TCP connect failed", inner);

    // Display shows only the high-level message.
    assert_eq!(format!("{outer}"), "TCP connect failed");

    // The source chain carries the original io::Error.
    let src = outer.source().expect("source must be present");
    let src_str = format!("{src}");
    assert!(
        src_str.contains("no listener at port 1"),
        "source should preserve original message: {src_str}"
    );
}

#[test]
fn io_error_with_source_accepts_arbitrary_error_types() {
    // The bound is `StdError + Send + Sync + 'static`. Confirm a few
    // representative concrete types compose.
    use std::io;

    // Synthetic custom error type — defined first so that the lints
    // about items-after-statements stay quiet.
    #[derive(Debug)]
    struct CustomError(&'static str);
    impl std::fmt::Display for CustomError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }
    impl Error for CustomError {}

    // io::Error
    let _ = IoError::with_source("io", io::Error::other("x"));

    // Custom Error type
    let _ = IoError::with_source("custom", CustomError("oops"));
}

#[test]
fn io_error_from_io_error_carries_source() {
    use std::io;

    let original = io::Error::new(io::ErrorKind::TimedOut, "read timed out");
    let wrapped: IoError = original.into();

    // Display message comes from the original io::Error.
    assert!(
        format!("{wrapped}").contains("read timed out"),
        "From<io::Error> should use the io::Error's Display as message",
    );
    assert!(
        wrapped.source().is_some(),
        "From<io::Error> should preserve source"
    );
}

#[test]
fn io_error_chains_through_smtp_error_source() {
    use std::io;

    // Verify the full chain: SmtpError -> IoError -> io::Error.
    // Caller-side error formatters that walk `.source()` repeatedly
    // should reach the original io::Error.
    let inner = io::Error::new(io::ErrorKind::BrokenPipe, "EPIPE");
    let io = IoError::with_source("write failed", inner);
    let smtp: SmtpError = io.into();

    let level1 = smtp.source().expect("SmtpError should have source");
    assert!(format!("{level1}").contains("write failed"));

    let level2 = level1.source().expect("IoError should have source too");
    assert!(format!("{level2}").contains("EPIPE"));
}

#[test]
fn io_error_send_sync_bounds_compile() {
    // The `Box<dyn StdError + Send + Sync>` source means an `IoError`
    // can be carried across thread boundaries, important for tokio
    // adapters where errors may surface on a different worker
    // thread than the one that observed them. We verify the bound
    // here at compile time.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<IoError>();
    assert_send_sync::<SmtpError>();
}

// -- io_kind() and is_* helpers ---------------------------------------------

#[test]
fn io_kind_extracts_kind_from_direct_io_error() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::TimedOut, "took too long");
    let wrapped = IoError::with_source("connect failed", inner);
    assert_eq!(wrapped.io_kind(), Some(io::ErrorKind::TimedOut));
}

#[test]
fn io_kind_extracts_kind_through_nested_source_chain() {
    use std::io;
    // Simulate an adapter that wraps an io::Error inside an
    // intermediate error type before handing it to IoError. The
    // io_kind walker should find the io::Error two levels down.
    #[derive(Debug)]
    struct Outer {
        inner: io::Error,
    }
    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("outer wrapper")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.inner)
        }
    }

    let outer = Outer {
        inner: io::Error::new(io::ErrorKind::ConnectionRefused, "no listener"),
    };
    let wrapped = IoError::with_source("smtp connect failed", outer);
    assert_eq!(wrapped.io_kind(), Some(io::ErrorKind::ConnectionRefused));
}

#[test]
fn io_kind_returns_none_when_no_io_error_in_chain() {
    // Source chain contains no io::Error. The walker must return
    // None rather than fabricating a kind.
    #[derive(Debug)]
    struct NotAnIoError;
    impl std::fmt::Display for NotAnIoError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("custom")
        }
    }
    impl std::error::Error for NotAnIoError {}

    let wrapped = IoError::with_source("certificate parse failed", NotAnIoError);
    assert_eq!(wrapped.io_kind(), None);
}

#[test]
fn io_kind_returns_none_when_no_source() {
    // IoError::new() produces an IoError with no source at all.
    let wrapped = IoError::new("plain message");
    assert_eq!(wrapped.io_kind(), None);
}

#[test]
fn is_timeout_recognizes_timed_out() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::TimedOut, "deadline");
    assert!(IoError::with_source("write failed", inner).is_timeout());
}

#[test]
fn is_timeout_rejects_other_kinds() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::ConnectionRefused, "no");
    assert!(!IoError::with_source("write failed", inner).is_timeout());
}

#[test]
fn is_connection_refused_recognizes_kind() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::ConnectionRefused, "no");
    assert!(IoError::with_source("connect failed", inner).is_connection_refused());
}

#[test]
fn is_connection_reset_recognizes_kind() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::ConnectionReset, "rst");
    assert!(IoError::with_source("read failed", inner).is_connection_reset());
}

#[test]
fn is_connection_aborted_recognizes_kind() {
    use std::io;
    let inner = io::Error::new(io::ErrorKind::ConnectionAborted, "abort");
    assert!(IoError::with_source("session failed", inner).is_connection_aborted());
}

#[test]
fn is_helpers_all_false_when_no_source() {
    let wrapped = IoError::new("just a message");
    assert!(!wrapped.is_timeout());
    assert!(!wrapped.is_connection_refused());
    assert!(!wrapped.is_connection_reset());
    assert!(!wrapped.is_connection_aborted());
}
