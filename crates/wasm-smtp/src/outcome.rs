//! [`SendOutcome`]: the result of a successful message submission.
//!
//! Returned by [`crate::SmtpClient::send_mail`] and its siblings.
//! Carries the SMTP reply code, the server's full reply text, and
//! a best-effort extraction of the server's queue identifier for
//! audit-logging and DSN-correlation use cases.

use std::fmt;

/// Outcome of a successful message submission.
///
/// Returned by:
/// - [`crate::SmtpClient::send_mail`]
/// - `SmtpClient::send_mail_smtputf8` (with the `smtputf8` feature)
/// - `SmtpClient::send_message` (with the `mail-builder` feature)
///
/// when the server accepts the message for delivery (any 2xx reply
/// to the post-`DATA` terminator). All three methods return the
/// same `SendOutcome` shape so callers can store outcomes
/// uniformly regardless of which submission method was used.
///
/// # Typical usage
///
/// ```ignore
/// let outcome = client.send_mail(from, to, body).await?;
/// audit_log!(
///     event = "auth.password.reset_email_sent",
///     queue_id = outcome.queue_id.as_deref().unwrap_or("<unknown>"),
///     server_message = outcome.server_message.as_str(),
/// );
/// ```
///
/// # Discarding the outcome
///
/// Callers who do not need the queue id or the server message can
/// drop the `SendOutcome` on the floor with `?`:
///
/// ```ignore
/// client.send_mail(from, to, body).await?;
/// ```
///
/// This is the most common pattern and works because the `?`
/// operator returns the `Ok` payload as an unused expression.
///
/// # Queue id extraction
///
/// `queue_id` is populated when the reply text matches one of the
/// patterns produced by the major submission servers:
///
/// | Server      | Reply pattern                                | `queue_id` |
/// |-------------|----------------------------------------------|-----------|
/// | Postfix     | `250 2.0.0 Ok: queued as 4ABCDE12345`        | `4ABCDE12345` |
/// | Exim        | `250 OK id=1abcDe-0001Bc-Df`                 | `1abcDe-0001Bc-Df` |
/// | Sendmail    | `250 2.0.0 a01ABCdEF123456 Message accepted` | (see note)  |
/// | Stalwart    | `250 2.0.0 Message queued with id <ID>`      | extracted   |
/// | Exchange    | (no queue id in reply)                       | `None`      |
///
/// Note for Sendmail: the queue id is the leading token after the
/// status code, which is ambiguous to parse without a sender-
/// specific rule. Our extractor does not attempt this. Callers who
/// specifically need the Sendmail queue id should parse
/// `server_message` themselves; the extracted token is the leading
/// `\\w{12,16}` group.
///
/// `queue_id` is `None` when no recognised pattern matches; this
/// is not an error condition. Some servers do not emit a queue id
/// at all (Microsoft Exchange / O365 frequently do not), and our
/// extractor is intentionally conservative — better to return
/// `None` than to fabricate a string that looks like a queue id
/// but is actually noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    /// SMTP reply code. Always in the 2xx range when a
    /// `SendOutcome` is returned (typically 250). Stored as `u16`
    /// to match the rest of the error API.
    pub code: u16,

    /// The full reply text returned by the server after the
    /// post-`DATA` terminator. Multiple reply lines are joined
    /// with single spaces. Useful for logging and for parsers
    /// that need to extract server-specific fields beyond
    /// `queue_id`.
    pub server_message: String,

    /// Server's queue identifier for this message, when
    /// extractable. `None` when the reply did not match any
    /// recognised queue-id pattern.
    pub queue_id: Option<String>,
}

impl SendOutcome {
    /// Construct a `SendOutcome` from the raw reply parts. Performs
    /// queue-id extraction from `server_message`.
    ///
    /// Used internally by the `SmtpClient::send_mail*` methods
    /// after they observe a 2xx reply to the post-`DATA` terminator.
    /// Public so callers writing custom client code on top of
    /// the lower-level protocol primitives can construct one too.
    #[must_use]
    pub fn new(code: u16, server_message: String) -> Self {
        let queue_id = extract_queue_id(&server_message);
        Self {
            code,
            server_message,
            queue_id,
        }
    }
}

impl fmt::Display for SendOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.queue_id {
            Some(qid) => write!(f, "{} (queue id {qid})", self.code),
            None => write!(f, "{}: {}", self.code, self.server_message),
        }
    }
}

/// Try to extract a queue id from an SMTP `DATA`-completion reply.
///
/// Returns the first match for any of:
///
/// - `queued as <ID>` (Postfix)
/// - `id=<ID>` (Exim)
/// - `id <ID>` (Stalwart and similar)
///
/// where `<ID>` is one or more characters from the queue-id
/// alphabet `[A-Za-z0-9-]` of length at least 6 (to suppress
/// matches on words like "us" or "the"). Trailing punctuation
/// like a period at the end of a sentence is excluded from the
/// extracted token.
///
/// Returns `None` if no pattern matches. The function is
/// conservative: it does not attempt to extract queue ids from
/// servers that do not use a recognisable prefix.
pub(crate) fn extract_queue_id(reply_text: &str) -> Option<String> {
    // Try each pattern in order. We don't pull in a regex crate;
    // a hand-rolled scan is sufficient for these three patterns
    // and avoids a dependency.
    if let Some(id) = scan_after(reply_text, "queued as ") {
        return Some(id);
    }
    if let Some(id) = scan_after(reply_text, " id=") {
        return Some(id);
    }
    // Match " id " but not at the start (avoid matching "id" inside
    // verbs like "rejected"). Require a leading space.
    if let Some(id) = scan_after(reply_text, " id ") {
        return Some(id);
    }
    None
}

