# RFC 005 — Core transport abstraction

**Status.** Implemented (v0.1.0)
**Priority.** P0
**Tracks.** Core architecture / Transport
**Touches.** `crates/wasm-smtp/src/transport.rs`, `crates/wasm-smtp/src/error.rs`

## Summary

Define the `Transport` async I/O trait that `wasm-smtp`'s session state
machine depends on, and the `StartTlsCapable` extension trait that marks
transports capable of in-place TLS upgrade (STARTTLS).

## Motivation

`wasm-smtp` must work on Cloudflare Workers, tokio-based servers, and
(in the future) WASI runtimes. Each provides a different socket API.
A trait-based abstraction is the standard Rust pattern for decoupling
protocol logic from I/O: the core defines what it needs, adapters
provide it, and the compiler enforces the contract.

Without this abstraction, the protocol logic would be tangled with
runtime-specific types, making it impossible to compile the core on
targets where those types are unavailable (e.g. `wasm32-unknown-unknown`
does not have tokio).

## Goals

- Define `Transport`: the minimum async I/O contract the core needs.
- Define `StartTlsCapable`: the marker for STARTTLS-upgradeable transports.
- Keep the trait surface small so that adapter implementations are short.
- Ensure the trait is object-safe where possible.
- Map transport errors uniformly to `IoError`.

## Non-goals

- Specifying any concrete transport implementation.
- TLS handshake logic (that belongs in each adapter).
- DNS resolution.
- Connection pooling or reconnection.

## Design

### `Transport` trait

```rust
pub trait Transport {
    /// Read one CRLF-terminated line into `buf`.
    ///
    /// Returns the number of bytes appended. Returns 0 on clean EOF.
    /// Must not block indefinitely; the caller is responsible for
    /// timeouts at the application layer.
    async fn read_line(&mut self, buf: &mut String) -> Result<usize, IoError>;

    /// Write all bytes in `data` to the transport.
    ///
    /// All-or-nothing: either the entire slice is written or an error
    /// is returned. The transport must not write partial slices and
    /// return success.
    async fn write_all(&mut self, data: &[u8]) -> Result<(), IoError>;

    /// Flush the write buffer.
    async fn flush(&mut self) -> Result<(), IoError>;

    /// Close the transport.
    ///
    /// Called after SMTP QUIT (or on abort). The transport should
    /// shut down the underlying connection cleanly. Implementations
    /// must not confuse transport-level close with SMTP-level QUIT;
    /// the session state machine sends QUIT before calling close.
    async fn close(&mut self) -> Result<(), IoError>;
}
```

### `StartTlsCapable` trait

```rust
pub trait StartTlsCapable: Transport {
    /// Upgrade the connection to TLS in place.
    ///
    /// Called by `SmtpClient::starttls()` after the SMTP STARTTLS
    /// verb has been accepted by the server. The implementation
    /// performs the TLS handshake on the existing byte stream and
    /// returns `Ok(())` on success. After this call the transport's
    /// `read_line` and `write_all` operate on the encrypted channel.
    ///
    /// This method is on a separate trait so that transports that
    /// only support implicit TLS (port 465) cannot be accidentally
    /// passed to `SmtpClient::connect_starttls`. The compiler enforces
    /// the distinction.
    async fn upgrade_to_tls(&mut self, hostname: &str) -> Result<(), IoError>;
}
```

### Error mapping

Adapter implementations map their native errors to `IoError`:

```rust
pub struct IoError {
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```

`IoError` carries a human-readable message and optionally preserves
the original error as a `source` chain for structured logging.

### Clean-EOF semantics

`read_line` returning `Ok(0)` means the connection was closed cleanly
by the server. The session interprets this as an unexpected disconnect
and returns `IoError`. Adapters must distinguish between "server closed
connection" (0 bytes, no error) and "I/O error" (error variant); both
map to the same `IoError` surface but the distinction is logged.

### Write-all contract

`write_all` must send the entire buffer or return an error. Partial
writes are not surfaced to the session; if the transport can only
deliver part of the buffer it must retry internally or return an error.
This matches the semantics of `AsyncWriteExt::write_all` from tokio.

## Security considerations

- The `Transport` trait carries no credential information. Credentials
  flow through the session state machine as `&str` arguments and are
  not stored in the transport.
- The `close` method flushes the write buffer and terminates the
  connection. Adapters must not leave sockets in a half-open state
  that could be reused across requests.
- `StartTlsCapable` being a separate trait prevents accidental use of
  a plaintext-only transport with `connect_starttls`.

## Simplicity and maintainability considerations

Four methods on `Transport` is the minimum needed for SMTP. Keeping the
surface small makes adapter implementations short (the Cloudflare adapter
is under 100 lines; the tokio adapter is under 150 lines) and easy to
audit.

## Alternatives considered

**`AsyncRead + AsyncWrite` bounds:** rejected because different runtimes
expose incompatible `AsyncRead`/`AsyncWrite` traits (tokio vs. futures).
A project-specific trait gives complete control over the contract.

**Callbacks instead of a trait:** rejected. Rust's trait system is the
standard pattern; callbacks would be more cumbersome and less type-safe.

**Single `SmtpTransport` trait without a separate `StartTlsCapable`:**
rejected. Merging the upgrade method into the base trait would require
all adapters to implement a method that panics for implicit-TLS-only
adapters, losing the compile-time guarantee that guards `connect_starttls`.

## Implementation plan

*Already implemented as of v0.1.0 (Transport), v0.9.x (StartTlsCapable).*

## Acceptance criteria

- `cargo build -p wasm-smtp --target wasm32-unknown-unknown` succeeds.
- The Cloudflare and tokio adapters each implement `Transport` with no
  SMTP logic of their own.
- `StartTlsCapable` is implemented only by adapters whose underlying
  socket supports in-place TLS upgrade.
- A test transport (`wasm-smtp-test`) implements `Transport` with
  scripted responses and is usable in `#[test]` without async runtime.

## Open questions

None. Implemented and stable.
