# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/nabbisen/wasm-smtp/commits/main
