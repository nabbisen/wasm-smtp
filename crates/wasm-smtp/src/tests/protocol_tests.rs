//! Tests for `protocol.rs`: reply parsing, command formatting,
//! dot-stuffing, base64, input validation, capability inspection.

use crate::error::ProtocolError;
use crate::protocol::{
    AuthMechanism, Reply, base64_encode, build_auth_plain_initial_response,
    dot_stuff_and_terminate, ehlo_advertises_auth, ehlo_advertises_enhanced_status_codes,
    ehlo_advertises_starttls, format_command, format_command_arg, format_mail_from, format_rcpt_to,
    parse_reply_line, select_auth_mechanism, validate_address, validate_ehlo_domain,
    validate_login_password, validate_login_username, validate_plain_password,
    validate_plain_username,
};

// XOAUTH2 helpers are only present when the feature is enabled.
#[cfg(feature = "xoauth2")]
use crate::protocol::{
    build_xoauth2_initial_response, validate_oauth2_token, validate_xoauth2_user,
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
    let r = Reply::new(451, vec!["temporary".into(), "failure".into()]);
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

/// Phase 9 / M-5: NUL bytes in LOGIN credentials would corrupt
/// SASL framing on the post-base64 server side, so the
/// validators must reject them. Before v0.5.0 these were thin
/// "non-empty only" checks; they are now thin aliases over the
/// stricter `validate_plain_*` validators.
#[test]
fn validate_login_username_rejects_nul() {
    assert!(validate_login_username("a\0b").is_err());
}

#[test]
fn validate_login_password_rejects_nul() {
    assert!(validate_login_password("a\0b").is_err());
}

// -- validate_address: RFC 5321 length limits (M-4) -------------------

#[test]
fn validate_address_rejects_overly_long_total() {
    // Construct an address that is exactly 1 octet over the
    // 254-octet path limit (RFC 5321 §4.5.3.1.3). Use a 60-octet
    // local-part + '@' + 194-octet domain = 255 octets total.
    let local = "a".repeat(60);
    let domain = format!("{}.example", "x".repeat(186)); // 186 + ".example" (8) = 194
    let addr = format!("{local}@{domain}");
    assert_eq!(addr.len(), 255);
    assert!(validate_address(&addr).is_err());
}

#[test]
fn validate_address_accepts_at_total_limit() {
    // Boundary: exactly 254 octets is allowed.
    let local = "a".repeat(60);
    let domain = format!("{}.example", "x".repeat(185)); // 185 + 8 = 193
    let addr = format!("{local}@{domain}");
    assert_eq!(addr.len(), 254);
    assert!(validate_address(&addr).is_ok());
}

#[test]
fn validate_address_rejects_overly_long_local_part() {
    // 65-octet local-part > MAX_LOCAL_PART_LEN (64).
    let addr = format!("{}@example.com", "a".repeat(65));
    assert!(validate_address(&addr).is_err());
}

#[test]
fn validate_address_accepts_at_local_part_limit() {
    // 64-octet local-part is allowed.
    let addr = format!("{}@example.com", "a".repeat(64));
    assert!(validate_address(&addr).is_ok());
}

#[test]
fn validate_address_rejects_overly_long_domain() {
    // 256-octet domain > MAX_DOMAIN_LEN (255).
    let addr = format!("user@{}", "x".repeat(256));
    assert!(validate_address(&addr).is_err());
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

// -- ehlo_advertises_starttls -----------------------------------------

#[test]
fn ehlo_advertises_starttls_finds_listed_extension() {
    let lines: Vec<String> = vec!["PIPELINING".into(), "STARTTLS".into(), "8BITMIME".into()];
    assert!(ehlo_advertises_starttls(&lines));
}

#[test]
fn ehlo_advertises_starttls_is_case_insensitive() {
    let lines: Vec<String> = vec!["starttls".into()];
    assert!(ehlo_advertises_starttls(&lines));
}

#[test]
fn ehlo_advertises_starttls_returns_false_when_absent() {
    let lines: Vec<String> = vec!["PIPELINING".into(), "AUTH PLAIN".into()];
    assert!(!ehlo_advertises_starttls(&lines));
}

#[test]
fn ehlo_advertises_starttls_handles_empty_caps() {
    let lines: Vec<String> = Vec::new();
    assert!(!ehlo_advertises_starttls(&lines));
}

#[test]
fn ehlo_advertises_starttls_does_not_match_substrings() {
    // `STARTTLS-FOO` (hypothetical) shouldn't match `STARTTLS` exactly.
    let lines: Vec<String> = vec!["STARTTLSPLUS".into()];
    assert!(!ehlo_advertises_starttls(&lines));
}

// -- ehlo_advertises_enhanced_status_codes ----------------------------

#[test]
fn ehlo_advertises_enhancedstatuscodes_finds_listed_extension() {
    let lines: Vec<String> = vec![
        "PIPELINING".into(),
        "ENHANCEDSTATUSCODES".into(),
        "8BITMIME".into(),
    ];
    assert!(ehlo_advertises_enhanced_status_codes(&lines));
}

#[test]
fn ehlo_advertises_enhancedstatuscodes_is_case_insensitive() {
    let lines: Vec<String> = vec!["enhancedstatuscodes".into()];
    assert!(ehlo_advertises_enhanced_status_codes(&lines));
}

#[test]
fn ehlo_advertises_enhancedstatuscodes_returns_false_when_absent() {
    let lines: Vec<String> = vec!["PIPELINING".into(), "AUTH PLAIN".into()];
    assert!(!ehlo_advertises_enhanced_status_codes(&lines));
}

#[test]
fn ehlo_advertises_enhancedstatuscodes_does_not_match_substrings() {
    // The keyword check splits on whitespace and compares exactly.
    let lines: Vec<String> = vec!["ENHANCEDSTATUSCODESPLUS".into()];
    assert!(!ehlo_advertises_enhanced_status_codes(&lines));
}

// -- EnhancedStatus parsing -------------------------------------------
//
// We cannot test `parse_enhanced_status_prefix` directly because it is
// private to protocol.rs. Instead we test it through `Reply::try_parse_enhanced`,
// which is the only caller and the API a downstream consumer would use.

#[test]
fn reply_parses_enhanced_status_basic() {
    let reply = Reply::new(550, vec!["5.7.1 relay denied".into()]);
    let es = reply.try_parse_enhanced().expect("should parse");
    assert_eq!(es.class, 5);
    assert_eq!(es.subject, 7);
    assert_eq!(es.detail, 1);
    assert_eq!(es.to_dotted(), "5.7.1");
    assert_eq!(format!("{es}"), "5.7.1");
}

#[test]
fn reply_parses_enhanced_status_class_2_and_4() {
    // RFC 3463 specifies class 2 (success), 4 (transient), 5 (permanent).
    for (class_byte, want) in [(b'2', 2), (b'4', 4), (b'5', 5)] {
        let line = format!("{}.0.0 ok", class_byte as char);
        let reply = Reply::new(250, vec![line]);
        let es = reply.try_parse_enhanced().expect("should parse");
        assert_eq!(es.class, want);
    }
}

#[test]
fn reply_rejects_invalid_enhanced_class_digits() {
    // Class 1, 3, 6, etc. must not be parsed: RFC 3463 only defines 2/4/5.
    for bad in [b'0', b'1', b'3', b'6', b'9'] {
        let line = format!("{}.0.0 something", bad as char);
        let reply = Reply::new(250, vec![line]);
        assert!(
            reply.try_parse_enhanced().is_none(),
            "class {} must not parse",
            bad as char
        );
    }
}

#[test]
fn reply_rejects_malformed_enhanced_status() {
    for bad in [
        "5..1 missing subject",
        "5.7. missing detail",
        "5-7-1 wrong separator",
        "5.7 too short",
        "noenhanced text only",
        "",
    ] {
        let reply = Reply::new(550, vec![bad.into()]);
        assert!(
            reply.try_parse_enhanced().is_none(),
            "{bad:?} must not parse"
        );
    }
}

#[test]
fn reply_message_text_strips_enhanced_prefix_when_present() {
    // The full text is preserved by joined_text(), but message_text()
    // strips the enhanced prefix for human-friendly display.
    let mut reply = Reply::new(550, vec!["5.7.1 relay access denied".into()]);
    // message_text() relies on the enhanced field being set, mimicking
    // what the client does when ENHANCEDSTATUSCODES is enabled.
    let es = reply.try_parse_enhanced().unwrap();
    reply.attach_enhanced_status(es);
    assert_eq!(reply.joined_text(), "5.7.1 relay access denied");
    assert_eq!(reply.message_text(), "relay access denied");
}

#[test]
fn reply_message_text_unchanged_without_enhanced() {
    // Without an enhanced code attached, message_text() == joined_text().
    let reply = Reply::new(550, vec!["something or other".into()]);
    assert_eq!(reply.message_text(), reply.joined_text());
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
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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

// -- XOAUTH2 ----------------------------------------------------------
//
// These tests cover helpers that are only present when the
// `xoauth2` feature is enabled. The `select_auth_mechanism` test
// is included here because it asserts a property about the
// _absence_ of XOAUTH2 from auto-selection, which is meaningful
// only when XOAUTH2 itself is available.
//
// The single test that does NOT belong here is
// `auth_mechanism_xoauth2_name_is_exact_keyword`, which tests the
// always-present `AuthMechanism::XOAuth2` variant's `name()`
// accessor. That one is unconditional below.

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_initial_response_canonical_example() {
    // Canonical Google example. Payload (with SOH = \x01):
    //   user=someuser@example.com\x01auth=Bearer ya29.vF9...\x01\x01
    // We just verify the wire bytes round-trip through base64.
    let response = build_xoauth2_initial_response("someuser@example.com", "ya29.test_token");

    // Decode to inspect the structure. We don't have a public base64
    // decoder, so we reconstruct the expected bytes and check that a
    // fresh encode yields the same string.
    let mut expected_payload = Vec::new();
    expected_payload.extend_from_slice(b"user=someuser@example.com");
    expected_payload.push(0x01);
    expected_payload.extend_from_slice(b"auth=Bearer ya29.test_token");
    expected_payload.push(0x01);
    expected_payload.push(0x01);
    let expected_b64 = base64_encode(&expected_payload);

    assert_eq!(response, expected_b64);
}

#[cfg(feature = "xoauth2")]
#[test]
fn xoauth2_initial_response_uses_soh_separators() {
    // The wire-format bytes (pre-base64) must contain exactly two
    // SOH bytes between fields and one trailing SOH-SOH. We reconstruct
    // and compare.
    let r1 = build_xoauth2_initial_response("u", "t");
    let mut payload = Vec::new();
    payload.extend_from_slice(b"user=u\x01auth=Bearer t\x01\x01");
    assert_eq!(r1, base64_encode(&payload));
}

#[cfg(feature = "xoauth2")]
#[test]
fn validate_xoauth2_user_rejects_empty_and_control_bytes() {
    assert!(validate_xoauth2_user("").is_err());
    assert!(validate_xoauth2_user("u\0v").is_err());
    assert!(validate_xoauth2_user("u\rv").is_err());
    assert!(validate_xoauth2_user("u\nv").is_err());
    // SOH would corrupt the SASL frame.
    assert!(validate_xoauth2_user("u\x01v").is_err());
}

#[cfg(feature = "xoauth2")]
#[test]
fn validate_xoauth2_user_accepts_typical_email_addresses() {
    assert!(validate_xoauth2_user("user@example.com").is_ok());
    assert!(validate_xoauth2_user("first.last+tag@example.co.uk").is_ok());
}

#[cfg(feature = "xoauth2")]
#[test]
fn validate_oauth2_token_rejects_empty_and_whitespace() {
    assert!(validate_oauth2_token("").is_err());
    assert!(validate_oauth2_token("token with space").is_err());
    assert!(validate_oauth2_token("token\twith\ttab").is_err());
    assert!(validate_oauth2_token("token\nwith\nnewline").is_err());
}

#[cfg(feature = "xoauth2")]
#[test]
fn validate_oauth2_token_rejects_non_ascii() {
    assert!(validate_oauth2_token("\u{00FF}token").is_err());
    assert!(validate_oauth2_token("token\u{4E2D}").is_err());
}

#[cfg(feature = "xoauth2")]
#[test]
fn validate_oauth2_token_accepts_typical_bearer_tokens() {
    // Realistic Google token shape.
    assert!(validate_oauth2_token("ya29.A0AfH6SMBx-LAUH4xRcZbqK_pE7Hk0_lOxe2eGdt9CD8s8I").is_ok());
    // Realistic Microsoft token shape (JWT).
    assert!(
        validate_oauth2_token("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.eyJzdWIifQ.signature_part")
            .is_ok()
    );
    // Punctuation characters allowed by RFC 6750.
    assert!(validate_oauth2_token("a-b_c.d+e/f=g~h").is_ok());
}

// Note: the next test asserts the absence of XOAUTH2 from
// auto-selection, which is meaningful only when XOAUTH2 itself
// is compiled in. Without the feature, `AuthMechanism::XOAuth2`
// can't be auto-picked because it can't be picked at all.
#[cfg(feature = "xoauth2")]
#[test]
fn select_auth_mechanism_does_not_pick_xoauth2() {
    // Even when XOAUTH2 is the only advertised mechanism,
    // `select_auth_mechanism` returns None — XOAUTH2 requires a
    // bearer token rather than a static password and must be
    // opted-in explicitly via `login_with` or `login_xoauth2`.
    let lines: Vec<String> = vec!["AUTH XOAUTH2".into()];
    assert!(select_auth_mechanism(&lines).is_none());
}

// The AuthMechanism::XOAuth2 enum variant is present in either
// feature configuration (the enum is non_exhaustive); only its
// associated I/O code paths and helpers are gated.
#[test]
fn auth_mechanism_xoauth2_name_is_exact_keyword() {
    assert_eq!(AuthMechanism::XOAuth2.name(), "XOAUTH2");
    assert_eq!(format!("{}", AuthMechanism::XOAuth2), "XOAUTH2");
}
