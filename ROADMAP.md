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
`Socket` to `wasm-smtp`.

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
- ✅ Worked end-to-end usage examples (`docs/src/reference/examples.md`):
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

## Phase 8 — Retrofit feature gates *(complete)*

Phase 7 introduced the project's first feature flag. The same
treatment is justified for several pieces of existing functionality
that some callers will never use; gating them lets size-sensitive
consumers (Cloudflare Workers' 3 MiB cap, in particular) pick only
what they need.

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

Future feature-gate candidates (not scheduled):

| Feature              | Default | Gates                                                    | Rationale                                |
|----------------------|---------|----------------------------------------------------------|------------------------------------------|
| `starttls`           | on      | `StartTlsCapable`, `SessionState::StartTls`, `connect_starttls`, `starttls()` | Most port-587 callers need it; default-on |
| `enhancedstatuscodes`| on      | `EnhancedStatus`, `Reply::enhanced`, `enhanced` field on errors | Small, broadly useful; default-on  |
| `auth-login`         | on      | `AuthMechanism::Login`, `run_auth_login`                | Legacy server compat; default-on for safety |

Each of these would be SemVer-evaluated individually before being
introduced. The above table is intentionally tentative.

## Phase 9 — Security hardening *(complete)*

Internal security audit (v0.4.0) covering the SMTP, WASM, and
internet-security threat surfaces produced eight findings, all
rated low or medium. The Phase 9 release (v0.5.0) addresses them.
None of the findings indicated existing exploitable conditions; the
work is precautionary defence-in-depth.

- ✅ **STARTTLS injection (RFC 3207 §5).** New
  `ProtocolError::StartTlsBufferResidue { byte_count }` variant.
  At the moment of TLS upgrade, `starttls()` checks the receive
  buffer for unread bytes; any residue causes the upgrade to be
  refused and the session closed. Defends against a class of
  injection attacks where pipelined plaintext commands persist
  across the upgrade boundary.
- ✅ **RFC 5321 §4.5.3.1 length limits.** `validate_address` and
  `validate_address_utf8` now enforce the 254-octet path limit, the
  64-octet local-part limit, and the 255-octet domain limit. New
  public constants `MAX_ADDRESS_LEN`, `MAX_LOCAL_PART_LEN`,
  `MAX_DOMAIN_LEN`.
- ✅ **`validate_login_*` realiased.** Now thin aliases for
  `validate_plain_*`, picking up the NUL-byte check the original
  helpers were missing. Source-compatible for v0.4.x callers.
- ✅ **Documentation: log injection / PII echo.** `Reply::joined_text`
  and `SmtpError` document that returned text may contain `\n` or
  server-supplied envelope addresses; suggest structured-field
  logging.
- ✅ **Documentation: credential lifetime.** `SmtpClient::login`
  documents that the crate retains no credential bytes after the
  call, and points callers at `zeroize` for caller-side memory
  hygiene.
- ✅ **Documentation: `Transport` security responsibilities.**
  `Transport` trait spells out certificate validation, hostname
  matching, and no-fallback handshake-failure semantics as
  implementor responsibilities.
- ✅ **Documentation: body size.** `SmtpClient::send_mail`
  documents that the crate imposes no `body.len()` limit, and
  recommends caller-side caps consistent with `SIZE` advertisement.

## Phase 11 — Tokio adapter & composition guidance *(complete)*

Closes the long-standing "tokio-based servers must hand-roll their
`Transport`" gap and answers the recurring "where do I build the
message body?" question.

- ✅ **`wasm-smtp-tokio` adapter crate (v0.7.0).** Production-quality
  `tokio` + `tokio-rustls` adapter. Sibling to `wasm-smtp-cloudflare`.
  - `TokioTlsTransport::connect_implicit_tls(host, port, sni)` for
    port-465 implicit-TLS submission.
  - `TokioPlainTransport::connect(host, port, sni)` paired with
    `SmtpClient::connect_starttls(...)` for port-587 STARTTLS.
  - `ConnectOptions` builder for alternate SNI, custom root stores
    (private CA / dev-only self-signed), and ALPN.
  - Two cargo features for trust-anchor source: `native-roots`
    (default) and `webpki-roots`. Mutually exclusive.
  - **No public API to disable certificate verification.**
    Test/dev convenience is supplied via custom root stores.
- ✅ **Composition guidance (`docs/src/core/composing-messages.md`).**
  After evaluating the request to ship a `wasm-smtp-message`
  sibling crate, the decision was to **not build it** —
  `mail-builder` (Stalwart Labs, no required deps, RFC 5322 +
  full MIME) already fills the niche, and a wrapper would either
  duplicate effort or add no value. The new chapter explains the
  recommended `mail-builder` integration, covers the typical
  pitfalls (CRLF normalization, RFC 2047 encoded-word, dot-
  stuffing responsibilities, header injection), and points to
  `lettre::message` and Stalwart's other mail crates for
  comparison.

## Phase 12 — Future work *(complete)*

Items that may be revisited in a future cycle. Some have already been
delivered; the rest are not commitments.

### Delivered

- ✅ **`wasm_smtp::IoError` source chain** (v0.7.1). New
  `IoError::with_source(message, source)` constructor and a
  `From<std::io::Error>` conversion let adapters preserve the
  underlying `io::Error` / TLS handshake error etc. as the
  `std::error::Error::source` chain. `wasm-smtp-tokio` adopted
  the new API in the same release. Backwards-compatible: the
  existing `IoError::new` continues to work and produces an
  `IoError` with no source.
- ✅ **`mail-builder` integration helper** (v0.8.0). New
  `SmtpClient::send_message` method behind the `mail-builder`
  cargo feature accepts `mail_builder::MessageBuilder` directly,
  saving the manual `write_to_string()?` step. Off by default;
  `mail-builder` is not pulled into the dependency graph unless
  the feature is enabled.
- ✅ **Connection reuse documentation** (v0.8.0). New chapter
  `docs/src/core/connection-reuse.md` documents the existing
  multi-message-per-connection pattern (state persistence, idle
  timeouts, retry semantics, intentional absence of a built-in
  connection pool). Code changes: none — the support has been
  there since Phase 1.
- ✅ **Crypto provider switching for the Tokio adapter** (v0.8.0).
  `wasm-smtp-tokio` now exposes mutually-exclusive `aws-lc-rs`
  (default) and `ring` cargo features so callers can choose
  between performance/FIPS (aws-lc-rs) and fast-build/zero-C
  (ring). Misconfiguration is caught at build time via
  `compile_error!`.
- ✅ **`AUTH SCRAM-SHA-256`** (v0.9.0). RFC 5802 / RFC 7677
  challenge-response SASL: the password never crosses the wire,
  even encrypted. Implements PBKDF2-HMAC-SHA-256 key derivation,
  the four-message client-first/server-first/client-final/
  server-final exchange, server-signature verification (constant
  time via `subtle`), iteration count clamping for DoS defense,
  and replay defense via the nonce-prefix check. Behind the
  default-on `scram-sha-256` cargo feature. The
  `select_auth_mechanism` helper now prefers SCRAM over PLAIN
  over LOGIN, so existing `login()` callers automatically benefit
  on servers that advertise it.

  Out of scope: `SCRAM-SHA-256-PLUS` (channel binding) requires
  per-connection binding tokens from the TLS layer that the
  current `Transport` contract does not expose. SCRAM-SHA-1 and
  `SASLprep` normalization are also not implemented; SHA-1 is
  obsolete and SASLprep matters only for non-ASCII credentials.

### Not yet scheduled

- Additional adapters for non-tokio runtimes (Deno, WASI sockets).
- `OAUTHBEARER` (RFC 7628) — the IETF-standard OAuth 2.0 SASL
  mechanism, complementing the ad-hoc `XOAUTH2` already supported.
- `SCRAM-SHA-256-PLUS` (channel binding) once a clean way to
  surface TLS binding tokens through the `Transport` contract is
  identified.
- Pipelining (RFC 2920) for slightly better latency on high-RTT links.
- DSN extension parameters (RFC 3461) for delivery-status routing.

## Phase 13 — RFC governance and anti-abuse hardening *(complete)*

Introduces RFC-based design governance, extracts the test transport into
a dedicated crate, and adds two new core features gated by the extension
development plan.

- ✅ **RFC lifecycle** (v0.10.0). 24 design documents across `rfcs/`
  with a 5-folder lifecycle (`draft/`, `proposed/`, `accepted/`, `done/`,
  `archive/`). RFC 000 (policy) through RFC 023 (browser secrets) cover
  the full extension roadmap.
- ✅ **`wasm-smtp-test` crate** (v0.10.0). `MockTransport`, `block_on`,
  and `flatten` extracted from the core's `#[cfg(test)]` harness into a
  standalone dev-dependency crate usable by downstream projects.
- ✅ **`SendPolicy` hook** (v0.10.0). Pre-send validation via the
  `SendPolicy` trait. Ships with `DefaultPolicy` (allow all) and
  `BoundedPolicy` (max-recipients, max-size). Rejections surface as the
  new `SmtpError::Policy(PolicyError)` variant.
- ✅ **Audit event model** (v0.10.0). `AuditSink` trait and
  `SmtpAuditEvent` enum. Events: `Connected`, `GreetingReceived`,
  `EhloCompleted`, `TlsUpgraded`, `AuthCompleted`, `MailFromAccepted`,
  `RecipientAccepted`, `RecipientRejected`, `MessageAccepted`,
  `QuitCompleted`, `SessionAborted`. Ships with `NoopAuditSink` and
  `VecAuditSink`. No credentials or message body in any event.
- ✅ **`SmtpClientOptions` builder** (v0.10.0). Attaches a policy and/or
  audit sink before connecting via `SmtpClient::connect_with`.

## Phase 14 — Byte-slice send and large-message hardening *(complete)*

- ✅ **`send_mail_bytes`** (v0.11.0). Accepts `&[u8]` instead of `&str`,
  bypassing the UTF-8 validity check while applying the same dot-stuffing,
  policy checks, and audit events as `send_mail`. Useful for pre-serialised
  payloads from `mail-builder` and for messages with non-UTF-8 octets.
- ✅ **Large-message and dot-stuffing stress tests** (v0.11.0). 100 KB,
  1 MB, and (opt-in) 10 MB body tests; 10 000-line dot-heavy body test;
  10-recipient test; max-line-length (998 bytes) test. All use
  `send_mail_bytes` and verify on-wire output. RFC 020 complete.
- ✅ **`no_std` feasibility study** (v0.11.0). Audited all `std::` usages
  in the core. Recommendation: **defer** — no concrete IoT use case yet,
  WASI adapter (RFC 016) is the better path, and streaming DATA (RFC 019
  Phase 3) is a prerequisite. RFC 021 complete.

## Phase 15 — WASI adapter *(complete)*

- ✅ **RFC 017 resolved** (v0.12.0): TLS Strategy A — rustls 0.23 + ring +
  webpki-roots over WASI streams. Certificate validation enforced; no API to
  disable it.
- ✅ **`wasm-smtp-wasi` crate** (v0.12.0):
  - `WasiTlsTransport` implementing `Transport` and `StartTlsCapable`.
  - DNS resolution via `wasi:sockets/ip-name-lookup`.
  - TCP connect via `wasi:sockets/tcp`.
  - TLS handshake via rustls `StreamOwned<ClientConnection, WasiStreamIo>`.
  - `connect_smtps` (implicit TLS) and `connect_smtp_starttls` (STARTTLS).
  - `ConnectOptions` builder: SNI override, custom root store, ALPN.
  - `plaintext-only` feature for TLS-offload deployments.
  - 9 native-host tests (no WASI runtime required for `cargo test`).
- ✅ Target: `wasm32-wasip2`.

## Phase 16 — Streaming DATA *(complete)*

- ✅ **`DotStufferState`** (v0.13.0). Streaming dot-stuffer state machine.
  Tracks `at_line_start`, `prev`, and `prev_prev` across chunk boundaries.
  `process_chunk(&[u8])` + `finish()` API. Verified to match the batch
  `dot_stuff_and_terminate` output byte-for-byte, including the
  byte-by-byte chunk boundary case.
- ✅ **`MessageBody` trait** (v0.13.0). Runtime-independent streaming
  body source. Built-in: `SliceBody<'a>`, `StrBody<'a>`.
- ✅ **`SmtpClient::send_mail_stream`** (v0.13.0). Reads body in 8 KB
  chunks. Full policy + audit integration. Wire output identical to
  `send_mail` / `send_mail_bytes` for the same content.

## Phase 17 — Component Model WIT interface *(complete)*

- ✅ **`wit/smtp.wit`** (v0.14.0). Language-neutral WIT interface.
  Defines `smtp-config`, `smtp-credentials`, `smtp-message`,
  `send-result`, `send-error`, and the `send` function. Compatible
  with jco (TypeScript), wit-bindgen-go (Go), componentize-py (Python).
- ✅ **`wasm-smtp-component` crate** (v0.14.0). Rust WIT implementation
  wrapping `wasm-smtp-wasi`. Uses `wit-bindgen 0.57` on `wasm32-wasip2`.
  5 native-host tests (no WASM runtime required for `cargo test`).
- Build: `cargo component build --target wasm32-wasip2 -p wasm-smtp-component`

## Out of scope (for now)

The following are deliberately omitted from the roadmap. They may be
revisited later, but are not implied commitments.

- MIME composition or attachment building (use `mail-builder`; see
  `docs/src/core/composing-messages.md`).
- Bulk delivery, retry queues, rate limiting.
