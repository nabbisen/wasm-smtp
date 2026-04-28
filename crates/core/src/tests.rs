//! Internal test suite for `wasm-smtp-core`.
//!
//! These tests exercise every layer of the crate that does not require a
//! real network:
//!
//! - `protocol_tests` covers reply-line parsing, command formatting,
//!   dot-stuffing, base64 encoding, input validation, and EHLO capability
//!   inspection.
//! - `session_tests` covers the [`crate::session::SessionState`] state
//!   machine.
//! - `error_tests` covers the public error surface and ensures
//!   [`crate::error::InvalidInputError`] cannot embed runtime-supplied
//!   strings.
//! - `client_tests` drives the full SMTP exchange against a synchronous
//!   mock transport.
//!
//! There is no executor: the mock transport always resolves immediately, so
//! a no-op waker is sufficient to drive the futures.

#![allow(
    // These pedantic lints are useful in production code but produce a lot
    // of noise in test fixtures, where short scripts and explicit byte
    // literals are the norm.
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::missing_panics_doc
)]

mod harness {
    use crate::error::IoError;
    use crate::transport::Transport;
    use core::future::Future;
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// Drive a future to completion using a no-op waker.
    ///
    /// This is sound only for futures whose `Pending` state would never be
    /// observed by a real executor: the mock transport in this module
    /// always resolves its `read` and `write_all` futures synchronously,
    /// so the very first `poll` will return `Ready`.
    pub fn block_on<F: Future>(fut: F) -> F::Output {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut fut = pin!(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("mock-driven future returned Pending"),
        }
    }

    /// Triple of (mock transport, captured outgoing bytes, close flag),
    /// returned by [`MockTransport::new`].
    pub type MockHandles = (MockTransport, Rc<RefCell<Vec<u8>>>, Rc<RefCell<bool>>);

    /// Synchronous mock transport.
    ///
    /// `incoming` is a queue of byte chunks; each chunk is one "wire
    /// delivery" and may be split across multiple `read` calls depending
    /// on the caller's buffer size. When the queue is exhausted, further
    /// `read`s return `Ok(0)`, which the SMTP state machine interprets as
    /// a clean close from the peer.
    ///
    /// `written` is held behind `Rc<RefCell<_>>` so the test can keep a
    /// handle to it after the transport has been moved into the client.
    pub struct MockTransport {
        incoming: VecDeque<Vec<u8>>,
        written: Rc<RefCell<Vec<u8>>>,
        closed: Rc<RefCell<bool>>,
    }

    impl MockTransport {
        /// Construct a mock transport from a list of byte chunks. Each
        /// chunk corresponds to one "wire packet". Returns the transport
        /// together with shared handles to the captured outgoing bytes
        /// and the close flag.
        pub fn new(chunks: &[&[u8]]) -> MockHandles {
            let written = Rc::new(RefCell::new(Vec::new()));
            let closed = Rc::new(RefCell::new(false));
            let mut q: VecDeque<Vec<u8>> = VecDeque::new();
            for c in chunks {
                q.push_back((*c).to_vec());
            }
            (
                Self {
                    incoming: q,
                    written: Rc::clone(&written),
                    closed: Rc::clone(&closed),
                },
                written,
                closed,
            )
        }
    }

    impl Transport for MockTransport {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
            let Some(chunk) = self.incoming.front_mut() else {
                return Ok(0);
            };
            let n = buf.len().min(chunk.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            chunk.drain(..n);
            if chunk.is_empty() {
                self.incoming.pop_front();
            }
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

    /// Concatenate several byte slices into one. Useful for assembling a
    /// scripted server reply that must be delivered in a single chunk.
    pub fn flatten(parts: &[&[u8]]) -> Vec<u8> {
        let mut v = Vec::new();
        for p in parts {
            v.extend_from_slice(p);
        }
        v
    }
}

// ---------------------------------------------------------------------------
// protocol.rs
// ---------------------------------------------------------------------------

mod protocol_tests {
    use crate::error::ProtocolError;
    use crate::protocol::{
        AuthMechanism, Reply, base64_encode, build_auth_plain_initial_response,
        dot_stuff_and_terminate, ehlo_advertises_auth, format_command, format_command_arg,
        format_mail_from, format_rcpt_to, parse_reply_line, select_auth_mechanism,
        validate_address, validate_ehlo_domain, validate_login_password, validate_login_username,
        validate_plain_password, validate_plain_username,
    };

