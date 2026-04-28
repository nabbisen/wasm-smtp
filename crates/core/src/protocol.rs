//! SMTP wire-format helpers.
//!
//! This module is the home for all logic that touches SMTP bytes directly:
//!
//! - [`parse_reply_line`] interprets a single CRLF-stripped reply line.
//! - [`Reply`] aggregates one or more lines into a complete reply.
//! - [`format_command`] / [`format_command_arg`] produce CRLF-terminated
//!   command bytes.
//! - [`dot_stuff_and_terminate`] produces a complete DATA payload from a
//!   user-supplied body, including the `\r\n.\r\n` terminator.
//! - [`base64_encode`] is a small, dependency-free encoder used for
//!   `AUTH LOGIN`. We do not need a decoder.
//! - The `validate_*` functions reject caller input that would inject CRLF
//!   sequences or otherwise violate SMTP grammar before any byte is sent.
//!
//! None of these helpers perform I/O; they operate on borrowed buffers and
//! return owned bytes or errors.

use crate::error::{InvalidInputError, ProtocolError};

/// Maximum length of a single reply line, excluding CRLF.
///
/// RFC 5321 §4.5.3.1.5 sets a 512-octet limit for reply lines including
/// CRLF. We accept up to 998 octets of text plus CRLF (the body line limit
/// from §4.5.3.1.6) to be lenient toward real-world server software that
/// occasionally exceeds the strict reply-line limit.
pub const MAX_REPLY_LINE_LEN: usize = 998;

/// Maximum number of lines accepted in a single multi-line reply.
///
/// SMTP does not specify a hard cap, but a reasonable defensive limit
/// prevents an unbounded server from causing unbounded allocation.
pub const MAX_REPLY_LINES: usize = 128;

// -----------------------------------------------------------------------------
// Reply parsing
// -----------------------------------------------------------------------------

/// One parsed line of an SMTP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyLine<'a> {
    /// The three-digit reply code.
    pub code: u16,
    /// `true` if this line was terminated with a space (last line of a
    /// reply); `false` if terminated with `-` (continuation).
    pub is_last: bool,
    /// The text portion after the separator. May be empty.
    pub text: &'a [u8],
}

/// Parse a single CRLF-stripped reply line.
///
/// The input must not contain the terminating CRLF.
pub fn parse_reply_line(line: &[u8]) -> Result<ReplyLine<'_>, ProtocolError> {
    if line.len() < 3 {
        return Err(malformed(line));
    }
    let d0 = ascii_digit_value(line[0]).ok_or_else(|| malformed(line))?;
    let d1 = ascii_digit_value(line[1]).ok_or_else(|| malformed(line))?;
    let d2 = ascii_digit_value(line[2]).ok_or_else(|| malformed(line))?;
    let code = u16::from(d0) * 100 + u16::from(d1) * 10 + u16::from(d2);

    if line.len() == 3 {
        // RFC 5321 requires a separator, but a code-only line with no text
        // and no separator is unambiguous: treat it as a last line.
        return Ok(ReplyLine {
            code,
            is_last: true,
            text: &[],
        });
    }
    let (is_last, text) = match line[3] {
        b' ' => (true, &line[4..]),
        b'-' => (false, &line[4..]),
        _ => return Err(malformed(line)),
    };
    Ok(ReplyLine {
        code,
        is_last,
        text,
    })
}

fn ascii_digit_value(b: u8) -> Option<u8> {
    if b.is_ascii_digit() {
        Some(b - b'0')
    } else {
        None
    }
}

fn malformed(line: &[u8]) -> ProtocolError {
    ProtocolError::Malformed(String::from_utf8_lossy(line).into_owned())
}

/// A complete SMTP reply, possibly assembled from multiple continuation
/// lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The three-digit reply code, shared by every line of the reply.
    pub code: u16,
    /// One entry per line, in the order received. Each entry is the line's
    /// text portion (after the code and separator) decoded as UTF-8 with
    /// invalid sequences replaced by `U+FFFD`.
    pub lines: Vec<String>,
}

impl Reply {
    /// The leading digit of the reply code, useful for class-based checks.
    pub fn class(&self) -> u8 {
        u8::try_from(self.code / 100).unwrap_or(0)
    }

