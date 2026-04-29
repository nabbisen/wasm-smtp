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

/// Maximum length of an envelope address (RFC 5321 §4.5.3.1.3).
///
/// The standard's `Path` limit is 256 octets, including the angle
/// brackets that frame the address on the wire. With brackets
/// stripped, the validated address may be at most 254 octets.
pub const MAX_ADDRESS_LEN: usize = 254;

/// Maximum length of an address local-part (RFC 5321 §4.5.3.1.1).
pub const MAX_LOCAL_PART_LEN: usize = 64;

/// Maximum length of an address domain (RFC 5321 §4.5.3.1.2).
pub const MAX_DOMAIN_LEN: usize = 255;

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

/// An enhanced status code from RFC 3463, parsed out of an SMTP reply
/// when the server has advertised the `ENHANCEDSTATUSCODES` extension
/// (RFC 2034).
///
/// Enhanced codes are formatted `class.subject.detail`, for example
/// `5.7.1` (relay access denied) or `4.7.0` (security feature
/// temporarily unavailable). The basic three-digit reply code (e.g.
/// `550`) and the enhanced code share the leading digit (the
/// "class"); the remaining two fields refine the diagnosis far
/// beyond what the basic code carries.
///
/// This type is preserved across the [`Reply`] on which it is parsed,
/// and reproduced in [`crate::ProtocolError::UnexpectedCode`] when an
/// unexpected reply triggers an error. Callers can use the structured
/// fields to make routing decisions ("if subject is 5.1.* the address
/// is permanently bad; if 4.x.x retry later").
///
/// Per RFC 3463 §2:
/// - `class` is one of 2, 4, or 5 (success / persistent transient /
///   permanent).
/// - `subject` and `detail` are 0–999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnhancedStatus {
    /// Leading class digit (2, 4, or 5).
    pub class: u8,
    /// Second field: the broad subject category (e.g. `1` = address,
    /// `7` = security/policy).
    pub subject: u16,
    /// Third field: the specific detail within the subject.
    pub detail: u16,
}

impl EnhancedStatus {
    /// Format as `class.subject.detail`. This is the wire form RFC 3463
    /// uses, with the leading dot-decimal and no padding.
    #[must_use]
    pub fn to_dotted(&self) -> String {
        format!("{}.{}.{}", self.class, self.subject, self.detail)
    }
}

impl core::fmt::Display for EnhancedStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}", self.class, self.subject, self.detail)
    }
}

/// Try to parse an [`EnhancedStatus`] from the start of a reply line's
/// text portion.
///
/// The expected format is `"x.y.z"` followed by either end-of-string,
/// whitespace, or any other non-digit-non-dot byte. Invalid prefixes
/// — including missing dots, non-digit characters, or class digits
/// other than `2`, `4`, `5` — return `None`. The caller advances
/// past the parsed prefix only when this returns `Some`.
///
/// Returns `(status, prefix_len)` where `prefix_len` is the number of
/// bytes consumed from `text`, including any single trailing
/// whitespace octet. This lets [`Reply::joined_text`] strip the code
/// before showing the user-facing message.
fn parse_enhanced_status_prefix(text: &str) -> Option<(EnhancedStatus, usize)> {
    // We require at least 5 chars (`x.y.z`) and a class digit in {2,4,5}.
    let bytes = text.as_bytes();
    if bytes.len() < 5 {
        return None;
    }
    let class_byte = bytes[0];
    if !matches!(class_byte, b'2' | b'4' | b'5') || bytes[1] != b'.' {
        return None;
    }

    // subject: digits, terminated by '.'.
    let mut i = 2;
    let subj_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == subj_start || i >= bytes.len() || bytes[i] != b'.' {
        return None;
    }
    let subject: u16 = text[subj_start..i].parse().ok()?;
    i += 1;

    // detail: digits, terminated by whitespace or end of string.
    let det_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == det_start {
        return None;
    }
    let detail: u16 = text[det_start..i].parse().ok()?;

    // The terminator: end-of-string, single space, or single tab.
    // We consume one whitespace byte so the user-facing message starts
    // cleanly. Any other non-digit byte is allowed but not consumed.
    let prefix_len = if i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i + 1
    } else {
        i
    };

    Some((
        EnhancedStatus {
            class: class_byte - b'0',
            subject,
            detail,
        },
        prefix_len,
    ))
}

