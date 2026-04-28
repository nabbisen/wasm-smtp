# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] — 2026-04-27

### Added

- **Phase 7 — `SMTPUTF8` (RFC 6531) — feature-gated.**
  - New `smtputf8` cargo feature, **off by default**. The crate's
    first feature flag, intended primarily to keep WASM bundle size
    down for the common case of ASCII-only submission.
  - `SmtpClient::send_mail_smtputf8(from, to, body)` for sending
    with the `SMTPUTF8` ESMTP parameter on `MAIL FROM`. Available
    only when the feature is enabled.
  - `protocol::validate_address_utf8` — Unicode-permissive address
    validator. Rejects only structural hazards: CR/LF/NUL, ASCII
    `<`/`>`, ASCII whitespace, ASCII control characters
    (C0 + DEL), and C1 control characters (U+0080-U+009F).
    Everything else, including non-Latin scripts and IDEOGRAPHIC
    SPACE U+3000, is accepted.
  - `protocol::ehlo_advertises_smtputf8` capability inspection.
  - `protocol::format_mail_from_smtputf8` for the `MAIL FROM:<addr>
    SMTPUTF8\r\n` wire form.
  - `wasm-smtp-cloudflare` exposes a matching `smtputf8` feature
    that pass-through-enables it in `wasm-smtp-core`, so adapter-
    only callers do not need to depend on the core crate by name
    to opt in.
  - No silent fallback: if the server does not advertise
    `SMTPUTF8`, `send_mail_smtputf8` returns
    `ProtocolError::ExtensionUnavailable { name: "SMTPUTF8" }` and
    closes the session.
- **Phase 8 — `xoauth2` cargo feature (default-on).**
  - `SmtpClient::login_xoauth2`, the `XOAuth2` arm of `login_with`,
    and the `protocol::build_xoauth2_initial_response` /
    `validate_xoauth2_user` / `validate_oauth2_token` helpers are
    now gated behind the new `xoauth2` cargo feature (default-on).
    Callers that do not authenticate via Gmail / Microsoft 365 OAuth
    can disable this feature with `default-features = false` to
    drop roughly 250 LOC of protocol code from their WASM bundle.
  - The `AuthMechanism::XOAuth2` and `SmtpOp::AuthXOAuth2` enum
    variants remain present in either configuration. Both enums are
    `non_exhaustive`, so the default-on→opt-in transition is not a
    SemVer-breaking change.
  - When the feature is disabled, calling `login_with(XOAuth2, ..)`
    or `login_xoauth2` fails fast with a clear `InvalidInputError`
    rather than a confusing "not advertised" mechanism error.
  - `AuthError::UnsupportedMechanism`'s Display message now
    reflects the active feature configuration (mentions XOAUTH2
    when the feature is enabled, omits it otherwise).

### Changed

- `validate_address`'s doc comment now explicitly notes that UTF-8
  addresses require the `smtputf8` feature. The function's behavior
  is unchanged from v0.3.0.

## [0.3.0] — 2026-04-27

### Added

- **Phase 6 — `ENHANCEDSTATUSCODES` (RFC 2034 / 3463).**
  - New public type `EnhancedStatus { class, subject, detail }` with
    `Display`, `to_dotted()`, and structured field access.
  - `ProtocolError::UnexpectedCode` gains an `enhanced:
    Option<EnhancedStatus>` field. The Display impl renders the code
    in square brackets between the basic code and the message:
    `during MAIL FROM, expected 2xx response but received 550
    [5.7.1]: relay access denied`.
  - `AuthError::Rejected` gains an `enhanced: Option<EnhancedStatus>`
    field, so callers can distinguish (e.g.) `5.7.8` from `5.7.9`
    without parsing reply text.
  - `Reply::enhanced()` and `Reply::message_text()` (the latter
    returns the reply text with the enhanced prefix stripped, for
    human-friendly display).
  - `protocol::ehlo_advertises_enhanced_status_codes` capability
    inspection helper.
  - Parsing is gated on EHLO advertisement: a stray
    `class.subject.detail`-shaped substring in a reply from a server
    that did not advertise the extension is not parsed.
- **Phase 6 — `AUTH XOAUTH2` (Google / Microsoft OAuth 2.0).**
  - `SmtpClient::login_xoauth2(user, access_token)` for opt-in
    OAuth 2.0 bearer-token authentication.
  - `AuthMechanism::XOAuth2` variant, `SmtpOp::AuthXOAuth2` for
    error tagging.
  - Full handling of the RFC 7628 §3.2.3 two-step error flow: on a
    `334` reply during XOAUTH2, the client sends an empty
    continuation line, reads the final 5xx, and surfaces it as
    `AuthError::Rejected` with the provider's diagnostic preserved.
  - `protocol::build_xoauth2_initial_response`,
    `protocol::validate_xoauth2_user`,
    `protocol::validate_oauth2_token` public helpers.
  - `select_auth_mechanism` deliberately does NOT pick XOAUTH2 even
    when advertised: bearer tokens have different semantics from
    static passwords and must be passed in explicitly.

### Changed

- `AuthError` is now `non_exhaustive`. This is a SemVer-incompatible
  change for callers that pattern-match on the enum without a
  wildcard arm, hence the minor bump from 0.2.0 to 0.3.0 under the
  pre-1.0 versioning convention.