    /// Reply text concatenated with `\n`. Suitable for diagnostics.
    pub fn joined_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Iterate over the trimmed text of each line. Useful for parsing EHLO
    /// capabilities, where the first line contains the greeting and the
    /// remaining lines each name a single capability (e.g. `AUTH LOGIN`,
    /// `PIPELINING`, `8BITMIME`).
    pub fn iter_lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }
}

// -----------------------------------------------------------------------------
// Command formatting
// -----------------------------------------------------------------------------

/// Format a command with no arguments, terminated with CRLF.
///
/// Example: `format_command("QUIT")` yields `b"QUIT\r\n"`.
pub fn format_command(verb: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(verb.len() + 2);
    buf.extend_from_slice(verb.as_bytes());
    buf.extend_from_slice(b"\r\n");
    buf
}

/// Format a command with a single argument, terminated with CRLF.
///
/// Example: `format_command_arg("EHLO", "client.example.com")` yields
/// `b"EHLO client.example.com\r\n"`.
///
/// Callers are responsible for argument validation; this function does no
/// escaping.
pub fn format_command_arg(verb: &str, arg: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(verb.len() + 1 + arg.len() + 2);
    buf.extend_from_slice(verb.as_bytes());
    buf.push(b' ');
    buf.extend_from_slice(arg.as_bytes());
    buf.extend_from_slice(b"\r\n");
    buf
}

/// Format `MAIL FROM:<addr>\r\n`. The caller must validate `addr` first.
pub fn format_mail_from(addr: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(13 + addr.len() + 2);
    buf.extend_from_slice(b"MAIL FROM:<");
    buf.extend_from_slice(addr.as_bytes());
    buf.extend_from_slice(b">\r\n");
    buf
}

/// Format `RCPT TO:<addr>\r\n`. The caller must validate `addr` first.
pub fn format_rcpt_to(addr: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(11 + addr.len() + 2);
    buf.extend_from_slice(b"RCPT TO:<");
    buf.extend_from_slice(addr.as_bytes());
    buf.extend_from_slice(b">\r\n");
    buf
}

// -----------------------------------------------------------------------------
// DATA payload
// -----------------------------------------------------------------------------

/// Produce the DATA-phase byte stream from a user-supplied body.
///
/// The output:
///
/// 1. has any line beginning with `.` doubled (RFC 5321 §4.5.2 dot-stuffing);
/// 2. is guaranteed to end with `\r\n` (a CRLF is appended if the input
///    does not already end with one);
/// 3. is followed by the end-of-data terminator `.\r\n`.
///
/// The body is expected to be CRLF-normalized. The function does not
/// translate lone LF or CR bytes; callers needing such translation should
/// preprocess the body.
///
/// The body's bytes are not inspected beyond `\r`, `\n`, and `.`, so the
/// payload may contain any 8-bit data the server is willing to accept (for
/// example, after a `250 8BITMIME` capability advertisement).
pub fn dot_stuff_and_terminate(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    let mut at_line_start = true;
    let mut prev: u8 = 0;
    for &b in body {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = prev == b'\r' && b == b'\n';
        prev = b;
    }
    if !out.ends_with(b"\r\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b".\r\n");
    out
}

// -----------------------------------------------------------------------------
// Base64
// -----------------------------------------------------------------------------

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 encoding (RFC 4648), padded with `=`.
///
/// Used for `AUTH LOGIN`. We deliberately avoid pulling in an external
/// base64 dependency; the implementation is small and easy to audit.
pub fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks_exact(3);
    let rem = chunks.remainder();
    for chunk in chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        push_b64(&mut out, n, 4);
    }
    match rem.len() {
        0 => {}
        1 => {
            let n = u32::from(rem[0]) << 16;
            push_b64(&mut out, n, 2);
            out.push_str("==");
        }
        2 => {
            let n = (u32::from(rem[0]) << 16) | (u32::from(rem[1]) << 8);
            push_b64(&mut out, n, 3);
            out.push('=');
        }
        _ => unreachable!(),
    }
    out
}

