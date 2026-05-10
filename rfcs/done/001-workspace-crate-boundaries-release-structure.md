# RFC 001 — Workspace, crate boundaries, and release structure

**Status.** Implemented (v0.10.0)
**Priority.** P0
**Tracks.** Project foundation / Release / Workspace
**Touches.** `Cargo.toml`, `crates/`, `crates/core/`, `crates/cloudflare/`, `crates/test/`, `README.md`, `ROADMAP.md`, `CHANGELOG.md`

## Summary

This RFC defines the workspace structure, crate boundaries, and release
conventions for `wasm-smtp` starting from `v0.10.0`. It formalises the
layered-adapter architecture that was informally established in
earlier releases and records the rationale for each boundary decision so
that future contributors do not relitigate settled questions.

## Motivation

`wasm-smtp` reached `v0.9.4` through an iterative delivery model that
focused on shipping working code quickly. The workspace layout, crate
responsibilities, and release naming conventions accumulated pragmatic
decisions that were never written down. The introduction of the RFC
process (RFC 002) makes this the right moment to crystallise the
architecture in a single authoritative document.

`v0.10.0` is the planned restructuring release. It may or may not ship
as a distinct crate-renamed release; the important outcome is that the
boundaries described here become the canonical reference regardless of
whether the workspace directory names change in a single step or
incrementally.

## Goals

- Define which crate owns each concern.
- Record what each crate is explicitly forbidden from doing.
- Establish release-artifact naming and versioning conventions.
- Give implementers an unambiguous answer to "where does this code go?"

## Non-goals

- Describing the SMTP protocol implementation in detail (see RFC 003–006).
- Specifying the Cloudflare socket implementation (RFC 013).
- Specifying the WASI adapter (RFC 016).
- Defining MIME composition (out of scope for this project).

## Design

### Workspace members

The workspace contains the following crates:

| Crate path | Published name | Role |
|---|---|---|
| `crates/wasm-smtp` | `wasm-smtp` | Protocol core — no I/O |
| `crates/wasm-smtp-cloudflare` | `wasm-smtp-cloudflare` | Cloudflare Workers adapter |
| `crates/wasm-smtp-tokio` | `wasm-smtp-tokio` | tokio + rustls adapter |
| `crates/wasm-smtp-test` | (dev-only, not published) | Test transport and fixtures |

`wasm-smtp-test` is a dev-dependency only; it is not published to
crates.io. It provides scripted-response transports and assertion
helpers that `wasm-smtp`'s own test suite and adapter test suites use.

Future crates (`wasm-smtp-wasi`, `wasm-smtp-direct-sockets`) are
anticipated but not yet present. When added they follow the same
pattern: thin adapters that depend on `wasm-smtp`, not the other way
around.

### `wasm-smtp` — protocol core

**Owns:**

- `Transport` async I/O trait
- `StartTlsCapable` extension trait
- SMTP response parser (reply codes, multi-line replies, length cap)
- SMTP command formatters (EHLO, AUTH, MAIL FROM, RCPT TO, DATA, QUIT, …)
- Session state machine and `SmtpClient` public API
- Error taxonomy: `SmtpError`, `IoError`, `ProtocolError`, `AuthError`,
  `InvalidInputError`
- Authentication mechanisms: AUTH LOGIN, AUTH PLAIN, AUTH SCRAM-SHA-256,
  XOAUTH2
- Optional `tracing` instrumentation (feature-gated)
- Optional `mail-builder` integration helper (feature-gated)

**Does NOT own:**

- Any runtime-specific socket type
- Any TLS handshake implementation
- Any DNS resolution
- Any MIME composition or attachment building
- Cloudflare Workers SDK types
- tokio types

**Dependency rule:** `wasm-smtp` must compile on every target that
`rustc` supports, including `wasm32-unknown-unknown` and
`wasm32-wasip2`, without any Cloudflare or tokio dependencies in
scope. CI enforces this.

### `wasm-smtp-cloudflare` — Cloudflare Workers adapter

**Owns:**

