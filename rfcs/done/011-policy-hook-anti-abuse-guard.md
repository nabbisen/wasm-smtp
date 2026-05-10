# RFC 011 — Policy hook and anti-abuse guard

**Status.** Implemented (v0.10.0)
**Priority.** P1
**Tracks.** Security / Policy
**Touches.** `crates/wasm-smtp/src/policy.rs`, `crates/wasm-smtp/src/client.rs`, `TERMS_OF_USE.md`

## Summary

Add a `SendPolicy` trait to `wasm-smtp` that lets callers inject
pre-send validation logic — sender allowlists, recipient constraints,
message size limits, and rate-limit hooks — before any bytes reach the
wire.

## Motivation

`TERMS_OF_USE.md` prohibits spam, impersonation, and bulk sending.
A legal document is necessary but not sufficient. The library itself
should provide design-level hooks that make it straightforward for
integrators to enforce these constraints without reimplementing the
checks in every application.

A policy hook also serves the "Local-first / Sovereign Software" use
case: applications that deploy `wasm-smtp` as an embedded component
need a structured way to restrict who can send to whom and at what rate.

## Goals

- Define a `SendPolicy` trait with methods for sender validation,
  recipient validation, and message-size validation.
- Define a `DefaultPolicy` that allows everything (preserves current
  behaviour for callers that don't opt in).
- Allow callers to attach a policy to `SmtpClient` at construction time.
- Ensure policy checks run before any SMTP command is sent.
- Ensure policy failures are distinct from SMTP failures in the error
  type (`InvalidInputError` or a new `PolicyError`).

## Non-goals

- Implementing a spam filter or content classifier.
- Providing a rate-limit *storage* backend (callers supply the counter).
- Bulk mailing features.
- DKIM / SPF / DMARC validation (those are sender-side authentication,
  not a client policy concern).

## Design

### `SendPolicy` trait

```rust
pub trait SendPolicy: Send + Sync {
    /// Called before MAIL FROM. Return Err to abort the send.
    fn check_sender(&self, from: &str) -> Result<(), PolicyError>;

    /// Called once per recipient before RCPT TO. Return Err to abort.
    fn check_recipient(&self, to: &str) -> Result<(), PolicyError>;

    /// Called with the message size (bytes) before DATA.
    /// Return Err to abort if the message is too large.
    fn check_message_size(&self, bytes: usize) -> Result<(), PolicyError>;
}
```

A `DefaultPolicy` implementation returns `Ok(())` for all three methods.

### `PolicyError`

```rust
pub struct PolicyError {
    pub message: String,
}
```

Maps to `SmtpError::InvalidInput(InvalidInputError { message })` or
a new `SmtpError::Policy(PolicyError)` variant (to be decided during
review).

### Attaching a policy to `SmtpClient`

Two options under review:

**Option A: Builder pattern**

```rust
SmtpClient::builder()
    .policy(Box::new(MyPolicy))
    .connect(transport, ehlo_domain)
    .await?
```

**Option B: `with_policy` method after `connect`**

```rust
let mut client = SmtpClient::connect(transport, ehlo_domain).await?;
client.set_policy(Box::new(MyPolicy));
```

Option A is preferred because it prevents a window where the client
is connected but has no policy.

### Execution points

- `check_sender` is called at the start of `send_mail`, before MAIL FROM.
- `check_recipient` is called for each entry in the `to` slice, before
  any RCPT TO is sent. If any recipient fails, the entire send is
  aborted without sending MAIL FROM.
- `check_message_size` is called after `check_recipient`, before DATA.

### Example: allowlist policy

```rust
struct AllowlistPolicy {
    allowed_senders: HashSet<String>,
    max_recipients: usize,
    max_bytes: usize,
}

impl SendPolicy for AllowlistPolicy {
    fn check_sender(&self, from: &str) -> Result<(), PolicyError> {
        if self.allowed_senders.contains(from) { Ok(()) }
        else { Err(PolicyError { message: format!("{from} not in sender allowlist") }) }
    }
    fn check_recipient(&self, _to: &str) -> Result<(), PolicyError> { Ok(()) }
    fn check_message_size(&self, bytes: usize) -> Result<(), PolicyError> {
        if bytes <= self.max_bytes { Ok(()) }
        else { Err(PolicyError { message: format!("message too large: {bytes} bytes") }) }
    }
}
```

### Rate-limit hook

Rate limiting requires external state (counters). The policy trait
deliberately does not manage state. Callers implement `check_sender`
or `check_recipient` to increment and check a counter (in-memory,
Redis, Cloudflare KV, etc.):

```rust
fn check_sender(&self, from: &str) -> Result<(), PolicyError> {
    if self.rate_limiter.try_acquire(from) { Ok(()) }
    else { Err(PolicyError { message: "rate limit exceeded".into() }) }
}
```

## Security considerations

- Policy checks run before any SMTP command, so a rejected send does
  not leave a half-open SMTP transaction.
- `PolicyError` must not include message body content.
- The `DefaultPolicy` (allow all) preserves backward compatibility
  but should be clearly documented as "no protection."

## Simplicity and maintainability considerations

The trait has exactly three methods, matching the three natural
validation points in an SMTP transaction. More granular hooks (e.g.,
`check_ehlo_domain`) are deferred; they can be added to the trait with
default implementations that return `Ok(())`.

## Alternatives considered

**Closure-based API:** `SmtpClient::with_pre_send(|from, recipients, size| ...)`.
Simpler for one-off checks but harder to compose and test than a named
trait.

**Middleware pattern (wrapping `SmtpClient`):** a `PolicyClient<T>` that
wraps `SmtpClient<T>`. More flexible but makes the type signature
heavier and introduces a forwarding layer for every method. Rejected
in favour of embedding the policy in `SmtpClient` directly.

## Implementation plan

1. Add `crates/wasm-smtp/src/policy.rs` with `SendPolicy`, `DefaultPolicy`,
   `PolicyError`.
2. Add `policy: Box<dyn SendPolicy>` field to the internal session state
   (behind the builder API).
3. Call `check_sender`, `check_recipient` (for each recipient),
   `check_message_size` at the appropriate points in `send_mail`.
4. Map `PolicyError` to `SmtpError`.
5. Update `docs/src/` with a policy configuration chapter.
6. Update `CHANGELOG.md`.

Target release: v0.13.0.

## Acceptance criteria

- A custom `SendPolicy` that rejects a specific sender prevents MAIL FROM
  from being sent (verified by test transport command capture).
- `DefaultPolicy` passes all checks for any input.
- `PolicyError` maps to a distinct `SmtpError` variant.
- `check_recipient` is called once per recipient; a rejection on any one
  aborts the whole send before MAIL FROM.
- `check_message_size` is called with the byte count of the message body.

## Open questions

1. Should `PolicyError` be a new top-level `SmtpError::Policy` variant,
   or should it map to the existing `SmtpError::InvalidInput`? The
   former is more explicit for callers; the latter avoids a breaking
   change to the error enum.
2. Should `check_recipient` receive all recipients at once (as a slice)
   or one at a time? Slice form enables cross-recipient policies (e.g.,
   max recipient count checks); per-call form is simpler and matches
   the RCPT TO loop structure.
