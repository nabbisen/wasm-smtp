//! SMTP session state machine.
//!
//! [`SessionState`] enumerates the well-defined points in an SMTP exchange.
//! [`crate::client::SmtpClient`] tracks the current state and uses
//! [`SessionState::can_transition_to`] to reject API misuse before any byte
//! is sent on the wire. This converts ordering bugs in caller code into
//! [`crate::error::InvalidInputError`] returns instead of confusing server
//! responses.
//!
//! ## State diagram
//!
//! ```text
//!   Greeting --> Ehlo --> Authentication --> MailFrom --> RcptTo --> Data
//!                  ^         |   \                ^             |        |
//!                  |         |    \               |             |        v
//!         (re-EHLO |         |     \--------------|             |       Quit
//!          after   |         |        (skip auth) |             |        |
//!          TLS)    v         v                    |             v        v
//!               StartTls<----+                    |         MailFrom   Closed
//!                                                 |         (next msg)
//!                                              (loop for more recipients)
//! ```
//!
//! `StartTls` is only entered when the caller invokes
//! [`crate::SmtpClient::starttls`] on a transport that implements
//! [`crate::transport::StartTlsCapable`]. After the TLS handshake completes
//! the state machine transitions back to `Ehlo` to re-issue the greeting
//! per RFC 3207 §4.2, and from there to `Authentication`.
//!
//! Any state may also transition directly to `Quit` and then `Closed` on a
//! caller-initiated shutdown or to `Closed` on a fatal error.

/// The phases of an SMTP exchange tracked by the client.
///
/// This enum is `non_exhaustive` so that future SMTP extensions can add
/// new phases without forcing a major version bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SessionState {
    /// Connection has been established but the server greeting has not yet
    /// been read.
    Greeting,
    /// The greeting has been received but `EHLO` has not yet been sent (or
    /// has not yet succeeded).
    Ehlo,
    /// `EHLO` has succeeded. Authentication may be performed, or skipped.
    Authentication,
    /// `STARTTLS` has been issued and accepted (`220` from server). The
    /// transport is being upgraded; on success the state moves to `Ehlo`
    /// to re-issue the greeting per RFC 3207 §4.2.
    StartTls,
    /// Ready to issue `MAIL FROM` for a new transaction.
    MailFrom,
    /// `MAIL FROM` has been accepted; ready to issue `RCPT TO`.
    RcptTo,
    /// At least one `RCPT TO` has been accepted; ready to issue `DATA`.
    Data,
    /// `QUIT` has been sent; the next operation is to close the transport.
    Quit,
    /// The session is finished, either cleanly or due to a fatal error.
    /// No further SMTP operations are permitted.
    Closed,
}

impl SessionState {
    /// Return `true` if the session is over and no further SMTP operations
    /// are permitted.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Return `true` if `next` is a valid follow-on state from `self`.
    ///
    /// This encodes the protocol's ordering rules. The
    /// [`crate::client::SmtpClient`] consults this before performing any
    /// operation and returns an [`crate::error::InvalidInputError`] if the
    /// transition is not allowed.
    // The arms below are intentionally kept separate so that each represents
    // one named protocol situation. Combining them into a single OR-pattern
    // would be terser but would lose the per-case documentation, so we
    // suppress `match_same_arms` for this function only.
    #[allow(clippy::match_same_arms)]
    pub const fn can_transition_to(self, next: Self) -> bool {
        use SessionState::{
            Authentication, Closed, Data, Ehlo, Greeting, MailFrom, Quit, RcptTo, StartTls,
        };
        match (self, next) {
            // The transport may close at any time, in which case the client
            // marks itself Closed.
            (_, Closed) => true,
            // QUIT may be sent from any active state.
            (Greeting | Ehlo | Authentication | MailFrom | RcptTo | Data, Quit) => true,
            // Normal forward progression.
            (Greeting, Ehlo) => true,
            (Ehlo, Authentication) => true,
            // STARTTLS path: after EHLO succeeds the caller may upgrade.
            (Authentication, StartTls) => true,
            // After the TLS upgrade we go back to Ehlo so RFC 3207's
            // re-EHLO requirement is captured by the same code path that
            // handles the initial EHLO.
            (StartTls, Ehlo) => true,
            // Authentication is optional: we can skip from Ehlo straight to
            // MailFrom for unauthenticated submission, jump from
            // Authentication to MailFrom after a successful login, or
            // re-enter MailFrom to start a new transaction on a session
            // that just completed one (RFC 5321 §3.3 allows multiple
            // transactions per session).
            (Ehlo | Authentication | MailFrom, MailFrom) => true,
            (MailFrom, RcptTo) => true,
            // Multiple RCPT TO commands stay in RcptTo.
            (RcptTo, RcptTo) => true,
            (RcptTo, Data) => true,
            // After DATA the same connection can start a new transaction.
            (Data, MailFrom) => true,
            _ => false,
        }
    }
}
