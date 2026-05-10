# RFC 019 — Streaming DATA and bounded memory

**Status.** Proposed
**Priority.** P2
**Tracks.** Performance / Memory / Core
**Touches.** `crates/wasm-smtp/src/session.rs`, `crates/wasm-smtp/src/transport.rs`, `docs/src/memory.md`

## Summary

Extend `wasm-smtp`'s message-send API to support streaming (chunk-at-a-time)
DATA transmission and bounded-memory operation, so that large messages
can be sent without loading the entire body into memory.

## Motivation

The current `send_mail(from, to, body: &str)` API requires the entire
message body as a `&str`. For large messages (attachments, rich HTML),
this means holding megabytes in memory simultaneously. On constrained
runtimes — Cloudflare Workers (128 MB memory limit), WASI IoT modules,
edge workers — this is a hard constraint.

Streaming DATA also removes the need to buffer the dot-stuffed, CRLF-
normalised output before sending. The dot-stuffing and CRLF logic can
operate on a sliding window of bytes, reducing peak memory usage from
O(message size) to O(line size).

## Goals

- Add `send_mail_bytes(from, to, body: &[u8])` as Phase 2 (same semantics
  as `send_mail` but accepts bytes instead of a string, no UTF-8 assumption).
- Design (not necessarily implement) a `send_mail_stream` API that accepts
  an `AsyncRead` or a byte-producing async iterator.
- Define the maximum line buffer size for the streaming dot-stuffer.
- Document the memory behaviour of the current implementation.
- Ensure the design does not require breaking changes to `Transport`.

## Non-goals

- MIME streaming builder.
- Full `no_std` / `alloc` migration (see RFC 021).
- Attachment chunking or multipart assembly.
- Message queuing.

## Design

### Phase 1 (current): `send_mail(&str)` — Implemented

The entire body is passed as a `&str`. The dot-stuffing and CRLF
normalisation helper produces a new `String` (O(n) allocation). This
is the current behaviour as of v0.9.4.

Peak memory: 2× message size (input string + normalised output string).

### Phase 2: `send_mail_bytes(&[u8])` — Planned

```rust
pub async fn send_mail_bytes(
    &mut self,
    from: &str,
    to: &[&str],
    body: &[u8],
) -> Result<(), SmtpError>
```

Identical semantics to `send_mail` but accepts raw bytes. Useful for:
- Pre-formatted messages produced by `mail-builder` as bytes.
- Messages with non-UTF-8 content (8BITMIME, though `wasm-smtp` does
  not validate 8BITMIME capability negotiation in this phase).

Implementation: the dot-stuffer operates on `&[u8]` rather than `&str`.
CRLF normalisation is byte-level (`0x0A` not preceded by `0x0D`).

Peak memory: same as Phase 1 (2× body size).

### Phase 3: `send_mail_stream` — Designed, not implemented

```rust
pub async fn send_mail_stream<R>(
    &mut self,
    from: &str,
    to: &[&str],
    body: R,
) -> Result<(), SmtpError>
where
    R: AsyncRead + Unpin,
```

Or, as an alternative that avoids `AsyncRead` (which is tokio/futures
specific):

```rust
pub trait MessageBody {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;
}
```

The streaming dot-stuffer operates on a fixed-size buffer (e.g., 8 KB
per chunk). It must handle the case where a line beginning with `.`
spans two chunks. The state machine carries a one-byte look-ahead from
the previous chunk.

Peak memory: O(buffer size) ≈ 8–16 KB for the stream buffer, plus
one line's worth of bytes for look-ahead. O(1) with respect to message
size.

### Bounded buffer design

For Phase 3, the dot-stuffer's line buffer has a configurable maximum
size. If a single line exceeds the maximum (default: 1000 bytes per
RFC 5321 §4.5.3, configurable up to 998), the send is aborted with
`InvalidInputError::LineTooLong`. This prevents a malicious or
malformed message body from causing unbounded allocation.

### Transport implications

The streaming API writes chunks to the transport as they are processed.
This requires the transport's `write_all` to be called multiple times
(once per chunk). This is already allowed by the `Transport` trait;
no changes are needed to the trait itself.

The end-of-DATA sentinel (`\r\n.\r\n`) is written after the stream
is exhausted.

## Security considerations

- The bounded line buffer prevents memory exhaustion from a message with
  extremely long lines.
- Streaming does not weaken dot-stuffing; the look-ahead byte ensures
  that a `.` split across chunk boundaries is correctly detected.
- `send_mail_stream` must not send any bytes until all `check_recipient`
  and `check_message_size` policy checks pass. Since the size is not
  known in advance for a stream, `check_message_size` is called with
  `usize::MAX` or a caller-supplied estimate, and the actual size is
  checked against a hard cap if configured.

## Simplicity and maintainability considerations

The three-phase approach allows incremental delivery. Phase 1 is stable
and working. Phase 2 is a small extension. Phase 3 is a larger addition
but does not require breaking changes to the existing API.

## Alternatives considered

**Require callers to pre-buffer:** the current approach. Adequate for
small messages but not for large attachments or constrained runtimes.

**`AsyncWrite` as the streaming body source:** natural for tokio-based
callers but would couple the streaming API to the tokio / futures
`AsyncWrite` trait. A project-defined `MessageBody` trait (similar to
`Transport`) keeps the core dependency-free.

## Implementation plan

*Phase 1 implemented. Phases 2 and 3 are not yet implemented.*

Phase 2 target: v0.16.0 alongside other streaming work.
Phase 3 target: v0.17.0 or later, after Phase 2 is validated.

## Acceptance criteria

**Phase 2 (`send_mail_bytes`):**
- Behaviour is identical to `send_mail` for the same content.
- Dot-stuffing and CRLF normalisation work on raw bytes.
- Existing tests pass with bytes.

**Phase 3 (`send_mail_stream`, design only for now):**
- The `MessageBody` trait design is finalized in this RFC and does not
  change the `Transport` trait.
- A proof-of-concept streaming dot-stuffer is tested against the RFC 004
  edge cases using a fixed-size chunk iterator.

## Open questions

1. Should Phase 3 use `AsyncRead + Unpin` (tokio-compatible) or a
   project-defined `MessageBody` trait? The latter maintains the
   core's runtime independence.
2. What should the default bounded buffer size be? 8 KB per chunk is
   a common choice; RFC 5321 line length is 998 bytes.
3. How should `check_message_size` in the policy hook (RFC 011) interact
   with a stream of unknown total size? Options: (a) skip the check,
   (b) pass `usize::MAX`, (c) require the caller to supply an estimated
   size.