fn push_b64(out: &mut String, n: u32, count: u8) {
    // count is the number of significant base64 characters to emit (2..=4)
    // shifts: index 0 -> 18, 1 -> 12, 2 -> 6, 3 -> 0
    for i in 0..count {
        let shift = 18 - 6 * i;
        let idx = ((n >> shift) & 0x3F) as usize;
        out.push(char::from(BASE64_ALPHABET[idx]));
    }
}

// -----------------------------------------------------------------------------
// Input validation
// -----------------------------------------------------------------------------

/// Validate a mail address (RFC 5321 reverse-path / forward-path content).
///
/// The check is intentionally conservative: it rejects the characters that
/// would either inject SMTP commands or violate the framing of `<addr>`.
/// It does not attempt full RFC 5321 grammar validation.
///
/// In particular:
/// - non-empty;
/// - ASCII only (SMTPUTF8 is not implemented);
/// - no `\r`, `\n`, or `\0`;
/// - no `<`, `>`, or space (which would corrupt the angle-bracket framing).
pub fn validate_address(addr: &str) -> Result<(), InvalidInputError> {
    if addr.is_empty() {
        return Err(InvalidInputError::new("mail address must not be empty"));
    }
    if !addr.is_ascii() {
        return Err(InvalidInputError::new(
            "mail address must be ASCII (SMTPUTF8 is not supported)",
        ));
    }
    for b in addr.bytes() {
        match b {
            b'\r' | b'\n' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain CR or LF",
                ));
            }
            0 => {
                return Err(InvalidInputError::new(
                    "mail address must not contain a NUL byte",
                ));
            }
            b'<' | b'>' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain '<' or '>'",
                ));
            }
            b' ' | b'\t' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain whitespace",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate the domain argument supplied to `EHLO`.
///
/// Accepts any non-empty sequence of printable ASCII (0x21..=0x7E). Address
/// literals (e.g. `[192.0.2.1]`) and dotted FQDNs both pass. The check is
/// intentionally lenient: its job is to prevent CRLF injection, not to
/// enforce DNS syntax.
pub fn validate_ehlo_domain(domain: &str) -> Result<(), InvalidInputError> {
    if domain.is_empty() {
        return Err(InvalidInputError::new("EHLO domain must not be empty"));
    }
    if !domain.is_ascii() {
        return Err(InvalidInputError::new("EHLO domain must be ASCII"));
    }
    if domain.bytes().any(|b| !(0x21..=0x7E).contains(&b)) {
        return Err(InvalidInputError::new(
            "EHLO domain must contain only printable ASCII characters",
        ));
    }
    Ok(())
}

/// Validate the username supplied to `AUTH LOGIN`.
pub fn validate_login_username(user: &str) -> Result<(), InvalidInputError> {
    if user.is_empty() {
        return Err(InvalidInputError::new("AUTH username must not be empty"));
    }
    Ok(())
}

/// Validate the password supplied to `AUTH LOGIN`.
pub fn validate_login_password(pass: &str) -> Result<(), InvalidInputError> {
    if pass.is_empty() {
        return Err(InvalidInputError::new("AUTH password must not be empty"));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// EHLO capability inspection
// -----------------------------------------------------------------------------

/// Return `true` if the EHLO capability lines advertise an `AUTH` mechanism
/// named `mechanism`. The check is case-insensitive on both the keyword
/// and the mechanism name.
///
/// `capability_lines` is the slice of lines that follows the greeting in
/// an `EHLO` reply: each line is one extension (e.g. `"AUTH LOGIN PLAIN"`,
/// `"PIPELINING"`, `"8BITMIME"`).
pub fn ehlo_advertises_auth<S: AsRef<str>>(capability_lines: &[S], mechanism: &str) -> bool {
    for line in capability_lines {
        let mut parts = line.as_ref().split_ascii_whitespace();
        let Some(head) = parts.next() else { continue };
        if !head.eq_ignore_ascii_case("AUTH") {
            continue;
        }
        for mech in parts {
            if mech.eq_ignore_ascii_case(mechanism) {
                return true;
            }
        }
    }
    false
}

// -----------------------------------------------------------------------------
// Authentication mechanisms
// -----------------------------------------------------------------------------

/// SASL authentication mechanisms supported by this client.
///
/// Today the crate implements `PLAIN` (RFC 4616) and `LOGIN` (the
/// historical mechanism used by many submission servers). The enum is
/// `non_exhaustive` so that future additions (e.g. `XOAUTH2`,
/// `SCRAM-SHA-256`) do not require a major version bump.
///
/// `PLAIN` is preferred when both are advertised: it is one network
/// round-trip rather than two, and is an IETF-standard SASL mechanism.
/// `LOGIN` is retained for compatibility with older submission servers
/// that advertise only it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthMechanism {
    /// SASL `PLAIN` (RFC 4616). Sends `\0user\0pass` base64-encoded as
    /// the initial response, completing in a single round-trip.
    Plain,
    /// `LOGIN`. Sends username and password as separate base64 lines
    /// in response to two `334` server prompts.
    Login,
}

impl AuthMechanism {
    /// SMTP-on-the-wire keyword for this mechanism, as it appears after
    /// `AUTH` in an `EHLO` advertisement (`"PLAIN"`, `"LOGIN"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "PLAIN",
            Self::Login => "LOGIN",
        }
    }
}

