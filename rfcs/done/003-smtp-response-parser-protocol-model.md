# RFC 003 — SMTP response parser and protocol model

**Status.** Implemented (v0.1.0)
**Priority.** P0
**Tracks.** Core protocol
**Touches.** `crates/wasm-smtp/src/protocol.rs`, `crates/wasm-smtp/src/error.rs`, `crates/wasm-smtp/src/tests/`

## Summary

Define the SMTP response parser and the core protocol data model:
reply codes, single-line and multi-line reply parsing, response
classification, and command formatting helpers. These are the building
blocks that the session state machine (RFC 006) and all higher-level
logic rest on.

## Motivation

SMTP's wire format is text-based but has several subtle parsing
requirements: three-digit reply codes, a separator character (`-` for
continuation, ` ` for final), optional text, and multi-line responses
where all but the last line use `-`. Getting these details wrong causes
silent failures (treating a multi-line response as complete too early)
or panics (indexing past a short line). The parser must be correct,
runtime-independent, and deterministically testable without a network.

## Goals

- Parse single-line SMTP replies (`250 OK`, `235 2.7.0 Authentication successful`).
- Parse multi-line SMTP replies (capability list from EHLO, 334 challenges).
- Classify replies by code class: 2xx success, 3xx intermediate, 4xx
  temporary failure, 5xx permanent failure.
- Format outbound SMTP command lines (EHLO, AUTH, MAIL FROM, RCPT TO,
  DATA, QUIT, RSET, NOOP).
- Produce correct `dot-stuffing` and CRLF termination for DATA bodies
  (see also RFC 004).
- Keep the implementation runtime-independent (no async, no I/O).

## Non-goals

- Session state management (RFC 006).
- Network I/O (RFC 005).
- Authentication logic (RFC 007).
- MIME parsing or composition.

## Design

### Reply code model

A reply is represented by a numeric code (u16, must be in range
100–599) and the associated text lines. The code's leading digit
determines its class:

| Class | Meaning |
|---|---|
| 2xx | Positive completion |
| 3xx | Positive intermediate (e.g. 354 start input) |
| 4xx | Transient negative (retry may succeed) |
| 5xx | Permanent negative (do not retry) |

### Parsing rules

A reply line has the format:

```
code separator text CRLF
```

where `separator` is either `-` (continuation) or ` ` (final). A
multi-line reply consists of one or more continuation lines followed
by exactly one final line; all lines share the same code. Lines that
exceed the RFC 5321 §4.5.3 maximum (998 bytes) are rejected with a
`ProtocolError`.

The parser reads lines one at a time from the transport until it
encounters a final line. It returns the code, all text lines, and
whether the code is in the 2xx/3xx/4xx/5xx class.

### Response classification

```rust
pub enum ReplyClass {
    Success,        // 2xx
    Intermediate,   // 3xx
    TransientFail,  // 4xx
    PermanentFail,  // 5xx
}
```

### Command formatting

Each SMTP command is formatted as a `String` (with CRLF) by a
dedicated helper. Callers do not manipulate the wire format directly.

### Debug output policy

`SmtpReply` does not implement `Display` with the raw text. The text
field is accessible to the session but is not formatted into any
error message that might propagate to logs by default. This prevents
server-supplied text (which may include credential hints) from leaking.

## Security considerations

- Reply text is never passed verbatim to error messages that bubble to
  the application layer without explicit opt-in. `ProtocolError`
  variants carry structured fields, not raw server strings.
- Line-length limits prevent unbounded allocation from a malicious
  server.
- Malformed responses (non-numeric code, truncated line) produce
  `ProtocolError::MalformedReply`, never a panic.

## Simplicity and maintainability considerations

The parser is a pure function over `&str` slices with no state. It is
independently unit-testable without any network fixture. This keeps
CI fast and makes regressions easy to identify.

## Alternatives considered

**Regex-based parser:** rejected because it requires a regex crate
(adding compile time and binary size) and is harder to audit for
correctness on edge cases like empty text fields or codes with leading
zeros.

**Nom / other parser combinator:** rejected as over-engineered for a
format this simple. Manual `split_at` / `chars` iteration is
sufficient and produces no transitive dependencies.

## Implementation plan

*Already implemented as of v0.1.0.*

## Acceptance criteria

All of the following are covered by deterministic unit tests:

- `250 OK` — single-line success.
- `250-STARTTLS\r\n250 SIZE 10240000` — two-line multi-line reply.
- `354 End data with <CR><LF>.<CR><LF>` — intermediate code.
- `421 Service not available` — transient failure.
- `535 5.7.8 Authentication credentials invalid` — permanent failure.
- Malformed: missing code, non-numeric code, truncated line.

## Open questions

None. Implemented and stable.
