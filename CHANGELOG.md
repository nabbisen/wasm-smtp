# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.1] — 2026-04-29

### Added

- **`IoError` source chain support (Phase 12).** Adapter crates can
  now preserve the underlying `io::Error`, rustls handshake error,
  etc. as the [`std::error::Error::source`] chain on
  [`wasm_smtp::IoError`]. Caller-side error formatters (anyhow's
  `{:#}`, eyre, manual `.source()` walks) see the full diagnostic
  while the high-level `Display` of `IoError` stays terse.
  - New constructor: `IoError::with_source(message, source)` accepts
    any `StdError + Send + Sync + 'static`.
  - New `From<std::io::Error> for IoError` conversion: `io_err.into()`
    produces an `IoError` carrying the original as its source.
  - `IoError` is now `Send + Sync` (its source field is
    `Box<dyn Error + Send + Sync>`), important for tokio-based
    adapters where errors may surface on a different worker
    thread than the one that observed them.
  - 6 new unit tests in `error_tests.rs` covering the new
    constructor, the `From<io::Error>` conversion, source-chain
    walking through `SmtpError → IoError → io::Error`, and the
    `Send + Sync` bounds at compile time.

### Changed

- **`wasm-smtp-tokio` adapter now preserves `io::Error` source.**
  The internal `map_io_err` helper switched from
  `IoError::new(static_context)` to
  `IoError::with_source(static_context, io_err)`, propagating the
  underlying TCP / TLS / handshake error into the source chain.
  No public API or behaviour change for callers using the adapter
  through its public API; the change is visible via `.source()`
  walks.
- **No changes to `IoError::new` or `IoError::message`.** Adapters
  not yet migrated to `with_source` continue to compile and behave
  as before — `new()` simply produces an `IoError` with no source.

### Documentation

- The `IoError` rustdoc carries a worked example showing how an
  adapter preserves an `io::Error` through the source chain.
- ROADMAP Phase 12 reflects this item as complete.

