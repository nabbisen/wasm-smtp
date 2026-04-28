//! Error types returned by SMTP operations.
//!
//! All operations in this crate ultimately return [`SmtpError`], a four-arm
//! enum that classifies failures at a coarse granularity: transport (`Io`),
//! wire-format / unexpected response (`Protocol`), authentication (`Auth`),
//! and caller-supplied input that violates SMTP grammar (`InvalidInput`).
//!
//! ## Sensitivity
//!
//! Error messages must never include credentials or message body content.
//! Constructors in this module are designed so that callers cannot
//! accidentally embed such material:
//!
//! - [`InvalidInputError`] takes a static reason string only.
//! - [`AuthError::Rejected`] carries the server's reply text (which the server
//!   itself produced) but no client-side credentials.
//! - [`ProtocolError::UnexpectedCode`] carries server-produced text.
//! - The DATA-phase code in [`crate::client`] never includes body bytes in any
//!   error.

use core::fmt;
use std::error::Error as StdError;

/// Top-level error type for all SMTP operations.
#[derive(Debug)]
pub enum SmtpError {
    /// Underlying transport (socket) failure, including connection close.
    Io(IoError),
    /// Server response did not match SMTP grammar or expected code.
    Protocol(ProtocolError),
    /// Authentication exchange failed or no compatible mechanism was offered.
    Auth(AuthError),
    /// Caller-supplied input violated SMTP constraints before any byte was
    /// sent on the wire.
    InvalidInput(InvalidInputError),
}

impl fmt::Display for SmtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "smtp transport error: {e}"),
            Self::Protocol(e) => write!(f, "smtp protocol error: {e}"),
            Self::Auth(e) => write!(f, "smtp auth error: {e}"),
            Self::InvalidInput(e) => write!(f, "smtp invalid input: {e}"),
        }
    }
}

impl StdError for SmtpError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Protocol(e) => Some(e),
            Self::Auth(e) => Some(e),
            Self::InvalidInput(e) => Some(e),
        }
    }
}

impl From<IoError> for SmtpError {
    fn from(value: IoError) -> Self {
        Self::Io(value)
    }
}

impl From<ProtocolError> for SmtpError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

impl From<AuthError> for SmtpError {
    fn from(value: AuthError) -> Self {
        Self::Auth(value)
    }
}

impl From<InvalidInputError> for SmtpError {
    fn from(value: InvalidInputError) -> Self {
        Self::InvalidInput(value)
    }
}

// -----------------------------------------------------------------------------
// IoError
// -----------------------------------------------------------------------------

/// A failure that originated below SMTP, in the transport (TCP, TLS, the
/// runtime's socket API).
///
/// Adapter crates (e.g. `wasm-smtp-cloudflare`) convert their runtime-specific
/// errors into this type at the transport boundary. The conversion is lossy by
/// design: it preserves a human-readable message but discards the original
/// type, which keeps adapter-specific types out of the core public API.
#[derive(Debug)]
pub struct IoError {
    message: String,
}

impl IoError {
    /// Construct from any `Display`-able message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The human-readable description of the failure.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for IoError {}

// -----------------------------------------------------------------------------
// ProtocolError
// -----------------------------------------------------------------------------

/// The SMTP operation that was in progress when an error was observed.
///
/// This is the granularity an operator looks for in a log message:
/// "MAIL FROM was rejected" is more useful than "the server returned
/// 550". Each variant corresponds to one user-visible step of the SMTP
/// state machine.
///
/// The enum is `non_exhaustive` so that future SMTP extensions (e.g.
/// `AUTH XOAUTH2`) can add a variant without forcing a major version
/// bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SmtpOp {
    /// Reading the server's initial greeting (a `2xx` line, typically
    /// `220`).
    Greeting,
    /// `EHLO` and the capability negotiation that follows.
    Ehlo,
    /// `STARTTLS` command and the `220` reply that precedes the TLS
    /// handshake (RFC 3207).
    StartTls,
    /// `AUTH PLAIN` (RFC 4616) initial-response exchange.
    AuthPlain,
    /// `AUTH LOGIN` exchange (any of its three round-trips).
    AuthLogin,
    /// `MAIL FROM:<...>` envelope-sender announcement.
    MailFrom,
    /// `RCPT TO:<...>` recipient announcement (any of several when the
    /// message has multiple recipients).
    RcptTo,
    /// The `DATA` command and the body that follows it.
    Data,
    /// `QUIT` shutdown handshake.
    Quit,
}

impl SmtpOp {
    /// A short, on-the-wire-style label for this operation. The string
    /// matches the SMTP command keyword whenever there is one
    /// (`"MAIL FROM"`, `"DATA"`, `"AUTH PLAIN"`); for the greeting
    /// (which is server-initiated) the label is `"greeting"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Greeting => "greeting",
            Self::Ehlo => "EHLO",
            Self::StartTls => "STARTTLS",
            Self::AuthPlain => "AUTH PLAIN",
            Self::AuthLogin => "AUTH LOGIN",
            Self::MailFrom => "MAIL FROM",
            Self::RcptTo => "RCPT TO",
            Self::Data => "DATA",
            Self::Quit => "QUIT",
        }
    }
}

