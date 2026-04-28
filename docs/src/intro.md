# Introduction

`wasm-smtp` is a Rust implementation of SMTP submission designed for
WebAssembly runtimes — initially Cloudflare Workers, with room to add
others. The project is split into two crates:

- **`wasm-smtp-core`** holds the SMTP state machine, response parser,
  command formatter, dot-stuffing, and error taxonomy. It does no I/O
  of its own. Anywhere that has a working `Future` machinery and an
  async byte stream can use it.
- **`wasm-smtp-cloudflare`** *(planned)* will adapt the Cloudflare
  Workers `Socket` API to the `Transport` trait that `wasm-smtp-core`
  consumes.

This split is the project's central design choice. By drawing the
boundary between SMTP and the host runtime as a single small trait
(`Transport`), we keep the core completely portable, easy to test
against a synchronous mock, and easy to maintain. New runtimes need
only an adapter; they never need to fork the protocol implementation.

## What this project is for

The realistic use case is *programmatic transactional email* from a
constrained runtime: contact-form delivery, password resets, alert
notifications, and similar single-message submissions on behalf of a
single application owner. The project standardizes on:

- **Implicit TLS on port 465 and STARTTLS on port 587.** Both
  submission models are supported. The TLS handshake itself is the
  transport's responsibility; the core sees an opaque byte stream
  and (for STARTTLS) a single upgrade signal.
- **`AUTH PLAIN` and `AUTH LOGIN` for authenticated submission.** The
  client auto-selects the best mechanism advertised by the server.
  SASL SCRAM, GSSAPI, XOAUTH2 are not supported.
- **Caller-supplied message bodies.** The library does not build MIME,
  attach files, or compose multipart payloads. The body is whatever
  RFC 5322 / 5321 octets the caller passes, optionally CRLF-normalized,
  always dot-stuffed by the library.

## What this project is *not*

`wasm-smtp` is **not** a mail-blast tool. It does not include:

- Bulk delivery, retry queues, or rate limiting.
- DSN parsing or extension-status-code processing.
- A relaxed input mode for hostile or unknown SMTP servers.

If your problem is "how do I send 50 000 newsletters", that problem is
better solved by an email-sending platform (with proper deliverability,
list hygiene, and abuse-handling infrastructure), not by this library.

See `TERMS_OF_USE.md` at the repository root for the full statement of
acceptable use.