[`std::error::Error::source`]: https://doc.rust-lang.org/std/error/trait.Error.html#method.source
[`wasm_smtp::IoError`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/struct.IoError.html

## [0.7.0] — 2026-04-29

### Added (Phase 11 — `wasm-smtp-tokio` adapter crate)

- **New sibling crate: `wasm-smtp-tokio`.** A production-quality
  `Transport` implementation for tokio + rustls, parallel to
  `wasm-smtp-cloudflare`. Lets axum / actix / warp / hyper / plain
  tokio servers connect to SMTP submission endpoints without writing
  the rustls + `tokio_rustls::TlsConnector` plumbing themselves.
  - `TokioTlsTransport::connect_implicit_tls(host, port, sni)` for
    implicit-TLS submission (port 465).
  - `TokioPlainTransport::connect(host, port, sni)` followed by
    `SmtpClient::connect_starttls(...)` for STARTTLS submission
    (port 587).
  - `ConnectOptions` builder for advanced cases: alternate SNI,
    custom root store (private CA, dev-only self-signed certs), ALPN.
  - Two cargo features for trust-anchor source: `native-roots`
    (default; `rustls-native-certs`) and `webpki-roots` (bundled
    Mozilla root set; for minimal/distroless containers). Mutually
    exclusive — pick one. Pass-through `xoauth2` and `smtputf8`
    features mirror the main crate.
  - Certificate validation is on by default and there is **no**
    public API to disable verification. Callers needing test
    convenience install a self-signed CA via
    `ConnectOptions::with_root_store`.
  - 9 unit tests covering builder ergonomics, error paths
    (unbound port, plaintext-server-on-TLS-port, invalid SNI), and
    pre/post-upgrade lifecycle.
  - 3 docstring examples in `lib.rs` (implicit TLS, STARTTLS,
    custom options).

### Changed

- **Workspace bumped 0.6.0 → 0.7.0.** Pure SemVer would not require
  a bump — adding a new crate at the same version line is
  technically additive. This release uses the bump anyway because
  it adds new top-level entries to `[workspace.dependencies]`
  (`tokio-rustls`, `rustls-pki-types`, `rustls-native-certs`,
  `webpki-roots`) which downstream lockfile-less builds will now
  resolve, and bumping to 0.7.0 makes that visible.
- `crates/cloudflare/Cargo.toml`'s pin on `wasm-smtp` is now
  `version = "0.7.0"`.
- Pure non-functional update for the `wasm-smtp` and
  `wasm-smtp-cloudflare` crates themselves — the bump tracks the
  workspace, but no public API or behaviour has changed in either.

### Documentation

- New `docs/src/composing-messages.md` chapter explaining the
  recommended path for callers who need to construct RFC 5322 / MIME
  message bodies before passing them to `SmtpClient::send_mail`.
  Recommends [`mail-builder`] as the composition partner — actively
  maintained by Stalwart Labs, no dependencies, RFC 5322 + RFC
  2045-2049 + automatic encoding selection. Covers the typical
  notification-email pattern, HTML + multipart, non-ASCII subjects
  (RFC 2047), the SMTPUTF8 capability interaction, and dot-stuffing
  responsibilities.
- Decision: **`wasm-smtp-message` will not be built.** The
  ecosystem already has `mail-builder` in this niche; rebuilding
  it would produce either a thin wrapper (no value) or a duplicate
  (continuing-maintenance cost). The new chapter documents the
  integration path instead.

[`mail-builder`]: https://docs.rs/mail-builder

## [0.6.0] — 2026-04-28

### Changed (breaking)

- **Crate renamed: `wasm-smtp-core` → `wasm-smtp`.** This crate is the
  main, externally-facing library — the public API
  (`SmtpClient`, `Transport`, `Reply`, the error types, etc.) lives
  here, and direct dependents (non-Cloudflare adapters, custom
  `Transport` implementations, host-tooling tests) all consume it
  directly. Calling it `-core` was an artifact of the early workspace
  layout and gave the misleading impression that `wasm-smtp-core` and
  `wasm-smtp-cloudflare` were peer-tier crates. They are not:
  `wasm-smtp` is the main library, `wasm-smtp-cloudflare` is one
  adapter for it. The rename makes that hierarchy obvious in the
  Rust-ecosystem-conventional way (`serde` / `serde_json`,
  `tokio` / `tokio-util`, `tracing` / `tracing-subscriber`, …).

  **Migration for callers depending on the main crate:**

  ```toml
  # before
  [dependencies]
  wasm-smtp-core = "0.5"

  # after
  [dependencies]
  wasm-smtp = "0.6"
  ```

  ```rust
  // before
  use wasm_smtp_core::{SmtpClient, Transport, SmtpError};

  // after
  use wasm_smtp::{SmtpClient, Transport, SmtpError};
  ```

  Callers depending only on `wasm-smtp-cloudflare` need no source
  changes — the adapter re-exports the public API of `wasm-smtp`
  exactly as before.

- **Workspace member directory renamed: `crates/core/` → `crates/wasm-smtp/`.**
  This is internal — it does not affect any package on crates.io —
  but it keeps the directory name consistent with the package name.

- **Workspace version bumped 0.5.1 → 0.6.0.** Pure Rust SemVer would
  not require a major (0.x) bump for a crate-name change because it
  is technically a different package. This release uses the bump
  anyway to make the discontinuity unmissable in dependency
  resolution: a caller upgrading mechanically will fail to find
  `wasm-smtp-core 0.6` and will see the failure immediately rather
  than silently picking up an unrelated 0.5.x.

### Notes

- All historical changelog entries below this section reference
  `wasm-smtp-core` as the crate's name at the time those releases
  shipped; that text is preserved as historical record.
- ROADMAP, README, and the `docs/` book have all been updated to use
  the new name.

## [0.5.1] — 2026-04-28

### Changed (Phase 10 — test-suite layout & dependency hygiene)

This is a non-functional refactor: no public API changes, no behaviour
changes.

- **In-tree test files split.** The 2,950-line `crates/core/src/tests.rs`
  has been split into a `crates/core/src/tests/` directory with one
  file per sub-module (`harness.rs`, `protocol_tests.rs`,
  `session_tests.rs`, `error_tests.rs`, `client_tests.rs`,
  `smtputf8_tests.rs`). The previous in-file sub-module structure is
  preserved exactly; only the physical file boundaries have changed.
  `crates/cloudflare/src/tests.rs` (305 lines) gets the same treatment
  for consistency: `tests/io_tests.rs` and
  `tests/e2e_via_tokio_mock.rs`. Tests remain in-tree (rather than
  being moved to a top-level `tests/` integration-test directory) so
  they can continue to reach `pub(crate)` items and module-private
  helpers without inflating the public API surface.
- **Centralised dependency floors via `[workspace.dependencies]`.**
  `tokio`, `tokio-test`, and `worker` are now declared at the
  workspace root with explicit minimum versions (`tokio >= 1.38`,
  `tokio-test >= 0.4.3`, `worker = 0.8`). Member crates inherit them
  with `{ workspace = true, features = [..] }`. The previous bare
  `tokio = "1"` would, in adversarial resolution scenarios, allow
  selecting tokio < 1.23.1, which is the patched floor for
  RUSTSEC-2023-0001 (`reject_remote_clients` configuration corruption
  on Windows named pipes). This crate does not use the affected
  `tokio::net::windows::named_pipe` API at all (we run on WASM /
  Cloudflare Workers, with `tokio` enabled only for `io-util`), so
  the floor is precautionary defence-in-depth — but it documents the
  intent and protects callers depending on lockfile-less builds.

## [0.5.0] — 2026-04-28

### Added (security hardening — Phase 9)

This release is a security-focused minor bump. Following an internal
audit, eight findings were addressed across the SMTP, WASM, and
general internet-security threat surfaces. None of the findings were
rated critical or high; the changes below collectively raise the
defensive posture of the crate.

- **STARTTLS injection defense (RFC 3207 §5).** The new
  `ProtocolError::StartTlsBufferResidue { byte_count }` variant is
  raised when bytes remain in the receive buffer at the moment of
  TLS upgrade. This is the signature of a CVE-2011-1575-class
  attack: an attacker pipelines additional SMTP commands onto the
  plaintext channel after the `220` reply, hoping the client will
  read them after the upgrade and treat them as authenticated
  post-TLS traffic. `SmtpClient::starttls` and
  `SmtpClient::connect_starttls` now refuse to upgrade and close
  the session in this case rather than silently absorbing the
  injected bytes.
- **RFC 5321 length limits enforced in address validation.**
  `validate_address` and `validate_address_utf8` now reject:
  - addresses longer than 254 octets total (§4.5.3.1.3),
  - local-parts longer than 64 octets (§4.5.3.1.1), and
  - domains longer than 255 octets (§4.5.3.1.2).

  Three new public constants — `MAX_ADDRESS_LEN`,
  `MAX_LOCAL_PART_LEN`, `MAX_DOMAIN_LEN` — expose these values for
  callers that want to validate before invocation.
- **`validate_login_username` / `validate_login_password` are now
  thin aliases for the corresponding `validate_plain_*` functions.**
  Previously they performed only an empty-string check, which would
  accept NUL bytes and other characters that corrupt SASL framing
  on the post-base64 server side. The aliases preserve source
  compatibility for v0.4.x callers; new code should call the
  `validate_plain_*` functions directly.

### Documentation

- `Reply::joined_text` documents that the returned text may contain
  `\n`, with explicit guidance for log-handler implementors to
  escape newlines and avoid log injection.
- `SmtpError`'s top-level doc carries a "Logging caveat" section
  explaining that `Display` output embeds server reply text, which
  may include envelope addresses or other PII. Suggests structured-
  field logging instead.
- `SmtpClient::login` documents that the crate does not retain
  credentials after the call, with a "Credential lifetime and
  zeroization" section pointing callers to the `zeroize` crate for
  caller-side memory hygiene.
- `Transport` trait gains a "Security responsibilities of
  implementors" section explicitly requiring certificate-chain
  validation, hostname matching, and no-fallback handshake failure
  semantics. Aimed at out-of-tree adapter authors.
- `SmtpClient::send_mail` carries a "Body size" note: the crate
  does not impose a body-length limit; callers should enforce
  application-appropriate caps and respect any `SIZE` advertised
  by the server (RFC 1870).

### Changed

- `ProtocolError` is `non_exhaustive`, so adding the
  `StartTlsBufferResidue` variant is not a SemVer-breaking change
  for callers using a wildcard arm in their pattern matches.
- The MockTransport test harness's `with_starttls` constructor now
  takes separate `pre_chunks` / `post_chunks` parameters, modelling
  real-server behavior where post-TLS reply bytes are not delivered
  on the plaintext channel. This affects internal tests only;
  callers do not depend on `MockTransport`.

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

[Unreleased]: https://github.com/nabbisen/wasm-smtp/compare/v0.7.1...HEAD
[0.7.1]: https://github.com/nabbisen/wasm-smtp/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/nabbisen/wasm-smtp/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/nabbisen/wasm-smtp/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/nabbisen/wasm-smtp/releases/tag/v0.1.0