/// A complete SMTP reply, possibly assembled from multiple continuation
/// lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The three-digit reply code, shared by every line of the reply.
    pub code: u16,
    /// One entry per line, in the order received. Each entry is the line's
    /// text portion (after the code and separator) decoded as UTF-8 with
    /// invalid sequences replaced by `U+FFFD`. The text retains any
    /// enhanced status code prefix; use [`Self::message_text`] to obtain
    /// the same text with the prefix stripped, or [`Self::enhanced`] to
    /// obtain the parsed code itself.
    pub lines: Vec<String>,
    /// Parsed enhanced status code (RFC 3463), set only when the server
    /// has advertised `ENHANCEDSTATUSCODES` for this session. The code
    /// is taken from the first reply line; multi-line replies that
    /// disagree on the code are flagged at parse time, so this is well
    /// defined when present.
    enhanced: Option<EnhancedStatus>,
}

impl Reply {
    /// Construct a reply with the given code and lines, with no enhanced
    /// status code attached. The client adds an enhanced code via the
    /// internal `attach_enhanced_status` setter when the session has
    /// `ENHANCEDSTATUSCODES` enabled.
    #[must_use]
    pub fn new(code: u16, lines: Vec<String>) -> Self {
        Self {
            code,
            lines,
            enhanced: None,
        }
    }

    /// The leading digit of the reply code, useful for class-based checks.
    pub fn class(&self) -> u8 {
        u8::try_from(self.code / 100).unwrap_or(0)
    }

    /// Reply text concatenated with `\n`. Suitable for diagnostics.
    /// If an enhanced status code prefix is present, it is preserved in
    /// the output; use [`Self::message_text`] for a presentation that
    /// hides it.
    ///
    /// # Caveat for log handlers
    ///
    /// The returned `String` may contain `\n` (used internally to
    /// separate multi-line replies). It does **not** contain `\r` —
    /// CRLF is stripped by the reply parser before storage — but
    /// applications that forward this text to line-oriented loggers
    /// (`syslog`, journald, structured JSON, etc.) should still
    /// escape or render newlines explicitly to avoid log injection
    /// where one logical reply renders as multiple log records. The
    /// same caveat applies to anything else that consumes the
    /// `Display` output of [`crate::ProtocolError`] or
    /// [`crate::AuthError`], since those types embed reply text.
    pub fn joined_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Reply text with any enhanced status code prefix stripped from
    /// each line. Suitable for human-facing error messages where the
    /// code is shown separately. Lines that have no enhanced prefix
    /// are returned unchanged.
    pub fn message_text(&self) -> String {
        if self.enhanced.is_none() {
            return self.joined_text();
        }
        let stripped: Vec<&str> = self
            .lines
            .iter()
            .map(|line| match parse_enhanced_status_prefix(line) {
                Some((_, prefix_len)) => &line[prefix_len..],
                None => line.as_str(),
            })
            .collect();
        stripped.join("\n")
    }

    /// Parsed enhanced status code, if the server has provided one and
    /// the session has it enabled.
    #[must_use]
    pub fn enhanced(&self) -> Option<EnhancedStatus> {
        self.enhanced
    }

    /// Set the enhanced status code on this reply. Used by the client
    /// after the EHLO capability set has been confirmed to include
    /// `ENHANCEDSTATUSCODES`.
    pub(crate) fn attach_enhanced_status(&mut self, status: EnhancedStatus) {
        self.enhanced = Some(status);
    }