- `CloudflareTransport` struct wrapping `worker::Socket`
- `connect_implicit_tls`, `connect_smtps`, `connect_starttls`,
  `connect_smtp_starttls` convenience constructors
- `StartTlsCapable` impl that delegates to `worker::Socket::start_tls()`
- Cloudflare-specific error mapping to `IoError`
- Request-scoped connection lifecycle management

**Does NOT own:**

- Any SMTP protocol logic — all protocol work goes through
  `wasm-smtp`'s session state machine
- Connection pooling (not possible in Cloudflare's request model)
- Persistent global-scope sockets (against Cloudflare's constraints)

### `wasm-smtp-tokio` — tokio + rustls adapter

**Owns:**

- `TokioTlsTransport` (TLS-wrapped TCP, for port 465 implicit TLS)
- `TokioPlainTransport` (plaintext TCP, for STARTTLS upgrade on port 587)
- `ConnectOptions` for SNI override, custom root stores, ALPN
- Feature flags: `native-roots` (default) / `webpki-roots` for trust
  anchors; `aws-lc-rs` (default) / `ring` for crypto provider
- No API surface to disable certificate verification

**Does NOT own:**

- Any SMTP protocol logic
- Connection pooling or retry logic

### Dependency graph

```
wasm-smtp-cloudflare  ──┐
wasm-smtp-tokio       ──┼──▶  wasm-smtp  (protocol core)
wasm-smtp-wasi (future) ┘
```

The core never depends on an adapter. Adapters never implement SMTP
logic directly.

### Release artifact naming

Archives are named `<crate>-v<version>.tar.gz`, e.g.:

- `wasm-smtp-v0.10.0.tar.gz`
- `wasm-smtp-cloudflare-v0.10.0.tar.gz`
- `wasm-smtp-tokio-v0.10.0.tar.gz`

### Versioning policy

All crates in the workspace share the same version number (defined
once in `[workspace.package]`). Breaking changes trigger a minor bump
in the `0.x` series; patch releases are backwards-compatible. The
project has not yet committed to semantic versioning's `1.0` stability
guarantee.

### Rationale for breaking changes in v0.10.0

`v0.10.0` is explicitly a structural release. Breaking changes are
acceptable because:

1. The RFC process requires a clean slate — new crate boundaries and
   workspace conventions must be established before feature work.
2. The existing `0.9.x` user base is small (pre-1.0, experimental).
3. Long-term technical debt from informal decisions costs more than a
   single announced breaking release now.

The `CHANGELOG.md` and release notes will clearly flag all breaking
changes.

## Security considerations

Crate boundary enforcement is a security measure: keeping TLS,
credential handling, and protocol logic in separate compilation units
prevents accidental coupling (e.g., credentials leaking into a
transport type's `Debug` impl). The boundary rules above are enforced
by CI.

## Simplicity and maintainability considerations

A flat adapter pattern (each adapter is a thin shim) keeps maintenance
surface small. Adding a new runtime requires only a new crate that
implements `Transport`; it does not touch the protocol core.

## Alternatives considered

**Single-crate with feature flags:** rejected because WASM targets
cannot compile tokio, and tokio-based servers should not pull in
Cloudflare's worker SDK. Feature flags on a single crate cannot
fully isolate compilation units.

**Monolithic crate with internal modules:** same problem as above,
plus it would require `cfg` attributes scattered throughout the
protocol implementation — exactly the kind of coupling this design
avoids.

## Implementation plan

1. Add `crates/wasm-smtp-test` to the workspace with the scripted
   transport currently embedded in core's test helpers.
2. Update `crates/` paths if needed to match the names above.
3. Update `README.md`, `ROADMAP.md`, and `docs/src/architecture.md`
   to reflect the canonical structure.
4. Tag `v0.10.0` after the CI passes cleanly with the new layout.

## Acceptance criteria

- Every crate's `Cargo.toml` clearly states what it depends on.
- `cargo build -p wasm-smtp --target wasm32-unknown-unknown` succeeds
  without Cloudflare or tokio in scope.
- `README.md` accurately describes the crate family.
- The release naming convention is documented in `CONTRIBUTING.md`.

## Open questions

None at acceptance time.
