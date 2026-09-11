# Introduction

`wasm-smtp` is a Rust implementation of SMTP submission designed for
WebAssembly runtimes, and for native async runtimes that want the same
client. The project is split into a protocol core and one adapter crate
per runtime:

- **`wasm-smtp`** holds the SMTP state machine, response parser,
  command formatter, dot-stuffing, and error taxonomy. It does no I/O
  of its own. Anywhere that has a working `Future` machinery and an
  async byte stream can use it.
- **`wasm-smtp-cloudflare`** adapts the Cloudflare Workers `Socket` API
  to the `Transport` trait that `wasm-smtp` consumes.
- **`wasm-smtp-tokio`** adapts tokio + rustls, for conventional async
  servers.
- **`wasm-smtp-wasi`** adapts WASI 0.2 sockets (`wasm32-wasip2`).
- **`wasm-smtp-component`** exports the WASM Component Model interface
  in `wit/smtp.wit`, for callers that are not written in Rust.

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
- **`AUTH SCRAM-SHA-256`, `AUTH PLAIN`, and `AUTH LOGIN` for
  authenticated submission.** The auto-selecting `login()` prefers
  SCRAM-SHA-256 (RFC 5802 / 7677, `scram-sha-256` feature, default-on)
  when the server advertises it, because the password never crosses the
  wire; otherwise it falls back to PLAIN, then LOGIN. Bearer-token
  authentication is opt-in per call: `login_xoauth2()` for the Google /
  Microsoft profile (`xoauth2` feature, default-on) and
  `login_oauthbearer()` for the IETF mechanism of RFC 7628
  (`oauthbearer` feature, default-on). Senders against a self-hosted
  Postfix or a commercial relay with a static password can drop the
  OAuth 2.0 paths with `default-features = false`. SCRAM-SHA-256-PLUS
  (channel binding) and GSSAPI are not supported.
- **`ENHANCEDSTATUSCODES` (RFC 2034 / 3463).** When the server
  advertises this extension, every reply is annotated with the
  parsed `class.subject.detail` code, propagated into
  `ProtocolError::UnexpectedCode` and `AuthError::Rejected` so
  callers can distinguish (e.g.) `5.1.1` user-unknown from `5.7.1`
  policy rejection programmatically.
- **`PIPELINING` (RFC 2920).** When the server advertises it,
  `MAIL FROM`, every `RCPT TO`, and `DATA` are written as one batch and
  the replies are read in order, which removes a round trip per
  recipient on high-latency links. Behind the default-on `pipelining`
  cargo feature; servers that do not advertise it get the sequential
  path unchanged.
- **`SMTPUTF8` (RFC 6531) — opt-in feature.** Behind the `smtputf8`
  cargo feature (off by default), `send_mail_smtputf8` accepts UTF-8
  in envelope addresses (`送信者@例え.jp`). Off by default to keep
  the WASM bundle small for the majority of callers who only send
  ASCII addresses.
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