    /// Iterate over the trimmed text of each line. Useful for parsing EHLO
    /// capabilities, where the first line contains the greeting and the
    /// remaining lines each name a single capability (e.g. `AUTH LOGIN`,
    /// `PIPELINING`, `8BITMIME`).
    pub fn iter_lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    /// Parse an enhanced status code from the first line's text, if
    /// present. Used by the client to populate `self.enhanced` only when
    /// the session has `ENHANCEDSTATUSCODES` enabled.
    #[must_use]
    pub fn try_parse_enhanced(&self) -> Option<EnhancedStatus> {
        self.lines
            .first()
            .and_then(|line| parse_enhanced_status_prefix(line).map(|(s, _)| s))
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

/// Standard base64 decoding (RFC 4648), padded with `=`.
///
/// The symmetric counterpart of [`base64_encode`]. Used for SCRAM
/// `server-first` and `server-final` decoding.
///
/// Returns `Err` for inputs whose length is not a multiple of 4, or
/// that contain characters outside the standard base64 alphabet
/// (`A-Z`, `a-z`, `0-9`, `+`, `/`, `=`). Padding is allowed only at
/// the end.
///
/// # Errors
///
/// Returns the static string `"invalid base64"` on any decode
/// failure. The caller is expected to wrap this in a
/// domain-appropriate error type.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        return Err("invalid base64");
    }

    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for (chunk_idx, chunk) in bytes.chunks_exact(4).enumerate() {
        let is_last = chunk_idx == (bytes.len() / 4) - 1;
        let mut buf = [0u8; 4];
        let mut pad = 0usize;
        for (i, &c) in chunk.iter().enumerate() {
            buf[i] = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    pad += 1;
                    0
                }
                _ => return Err("invalid base64"),
            };
        }
        if pad > 0 && !is_last {
            return Err("invalid base64");
        }
        let n = (u32::from(buf[0]) << 18)
            | (u32::from(buf[1]) << 12)
            | (u32::from(buf[2]) << 6)
            | u32::from(buf[3]);
        out.push(((n >> 16) & 0xff) as u8);
        if pad < 2 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if pad < 1 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
}

// -----------------------------------------------------------------------------
// Input validation
// -----------------------------------------------------------------------------

