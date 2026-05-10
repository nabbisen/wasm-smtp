# RFC 006 — SMTP session state machine

**Status.** Implemented (v0.1.0)
**Priority.** P0
**Tracks.** Core session
**Touches.** `crates/wasm-smtp/src/session.rs`, `crates/wasm-smtp/src/client.rs`, `crates/wasm-smtp/src/protocol.rs`

## Summary

Define the SMTP session state machine: the ordered sequence of protocol
states from TCP connection to QUIT, the valid transitions, the behavior
on unexpected responses, and the `SmtpClient` public API that drives the
machine.

## Motivation

SMTP is a stateful protocol. Commands must be issued in a specific order;
issuing them out of order is a protocol error. The state machine
enforces this order at the type level where possible and at runtime
where not, making it harder to accidentally send MAIL FROM before EHLO
or DATA before RCPT TO.

An explicit state machine also makes the error classification problem
tractable: a `535` response during AUTH means something different from a
`535` during MAIL FROM. Tagging errors with the operation that produced
them (the `SmtpOp` field on `ProtocolError::UnexpectedCode`) gives
callers structured failure information.

## Goals

- Model the full SMTP submission client flow: Greeting → EHLO → AUTH →
  MAIL FROM → RCPT TO → DATA → QUIT.
- Emit tagged errors (`SmtpOp`) that identify which step failed.
- Support multiple messages on a single connection (RFC 5321 §3.3):
  after a successful DATA, return to a state that allows a new MAIL FROM
  without re-authenticating or re-doing EHLO.
- Support STARTTLS upgrade in the state machine (RFC 3207): after the
  server accepts STARTTLS, re-run EHLO on the encrypted channel before
  AUTH.
- Mark `SessionState` and `ProtocolError` as `#[non_exhaustive]` so
  future variants can be added without a breaking change.

## Non-goals

- Retry logic or connection pooling.
- Automatic reconnection on transport failure.
- Server-side (incoming) SMTP.
- MIME construction.

## Design

### State enum

```rust
#[non_exhaustive]
pub enum SessionState {
    Greeting,
    Ehlo,
    StartTls,       // After STARTTLS accepted; before TLS handshake
    Authentication, // After EHLO; before or after AUTH
    MailFrom,
    RcptTo,
    Data,
    Quit,
    Closed,
}
```

Transitions:

```
Greeting ──▶ Ehlo
Ehlo ──▶ StartTls (if STARTTLS requested) ──▶ Ehlo (re-EHLO post-TLS) ──▶ Authentication
Ehlo ──▶ Authentication (if no STARTTLS)
Authentication ──▶ MailFrom
MailFrom ──▶ RcptTo
RcptTo ──▶ Data
Data ──▶ MailFrom  (multiple messages on one connection, RFC 5321 §3.3)
Data ──▶ Quit
* ──▶ Closed (on transport error or QUIT completion)
```

### `SmtpClient` API

```rust
impl<T: Transport> SmtpClient<T> {
    /// Connect using an already-established transport.
    /// Reads the server greeting and issues EHLO.
    pub async fn connect(transport: T, ehlo_domain: &str)
        -> Result<Self, SmtpError>;

    /// Authenticate with the best available mechanism.
    /// Prefers SCRAM-SHA-256 > AUTH PLAIN > AUTH LOGIN.
    pub async fn login(&mut self, username: &str, password: &str)
        -> Result<(), SmtpError>;

    /// Authenticate with a specific mechanism.
    pub async fn login_with(&mut self, mechanism: AuthMechanism,
                             username: &str, password: &str)
        -> Result<(), SmtpError>;

    /// Send one message. May be called repeatedly on the same client
    /// for multiple-message sessions.
    pub async fn send_mail(&mut self, from: &str, to: &[&str], body: &str)
        -> Result<(), SmtpError>;

    /// Issue QUIT and close the transport.
    pub async fn quit(&mut self) -> Result<(), SmtpError>;
}

impl<T: StartTlsCapable> SmtpClient<T> {
    /// Connect over a plaintext transport and upgrade to TLS via STARTTLS.
    /// Issues EHLO twice: once plaintext, once post-TLS.
    pub async fn connect_starttls(transport: T, ehlo_domain: &str)
        -> Result<Self, SmtpError>;

    /// Perform the STARTTLS upgrade on a session already in the
    /// Authentication state. Returns an error if the server did not
    /// advertise STARTTLS in the EHLO capability list.
    pub async fn starttls(&mut self) -> Result<(), SmtpError>;
}
```

### SmtpOp tagging

Every `ProtocolError::UnexpectedCode` carries a `during: SmtpOp` field:

```rust
pub enum SmtpOp {
    Greeting, Ehlo, StartTls, Auth,
    MailFrom, RcptTo, Data, Quit, Rset,
}
```

This allows callers to distinguish "the server rejected my credentials"
from "the server rejected my MAIL FROM address" without parsing the
response text.

### Multiple messages

After a successful DATA exchange, the state returns to `MailFrom`.
The session does not re-authenticate or re-issue EHLO. This matches
RFC 5321 §3.3's specification for transaction reuse.

### STARTTLS injection defence

Before upgrading, the session discards any buffered input from the
server. This prevents a CVE-2011-1575-class injection where a
malicious server pre-populates the buffer with commands that the client
would execute on the encrypted channel as if they came from the server.

## Security considerations

- `SmtpOp` tags expose the failed step but not the server's reply text,
  which may contain sensitive server information.
- The STARTTLS injection defence (pre-upgrade buffer flush) is
  mandatory, not optional.
- `SessionState::Closed` is a terminal state; any call to an API method
  on a closed session returns `InvalidInputError` without attempting I/O.

## Simplicity and maintainability considerations

The state machine is implemented as an explicit `match` on
`SessionState`, not as a trait object hierarchy. This keeps all
transitions visible in one place and avoids dynamic dispatch overhead.

## Alternatives considered

**Typestate pattern (encode state in generic parameter):** appealing for
compile-time enforcement, but makes the `SmtpClient` type unusable in
`Box<dyn ...>` contexts and complicates the API surface for callers who
want to pass a client around. The runtime `SessionState` check is
sufficient.

**Single flat `send` method that runs the whole flow:** rejected because
it prevents callers from sending multiple messages on one connection or
from using XOAUTH2 / SCRAM instead of password auth.

## Implementation plan

*Already implemented as of v0.1.0 (base state machine), v0.5.x (STARTTLS), v0.4.x (SmtpOp).*

## Acceptance criteria

- Correct SMTP command sequence can be exercised against the scripted
  test transport without a real server.
- Issuing a command in the wrong state returns `InvalidInputError`.
- `UnexpectedCode` always carries a non-empty `SmtpOp`.
- Multiple messages on one connection work without re-authentication.
- STARTTLS re-issues EHLO and replaces the capability cache.

## Open questions

None. Implemented and stable.
