# RFC 017 — WASI TLS strategy

**Status.** Implemented (v0.12.0)
**Priority.** P2
**Tracks.** WASI / TLS / Security
**Touches.** `docs/src/wasi-tls.md`, `crates/wasm-smtp-wasi/` (future)

## Summary

Analyse the TLS options for the WASI adapter (`wasm-smtp-wasi`) and
select a strategy that provides adequate security without waiting for
a fully standardised `wasi-tls` interface.

## Motivation

WASI 0.2 sockets (`wasi-sockets`) provide TCP streams but not TLS.
SMTP submission requires TLS (implicit TLS on port 465, or STARTTLS on
587). The `wasm-smtp-wasi` adapter cannot ship as production-ready until
this gap is addressed.

There are several candidate strategies, each with different tradeoffs in
security, complexity, and runtime compatibility. This RFC evaluates
them and makes a decision.

## Goals

- Identify all viable TLS strategies for the WASI context.
- Evaluate each against: security, complexity, runtime support, binary size.
- Make a concrete decision (or document the condition under which each
  alternative should be chosen).
- Ensure the chosen strategy does not require changes to `wasm-smtp` core.

## Non-goals

- Implementing TLS from scratch.
- Modifying the `Transport` trait (the TLS boundary is at the adapter).
- Providing TLS for the Cloudflare or tokio adapters (each handles its
  own TLS).

## Design

### Candidate strategies

#### Strategy A: Rust TLS library over WASI streams (rustls + ring/aws-lc-rs)

Run `rustls` (a pure-Rust TLS 1.2/1.3 library) on top of WASI
`InputStream` / `OutputStream`. `rustls` does not depend on the OS
for TLS; it operates on any `AsyncRead + AsyncWrite` byte stream.

**Pros:**
- Self-contained: works with any WASM runtime that implements WASI 0.2 sockets.
- Full certificate validation, SNI, TLS 1.3.
- No waiting for `wasi-tls` standardisation.
- Consistent with `wasm-smtp-tokio`'s approach.

