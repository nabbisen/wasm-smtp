# Roadmap

This roadmap defines the release sequence for `wasm-smtp`. Phases are
shipped as cohesive units of value: every phase ends with a usable,
documented state, not a half-finished feature.

## Phase 1 — Core foundations *(complete)*

Everything required to drive a real SMTP server with a hand-written
mock transport.

- `Transport` async I/O contract
- Error taxonomy: `IoError`, `ProtocolError`, `AuthError`, `InvalidInputError`
- Reply parser: code, separator, multi-line, line-length cap
- Minimum state machine: `Greeting`, `Ehlo`, `Authentication`, `MailFrom`,
  `RcptTo`, `Data`, `Quit`, `Closed`
- Unit tests for every parser, formatter, and state transition

## Phase 2 — Core completion *(complete)*

The full single-message exchange end-to-end against a mock transport.

- `EHLO` with capability capture
- `AUTH LOGIN` (base64 username/password)
- `MAIL FROM`, `RCPT TO` (multiple recipients including 251 forwarding)
- `DATA` with full dot-stuffing and CRLF terminator
- `QUIT` with transport close
- Multiple transactions on a single session (RFC 5321 §3.3)
- Integration scaffolding: synchronous mock transport in `tests.rs`

## Phase 3 — Cloudflare adapter *(complete)*

A `wasm-smtp-cloudflare` crate that connects a Cloudflare Workers
`Socket` to `wasm-smtp-core`.

- Map the Workers `Socket` (which already implements
  `tokio::io::AsyncRead + AsyncWrite`) onto the `Transport` trait
- `connect_implicit_tls(host, port)` opens a TLS-secured TCP socket
  with `SecureTransport::On` and waits for `Socket::opened`
- `connect_smtps(host, port, ehlo_domain)` returns a ready-to-use
  `SmtpClient<CloudflareTransport>` after the SMTP greeting and
  `EHLO` exchange
- Worker-side connection lifecycle: `Transport::close` calls
  `Socket::close`, which shuts both halves cleanly
- Adapter-level tests exercise the byte-pushing helpers with
  `tokio_test::io::Builder`, plus full SMTP transactions
  (authenticated and unauthenticated) over the same mock

End-to-end runtime tests with `wrangler dev` against a real
submission server are part of the project's manual QA pass; they
are not run by `cargo test`.

## Phase 4 — Hardening *(complete)*

Production-quality polish on top of a working stack.

- ✅ `AUTH PLAIN` (RFC 4616) in addition to `AUTH LOGIN`, with
  auto-selection in `login` preferring `PLAIN` and explicit control
  via `login_with`.
- ✅ Improved `AuthError::UnsupportedMechanism` message that lists the
  mechanisms this client understands.
- ✅ `SmtpOp` enum and `during: SmtpOp` field on
  `ProtocolError::UnexpectedCode`: every protocol error now identifies
  the exact SMTP step that failed, both in Display output and as a
  programmatically-readable field.
- ✅ Documentation expansion: protocol reference covers both AUTH
  mechanisms, usage guide explains mechanism selection.
- ✅ Worked end-to-end usage examples (`docs/src/examples.md`):
  contact-form delivery, transactional alert, multiple recipients,
  multiple messages on one connection.

## Phase 5 — Future work *(not scheduled)*

Items that may be revisited in a future cycle. None of these is a
commitment.

- Additional adapters for non-Cloudflare runtimes.
- `STARTTLS` for ports 587/25 (still secondary to Implicit TLS).
- Extra SASL mechanisms (`XOAUTH2`, `SCRAM-SHA-256`).
- Pipelining (RFC 2920) for slightly better latency on high-RTT links.
- DSN / ENHANCEDSTATUSCODES extension parsing.

## Out of scope (for now)

The following are deliberately omitted from the initial roadmap. They
may be revisited later, but are not implied commitments.

- STARTTLS (Implicit TLS on 465 is the standard model for this project)
- SMTPUTF8 / international addresses
- MIME composition or attachment building
- Bulk delivery, retry queues, rate limiting
- DSN, ENHANCEDSTATUSCODES processing
- Pipelining (we read one reply at a time)
