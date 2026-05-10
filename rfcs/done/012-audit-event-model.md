# RFC 012 — Audit event model

**Status.** Implemented (v0.10.0)
**Priority.** P1
**Tracks.** Audit / Security
**Touches.** `crates/wasm-smtp/src/audit.rs`, `crates/wasm-smtp/src/session.rs`, `docs/src/audit.md`

## Summary

Define an `AuditSink` trait and `SmtpAuditEvent` enum that let callers
observe SMTP session milestones — connection, authentication, envelope
acceptance, message acceptance — without exposing message bodies or
credentials.

## Motivation

Applications that send transactional or operational email need audit
trails: "Did the message get accepted? Which recipient failed? When did
the connection happen?" The `tracing` feature provides structured logs
for operators, but `tracing` is a logging infrastructure decision.
Applications may want to record events to a database, a metrics
endpoint, or a custom audit log without depending on the
`tracing` ecosystem.

The audit event model is also a natural complement to the policy hook
(RFC 011): policy prevents disallowed sends; the audit log records what
was sent and whether it succeeded.

## Goals

- Define `SmtpAuditEvent`: a small, stable enum of named milestones.
- Define `AuditSink`: a trait with a single method that receives events.
- Events carry structured, machine-readable data (reply codes, recipient
  counts) but never message bodies or credentials.
- `AuditSink` is optional; callers that don't configure one pay no cost.
- Events are emitted synchronously within the session's async context
  (the sink is a `&dyn AuditSink`, not `async`).

## Non-goals

- Implementing a logging backend (that's the caller's job).
- Emitting message body content in any event.
- Emitting credentials or authentication tokens in any event.
- Full `tracing` span integration (the existing `tracing` feature covers
  that separately).
- Delivery-status notification (DSN) correlation.

## Design

### `SmtpAuditEvent`

```rust
#[non_exhaustive]
pub enum SmtpAuditEvent<'a> {
    /// TCP connection established. TLS state not yet known.
    Connected { host: &'a str, port: u16 },
    /// Server greeting received.
    GreetingReceived { code: u16 },
    /// EHLO exchange completed; capability list parsed.
    EhloCompleted,
    /// TLS upgrade completed (STARTTLS path).
    TlsUpgraded,
    /// Authentication completed successfully.
    AuthCompleted { mechanism: &'a str },
    /// MAIL FROM accepted by the server.
    MailFromAccepted { code: u16 },
    /// RCPT TO accepted for one recipient.
    RecipientAccepted { code: u16 },
    /// RCPT TO rejected for one recipient.
    RecipientRejected { code: u16 },
    /// DATA end-of-body marker accepted; message queued by server.
    MessageAccepted { code: u16 },
    /// QUIT completed and connection closed.
    QuitCompleted,
    /// Session ended abnormally (transport error, unexpected close).
    SessionAborted,
}
```

Notable omissions:
- No `from` address or `to` address in events — privacy-preserving default.
- No message body or size in events.
- No authentication credentials.
- No server reply text (may contain sensitive server-side information).

### `AuditSink` trait

```rust
pub trait AuditSink: Send + Sync {
    fn on_event(&self, event: &SmtpAuditEvent<'_>);
}
```

The method is synchronous. The implementation may queue events for
async processing; the trait itself is not async to keep the session
state machine simple.

### Attaching a sink

Mirror the policy hook builder pattern:

```rust
SmtpClient::builder()
    .audit(Box::new(MyAuditSink))
    .connect(transport, ehlo_domain)
    .await?
```

When no sink is configured, the default is a no-op sink. Zero overhead.

### Privacy considerations for recipient events

`RecipientAccepted` and `RecipientRejected` do not include the recipient
address. This is the default; a caller who needs to correlate events to
addresses must maintain that mapping in their own sink implementation.

The address-in-event behaviour may be added later as an explicit
opt-in (e.g., `RecipientAccepted { address: Option<&str>, code: u16 }`
where the address is only populated if the sink opts in). This is an
open question (see below).

### Relationship to `tracing`

The `tracing` feature (existing) emits spans and events to the `tracing`
subscriber. The audit sink is a separate, lower-level mechanism. Both
can be active simultaneously. The audit sink does not depend on `tracing`.

## Security considerations

- `SmtpAuditEvent` variants must never carry credentials or message body
  content. This is enforced by the type definitions: none of the variants
  have fields of type `&str` that could plausibly hold a password or body.
- Implementations of `AuditSink` are caller-supplied and not controlled
  by `wasm-smtp`. Callers are responsible for the security of their own
  sink.
- The `Connected` event exposes the SMTP host and port, which is
  metadata (not content). This is acceptable.

## Simplicity and maintainability considerations

Thirteen events is more than a minimal set but covers every meaningful
session milestone. Future variants can be added using
`#[non_exhaustive]`; existing sinks compile with a `_ => {}` arm.

## Alternatives considered

**`tracing`-only:** rejected. `tracing` requires a subscriber setup
that not all applications want, and its structured data is oriented
toward logging systems, not application-level event processing.

**Callback per event type:** `on_connected(...)`, `on_auth_completed(...)`,
etc. Avoids the match overhead but produces a large trait with many
methods, most of which most callers will leave as no-ops. A single
`on_event` is simpler.

**`async fn on_event`:** rejected for now. An async sink would require
the session state machine to await the sink, introducing backpressure
concerns and making the session code more complex. Callers who need
async processing can spawn a task from the sync `on_event`.

## Implementation plan

1. Add `crates/wasm-smtp/src/audit.rs` with `SmtpAuditEvent`,
   `AuditSink`, and a `NoopAuditSink`.
2. Add `audit: Box<dyn AuditSink>` to the session state (via builder).
3. Emit events at each session milestone.
4. Add tests that use a `VecAuditSink` (collects events to a `Vec`)
   to verify event sequence.
5. Add `docs/src/audit.md`.

Target release: v0.13.0.

## Acceptance criteria

- A `VecAuditSink` test verifies that the event sequence for a
  successful authenticated send is exactly:
  `Connected → GreetingReceived → EhloCompleted → AuthCompleted →
  MailFromAccepted → RecipientAccepted → MessageAccepted → QuitCompleted`.
- No event includes a credential or message body.
- A session with no audit sink configured compiles and runs without any
  overhead (confirmed by the `NoopAuditSink` default).
- `#[non_exhaustive]` is on `SmtpAuditEvent` and the existing tests
  compile without matching every variant explicitly.

## Open questions

1. Should `RecipientAccepted` / `RecipientRejected` optionally carry
   the recipient address? If yes, it should be an opt-in field
   (`Option<&str>`) that defaults to `None`.
2. Should there be a `DataStarted` event (emitted when the `DATA`
   command is accepted and before the body is sent)? This would let
   sinks record the start time for timing measurements.
3. Should `AuditSink` be `async`? This is deferred pending real-world
   usage feedback.