impl core::fmt::Display for AuthMechanism {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Pick the best mechanism advertised by the server, preferring `PLAIN`
/// over `LOGIN`. Returns `None` if the server advertised neither.
///
/// Use this when you want a single `login` call to do the right thing
/// across the variety of submission servers in deployment. If you need
/// to lock in a specific mechanism (for example, to reproduce a
/// production failure in a test), call [`crate::client::SmtpClient::login_with`]
/// directly.
pub fn select_auth_mechanism<S: AsRef<str>>(capability_lines: &[S]) -> Option<AuthMechanism> {
    if ehlo_advertises_auth(capability_lines, "PLAIN") {
        Some(AuthMechanism::Plain)
    } else if ehlo_advertises_auth(capability_lines, "LOGIN") {
        Some(AuthMechanism::Login)
    } else {
        None
    }
}

/// Build the SASL `PLAIN` initial response for the given credentials.
///
/// The result is the base64 encoding of `\0user\0pass` (RFC 4616 §2).
/// The empty authorization identity (the part before the first NUL)
/// means "act as the authenticated user", which is the correct default
/// for SMTP submission.
///
/// The caller is responsible for the surrounding command framing; the
/// full on-wire bytes are `b"AUTH PLAIN " + result + b"\r\n"`.
///
/// # Encoding
///
/// `user` and `pass` are encoded as their UTF-8 bytes. RFC 4616 mandates
/// UTF-8 for both fields; this matches Rust's `String` representation.
#[must_use]
pub fn build_auth_plain_initial_response(user: &str, pass: &str) -> String {
    let mut payload = Vec::with_capacity(2 + user.len() + pass.len());
    payload.push(0u8); // empty authzid
    payload.extend_from_slice(user.as_bytes());
    payload.push(0u8);
    payload.extend_from_slice(pass.as_bytes());
    base64_encode(&payload)
}

/// Validate the username supplied to a SASL `PLAIN` `AUTH` exchange.
///
/// RFC 4616 forbids NUL bytes in the authcid (NUL is the field
/// separator). Empty usernames are also refused: while RFC 4616 itself
/// allows them, no SMTP submission server accepts an empty login, and
/// rejecting them up-front turns a server-side failure into a
/// programmer-visible one.
pub fn validate_plain_username(user: &str) -> Result<(), InvalidInputError> {
    if user.is_empty() {
        return Err(InvalidInputError::new("AUTH username must not be empty"));
    }
    if user.bytes().any(|b| b == 0) {
        return Err(InvalidInputError::new(
            "AUTH username must not contain a NUL byte",
        ));
    }
    Ok(())
}

/// Validate the password supplied to a SASL `PLAIN` `AUTH` exchange.
///
/// As with [`validate_plain_username`], NUL bytes are forbidden because
/// they would corrupt the SASL framing.
pub fn validate_plain_password(pass: &str) -> Result<(), InvalidInputError> {
    if pass.is_empty() {
        return Err(InvalidInputError::new("AUTH password must not be empty"));
    }
    if pass.bytes().any(|b| b == 0) {
        return Err(InvalidInputError::new(
            "AUTH password must not contain a NUL byte",
        ));
    }
    Ok(())
}