/// Validate a mail address (RFC 5321 reverse-path / forward-path content).
///
/// The check is intentionally conservative: it rejects the characters that
/// would either inject SMTP commands or violate the framing of `<addr>`.
/// Validate an envelope address (used in MAIL FROM / RCPT TO) against
/// RFC 5321 grammar and the length limits in §4.5.3.1.
///
/// The check is conservative — it does not parse RFC 5321 grammar in
/// detail, but it forbids any byte that would corrupt the command
/// framing, and rejects values that exceed the standard's per-field
/// length limits.
///
/// In particular:
///
/// - non-empty;
/// - ASCII only — UTF-8 addresses require the `smtputf8` feature
///   (which exposes a separate UTF-8-permissive validator);
/// - no `\r`, `\n`, or `\0`;
/// - no `<`, `>`, or space (which would corrupt the angle-bracket framing);
/// - the whole address (local-part + `@` + domain) must be no longer
///   than 254 octets — RFC 5321 §4.5.3.1.3 specifies 256 for the
///   `Path` token including angle brackets, leaving 254 for the
///   bracket-stripped address;
/// - if an `@` is present, the local-part is no longer than 64 octets
///   and the domain is no longer than 255 octets (§4.5.3.1.1 /
///   §4.5.3.1.2). These limits are advisory: many real-world relays
///   accept longer values, but rejecting at the client boundary
///   prevents a misformed input from generating a wire `MAIL FROM`
///   line that exceeds the SMTP line-length limit (§4.5.3.1.5).
pub fn validate_address(addr: &str) -> Result<(), InvalidInputError> {
    if addr.is_empty() {
        return Err(InvalidInputError::new("mail address must not be empty"));
    }
    if !addr.is_ascii() {
        return Err(InvalidInputError::new(
            "mail address must be ASCII (SMTPUTF8 is not supported)",
        ));
    }
    if addr.len() > MAX_ADDRESS_LEN {
        return Err(InvalidInputError::new(
            "mail address exceeds RFC 5321 §4.5.3.1.3 length limit (254 octets)",
        ));
    }
    if let Some(at_pos) = addr.rfind('@') {
        let (local, domain) = addr.split_at(at_pos);
        // domain still has the leading '@' — strip it.
        let domain = &domain[1..];
        if local.len() > MAX_LOCAL_PART_LEN {
            return Err(InvalidInputError::new(
                "mail address local-part exceeds RFC 5321 §4.5.3.1.1 length limit (64 octets)",
            ));
        }
        if domain.len() > MAX_DOMAIN_LEN {
            return Err(InvalidInputError::new(
                "mail address domain exceeds RFC 5321 §4.5.3.1.2 length limit (255 octets)",
            ));
        }
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
///
/// As of v0.5.0 this is a thin alias for [`validate_plain_username`]:
/// the two SASL mechanisms (PLAIN and LOGIN) accept the same shape
/// of credential string and the same constraints apply. NUL bytes
/// are rejected because they would corrupt the SASL framing on the
/// post-base64 server side.
///
/// The function is retained for source compatibility with v0.4.x
/// callers, but new code should use [`validate_plain_username`]
/// directly. A future major release may remove this alias.
pub fn validate_login_username(user: &str) -> Result<(), InvalidInputError> {
    validate_plain_username(user)
}

/// Validate the password supplied to `AUTH LOGIN`.
///
/// As of v0.5.0 this is a thin alias for [`validate_plain_password`].
/// See [`validate_login_username`] for the rationale.
pub fn validate_login_password(pass: &str) -> Result<(), InvalidInputError> {
    validate_plain_password(pass)
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

/// Return `true` if the EHLO capability lines advertise the `STARTTLS`
/// extension (RFC 3207). The check is case-insensitive on the keyword.
///
/// `capability_lines` is the slice of lines that follows the greeting in
/// an `EHLO` reply; each line is one extension keyword optionally
/// followed by parameters.
pub fn ehlo_advertises_starttls<S: AsRef<str>>(capability_lines: &[S]) -> bool {
    for line in capability_lines {
        if let Some(head) = line.as_ref().split_ascii_whitespace().next()
            && head.eq_ignore_ascii_case("STARTTLS")
        {
            return true;
        }
    }
    false
}

/// Return `true` if the EHLO capability lines advertise the
/// `ENHANCEDSTATUSCODES` extension (RFC 2034). The check is
/// case-insensitive on the keyword.
///
/// When this is `true` for a session, the SMTP client parses the
/// `class.subject.detail` prefix off each reply and exposes it as
/// [`EnhancedStatus`] both on the [`Reply`] itself and on
/// [`crate::ProtocolError::UnexpectedCode`]. When the keyword is not
/// advertised, the same byte sequence in a reply (a stray "5.1.1"
/// for instance) is left as-is in the message text and not parsed.
pub fn ehlo_advertises_enhanced_status_codes<S: AsRef<str>>(capability_lines: &[S]) -> bool {
    for line in capability_lines {
        if let Some(head) = line.as_ref().split_ascii_whitespace().next()
            && head.eq_ignore_ascii_case("ENHANCEDSTATUSCODES")
        {
            return true;
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
    /// SASL `XOAUTH2` (Google / Microsoft OAuth 2.0 SMTP extension).
    /// Sends `user={user}\x01auth=Bearer {token}\x01\x01`
    /// base64-encoded as the initial response. The "credential" passed
    /// to `login_with` for this mechanism is an OAuth 2.0 access
    /// token, not a static password — auto-selection by `login()`
    /// deliberately does NOT pick this mechanism for that reason.
    XOAuth2,
    /// SASL `SCRAM-SHA-256` (RFC 5802 / RFC 7677). Challenge-response
    /// authentication: the client never transmits the password, and
    /// the server proves possession of the salted hash through a
    /// signature step. Auto-selection by `login()` prefers this
    /// mechanism over `PLAIN` and `LOGIN` when the server advertises
    /// it.
    ///
    /// Available only with the `scram-sha-256` cargo feature
    /// (default-on).
    ScramSha256,
}

impl AuthMechanism {
    /// SMTP-on-the-wire keyword for this mechanism, as it appears after
    /// `AUTH` in an `EHLO` advertisement (`"PLAIN"`, `"LOGIN"`,
    /// `"XOAUTH2"`, `"SCRAM-SHA-256"`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Plain => "PLAIN",
            Self::Login => "LOGIN",
            Self::XOAuth2 => "XOAUTH2",
            Self::ScramSha256 => "SCRAM-SHA-256",
        }
    }
}

impl core::fmt::Display for AuthMechanism {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

/// Pick the best mechanism advertised by the server, preferring
/// `SCRAM-SHA-256` over `PLAIN` over `LOGIN`. Returns `None` if the
/// server advertised none of these.
///
/// Use this when you want a single `login` call to do the right thing
/// across the variety of submission servers in deployment. If you need
/// to lock in a specific mechanism (for example, to reproduce a
/// production failure in a test), call [`crate::client::SmtpClient::login_with`]
/// directly.
///
/// `SCRAM-SHA-256` is the modern default: it does not transmit the
/// password in plaintext and is supported by all current submission
/// servers (Postfix + Dovecot SASL, Exchange, Stalwart). `PLAIN` is
/// the universal fallback. `LOGIN` is retained only for very old
/// servers.
///
/// Note: when the `scram-sha-256` feature is disabled, the function
/// behaves as if SCRAM were not in the picture and falls through to
/// the PLAIN/LOGIN preference.
pub fn select_auth_mechanism<S: AsRef<str>>(capability_lines: &[S]) -> Option<AuthMechanism> {
    #[cfg(feature = "scram-sha-256")]
    if ehlo_advertises_auth(capability_lines, "SCRAM-SHA-256") {
        return Some(AuthMechanism::ScramSha256);
    }

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

// -----------------------------------------------------------------------------
// XOAUTH2 (Google / Microsoft OAuth 2.0 SASL profile)
//
// The three helpers in this section are feature-gated behind
// `xoauth2` (default-on). Disabling the feature removes them
// entirely along with the corresponding `SmtpClient::login_xoauth2`
// method and the `XOAuth2` arm of `login_with`. The
// `AuthMechanism::XOAuth2` and `SmtpOp::AuthXOAuth2` enum variants
// remain present in either configuration; both enums are
// `non_exhaustive` and the variants without the feature are simply
// unreachable through the public API.
// -----------------------------------------------------------------------------

/// Build the SASL `XOAUTH2` initial response.
///
/// The wire format, before base64, is:
///
/// ```text
/// user={user}\x01auth=Bearer {token}\x01\x01
/// ```
///
/// where `\x01` is the SOH (Ctrl-A) byte that separates fields. The
/// `Bearer ` prefix is fixed and case-sensitive. Both the user and the
/// token are passed through verbatim; the caller must have validated
/// them with [`validate_xoauth2_user`] and [`validate_oauth2_token`]
/// first.
///
/// The returned string is the base64 encoding of the entire payload,
/// suitable for placement after `AUTH XOAUTH2 ` on the wire. The
/// caller is responsible for the surrounding command framing.
///
/// Available only with the `xoauth2` cargo feature enabled (default-on).
#[cfg(feature = "xoauth2")]
#[must_use]
pub fn build_xoauth2_initial_response(user: &str, token: &str) -> String {
    // Length: "user=" (5) + user + 1 (SOH) + "auth=Bearer " (12) + token
    //         + 1 (SOH) + 1 (final SOH) = 19 + user.len() + token.len()
    let mut payload = Vec::with_capacity(19 + user.len() + token.len());
    payload.extend_from_slice(b"user=");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0x01);
    payload.extend_from_slice(b"auth=Bearer ");
    payload.extend_from_slice(token.as_bytes());
    payload.push(0x01);
    payload.push(0x01);
    base64_encode(&payload)
}

/// Validate the username supplied to a SASL `XOAUTH2` exchange.
///
/// XOAUTH2 (Google / Microsoft) does not formally constrain the user
/// field, but to prevent injection of the SOH separator, NUL, CR,
/// or LF into the SASL payload, we forbid those bytes. Empty
/// usernames are also rejected.
///
/// Available only with the `xoauth2` cargo feature enabled (default-on).
#[cfg(feature = "xoauth2")]
pub fn validate_xoauth2_user(user: &str) -> Result<(), InvalidInputError> {
    if user.is_empty() {
        return Err(InvalidInputError::new("XOAUTH2 user must not be empty"));
    }
    if user.bytes().any(|b| matches!(b, 0 | b'\r' | b'\n' | 0x01)) {
        return Err(InvalidInputError::new(
            "XOAUTH2 user must not contain NUL, CR, LF, or SOH",
        ));
    }
    Ok(())
}

/// Validate an OAuth 2.0 access token before sending it on the wire.
///
/// RFC 6750 §2.1 limits a Bearer token to ASCII printable characters
/// (and a small set of punctuation), with no whitespace or control
/// characters. We enforce that subset: every byte must be in the
/// printable ASCII range `0x20..=0x7E` *except* whitespace
/// (`0x20` space and `0x09` tab are also disallowed because RFC 6750
/// requires `b64token` characters only). The SOH separator used by
/// XOAUTH2 is implicitly excluded by the printable-only rule.
///
/// This is conservative: it will reject some technically-valid token
/// shapes that real-world providers nonetheless never emit. In
/// practice both Google and Microsoft access tokens consist of
/// `[A-Za-z0-9._~+/=-]` and pass this check trivially.
///
/// Available only with the `xoauth2` cargo feature enabled (default-on).
#[cfg(feature = "xoauth2")]
pub fn validate_oauth2_token(token: &str) -> Result<(), InvalidInputError> {
    if token.is_empty() {
        return Err(InvalidInputError::new(
            "OAuth2 access token must not be empty",
        ));
    }
    for b in token.bytes() {
        // 0x21..=0x7E covers printable ASCII excluding space.
        if !(0x21..=0x7E).contains(&b) {
            return Err(InvalidInputError::new(
                "OAuth2 access token must contain only printable ASCII (no whitespace or control bytes)",
            ));
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// SMTPUTF8 (RFC 6531) — feature-gated
// -----------------------------------------------------------------------------
//
// SMTPUTF8 lets a session carry mail addresses outside the ASCII
// repertoire — e.g. `送信者@例え.jp`. The crate gates the related
// helpers behind the `smtputf8` cargo feature: callers who only ever
// submit ASCII addresses pay no code-size cost for the UTF-8 validator,
// the `MAIL FROM ... SMTPUTF8` formatter, or the capability check.
//
// When the feature is disabled, none of the items below exist; the
// default `validate_address` and `format_mail_from` continue to enforce
// ASCII, as they always have.

/// Return `true` if the EHLO capability lines advertise the `SMTPUTF8`
/// extension (RFC 6531). The check is case-insensitive on the keyword.
///
/// `capability_lines` is the slice of lines that follows the greeting in
/// an `EHLO` reply.
#[cfg(feature = "smtputf8")]
pub fn ehlo_advertises_smtputf8<S: AsRef<str>>(capability_lines: &[S]) -> bool {
    for line in capability_lines {
        if let Some(head) = line.as_ref().split_ascii_whitespace().next()
            && head.eq_ignore_ascii_case("SMTPUTF8")
        {
            return true;
        }
    }
    false
}

/// Validate an envelope address, allowing UTF-8 codepoints in addition
/// to the ASCII subset accepted by [`validate_address`].
///
/// The structural rules are the same as the ASCII validator — the
/// address must be non-empty, must not contain CR / LF / NUL, must
/// not contain `<`, `>`, ASCII whitespace, ASCII control characters
/// (C0 + DEL), or C1 control characters (U+0080-U+009F). Any other
/// Unicode codepoint is permitted; the dot-atom structure is left
/// for the server to validate.
///
/// Note that ASCII whitespace (`' '` and `'\t'`) is rejected because
/// it would corrupt the SMTP command framing, but other Unicode
/// whitespace categories such as U+3000 IDEOGRAPHIC SPACE are
/// allowed: they are valid characters in mailbox local parts in
/// some scripts and the SMTP layer never tokenizes on them.
#[cfg(feature = "smtputf8")]
pub fn validate_address_utf8(addr: &str) -> Result<(), InvalidInputError> {
    if addr.is_empty() {
        return Err(InvalidInputError::new("mail address must not be empty"));
    }
    // RFC 5321 / 6531 length limits apply on octet counts, not on
    // character counts — UTF-8 encoded length is what travels on the
    // wire and what counts toward the 254-octet path limit.
    if addr.len() > MAX_ADDRESS_LEN {
        return Err(InvalidInputError::new(
            "mail address exceeds RFC 5321 §4.5.3.1.3 length limit (254 octets)",
        ));
    }
    if let Some(at_pos) = addr.rfind('@') {
        let (local, domain) = addr.split_at(at_pos);
        let domain = &domain[1..];
        if local.len() > MAX_LOCAL_PART_LEN {
            return Err(InvalidInputError::new(
                "mail address local-part exceeds RFC 5321 §4.5.3.1.1 length limit (64 octets)",
            ));
        }
        if domain.len() > MAX_DOMAIN_LEN {
            return Err(InvalidInputError::new(
                "mail address domain exceeds RFC 5321 §4.5.3.1.2 length limit (255 octets)",
            ));
        }
    }
    for ch in addr.chars() {
        match ch {
            '\r' | '\n' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain CR or LF",
                ));
            }
            '\0' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain a NUL byte",
                ));
            }
            '<' | '>' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain ASCII < or >",
                ));
            }
            ' ' | '\t' => {
                return Err(InvalidInputError::new(
                    "mail address must not contain ASCII whitespace",
                ));
            }
            // ASCII control characters (C0 + DEL) other than the
            // CR/LF/NUL we caught above. (Tab was caught as
            // whitespace above.)
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                return Err(InvalidInputError::new(
                    "mail address must not contain ASCII control characters",
                ));
            }
            // C1 control characters (U+0080-U+009F).
            c if (0x80..=0x9F).contains(&(c as u32)) => {
                return Err(InvalidInputError::new(
                    "mail address must not contain C1 control characters",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Format `MAIL FROM:<addr> SMTPUTF8\r\n` as bytes.
///
/// The `SMTPUTF8` ESMTP parameter (RFC 6531 §3.4) signals to the
/// server that the upcoming envelope and message contain UTF-8.
/// Servers that did not advertise the extension will reject the
/// command; callers should confirm advertisement with
/// [`ehlo_advertises_smtputf8`] before invoking this helper.
///
/// Address validation is the caller's responsibility (use
/// [`validate_address_utf8`]); this helper formats unconditionally.
#[cfg(feature = "smtputf8")]
#[must_use]
pub fn format_mail_from_smtputf8(addr: &str) -> Vec<u8> {
    // "MAIL FROM:<" (11) + addr + "> SMTPUTF8\r\n" (12) = 23 + addr.len()
    let mut out = Vec::with_capacity(23 + addr.len());
    out.extend_from_slice(b"MAIL FROM:<");
    out.extend_from_slice(addr.as_bytes());
    out.extend_from_slice(b"> SMTPUTF8\r\n");
    out
}