    // -- parse_reply_line ----------------------------------------------------

    #[test]
    fn parse_reply_line_single_line() {
        let r = parse_reply_line(b"250 OK").expect("must parse");
        assert_eq!(r.code, 250);
        assert!(r.is_last);
        assert_eq!(r.text, b"OK");
    }

    #[test]
    fn parse_reply_line_continuation() {
        let r = parse_reply_line(b"250-mail.example.com Hello").expect("must parse");
        assert_eq!(r.code, 250);
        assert!(!r.is_last);
        assert_eq!(r.text, b"mail.example.com Hello");
    }

    #[test]
    fn parse_reply_line_three_digit_only_is_last() {
        let r = parse_reply_line(b"220").expect("must parse");
        assert_eq!(r.code, 220);
        assert!(r.is_last);
        assert_eq!(r.text, b"");
    }

    #[test]
    fn parse_reply_line_separator_with_empty_text() {
        let r = parse_reply_line(b"250 ").expect("must parse");
        assert_eq!(r.code, 250);
        assert!(r.is_last);
        assert_eq!(r.text, b"");
    }

    #[test]
    fn parse_reply_line_too_short() {
        assert!(matches!(
            parse_reply_line(b""),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            parse_reply_line(b"22"),
            Err(ProtocolError::Malformed(_))
        ));
    }

    #[test]
    fn parse_reply_line_non_digit_code() {
        assert!(matches!(
            parse_reply_line(b"abc OK"),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            parse_reply_line(b"2x0 OK"),
            Err(ProtocolError::Malformed(_))
        ));
    }

    #[test]
    fn parse_reply_line_invalid_separator() {
        assert!(matches!(
            parse_reply_line(b"250?Something"),
            Err(ProtocolError::Malformed(_))
        ));
        assert!(matches!(
            parse_reply_line(b"250\tSomething"),
            Err(ProtocolError::Malformed(_))
        ));
    }

    // -- Reply convenience methods ------------------------------------------

    #[test]
    fn reply_class_and_joined_text() {
        let r = Reply {
            code: 451,
            lines: vec!["temporary".into(), "failure".into()],
        };
        assert_eq!(r.class(), 4);
        assert_eq!(r.joined_text(), "temporary\nfailure");
        let collected: Vec<&str> = r.iter_lines().collect();
        assert_eq!(collected, vec!["temporary", "failure"]);
    }

    // -- format_* -----------------------------------------------------------

    #[test]
    fn format_command_basic() {
        assert_eq!(format_command("QUIT"), b"QUIT\r\n");
        assert_eq!(format_command("RSET"), b"RSET\r\n");
        assert_eq!(format_command("DATA"), b"DATA\r\n");
    }

    #[test]
    fn format_command_arg_basic() {
        assert_eq!(
            format_command_arg("EHLO", "client.example.com"),
            b"EHLO client.example.com\r\n"
        );
    }

    #[test]
    fn format_mail_from_wraps_in_brackets() {
        assert_eq!(
            format_mail_from("user@example.com"),
            b"MAIL FROM:<user@example.com>\r\n"
        );
    }

    #[test]
    fn format_rcpt_to_wraps_in_brackets() {
        assert_eq!(
            format_rcpt_to("recipient@example.org"),
            b"RCPT TO:<recipient@example.org>\r\n"
        );
    }

    // -- dot_stuff_and_terminate --------------------------------------------

