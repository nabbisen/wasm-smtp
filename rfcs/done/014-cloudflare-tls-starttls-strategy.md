# RFC 014 — Cloudflare TLS and STARTTLS strategy

**Status.** Implemented (v0.5.0)
**Priority.** P1
**Tracks.** Adapter / Security / TLS
**Touches.** `crates/wasm-smtp-cloudflare/src/socket.rs`, `docs/src/cloudflare-adapter.md`

## Summary

Define how `wasm-smtp-cloudflare` handles TLS: the choice of implicit
TLS as the recommended mode, the STARTTLS strategy, and the prohibition
on plaintext connections in production.

## Motivation

TLS strategy is the most security-critical decision in an SMTP adapter.
Getting it wrong — defaulting to plaintext, making certificate
validation optional, or implementing STARTTLS incorrectly — exposes
credentials to network interception. This RFC records the decisions and
their rationale so that future changes require conscious deliberation.

## Goals

- Implicit TLS (port 465, `SecureTransport::On`) is the default and
  recommended mode.
- STARTTLS (port 587, `SecureTransport::StartTls`) is supported as
  an option for servers that do not offer port 465.
- Plaintext mode is not exposed in any production API.
- TLS is handled entirely by the Cloudflare runtime; no Rust TLS
  library is needed in the adapter.
- All TLS configuration decisions are documented.

## Non-goals

- Implementing TLS in Rust inside the adapter (Cloudflare handles this).
- Exposing a plaintext mode for production use.
- Certificate pinning (not supported by Cloudflare's socket API).
- Mutual TLS / client certificates.

## Design

### Implicit TLS (`SecureTransport::On`, port 465)

Cloudflare performs the TLS handshake during `connect()`. The adapter
calls `Socket::opened()` to wait for the handshake to complete before
returning the transport to the caller. From the `Transport` trait's
perspective, the connection is already encrypted from the first byte.

This is the recommended path because:

- No downgrade attack surface (the connection is never plaintext).
- Simpler state machine (no pre-TLS SMTP exchange).
- Increasingly supported by major SMTP submission providers (Gmail,
  Fastmail, Mailgun, Postmark, etc.).

### STARTTLS (`SecureTransport::StartTls`, port 587)

For servers that require STARTTLS on port 587:

1. Adapter opens a plaintext TCP connection with
   `SecureTransport::StartTls`.
2. `wasm-smtp` core drives the SMTP exchange up to and including the
   `STARTTLS` verb (per the `StartTlsCapable` trait and RFC 006).
3. Core calls `upgrade_to_tls()` on the transport.
4. Adapter calls `Socket::start_tls()` on the underlying `worker::Socket`.
5. Cloudflare performs the TLS handshake in place.
6. Core re-issues EHLO on the now-encrypted channel.

The `StartTlsCapable` trait bound on `SmtpClient::connect_starttls` ensures
that a caller cannot accidentally use a non-upgradeable transport for
STARTTLS.

### No production plaintext

There is no public API in `wasm-smtp-cloudflare` that creates a
plaintext SMTP connection without TLS. The only way to get plaintext
bytes on the wire is to construct `CloudflareTransport` directly from
a `worker::Socket` with `SecureTransport::Off` — which requires using
the private constructor and is explicitly flagged in documentation as
test-only.

### TLS handled by Cloudflare runtime

Cloudflare Workers validates server certificates using its own trust
store. The adapter does not control certificate validation parameters.
This is a constraint, not a feature: it means the adapter cannot
implement custom root stores or certificate pinning, but it also means
there is no API surface for disabling validation.

### Documentation

`docs/src/cloudflare-adapter.md` explicitly states:

- Port 25 is blocked by Cloudflare.
- Plaintext connections are not supported in production.
- Certificate validation is performed by the Cloudflare runtime.
- STARTTLS is optional; implicit TLS on 465 is preferred.

## Security considerations

- The `SecureTransport::On` path (implicit TLS) is never in a
  downgrade-vulnerable state.
- The `SecureTransport::StartTls` path has a pre-TLS exchange that
  could theoretically be attacked. Cloudflare's `start_tls()` mitigates
  this by refusing to proceed if the pre-TLS exchange contains SMTP
  command injections (Cloudflare's runtime validates this).
- The core's own STARTTLS injection defence (RFC 006: discard buffered
  input before upgrade) provides an additional layer.

## Simplicity and maintainability considerations

Delegating TLS entirely to the Cloudflare runtime eliminates the
rustls / tokio-rustls dependency from the adapter. This keeps the WASM
binary smaller and avoids the complexity of crypto provider selection
that `wasm-smtp-tokio` has to manage.

## Alternatives considered

**Bundling rustls in the Cloudflare adapter:** rejected. Cloudflare
Workers does not expose a raw TCP stream; its socket API already wraps
TLS. Bundling a Rust TLS implementation would result in double-TLS
or unused code.

**Opportunistic TLS (try STARTTLS, fall back to plaintext):** rejected
explicitly. Opportunistic TLS is the configuration that failed in
CVE-2011-1575. This crate makes STARTTLS mandatory-or-fail (it is
controlled by the `StartTlsCapable` bound, not a runtime flag).

## Implementation plan

*Implicit TLS implemented Phase 3 (v0.3.x), STARTTLS added Phase 5 (v0.5.x).*

## Acceptance criteria

- `connect_smtps` connects via `SecureTransport::On`.
- `connect_starttls` connects via `SecureTransport::StartTls` and
  calls `Socket::start_tls()` when `upgrade_to_tls` is invoked.
- No public API creates a `SecureTransport::Off` connection.
- Documentation states that port 25 is blocked.

## Open questions

None. Implemented and stable.
