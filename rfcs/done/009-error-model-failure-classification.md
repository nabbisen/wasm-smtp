# RFC 009 — Error model and failure classification

**Status.** Implemented (v0.1.0)
**Priority.** P0
**Tracks.** Core error / Security
**Touches.** `crates/wasm-smtp/src/error.rs`

## Summary

Define the error taxonomy for `wasm-smtp`: the top-level `SmtpError`
enum, its variants, the sub-error types, and the rules for what
information each error type may and may not carry.

## Motivation

A good error taxonomy lets callers make programmatic decisions (retry
on 4xx, surface to the user on 5xx, check credentials on auth failure)
without parsing error strings. It also prevents accidental credential
or message-body leakage through `Debug` output or error forwarding
chains.

## Goals

- Distinguish transport failures, protocol failures, authentication
  failures, and invalid input at the type level.
- Classify server responses as temporary (4xx) or permanent (5xx).
- Tag each `ProtocolError` with the SMTP operation that failed.
- Ensure no variant includes credentials or message body content.
- Provide `std::error::Error` source chains for structured logging.

## Non-goals

- Application-level retry logic.
- Logging implementation.
- Mapping to HTTP status codes or other external error taxonomies.

## Design

### Top-level `SmtpError`

```rust
#[non_exhaustive]
pub enum SmtpError {
    /// Transport-level I/O failure.
    Io(IoError),
    /// Protocol-level failure: unexpected response code, malformed reply.
    Protocol(ProtocolError),
    /// Authentication failure.
    Auth(AuthError),
    /// Caller-supplied argument was invalid.
    InvalidInput(InvalidInputError),
}
```

### `IoError`

Wraps transport-level failures. Carries a human-readable `message` and
an optional `source` (`Box<dyn std::error::Error + Send + Sync>`) for
preserving the underlying I/O error in the chain.

```rust
pub struct IoError {
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
impl IoError {
    pub fn new(message: impl Into<String>) -> Self;
    pub fn with_source(message: impl Into<String>,
                       source: impl std::error::Error + Send + Sync + 'static) -> Self;
}
```

`IoError::message` must never contain credentials or message bodies.

### `ProtocolError`

```rust
#[non_exhaustive]
pub enum ProtocolError {
    /// Server returned a reply code we did not expect for `during`.
    UnexpectedCode {
        during: SmtpOp,
        expected: u16,      // or range class
        got: u16,
        temporary: bool,    // true = 4xx, false = 5xx
    },
    /// Server reply line exceeded the RFC 5321 length limit.
    ReplyLineTooLong,
    /// Server reply was structurally malformed.
    MalformedReply,
    /// Server did not advertise a required extension.
    ExtensionUnavailable { name: String },
}
```

`UnexpectedCode` deliberately does not carry the server reply text.
The text may contain sensitive server-side information and is not
needed for programmatic classification.

### `AuthError`

```rust
#[non_exhaustive]
pub enum AuthError {
    /// Server returned 535 or equivalent rejection.
    Rejected,
    /// None of the server's advertised mechanisms are supported.
    UnsupportedMechanism {
        /// What the server offered.
        offered: Vec<String>,
        /// What this client understands.
        supported: Vec<String>,
    },
    /// SCRAM server-signature verification failed.
    ServerSignatureMismatch,
}
```

No `AuthError` variant includes the password or any credential data.

### `InvalidInputError`

```rust
pub struct InvalidInputError {
    message: String,
}
```

For calls with structurally invalid arguments (empty `from` address,
zero recipients, embedded null bytes in body).

### Temporary vs. permanent failure

`ProtocolError::UnexpectedCode::temporary` is `true` for 4xx codes
and `false` for 5xx codes. Callers use this to decide whether to retry.
`SmtpError` does not have a top-level `is_temporary()` helper; the
classification is on the variant to force callers to handle the
`Protocol` case explicitly.

### `Debug` output policy

All error types implement `Debug` in a way that never prints
credentials or message bodies. `IoError`'s `Debug` prints only the
message string and the source chain type name, not the source content,
to prevent downstream `format!("{:?}", e)` from leaking sensitive data.

## Security considerations

- `AuthError::Rejected` carries no information about which part of the
  credential was wrong, preventing oracle attacks where the error text
  reveals whether the username or password was incorrect.
- `ProtocolError::UnexpectedCode` omits server reply text; the `during`
  field gives enough context for debugging without exposing server state.
- The source chain in `IoError` preserves structured error context for
  operators without surfacing it in user-facing error messages.

## Simplicity and maintainability considerations

The taxonomy has exactly four top-level variants, matching the four
failure modes that callers need to handle differently. Sub-variants are
kept small. Future extensions use `#[non_exhaustive]` to avoid breaking
changes.

## Alternatives considered

**Single `Error` string type:** rejected because it makes programmatic
classification impossible.

**Carrying server reply text in all errors:** rejected because of the
security risk of leaking server-supplied strings into logs or user
interfaces.

## Implementation plan

*Already implemented as of v0.1.0 (base taxonomy), v0.4.x (SmtpOp), v0.7.x (IoError source chain).*

## Acceptance criteria

- `cargo test` passes with tests for each error variant.
- `format!("{:?}", SmtpError::Auth(AuthError::Rejected))` contains no
  credential data.
- 4xx codes set `temporary: true`; 5xx codes set `temporary: false`.
- `std::error::Error::source` chain is present when `IoError::with_source`
  was used.

## Open questions

None. Implemented and stable.