    #[test]
    fn dot_stuff_simple_body() {
        let out = dot_stuff_and_terminate(b"Hello world");
        assert_eq!(out, b"Hello world\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_already_crlf_terminated() {
        let out = dot_stuff_and_terminate(b"Hello\r\n");
        assert_eq!(out, b"Hello\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_dot_at_first_byte() {
        let out = dot_stuff_and_terminate(b".dotted");
        assert_eq!(out, b"..dotted\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_dot_after_crlf() {
        let out = dot_stuff_and_terminate(b"first\r\n.second\r\n");
        assert_eq!(out, b"first\r\n..second\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_dot_only_line() {
        // A bare "." line would otherwise be confused with the terminator.
        let out = dot_stuff_and_terminate(b".\r\n");
        assert_eq!(out, b"..\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_dot_inside_line_not_stuffed() {
        let out = dot_stuff_and_terminate(b"a.b\r\n");
        assert_eq!(out, b"a.b\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_multiple_consecutive_dot_lines() {
        let out = dot_stuff_and_terminate(b".a\r\n.b\r\n.c\r\n");
        assert_eq!(out, b"..a\r\n..b\r\n..c\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_double_dot_only_first_is_at_line_start() {
        // First '.' is dot-stuffed (line start); second '.' is content.
        let out = dot_stuff_and_terminate(b"..line\r\n");
        assert_eq!(out, b"...line\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_empty_body() {
        let out = dot_stuff_and_terminate(b"");
        assert_eq!(out, b"\r\n.\r\n");
    }

    #[test]
    fn dot_stuff_terminator_pattern_inside_body_is_stuffed() {
        // The literal byte sequence "\r\n.\r\n" inside the body must not
        // look like a terminator on the wire.
        let out = dot_stuff_and_terminate(b"line\r\n.\r\nmore\r\n");
        assert_eq!(out, b"line\r\n..\r\nmore\r\n.\r\n");
    }

    // -- base64_encode ------------------------------------------------------

    #[test]
    fn base64_encode_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_encode_auth_login_canonical_examples() {
        assert_eq!(base64_encode(b"user"), "dXNlcg==");
        assert_eq!(base64_encode(b"pass"), "cGFzcw==");
        assert_eq!(base64_encode(b"Username:"), "VXNlcm5hbWU6");
        assert_eq!(base64_encode(b"Password:"), "UGFzc3dvcmQ6");
    }

    #[test]
    fn base64_encode_handles_high_bytes() {
        let out = base64_encode(&[0xFF, 0x00, 0xAA]);
        assert_eq!(out, "/wCq");
    }

    // -- validate_address ---------------------------------------------------

    #[test]
    fn validate_address_accepts_simple() {
        assert!(validate_address("a@b.com").is_ok());
        assert!(validate_address("first.last+tag@example.co.jp").is_ok());
    }

    #[test]
    fn validate_address_rejects_empty() {
        assert!(validate_address("").is_err());
    }

    #[test]
    fn validate_address_rejects_crlf_injection() {
        assert!(validate_address("a\r\n@b.com").is_err());
        assert!(validate_address("a@b.com\r").is_err());
        assert!(validate_address("a@b.com\n").is_err());
        assert!(validate_address("a@b.com\r\nRSET").is_err());
    }

    #[test]
    fn validate_address_rejects_brackets() {
        assert!(validate_address("<a@b.com>").is_err());
        assert!(validate_address("a@b<.com").is_err());
    }

    #[test]
    fn validate_address_rejects_whitespace() {
        assert!(validate_address("a @b.com").is_err());
        assert!(validate_address("a@b.com ").is_err());
        assert!(validate_address("a\tb@c.com").is_err());
    }

    #[test]
    fn validate_address_rejects_non_ascii() {
        assert!(validate_address("\u{30E6}\u{30FC}\u{30B6}@example.com").is_err());
    }

    #[test]
    fn validate_address_rejects_nul() {
        assert!(validate_address("a\0b@example.com").is_err());
    }

    // -- validate_ehlo_domain -----------------------------------------------

    #[test]
    fn validate_ehlo_domain_accepts_fqdn_and_address_literal() {
        assert!(validate_ehlo_domain("client.example.com").is_ok());
        assert!(validate_ehlo_domain("[192.0.2.1]").is_ok());
        assert!(validate_ehlo_domain("[IPv6:2001:db8::1]").is_ok());
    }

    #[test]
    fn validate_ehlo_domain_rejects_empty() {
        assert!(validate_ehlo_domain("").is_err());
    }

    #[test]
    fn validate_ehlo_domain_rejects_whitespace_and_crlf() {
        assert!(validate_ehlo_domain("client example com").is_err());
        assert!(validate_ehlo_domain("client.example.com\r\nRSET").is_err());
    }

    #[test]
    fn validate_ehlo_domain_rejects_non_ascii() {
        assert!(validate_ehlo_domain("\u{4F8B}.example").is_err());
    }

    // -- validate_login_* ---------------------------------------------------

    #[test]
    fn validate_login_credentials_reject_empty() {
        assert!(validate_login_username("").is_err());
        assert!(validate_login_password("").is_err());
        assert!(validate_login_username("user").is_ok());
        assert!(validate_login_password("pass").is_ok());
    }

    // -- ehlo_advertises_auth -----------------------------------------------

    #[test]
    fn ehlo_advertises_auth_finds_listed_mechanisms() {
        let lines: Vec<String> = vec![
            "PIPELINING".into(),
            "AUTH LOGIN PLAIN".into(),
            "8BITMIME".into(),
        ];
        assert!(ehlo_advertises_auth(&lines, "LOGIN"));
        assert!(ehlo_advertises_auth(&lines, "PLAIN"));
        assert!(!ehlo_advertises_auth(&lines, "CRAM-MD5"));
    }

    #[test]
    fn ehlo_advertises_auth_is_case_insensitive() {
        let lines: Vec<String> = vec!["auth login".into()];
        assert!(ehlo_advertises_auth(&lines, "LOGIN"));
        assert!(ehlo_advertises_auth(&lines, "login"));
    }

    #[test]
    fn ehlo_advertises_auth_no_auth_line_means_false() {
        let lines: Vec<String> = vec!["PIPELINING".into(), "8BITMIME".into()];
        assert!(!ehlo_advertises_auth(&lines, "LOGIN"));
    }

    // -- AUTH PLAIN ---------------------------------------------------------

    #[test]
    fn auth_plain_initial_response_canonical_example() {
        // Canonical example: empty authzid, "user", "pass".
        // Payload: \0 u s e r \0 p a s s = 0x00 0x75 0x73 0x65 0x72 0x00 0x70 0x61 0x73 0x73
        // Base64: AHVzZXIAcGFzcw==
        assert_eq!(
            build_auth_plain_initial_response("user", "pass"),
            "AHVzZXIAcGFzcw=="
        );
    }

    #[test]
    fn auth_plain_initial_response_round_trips_through_base64() {
        // Decoding the response should yield exactly \0user\0pass.
        let user = "alice@example.com";
        let pass = "s3cr3t!";
        let b64 = build_auth_plain_initial_response(user, pass);
        let decoded = decode_b64_in_test(&b64);
        let mut expected = Vec::new();
        expected.push(0u8);
        expected.extend_from_slice(user.as_bytes());
        expected.push(0u8);
        expected.extend_from_slice(pass.as_bytes());
        assert_eq!(decoded, expected);
    }

    #[test]
    fn auth_plain_initial_response_handles_utf8_password() {
        // RFC 4616 specifies UTF-8 for both fields; non-ASCII passwords
        // should pass through unchanged in the base64 payload.
        let pass = "p\u{00E1}ssw\u{00F8}rd";
        let b64 = build_auth_plain_initial_response("u", pass);
        let decoded = decode_b64_in_test(&b64);
        assert_eq!(decoded[0], 0);
        assert_eq!(&decoded[1..2], b"u");
        assert_eq!(decoded[2], 0);
        assert_eq!(&decoded[3..], pass.as_bytes());
    }

    /// Tiny base64 decoder used only by test code, to avoid depending on
    /// an external crate just for round-trip verification.
    fn decode_b64_in_test(s: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut idx = [255u8; 256];
        for (i, &b) in ALPHABET.iter().enumerate() {
            idx[b as usize] = u8::try_from(i).expect("alphabet fits in u8");
        }
        let chars: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
        let mut out = Vec::new();
        for quad in chars.chunks(4) {
            let mut n = 0u32;
            for (i, &c) in quad.iter().enumerate() {
                let v = idx[c as usize];
                assert!(v != 255, "non-base64 byte in test input");
                n |= u32::from(v) << (18 - 6 * i);
            }
            let bytes_out = match quad.len() {
                4 => 3,
                3 => 2,
                2 => 1,
                _ => panic!("invalid base64 length"),
            };
            for i in 0..bytes_out {
                out.push(((n >> (16 - 8 * i)) & 0xFF) as u8);
            }
        }
        out
    }

    // -- select_auth_mechanism ---------------------------------------------

    #[test]
    fn select_auth_mechanism_prefers_plain() {
        let lines: Vec<String> = vec!["AUTH PLAIN LOGIN".into()];
        assert_eq!(select_auth_mechanism(&lines), Some(AuthMechanism::Plain));
    }

    #[test]
    fn select_auth_mechanism_falls_back_to_login() {
        let lines: Vec<String> = vec!["AUTH LOGIN".into()];
        assert_eq!(select_auth_mechanism(&lines), Some(AuthMechanism::Login));
    }

    #[test]
    fn select_auth_mechanism_returns_none_when_unsupported_only() {
        let lines: Vec<String> = vec!["AUTH CRAM-MD5".into(), "PIPELINING".into()];
        assert_eq!(select_auth_mechanism(&lines), None);
    }

    #[test]
    fn select_auth_mechanism_returns_none_when_no_auth_advertised() {
        let lines: Vec<String> = vec!["PIPELINING".into(), "8BITMIME".into()];
        assert_eq!(select_auth_mechanism(&lines), None);
    }

    #[test]
    fn select_auth_mechanism_handles_empty_capabilities() {
        let lines: Vec<String> = Vec::new();
        assert_eq!(select_auth_mechanism(&lines), None);
    }

    #[test]
    fn select_auth_mechanism_handles_multiple_auth_lines() {
        // Some servers split AUTH across several capability lines.
        let lines: Vec<String> = vec!["AUTH LOGIN".into(), "AUTH PLAIN".into()];
        assert_eq!(select_auth_mechanism(&lines), Some(AuthMechanism::Plain));
    }

    // -- AuthMechanism Display / name --------------------------------------

    #[test]
    fn auth_mechanism_name_and_display() {
        assert_eq!(AuthMechanism::Plain.name(), "PLAIN");
        assert_eq!(AuthMechanism::Login.name(), "LOGIN");
        assert_eq!(format!("{}", AuthMechanism::Plain), "PLAIN");
        assert_eq!(format!("{}", AuthMechanism::Login), "LOGIN");
    }

    // -- validate_plain_* --------------------------------------------------

    #[test]
    fn validate_plain_credentials_reject_empty() {
        assert!(validate_plain_username("").is_err());
        assert!(validate_plain_password("").is_err());
        assert!(validate_plain_username("user").is_ok());
        assert!(validate_plain_password("pass").is_ok());
    }

    #[test]
    fn validate_plain_credentials_reject_nul_bytes() {
        // NUL is the SASL PLAIN field separator and must never appear
        // inside a credential.
        assert!(validate_plain_username("a\0b").is_err());
        assert!(validate_plain_password("c\0d").is_err());
    }

    #[test]
    fn validate_plain_password_accepts_utf8_and_special_chars() {
        // RFC 4616 explicitly allows UTF-8 in the password field.
        assert!(validate_plain_password("\u{00E1}\u{00F1}\u{4E2D}").is_ok());
        assert!(validate_plain_password("a b\tc").is_ok());
        assert!(validate_plain_password("p@ss w0rd!").is_ok());
    }
}

// ---------------------------------------------------------------------------
// session.rs
// ---------------------------------------------------------------------------

mod session_tests {
    use crate::session::SessionState::{
        Authentication, Closed, Data, Ehlo, Greeting, MailFrom, Quit, RcptTo,
    };

    #[test]
    fn forward_progression_is_allowed() {
        assert!(Greeting.can_transition_to(Ehlo));
        assert!(Ehlo.can_transition_to(Authentication));
        assert!(Authentication.can_transition_to(MailFrom));
        assert!(MailFrom.can_transition_to(RcptTo));
        assert!(RcptTo.can_transition_to(Data));
        assert!(Data.can_transition_to(MailFrom));
    }

    #[test]
    fn skipping_authentication_is_allowed() {
        // Unauthenticated submission goes Ehlo -> MailFrom directly.
        assert!(Ehlo.can_transition_to(MailFrom));
    }

    #[test]
    fn starting_a_second_transaction_is_allowed() {
        // After one successful transaction the state is MailFrom; it
        // must be possible to begin another transaction.
        assert!(MailFrom.can_transition_to(MailFrom));
    }

    #[test]
    fn multiple_recipients_stay_in_rcptto() {
        assert!(RcptTo.can_transition_to(RcptTo));
    }

    #[test]
    fn quit_is_allowed_from_every_active_state() {
        for from in [Greeting, Ehlo, Authentication, MailFrom, RcptTo, Data] {
            assert!(from.can_transition_to(Quit), "{from:?} should allow QUIT");
        }
    }

    #[test]
    fn closed_is_reachable_from_every_state() {
        for from in [
            Greeting,
            Ehlo,
            Authentication,
            MailFrom,
            RcptTo,
            Data,
            Quit,
            Closed,
        ] {
            assert!(from.can_transition_to(Closed), "{from:?} -> Closed");
        }
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        assert!(!Greeting.can_transition_to(Authentication));
        assert!(!Greeting.can_transition_to(MailFrom));
        assert!(!Ehlo.can_transition_to(RcptTo));
        assert!(!Ehlo.can_transition_to(Data));
        assert!(!MailFrom.can_transition_to(Data));
        assert!(!MailFrom.can_transition_to(Authentication));
        assert!(!Data.can_transition_to(RcptTo));
        // Once Closed, the only transition is to Closed itself.
        assert!(!Closed.can_transition_to(Ehlo));
        assert!(!Closed.can_transition_to(MailFrom));
    }

    #[test]
    fn closed_is_the_only_terminal_state() {
        assert!(Closed.is_terminal());
        for s in [Greeting, Ehlo, Authentication, MailFrom, RcptTo, Data, Quit] {
            assert!(!s.is_terminal(), "{s:?} should not be terminal");
        }
    }
}

// ---------------------------------------------------------------------------
// error.rs
// ---------------------------------------------------------------------------

mod error_tests {
    use crate::error::{AuthError, InvalidInputError, IoError, ProtocolError, SmtpError, SmtpOp};
    use std::error::Error;

    #[test]
    fn smtp_error_display_protocol_includes_code_and_message() {
        let e = SmtpError::Protocol(ProtocolError::UnexpectedCode {
            during: SmtpOp::MailFrom,
            expected_class: 2,
            actual: 451,
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
            (SmtpOp::AuthPlain, "AUTH PLAIN"),
            (SmtpOp::AuthLogin, "AUTH LOGIN"),
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
}

// ---------------------------------------------------------------------------
// client.rs (integration with mock transport)
// ---------------------------------------------------------------------------

mod client_tests {
    use super::harness::{MockTransport, block_on, flatten};
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
        let client =
            block_on(SmtpClient::connect(transport, "client.example.com")).expect("connect");

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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let err = block_on(client.send_mail("a@b.com\r\nRSET", &["c@d.com"], "x"))
            .expect_err("must reject");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
        let err = block_on(client.login("user", "pass")).expect_err("must fail");
        match err {
            SmtpError::Auth(AuthError::Rejected { code, message }) => {
                assert_eq!(code, 535);
                assert!(message.contains("5.7.8"));
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
        let err = block_on(client.login_with(AuthMechanism::Plain, "user", "pass"))
            .expect_err("must fail");
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
        let mut client =
            block_on(SmtpClient::connect(transport, "client.example")).expect("connect");
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
}