impl fmt::Display for SmtpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A wire-format failure or an unexpected response from the server.
///
/// This enum is `non_exhaustive` so that future SMTP extensions can add
/// new failure modes without forcing a major version bump.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProtocolError {
    /// The server returned a reply whose code class did not match what the
    /// state machine required at this point.
    ///
    /// `during` records the SMTP operation that was in progress. This
    /// lets a caller surface "MAIL FROM rejected (550)" rather than the
    /// less actionable "550".
    ///
    /// `expected_class` is one of `2`, `3`, etc., representing the leading
    /// digit. `actual` is the full three-digit code as observed.
    UnexpectedCode {
        /// The SMTP operation that was in progress.
        during: SmtpOp,
        /// The leading reply-code digit the state machine required.
        expected_class: u8,
        /// The full three-digit reply code actually returned.
        actual: u16,
        /// The server-supplied reply text (joined across multi-line replies
        /// with `\n`).
        message: String,
    },
    /// A reply line did not parse: wrong length, non-digit code, illegal
    /// continuation marker, or non-UTF-8 in a position where text was
    /// expected.
    Malformed(String),
    /// The server closed the connection while the state machine was waiting
    /// for more data.
    UnexpectedClose,
    /// A reply line exceeded the SMTP line-length limit (RFC 5321 §4.5.3.1.5,
    /// 1000 octets including CRLF).
    LineTooLong,
    /// A multi-line reply contained inconsistent reply codes across lines.
    /// RFC 5321 requires every line of a multi-line reply to share the same
    /// three-digit code.
    InconsistentMultiline {
        /// The code on the first line.
        first: u16,
        /// The differing code observed on a later line.
        later: u16,
    },
    /// The client requested an SMTP extension that the server did not
    /// advertise in its `EHLO` response.
    ///
    /// Today this is raised only when the caller asks for `STARTTLS` but
    /// the server's `EHLO` capability list does not include it.
    ExtensionUnavailable {
        /// The extension keyword as it would appear in `EHLO`
        /// (e.g. `"STARTTLS"`).
        name: &'static str,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedCode {
                during,
                expected_class,
                actual,
                message,
            } => write!(
                f,
                "during {during}, expected {expected_class}xx response but received {actual}: {message}",
            ),
            Self::Malformed(s) => write!(f, "malformed server reply: {s}"),
            Self::UnexpectedClose => f.write_str("server closed connection unexpectedly"),
            Self::LineTooLong => f.write_str("server reply line exceeded SMTP line-length limit"),
            Self::InconsistentMultiline { first, later } => {
                write!(f, "multi-line reply mixed codes {first} and {later}",)
            }
            Self::ExtensionUnavailable { name } => {
                write!(f, "server did not advertise the {name} extension")
            }
        }
    }
}

impl StdError for ProtocolError {}

// -----------------------------------------------------------------------------
// AuthError
// -----------------------------------------------------------------------------

/// An authentication-specific failure.
#[derive(Debug)]
pub enum AuthError {
    /// The server rejected the credentials. The reply code (typically 535) and
    /// server message are preserved; client credentials are not.
    Rejected {
        /// SMTP reply code returned by the server.
        code: u16,
        /// Server-supplied reply text.
        message: String,
    },
    /// The server's EHLO response did not advertise an `AUTH` mechanism that
    /// this client supports. The current implementation supports
    /// `AUTH PLAIN` (RFC 4616) and `AUTH LOGIN`.
    UnsupportedMechanism,
    /// The server returned a 334 prompt that did not look like a valid
    /// base64 challenge.
    MalformedChallenge(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected { code, message } => {
                write!(f, "server rejected authentication ({code}): {message}")
            }
            Self::UnsupportedMechanism => f.write_str(
                "server did not advertise an AUTH mechanism supported by this client \
                 (expected PLAIN or LOGIN)",
            ),
            Self::MalformedChallenge(s) => {
                write!(f, "server sent a malformed AUTH challenge: {s}")
            }
        }
    }
}

impl StdError for AuthError {}

// -----------------------------------------------------------------------------
// InvalidInputError
// -----------------------------------------------------------------------------

/// Caller-supplied input did not satisfy SMTP grammar.
///
/// The error carries a static reason string and never echoes the offending
/// input, which would risk leaking message content or credentials into logs.
#[derive(Debug)]
pub struct InvalidInputError {
    reason: &'static str,
}

impl InvalidInputError {
    /// Construct from a static reason. Reasons are static to make it
    /// statically impossible to embed runtime-supplied user input.
    pub const fn new(reason: &'static str) -> Self {
        Self { reason }
    }

    /// The reason string.
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

impl fmt::Display for InvalidInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason)
    }
}

impl StdError for InvalidInputError {}