**Cons:**
- Adds rustls + a crypto provider (ring or aws-lc-rs) to the WASM binary.
  ring compiles to WASM; aws-lc-rs requires C code (possible with
  wasmtime's component model, harder in some edge runtimes).
- Binary size increase (~500 KB for ring, larger for aws-lc-rs).
- Trust anchors must be bundled (webpki-roots) since WASI 0.2 has no
  system trust store API.

**Verdict:** The recommended strategy for initial implementation.
ring compiles cleanly to WASM and webpki-roots bundles the Mozilla
trust set. Binary size is acceptable for server-side WASM; for
IoT/constrained devices, see Strategy C.

#### Strategy B: Host-provided TLS via `wasi-tls` (future)

The WebAssembly/wasi-tls proposal (in early draft as of 2025-05) would
add a `wasi:tls/client` interface that lets the host perform the TLS
handshake. The adapter would call into the host for the upgrade instead
of doing it itself.

**Pros:**
- Zero crypto code in the WASM binary.
- Trust anchor management is the host's problem.
- Smallest possible binary size for the adapter.

**Cons:**
- `wasi-tls` is not standardised and has no reference implementation.
- No known runtime (wasmtime, WAMR) supports it as of this writing.
- Dependency on a moving target.

**Verdict:** Preferred long-term. Not viable now. The adapter will be
structured so that switching from Strategy A to Strategy B requires
only a change to the TLS setup code, not the SMTP session logic.

#### Strategy C: Plaintext only, with proxy TLS offload

Deploy the WASM module behind a TLS-terminating proxy (stunnel, Nginx
stream proxy, HAProxy). The proxy connects to the real SMTP server over
TLS; the WASM module connects to the proxy over plaintext loopback.

**Pros:**
- Smallest binary (no TLS code at all).
- Works today.
- Correct for IoT / constrained devices where TLS in WASM is too heavy.

**Cons:**
- Plaintext between the WASM module and the proxy (even on loopback,
  this is a risk in multi-tenant environments).
- Operational complexity: the proxy must be deployed and configured.
- Cannot use STARTTLS (the proxy handles the outer TLS).

**Verdict:** Documented as a deployment option, not the default. The
adapter will have a feature flag `plaintext-only` for this case with a
prominent warning in the documentation.

### Recommended decision

**Ship `wasm-smtp-wasi` v0.1.0 with Strategy A (rustls + ring +
webpki-roots).** This provides production-quality TLS today. The adapter
is structured with a `TlsStream<WasiTcpStream>` type that wraps the
WASI streams in a rustls client connection; when `wasi-tls` matures,
this can be replaced with a host call.

The `plaintext-only` feature flag (Strategy C) is available for
constrained environments that offload TLS externally.

### `StartTlsCapable` implementation

With Strategy A, `WasiTcpTransport` can implement `StartTlsCapable` by:

1. Establishing a plaintext WASI TCP connection.
2. Yielding the raw streams back to the caller after the SMTP STARTTLS
   command (via the `upgrade_to_tls` method).
3. Wrapping the streams in a rustls `ClientConnection` for the post-TLS exchange.

This mirrors the tokio adapter's approach.

### Trust anchors

`webpki-roots` (Mozilla's root CA bundle) is the default. Users who
need custom trust stores (private CA, development self-signed cert) can
supply a custom `rustls::RootCertStore` via `ConnectOptions`.

### Binary size

rustls + ring + webpki-roots adds approximately:

- rustls: ~150 KB (wasm)
- ring: ~400 KB (wasm)
- webpki-roots: ~150 KB (certificates)

Total: ~700 KB increase in WASM binary. Acceptable for server-side
use cases; noted as a concern for IoT/constrained targets.

## Security considerations

- Strategy A with rustls provides certificate validation and SNI on all
  connections. Certificate validation cannot be disabled through the
  public API.
- The `plaintext-only` feature flag is documented as a security
  trade-off and emits a compile-time warning when enabled.
- STARTTLS with Strategy A is subject to the same injection-defence
  rules as the other adapters (RFC 006).

## Simplicity and maintainability considerations

- Choosing rustls is consistent with `wasm-smtp-tokio`, which reduces
  the number of TLS libraries maintainers must understand.
- The `TlsStream` wrapper is the only TLS-specific code in the adapter;
  the rest is standard WASI stream operations.

## Alternatives considered

*Covered in the Candidate strategies section above.*

## Implementation plan

*Not yet implemented.*

1. Resolve rustls WASM support for the chosen crypto provider (ring is
   known to work on `wasm32-unknown-unknown`; verify on `wasm32-wasip2`).
2. Add rustls + ring + webpki-roots to `crates/wasm-smtp-wasi/Cargo.toml`.
3. Implement `TlsStream<WasiTcpStream>` using `rustls::ClientConnection`.
4. Implement `StartTlsCapable` for `WasiTcpTransport`.
5. Add `plaintext-only` feature flag.
6. Write `docs/src/wasi-tls.md`.

Target: resolve before `wasm-smtp-wasi` v0.1.0.

## Acceptance criteria

- `WasiTcpTransport` with Strategy A establishes TLS-secured connections
  (verified by integration test against a test SMTP server with a
  self-signed cert in a custom trust store).
- Certificate validation fails for a connection to a host with an
  untrusted cert (absent custom root store).
- The `plaintext-only` feature flag compiles without TLS deps and emits
  a documented warning.
- Binary size increase is measured and documented.

## Open questions

1. Does `ring` compile to `wasm32-wasip2` (Component Model target)?
   `wasm32-unknown-unknown` is documented; `wasip2` may have subtly
   different platform constraints.
2. Is `aws-lc-rs` a viable alternative to `ring` on WASI targets?
   It provides better performance and FIPS compliance but requires C
   code compilation.
3. Should the initial release support STARTTLS (Strategy A +
   `StartTlsCapable`) or defer STARTTLS to a follow-up?
