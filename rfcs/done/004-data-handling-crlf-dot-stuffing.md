# RFC 004 — DATA handling, CRLF normalization, and dot-stuffing

**Status.** Implemented (v0.1.0)
**Priority.** P0
**Tracks.** Core protocol / DATA
**Touches.** `crates/wasm-smtp/src/protocol.rs`, `crates/wasm-smtp/src/session.rs`, `crates/wasm-smtp/src/tests/`

## Summary

Define how `wasm-smtp` processes the message body during the SMTP
DATA phase: CRLF normalization, dot-stuffing (RFC 5321 §4.5.2), and
the end-of-data terminator (`\r\n.\r\n`).

## Motivation

The SMTP DATA command sends a message body terminated by a line
containing only a period. Any line in the body that begins with a
period must be "dot-stuffed" (an extra period prepended) to prevent
premature termination. CRLF handling adds a second concern: the body
MUST use CRLF line endings on the wire, but application code often
works with LF-only strings. Getting either detail wrong causes
message corruption or delivery failure.

## Goals

- Accept a message body as a `&str` from the caller.
- Normalize bare `\n` to `\r\n`; leave existing `\r\n` intact.
- Dot-stuff any line that begins with `.`.
- Append `\r\n.\r\n` after the last line.
- Handle an empty body.
- Handle a body whose final line has no trailing newline.
- Be deterministic and independently testable.

## Non-goals

- MIME structure or header generation.
- Attachment encoding.
- Streaming DATA (see RFC 019).
- DKIM signing.
- Binary (8BITMIME) mode — this RFC covers 7-bit ASCII / UTF-8 text.

## Design

### Input contract

The caller passes a `&str` containing a fully composed RFC 5322
message (headers + blank line + body). The string may use LF or
CRLF line endings. It must not contain embedded null bytes; such input
is rejected as `InvalidInputError`.

### Processing steps

1. **CRLF normalization:** iterate over lines. For each `\n` not
   preceded by `\r`, replace with `\r\n`. Existing `\r\n` pairs
   pass through unchanged.
2. **Dot-stuffing:** for each line that starts with `.`, prepend an
   extra `.`.
3. **Terminal period:** append `\r\n.\r\n` after the last line.
   If the body is empty, send `\r\n.\r\n` directly (a blank body
   is valid SMTP DATA).

### Empty body

An empty `&str` input sends the DATA body as just `\r\n.\r\n`, which
represents a zero-byte body. This is legal per RFC 5321.

### Unterminated final line

If the input does not end with `\n` or `\r\n`, the normalizer appends
`\r\n` before the terminal period. This matches what real MUAs do when
a body is assembled without a trailing newline.

### Interaction with transport

The entire prepared buffer is written to the transport in one
`write_all` call per the `Transport` contract. This ensures the DATA
payload is sent atomically from the transport's perspective. Streaming
writes are deferred to RFC 019.

## Security considerations

- Dot-stuffing must always run, even if the caller claims the body is
  already normalized. Trusting caller-supplied dot-free bodies would
  allow injection of the end-of-data sentinel.
- The length of the body is not capped here; callers are responsible
  for enforcing size limits appropriate to their environment.
  RFC 011 (policy hook) provides a hook for this.

## Simplicity and maintainability considerations

The normalization and dot-stuffing are pure functions on `&str`
returning `String`. They can be called directly from tests without any
SMTP session setup. The session code calls them as helpers; it does not
re-implement the logic inline.

## Alternatives considered

**Require the caller to pre-normalize:** rejected because every caller
would need to perform the same steps, creating multiple opportunities
for subtle bugs (e.g., forgetting to handle the terminal period).

**Streaming writer:** desirable for large messages, but adds complexity
(async generator / AsyncWrite integration). Deferred to RFC 019.

## Implementation plan

*Already implemented as of v0.1.0.*

## Acceptance criteria

Unit tests covering:

- Body with `.` at line start is dot-stuffed.
- Terminal `.` is not confused with dot-stuffed content.
- LF-only input is normalized to CRLF.
- CRLF input passes through unchanged.
- Empty body produces `\r\n.\r\n`.
- Final line without trailing newline gets one appended.

## Open questions

None. Implemented and stable.
