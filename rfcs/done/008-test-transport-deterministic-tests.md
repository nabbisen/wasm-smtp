# RFC 008 — Test transport and deterministic protocol tests

**Status.** Implemented (v0.1.0)
**Priority.** P1
**Tracks.** Testing
**Touches.** `crates/wasm-smtp/src/tests/`, `crates/wasm-smtp-cloudflare/src/tests/`, `crates/wasm-smtp-tokio/src/tests/`

## Summary

Define the test transport design and the deterministic protocol test
strategy that lets `wasm-smtp`'s session state machine be verified
without a real SMTP server or an async runtime.

## Motivation

`wasm-smtp` is a protocol library. Protocol libraries need to test
every state transition, including rare error paths, without depending
on external services. A scripted test transport — one that returns
pre-programmed responses and records outbound commands — makes every
test scenario reproducible, fast, and CI-friendly.

## Goals

- A `MockTransport` that accepts a sequence of scripted server responses
  and records the commands the session sends.
- Tests that run with `#[test]` (synchronous), without tokio or any
  other async executor, to keep test compilation fast and simple.
- Coverage of the normal path (success), error paths (4xx, 5xx, auth
  failure), edge cases (empty recipients, dot-stuffing), and STARTTLS.
- Adapter-level tests that verify the byte-pushing helpers independently
  of the session state machine.

## Non-goals

- Integration tests against a real SMTP server (those are manual QA).
- Performance / load testing.
- Cloudflare Workers runtime integration tests (require `wrangler dev`).

## Design

### `MockTransport`

```rust
pub struct MockTransport {
    /// Server responses returned in order.
    responses: VecDeque<String>,
    /// Commands sent by the session, recorded in order.
    sent: Vec<Vec<u8>>,
}

impl MockTransport {
    pub fn new(responses: &[&str]) -> Self { ... }
    pub fn sent_commands(&self) -> &[Vec<u8>] { &self.sent }
}

impl Transport for MockTransport { ... }
```

`read_line` pops from `responses`. `write_all` appends to `sent`.
`close` is a no-op. If `responses` is exhausted, `read_line` returns
`Ok(0)` (clean EOF), which the session treats as an unexpected close.

### Synchronous test execution

The test transport implements `Transport` using a synchronous poll-based
executor shim so that tests can use `#[test]` instead of
`#[tokio::test]`. This avoids pulling `tokio` into the core's test
dependencies.

### Coverage targets

- Normal authenticated send (SCRAM-SHA-256, PLAIN, LOGIN).
- EHLO capability parsing (multiple extensions, STARTTLS advertisement).
- MAIL FROM with 250, 251 (forwarding accepted).
- RCPT TO with multiple recipients and one 550 (rejected recipient).
- DATA with dot-stuffing: bodies containing lines starting with `.`.
- Empty body.
- QUIT with and without prior MAIL FROM.
- 4xx on MAIL FROM: session surfaces `TransientFailure`.
- 5xx on AUTH: surfaces `AuthError::Rejected`.
- Premature close (mock returns `Ok(0)` mid-session).
- STARTTLS flow: plaintext EHLO → STARTTLS → upgrade → re-EHLO → AUTH.

### Adapter tests

Each adapter crate has its own `tests/` module that uses
`tokio_test::io::Builder` (for the tokio adapter) or custom scripted
streams (for the Cloudflare adapter) to verify that the adapter
correctly maps its native I/O to the `Transport` contract.

## Security considerations

Test transports must not be usable in production code. They are in the
`#[cfg(test)]` or `wasm-smtp-test` dev-dependency crate only. The
production API has no method to inject a test transport without
depending on the test crate.

## Simplicity and maintainability considerations

Scripted responses are plain `&str` slices, not complex fixtures or
JSON configs. Tests are readable at a glance: the response sequence is
adjacent to the assertion on what commands were sent.

## Alternatives considered

**Tokio-based mock with `tokio_test::io`:** used in adapter crates where
tokio is already a dev-dependency. Not used in core to avoid adding
tokio to core's test build.

**Property-based testing (proptest):** useful for the parser but not for
the state machine, where the interesting cases are specific sequences of
server responses. Manual scripted tests cover the meaningful scenarios.

## Implementation plan

*Already implemented as of v0.1.0. STARTTLS tests added v0.5.x.*

## Acceptance criteria

- `cargo test -p wasm-smtp` runs without an async runtime in scope for
  the core test suite.
- All 5xx and 4xx error paths are covered by at least one test each.
- Dot-stuffing tests cover the edge cases described in RFC 004.
- STARTTLS tests verify that re-EHLO replaces the capability cache.

## Open questions

None. Implemented and stable.
