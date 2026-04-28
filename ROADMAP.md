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

## Phase 5 — STARTTLS *(complete)*

Support for the in-place TLS upgrade flow (RFC 3207), enabling the
crate to work against submission servers that listen on port 587 in
addition to the existing Implicit-TLS path on port 465.

- ✅ `StartTlsCapable: Transport` trait — a separate marker so that
  STARTTLS-incapable transports remain usable for Implicit TLS, and
  so that calling `starttls()` on the wrong transport is a
  compile-time error.
- ✅ `SessionState::StartTls` variant and the `Authentication →
  StartTls → Ehlo → Authentication` transition path. The state
  machine now models RFC 3207 §4.2's mandatory re-EHLO directly.
- ✅ `SessionState` and `ProtocolError` are now `non_exhaustive`, so
  future extensions can add variants without forcing a major bump.
- ✅ `ProtocolError::ExtensionUnavailable { name }` for the case
  where `STARTTLS` is requested but not advertised.
- ✅ `SmtpOp::StartTls` so `UnexpectedCode` errors during the
  STARTTLS handshake are tagged just like `MAIL FROM` etc.
- ✅ `SmtpClient::starttls()` and `SmtpClient::connect_starttls()` —
  the explicit and convenience entry points respectively.
- ✅ `wasm-smtp-cloudflare`: `connect_starttls(host, port)` and
  `connect_smtp_starttls(host, port, ehlo_domain)`, plus a
  `StartTlsCapable` impl on `CloudflareTransport` that calls
  `worker::Socket::start_tls()` on the underlying socket.

## Phase 6 — Diagnostics & OAuth 2.0 *(complete)*

Two parallel improvements that together raise the bar on what
production deployments can rely on the crate for:

- ✅ **`ENHANCEDSTATUSCODES` (RFC 2034 / 3463).** When the server
  advertises this extension, every reply is annotated with the
  parsed `class.subject.detail` code and that code is propagated
  into both `ProtocolError::UnexpectedCode { enhanced, .. }` and
  `AuthError::Rejected { enhanced, .. }`. Callers can route on
  structured codes (e.g. `5.1.1` user-unknown vs `5.7.1` policy
  rejection) instead of grepping reply text.
- ✅ **`AUTH XOAUTH2`.** The Google / Microsoft OAuth 2.0 SASL
  profile is now supported via `SmtpClient::login_xoauth2()`, with
  full handling of RFC 7628 §3.2.3's two-step error flow (334 with
  base64 JSON detail, empty client continuation, final 5xx). Auto-
  selection in `login()` deliberately does not pick XOAUTH2 — it is
  opt-in only, since the credential semantics differ from a static
  password.
- ✅ `EnhancedStatus { class, subject, detail }` public type with
  Display, `to_dotted()`, structured field access for programmatic
  routing.
- ✅ `AuthMechanism::XOAuth2` variant; `AuthError` made
  `non_exhaustive`; reusable `validate_xoauth2_user` and
  `validate_oauth2_token` for caller-side input checking.

## Phase 7 — SMTPUTF8 *(complete)*

International-address support introduced as the project's first
feature-gated capability.

- ✅ `smtputf8` cargo feature (off by default), gating the entire
  SMTPUTF8 surface so callers who only ever submit ASCII addresses
  pay no code-size cost.
- ✅ `SmtpClient::send_mail_smtputf8(from, to, body)` for sending
  with the `SMTPUTF8` ESMTP parameter on `MAIL FROM`.
- ✅ `protocol::validate_address_utf8` — Unicode-permissive address
  validator that still rejects structural hazards (CR/LF/NUL,
  `<>`, ASCII whitespace, C0/C1 control characters).
- ✅ `protocol::ehlo_advertises_smtputf8` capability inspection
  helper.
- ✅ `protocol::format_mail_from_smtputf8` formatter.
- ✅ Feature pass-through via `wasm-smtp-cloudflare`'s `smtputf8`
  feature so adapter-only callers can enable SMTPUTF8 without
  naming the core crate directly.
- ✅ No silent fallback: requesting SMTPUTF8 against a server that
  did not advertise it returns `ProtocolError::ExtensionUnavailable
  { name: "SMTPUTF8" }` and closes the session.

## Phase 8 — Retrofit feature gates *(in progress)*

Phase 7 introduced the project's first feature flag. The same
treatment is justified for several pieces of existing functionality
that some callers will never use; gating them lets size-sensitive
consumers (Cloudflare Workers' 3 MiB cap, in particular) pick only
what they need.

Status:

- ✅ **`xoauth2`** (default-on, completed in v0.4.0). Gates
  `SmtpClient::login_xoauth2`, the `XOAuth2` arm of `login_with`,
  and the `protocol::build_xoauth2_initial_response` /
  `validate_xoauth2_user` / `validate_oauth2_token` helpers. The
  `AuthMechanism::XOAuth2` and `SmtpOp::AuthXOAuth2` enum variants
  remain present in either configuration. Default-on for backwards
  compatibility with v0.3.x; Gmail/M365-bound callers see no change,
  while transactional callers can drop XOAUTH2 entirely with
  `default-features = false`.
- ✅ **`smtputf8`** (default-off, completed in v0.4.0). Already
  gated when introduced in Phase 7.

Future candidates (not scheduled):

| Feature              | Default | Gates                                                    | Rationale                                |
|----------------------|---------|----------------------------------------------------------|------------------------------------------|
| `starttls`           | on      | `StartTlsCapable`, `SessionState::StartTls`, `connect_starttls`, `starttls()` | Most port-587 callers need it; default-on |
| `enhancedstatuscodes`| on      | `EnhancedStatus`, `Reply::enhanced`, `enhanced` field on errors | Small, broadly useful; default-on  |
| `auth-login`         | on      | `AuthMechanism::Login`, `run_auth_login`                | Legacy server compat; default-on for safety |

Each of these would be SemVer-evaluated individually before being
introduced. The above table is intentionally tentative.

## Phase 9 — Future work *(not scheduled)*

Items that may be revisited in a future cycle. None of these is a
commitment.

- Additional adapters for non-Cloudflare runtimes (tokio, Deno,
  WASI sockets).
- Extra SASL mechanisms (`SCRAM-SHA-256`, `OAUTHBEARER`).
- Pipelining (RFC 2920) for slightly better latency on high-RTT links.
- DSN extension parameters (RFC 3461) for delivery-status routing.

## Out of scope (for now)

The following are deliberately omitted from the roadmap. They may be
revisited later, but are not implied commitments.

- MIME composition or attachment building
- Bulk delivery, retry queues, rate limiting
