# RFC 016 — WASI adapter design

**Status.** Proposed
**Priority.** P2
**Tracks.** Adapter / WASI
**Touches.** `crates/wasm-smtp-wasi/` (new), `docs/src/wasi-adapter.md`

## Summary

Design the `wasm-smtp-wasi` adapter: the crate that connects WASI
sockets to `wasm-smtp`'s `Transport` trait, targeting WASI 0.2+
(Component Model) runtimes such as wasmtime, WAMR, and future WASI 1.0
runtimes.

## Motivation

WASI sockets (`wasi-sockets`, included in WASI 0.2's interfaces) are
the standard path for SMTP submission from WASM modules running outside
a browser or Cloudflare. Wasmtime, the primary reference runtime,
already supports WASI 0.2 sockets. As the WASM Component Model
matures, WASI becomes the natural deployment target for
platform-independent SMTP components.

This adapter is the "next major target" in the extension plan (after
Cloudflare, which is already implemented). Designing it now — even
without a full implementation — lets us verify that the `Transport`
trait boundary in the core is correct and that no future core changes
are needed.

## Goals

- Define the responsibility boundary of `wasm-smtp-wasi`.
- Identify the concrete WASI interfaces the adapter depends on
  (`wasi:io/streams`, `wasi:sockets/tcp`, `wasi:sockets/network`,
  `wasi:sockets/instance-network`).
- Clarify TLS strategy (TLS is not in WASI 0.2 sockets; see RFC 017).
- Provide a mock-compatible test path so the boundary can be verified
  without a full wasmtime setup.
- Produce a WIT interface sketch for the component-model path (see RFC 018).

## Non-goals

- Full production implementation (this is a design RFC).
- TLS implementation in the adapter (see RFC 017).
- DNS implementation beyond `wasi:sockets/ip-name-lookup`.
- `wasm32-unknown-unknown` compatibility (WASI targets are
  `wasm32-wasip2` or `wasm32-wasip1`).

## Design

### Target WASI interfaces (WASI 0.2)

The adapter depends on:

- `wasi:io/streams@0.2.x` — `InputStream` / `OutputStream` for reading
  and writing bytes.
- `wasi:sockets/tcp@0.2.x` — `TcpSocket` for outbound TCP connections.
- `wasi:sockets/network@0.2.x` — `Network` capability for creating sockets.
- `wasi:sockets/instance-network@0.2.x` — obtain the default `Network`.
- `wasi:sockets/ip-name-lookup@0.2.x` — DNS name resolution.

These are declared as world imports in the component's WIT file.

### Adapter structure

```text
crates/wasm-smtp-wasi/
├─ Cargo.toml
└─ src/
   ├─ lib.rs           — public API (connect helpers)
   ├─ transport.rs     — WasiTcpTransport implementing Transport
   ├─ dns.rs           — name resolution via wasi:sockets/ip-name-lookup
   └─ tests.rs
```

### `WasiTcpTransport`

```rust
pub struct WasiTcpTransport {
    reader: InputStream,
    writer: OutputStream,
}

impl Transport for WasiTcpTransport { ... }
```

The connection setup function:

```rust
pub async fn connect(host: &str, port: u16)
    -> Result<WasiTcpTransport, IoError>
```

Performs DNS resolution, creates a `TcpSocket`, binds, connects, and
splits the socket into read/write streams.

### TLS strategy

TLS is not part of WASI 0.2 sockets (`wasi-tls` is a separate,
in-progress WASI interface). The adapter's initial scope is therefore
**plaintext TCP only**. TLS strategy is covered by RFC 017; options
include:

1. Await `wasi-tls` standardisation and implement in the adapter.
2. Use a Rust TLS library (rustls) operating on top of the WASI streams.
3. Delegate TLS to the host runtime via a future WASI TLS interface.

Until TLS is resolved, `wasm-smtp-wasi` will document the limitation
and be marked as suitable only for SMTP connections where TLS is handled
by the host (e.g., a proxy in front of the WASM module).

### STARTTLS readiness

The `StartTlsCapable` trait is defined in the core. Once a TLS strategy
is chosen (RFC 017), `WasiTcpTransport` can optionally implement
`StartTlsCapable`. The transport design does not preclude this.

### Mock compatibility

A `MockWasiTransport` can be constructed from in-memory byte buffers,
allowing the WASI transport layer to be tested on any host without a
wasmtime installation. The test uses the same `Transport` trait
contract as the real adapter.

## Security considerations

- WASI sockets do not have a built-in firewall; the host runtime's
  capability grants control which networks the WASM module can reach.
  Administrators must configure the `--allow-ip` / `--allow-address`
  flags appropriately.
- Without TLS, SMTP credentials are transmitted in cleartext. The adapter
  documentation must warn against using plaintext connections to
  submission endpoints over untrusted networks.
- The TLS limitation must be prominently documented, not buried in fine
  print.

## Simplicity and maintainability considerations

The adapter follows the same pattern as `wasm-smtp-cloudflare`: thin
wrapper, no SMTP logic. The DNS module (`dns.rs`) is the most complex
part because WASI name lookup is asynchronous and may return multiple
addresses; the adapter tries them in order.

## Alternatives considered

**Implement WASM-targeted rustls over WASI streams (before wasi-tls):**
technically feasible but adds a significant dependency and requires
crypto provider setup. Deferred to RFC 017's decision.

**Wait for WASI 1.0 instead of targeting 0.2:** WASI 0.2 with sockets
is available in wasmtime today. Targeting 0.2 gives the adapter a real
deployment path sooner; migrating to 1.0 later should be a small
change.

## Implementation plan

*Not yet implemented. Design phase only.*

1. Create `crates/wasm-smtp-wasi/Cargo.toml` with WASI 0.2 bindings.
2. Implement `dns.rs` and `transport.rs`.
3. Verify `Transport` contract compliance with mock transport tests.
4. Document the TLS limitation prominently.
5. Resolve RFC 017 before marking this RFC Implemented.

Target release: v0.15.0 (design) → v0.16.0 (implementation, pending RFC 017).

## Acceptance criteria

- `WasiTcpTransport` implements `Transport` and passes the mock transport
  contract tests.
- `cargo check --target wasm32-wasip2 -p wasm-smtp-wasi` succeeds.
- The TLS limitation is documented in the crate-level doc comment and in
  `docs/src/wasi-adapter.md`.
- No SMTP protocol logic appears in the adapter code.

## Open questions

1. Which wasmtime version and WASI 0.2 bindings crate should be used?
   (`wit-bindgen`, `wasi` crate, or raw `wasm-bridge`?)
2. Should the adapter target `wasm32-wasip2` exclusively or also support
   `wasm32-wasip1` (WASI preview 1)?
3. Should DNS resolution retry on failure, or propagate the error
   immediately? WASI name lookup has no built-in retry.
