# RFC 013 — Cloudflare adapter design

**Status.** Implemented (v0.3.0)
**Priority.** P1
**Tracks.** Adapter / Cloudflare
**Touches.** `crates/wasm-smtp-cloudflare/`, `docs/src/cloudflare-adapter.md`

## Summary

Define the design and responsibilities of `wasm-smtp-cloudflare`:
the adapter crate that connects Cloudflare Workers' outbound TCP socket
API to the `wasm-smtp` protocol core.

## Motivation

Cloudflare Workers is the most immediately practical deployment target
for `wasm-smtp`. Workers provides an outbound TCP `connect()` API with
`ReadableStream`/`WritableStream` streams. Wrapping this in a
`Transport` implementation lets the `wasm-smtp` core run unmodified on
Workers, with the adapter absorbing all Workers-specific API calls.

## Goals

- Implement `Transport` for Cloudflare Workers' `Socket` type.
- Provide convenience constructors for the common cases (implicit TLS
  on port 465, STARTTLS on port 587).
- Map Cloudflare-specific errors to `IoError`.
- Keep the adapter thin: no SMTP logic, no protocol state.
- Document all Cloudflare-specific constraints.

## Non-goals

- Re-implementing any SMTP protocol logic.
- Connection pooling (not feasible in Cloudflare's request-scoped model).
- Global-scope persistent sockets (Cloudflare prohibits these).
- Supporting port 25 (blocked by Cloudflare).
- Connecting to private networks (Cloudflare's connect() blocks RFC 1918
  addresses and Cloudflare's own IP ranges).

## Design

### `CloudflareTransport`

```rust
pub struct CloudflareTransport {
    socket: worker::Socket,
    reader: /* stream reader wrapping Socket's readable side */,
    writer: /* stream writer wrapping Socket's writable side */,
}
```

`CloudflareTransport` implements `Transport`. The `Socket` type from
the `worker` crate already implements `tokio::io::AsyncRead + AsyncWrite`
in Workers' runtime environment, making the read/write mapping
straightforward.

### Implicit TLS (port 465)

```rust
pub async fn connect_implicit_tls(host: &str, port: u16)
    -> Result<CloudflareTransport, IoError>
```

Calls `worker::Socket::builder().secure_transport(SecureTransport::On)
.connect(host, port)`, then awaits `Socket::opened()` before returning.
The adapter waits for `opened()` to ensure the TLS handshake is complete
before any SMTP bytes are exchanged.

### STARTTLS (port 587)

```rust
pub async fn connect_starttls(host: &str, port: u16)
    -> Result<CloudflareTransport, IoError>

impl StartTlsCapable for CloudflareTransport {
    async fn upgrade_to_tls(&mut self, _hostname: &str) -> Result<(), IoError>;
}
```

Plain TCP connect (`SecureTransport::StartTls`) followed by `Socket::start_tls()`
when the core calls `upgrade_to_tls`. Cloudflare's `start_tls()` performs
the handshake in place; the streams remain valid after the call.

### Convenience constructors

```rust
/// Implicit-TLS connect + SMTP greeting + EHLO.
pub async fn connect_smtps(host: &str, port: u16, ehlo_domain: &str)
    -> Result<SmtpClient<CloudflareTransport>, SmtpError>

/// STARTTLS connect + SMTP greeting + EHLO + upgrade + re-EHLO.
pub async fn connect_smtp_starttls(host: &str, port: u16, ehlo_domain: &str)
    -> Result<SmtpClient<CloudflareTransport>, SmtpError>
```

These are the recommended entry points; they return a fully connected
`SmtpClient` ready for `login()`.

### Error mapping

Cloudflare `JsError` and stream errors are wrapped in `IoError::with_source`
where the source can be downcast. The message is a human-readable
description of the Cloudflare API call that failed.

### Connection lifecycle

Each Cloudflare Workers request handler creates its own `CloudflareTransport`.
The transport must not be shared across requests. `Transport::close` calls
`Socket::close()`.

## Security considerations

- TLS is the default; `SecureTransport::Off` (plaintext) is not exposed
  through any public API in this crate.
- Port 25 is not documented as a supported port; connecting to it will
  fail at the Cloudflare layer.
- The adapter does not store credentials. Credentials flow through
  `SmtpClient::login()`.

## Simplicity and maintainability considerations

The adapter is deliberately thin. If the Cloudflare socket API changes,
only the adapter changes; the core and the application are unaffected.
The adapter's `src/` has three files: `lib.rs`, `socket.rs` (the
`Transport` impl), and `adapter.rs` (the convenience constructors).

## Alternatives considered

**Bundling adapter in the core crate with a feature flag:** rejected.
`wasm-smtp` core must compile on non-WASM targets. Cloudflare's `worker`
crate only compiles on `wasm32-unknown-unknown`. Feature-flagging it in
the core would require `cfg` attributes throughout the protocol code.

## Implementation plan

*Implemented as of Phase 3 (v0.3.x), STARTTLS added Phase 5 (v0.5.x).*

## Acceptance criteria

- `wasm-smtp-cloudflare` compiles on `wasm32-unknown-unknown`.
- `wasm-smtp` core does not depend on `worker`.
- `connect_smtps` returns a ready `SmtpClient` after greeting and EHLO.
- Cloudflare-specific constraints (no port 25, no private networks,
  request-scoped only) are documented in `docs/src/cloudflare-adapter.md`.

## Open questions

None. Implemented and stable.