- `AuthError::UnsupportedMechanism`'s Display message now lists all
  three supported mechanisms (PLAIN, LOGIN, XOAUTH2) rather than
  just the two it covered before.
- `Reply` now has a private `enhanced` field; constructed via
  `Reply::new(code, lines)` rather than struct literal. External
  callers that built `Reply` directly (an unusual pattern, but
  technically possible) will need to switch to the constructor.

## [0.2.0] — 2026-04-27

### Added

- **Phase 5 — STARTTLS support (RFC 3207).**
  - New `StartTlsCapable: Transport` trait for transports that can be
    upgraded to TLS in-place. Transports that connect with Implicit
    TLS (port 465) need not implement it.
  - `SmtpClient::starttls(&mut self)` — explicit STARTTLS upgrade on
    a connected client.
  - `SmtpClient::connect_starttls(transport, ehlo_domain)` —
    convenience entry point that performs greeting, `EHLO`,
    `STARTTLS`, transport upgrade, and re-`EHLO` per RFC 3207 §4.2
    in a single call.
  - `SessionState::StartTls` variant to model the
    `Authentication → StartTls → Ehlo` transition.
  - `ProtocolError::ExtensionUnavailable { name: &'static str }` for
    the case where `STARTTLS` was requested but not advertised.
  - `SmtpOp::StartTls` so protocol errors during the upgrade
    handshake are tagged like every other SMTP step.
  - `protocol::ehlo_advertises_starttls` capability inspection helper.
- **`wasm-smtp-cloudflare`:**
  - `connect_starttls(host, port)` — open a plaintext socket
    pre-configured for in-place TLS upgrade
    (`SecureTransport::StartTls`).
  - `connect_smtp_starttls(host, port, ehlo_domain)` — one-call
    STARTTLS connect, greeting, `EHLO`, upgrade, re-`EHLO`.
  - `StartTlsCapable` impl on `CloudflareTransport` that drives the
    `worker::Socket::start_tls()` consume-and-replace upgrade.

### Changed

- `SessionState` and `ProtocolError` are now `non_exhaustive`. This is
  a SemVer-incompatible change for callers that pattern-match on these
  enums without a wildcard arm — hence the minor bump from 0.1.0 to
  0.2.0 under the pre-1.0 versioning convention. Callers using
  `match … { … _ => … }` are unaffected.
- `CloudflareTransport`'s inner socket is now held in an `Option`
  internally so that `Socket::start_tls()` (which consumes `self`)
  can be called from a `&mut self` method. `into_inner()` now
  returns `Option<Socket>`. The change is invisible to read/write
  code paths.

## [0.1.0] — 2026-04-27

### Added

- `wasm-smtp-core` v0.1.0: SMTP state machine, response parser, command
  formatter, dot-stuffing, base64 helper, and `AUTH LOGIN` flow.
- `Transport` async trait as the only I/O contract.
- Error taxonomy: `IoError`, `ProtocolError`, `AuthError`,
  `InvalidInputError`.
- `SessionState` with an explicit transition table.
- In-tree synchronous mock transport for unit and integration tests.
- `wasm-smtp-cloudflare` v0.1.0: Cloudflare Workers socket adapter.
  - `CloudflareTransport` wrapping `worker::Socket`.
  - `connect_implicit_tls(host, port)` — Implicit TLS on the caller's
    port (typically 465) via `SecureTransport::On`.
  - `connect_smtps(host, port, ehlo_domain)` — one-call connect,
    greeting, and `EHLO` returning a ready-to-use client.
  - Adapter-level unit tests against `tokio_test::io::Builder`,
    including a full authenticated SMTP transaction over the mock.
- **Phase 4 hardening:**
  - `AUTH PLAIN` (RFC 4616) using the initial-response form (one
    round-trip).
  - `AuthMechanism` enum, re-exported at the crate root.
  - `SmtpClient::login_with(mechanism, user, pass)` for explicit
    mechanism selection.
  - `protocol::select_auth_mechanism` and
    `protocol::build_auth_plain_initial_response` public helpers.
  - `protocol::validate_plain_username` /
    `protocol::validate_plain_password` reject NUL bytes that would
    corrupt SASL framing.
  - `AuthError::UnsupportedMechanism` Display now lists the supported
    mechanisms (`PLAIN` and `LOGIN`) so operators can diagnose
    incompatibilities directly from the error message.
  - `SmtpOp` enum and a new `during: SmtpOp` field on
    `ProtocolError::UnexpectedCode`. Errors now identify the exact
    SMTP step that failed: "during MAIL FROM, expected 2xx response
    but received 550: …" rather than just "550".
  - Worked `examples.md` covering contact-form delivery, transactional
    alerts, multi-recipient messages, and connection reuse.
- Project-level `ROADMAP`, `TERMS_OF_USE`, `NOTICE`, GitHub policy
  documents (`SECURITY`, `CODE_OF_CONDUCT`, `CONTRIBUTING`,
  `ISSUE_TEMPLATE`).
- Long-form documentation under `docs/src` (mdBook-ready structure).

### Changed

- `SmtpClient::login` now auto-selects the best mechanism advertised
  by the server, preferring `PLAIN` over `LOGIN`. Servers that
  advertise only `LOGIN` continue to work unchanged.

[Unreleased]: https://github.com/nabbisen/wasm-smtp/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/wasm-smtp/releases/tag/v0.1.0
