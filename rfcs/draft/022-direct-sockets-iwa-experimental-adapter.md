# RFC 022 — Direct Sockets / IWA experimental adapter

**Status.** Draft
**Priority.** P3
**Tracks.** Experimental / Browser / Adapter
**Touches.** `crates/wasm-smtp-direct-sockets/` (future), `docs/src/direct-sockets.md`

## Summary

Design an experimental adapter for the W3C Direct Sockets API, targeting
Isolated Web Apps (IWA) and other high-trust browser contexts where raw
TCP connections from a browser are permitted.

This adapter is explicitly **experimental**. It is not intended for
general-purpose web applications.

## Motivation

The W3C Direct Sockets API (WICG proposal) gives browser contexts direct
TCP/UDP access, bypassing the HTTP API layer. As of 2025, this API is
available only in:

- Chrome's **Isolated Web Apps** (IWA): Signed WebBundle packaged apps
  operating outside the normal same-origin sandbox.
- Certain **managed enterprise / ChromeOS** contexts.
- Developer-mode Chrome with experimental flags.

It is **not** available to ordinary web pages or standard PWAs.

When available, Direct Sockets would allow a browser-delivered
application to connect directly to an SMTP submission endpoint, enabling
fully client-side email sending without a backend server. The security
and UX implications require careful design.

## Goals

- Define the `wasm-smtp-direct-sockets` adapter position and scope.
- Identify the Direct Sockets API surface needed.
- Define the permission / capability error mapping.
- Design the browser-side secret handling policy (see also RFC 023).
- Produce a `DirectSocketsTransport` that implements `Transport`.
- Explicitly mark the adapter as experimental and not for production
  web apps.

## Non-goals

- Exposing this adapter to ordinary (non-IWA) web pages.
- Persisting SMTP credentials in `localStorage` or `IndexedDB`.
- Permission bypass or silent connection without user consent.
- Compatibility guarantee across Chrome versions.

## Design

### API surface required

```typescript
// Direct Sockets API (browser-side, TypeScript perspective)
const socket = await navigator.openTCPSocket({ remoteAddress, remotePort });
const reader = socket.readable.getReader();
const writer = socket.writable.getWriter();
// TLS upgrade: socket.startTLS() (not yet in spec; may be separate API)
```

The adapter bridges this Web API to the `Transport` trait using
`wasm-bindgen` + the `web-sys` bindings for Direct Sockets (once
available in `web-sys`).

### `DirectSocketsTransport`

```rust
pub struct DirectSocketsTransport {
    reader: /* wrapping ReadableStreamDefaultReader */,
    writer: /* wrapping WritableStreamDefaultWriter */,
}

impl Transport for DirectSocketsTransport { ... }
```

### TLS strategy

Direct Sockets' TLS support is under discussion in the WICG. Options:

1. **Host-managed TLS:** the browser performs TLS before exposing the
   stream (analogous to `SecureTransport::On` in Cloudflare).
2. **`socket.startTLS()`:** a proposed method to upgrade a plaintext
   connection (analogous to Cloudflare's `start_tls()`).
3. **Rust TLS over raw streams:** if the spec exposes plaintext bytes,
   run `rustls` as in the WASI adapter.

Current status (2025): option 1 appears to be the spec direction. The
adapter will follow whichever path the spec stabilises on.

### Permission model

Direct Sockets requires the IWA to declare TCP socket permission in its
manifest. If the API is unavailable (normal web page context), calling
`navigator.openTCPSocket` throws a `DOMException`. The adapter maps
this to `IoError` with a descriptive message pointing to the IWA setup
documentation.

### Production readiness disclaimer

The adapter documentation must include a prominent warning:

> **Experimental:** `wasm-smtp-direct-sockets` targets Isolated Web Apps
> and is not suitable for general-purpose web applications. The Direct
> Sockets API is not available in standard browser contexts.
> Certificate pinning and credential persistence are not supported.
> This adapter is not recommended for production deployments without
> a thorough security review.

## Security considerations

Browser-side SMTP is a high-risk context:

- **Credential exposure:** SMTP credentials in a browser page can be
  exfiltrated by XSS or malicious extensions. IWA's CSP and sandboxing
  mitigate this; standard web pages do not.
- **No credential persistence:** the adapter must not store credentials
  in `localStorage`, `IndexedDB`, cookies, or any persistent browser
  API. Credentials exist only in JS memory for the duration of the call.
- **User consent:** the connection is initiated by user action (e.g.,
  submitting a form), not automatically on page load.
- **TLS required:** no plaintext SMTP connections are exposed. If the
  Direct Sockets TLS API is unavailable, the adapter falls back to an
  error, not to plaintext.

See RFC 023 for detailed browser-side secret and consent design.

## Simplicity and maintainability considerations

This adapter has more uncertainty than the others because:
- The Direct Sockets spec is still in WICG (not W3C) and may change.
- `web-sys` bindings for Direct Sockets do not yet exist.
- Browser TLS integration is unspecified.

The adapter should be kept as thin as possible, with the expectation
that it will need to track spec changes. The core `Transport` trait
is stable; only the adapter code needs to change when the API evolves.

## Alternatives considered

**WebSocket proxy:** route SMTP through a WebSocket-to-TCP proxy server.
No Direct Sockets needed. This is the production approach for ordinary
web apps; Direct Sockets is only worth the complexity for IWA / offline-
first contexts where a backend proxy defeats the purpose.

**Not implementing this adapter:** valid. The IWA market is small today.
This RFC is Draft; it will stay Draft until there is a real IWA use case.

## Implementation plan

*Not yet implemented. Waiting for Direct Sockets spec stabilisation.*

Conditions for moving to Proposed:

1. Direct Sockets API (including TLS) is at Candidate Recommendation.
2. `web-sys` crate includes Direct Sockets bindings.
3. At least one IWA deployment of `wasm-smtp` exists or is planned.

## Acceptance criteria

*Applicable when the adapter is eventually implemented:*

- `DirectSocketsTransport` compiles for `wasm32-unknown-unknown`.
- Calling the adapter outside an IWA context returns `IoError` with a
  descriptive message (not a panic).
- No credential is persisted to any browser storage API.
- The "Experimental" disclaimer appears in the crate-level rustdoc.

## Open questions

1. Does the W3C Direct Sockets spec include a TLS upgrade path? The
   current WICG text (`TCPSocket`) does not; a separate `SecureSocket`
   type may be added.
2. Does Chrome's IWA implementation support STARTTLS (port 587) or only
   implicit TLS (port 465)?
3. Is the adapter the right abstraction, or should it be a WASM component
   with a WIT interface (see RFC 018) that the browser-side JS calls?