/// Scan for `marker` in `text` and return the queue-id-like token
/// that immediately follows it.
///
/// The token consists of characters matching `is_queue_id_char` and
/// must be at least 6 characters long; otherwise we return `None`
/// (avoids matching `id 1` etc.). The match is case-sensitive on
/// `marker` because all the canonical patterns use lowercase.
fn scan_after(text: &str, marker: &str) -> Option<String> {
    let mut idx = 0;
    while let Some(pos) = text[idx..].find(marker) {
        let start = idx + pos + marker.len();
        let token: String = text[start..]
            .chars()
            .take_while(|&c| is_queue_id_char(c))
            .collect();
        if token.len() >= 6 {
            return Some(token);
        }
        // Skip past this marker and try the next occurrence.
        idx = start;
        if idx >= text.len() {
            break;
        }
    }
    None
}

/// Characters allowed in a queue id token. Conservative set: ASCII
/// alphanumerics plus `-` (for Exim's `1abcDe-0001Bc-Df` style).
/// Excludes `.`, `_`, whitespace, and other punctuation that would
/// terminate the id at the end of a sentence.
fn is_queue_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_queue_id ---------------------------------------------------

    #[test]
    fn extract_queue_id_postfix_format() {
        assert_eq!(
            extract_queue_id("2.0.0 Ok: queued as 4ABCDE12345"),
            Some("4ABCDE12345".to_string())
        );
    }

    #[test]
    fn extract_queue_id_postfix_at_end_of_line() {
        // No trailing punctuation; queue id runs to end of string.
        assert_eq!(
            extract_queue_id("queued as ABCDEF1234"),
            Some("ABCDEF1234".to_string())
        );
    }

    #[test]
    fn extract_queue_id_exim_format() {
        assert_eq!(
            extract_queue_id("OK id=1abcDe-0001Bc-Df"),
            Some("1abcDe-0001Bc-Df".to_string())
        );
    }

    #[test]
    fn extract_queue_id_stalwart_id_space_format() {
        assert_eq!(
            extract_queue_id("2.0.0 Message queued with id ABCDEF123456"),
            Some("ABCDEF123456".to_string())
        );
    }

    #[test]
    fn extract_queue_id_returns_none_when_no_pattern_matches() {
        assert_eq!(extract_queue_id("Ok"), None);
        assert_eq!(extract_queue_id("Message accepted for delivery"), None);
        assert_eq!(extract_queue_id(""), None);
    }

    #[test]
    fn extract_queue_id_rejects_too_short_tokens() {
        // "id " followed by a 3-char token: too short, reject.
        assert_eq!(extract_queue_id("foo id 123"), None);
        // 5 chars: still reject.
        assert_eq!(extract_queue_id("foo queued as ABCDE"), None);
    }

    #[test]
    fn extract_queue_id_with_trailing_punctuation() {
        // Period at end of message — the queue id should not include it.
        assert_eq!(
            extract_queue_id("queued as 4ABCDE12345."),
            Some("4ABCDE12345".to_string())
        );
    }

    #[test]
    fn extract_queue_id_handles_multiple_potential_matches() {
        // Take the FIRST match. "queued as" wins over a later "id=".
        assert_eq!(
            extract_queue_id("queued as FIRST123 id=SECOND456"),
            Some("FIRST123".to_string())
        );
    }

    // -- SendOutcome::new ---------------------------------------------------

    #[test]
    fn send_outcome_new_extracts_queue_id() {
        let outcome = SendOutcome::new(250, "Ok: queued as 4ABCDE12345".to_string());
        assert_eq!(outcome.code, 250);
        assert_eq!(outcome.server_message, "Ok: queued as 4ABCDE12345");
        assert_eq!(outcome.queue_id.as_deref(), Some("4ABCDE12345"));
    }

    #[test]
    fn send_outcome_new_with_no_queue_id() {
        let outcome = SendOutcome::new(250, "Message accepted for delivery".to_string());
        assert_eq!(outcome.code, 250);
        assert_eq!(outcome.queue_id, None);
    }

    // -- Display ------------------------------------------------------------

    #[test]
    fn send_outcome_display_with_queue_id() {
        let outcome = SendOutcome {
            code: 250,
            server_message: "Ok: queued as ABCDEF1234".to_string(),
            queue_id: Some("ABCDEF1234".to_string()),
        };
        assert_eq!(format!("{outcome}"), "250 (queue id ABCDEF1234)");
    }

    #[test]
    fn send_outcome_display_without_queue_id() {
        let outcome = SendOutcome {
            code: 250,
            server_message: "Message accepted".to_string(),
            queue_id: None,
        };
        assert_eq!(format!("{outcome}"), "250: Message accepted");
    }
}
