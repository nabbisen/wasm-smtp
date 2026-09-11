# Architecture

## The core / adapter split

```text
   ┌─────────────────────────┐         ┌─────────────────────────────┐
   │ application code        │         │ runtime entry point         │
   │ (Worker, server, etc.)  │         │ (Worker, main, component)   │
   └────────────┬────────────┘         └──────────────┬──────────────┘
                │                                     │
                │  uses SmtpClient API                │  builds a Transport
                ▼                                     ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                          wasm-smtp                          │
   │  client/ · session.rs · protocol.rs · policy.rs · audit.rs  │
   │                  (no I/O, no host APIs)                     │
   └─────────────────────────────────────────────────────────────┘
                ▲
                │  implements `trait Transport`
                │
   ┌──────────────────┬──────────────────┬───────────────────────┐
   │ wasm-smtp-       │ wasm-smtp-tokio  │ wasm-smtp-wasi        │
   │ cloudflare       │ tokio + rustls   │ WASI 0.2 sockets      │
   │ Workers Socket   │                  │ (wasm32-wasip2)       │
   └──────────────────┴──────────────────┴───────────────────────┘
   ┌──────────────────┬──────────────────────────────────────────┐
   │ wasm-smtp-       │ wasm-smtp-test                           │
   │ component        │ mock Transport for tests (dev-only)      │
   │ WIT export       │                                          │
   └──────────────────┴──────────────────────────────────────────┘
```

`wasm-smtp` is a library of pure protocol logic. The only contract
it has with the outside world is the `Transport` trait, which exposes
four async methods: `read`, `write_all`, `flush`, and `close`. `flush`
has a default no-op body, so transports that do not buffer writes need
implement only the other three. The trait is intentionally minimal so
that any runtime, real or mocked, can satisfy it.

Transports that need to support STARTTLS (RFC 3207) additionally
implement the `StartTlsCapable` sub-trait, whose single method
`upgrade_to_tls` is invoked by `SmtpClient::starttls()` after the
server has accepted the `STARTTLS` command. Keeping this on a
separate trait means: (a) Implicit-TLS-only transports compile
without any STARTTLS scaffolding, (b) calling `starttls()` on an
incompatible transport is a compile-time error, and (c) the core
state machine is the same regardless of which TLS model the caller
chose — the transport handles all of the bytes-on-the-wire details.

`wasm-smtp-cloudflare` was the first concrete adapter: it translates
between Cloudflare Workers' `Socket` (and its `ReadableStream` /
`WritableStream` halves) and the `Transport` trait. `wasm-smtp-tokio`,
`wasm-smtp-wasi`, and the mock transport in `wasm-smtp-test` do the same
for their runtimes. None of them does any SMTP bookkeeping of its own.

## Module layout in `wasm-smtp`

| Path              | Responsibility                                                      |
| ----------------- | ------------------------------------------------------------------- |
| `lib.rs`          | Public re-exports. Module declarations.                             |
| `transport.rs`    | The `Transport` and `StartTlsCapable` traits. The only I/O contract. |
| `protocol.rs`     | Reply parsing, command formatting, dot-stuffing, base64, validators. |
| `session.rs`      | The `SessionState` enum and the explicit transition table.          |
| `client/`         | `SmtpClient`: `mod.rs` (connect, quit, state), `auth.rs`, `send.rs`, `io.rs`, `starttls.rs`. |
| `error.rs`        | `SmtpError`, `IoError`, `ProtocolError`, `AuthError`, `InvalidInputError`, `PolicyError`, `SmtpOp`. |
| `policy.rs`       | The `SendPolicy` pre-send hook and the bundled policies.            |
| `audit.rs`        | The `AuditSink` hook and the `SmtpAuditEvent` model.                |
| `outcome.rs`      | `SendOutcome`: the accepted reply code and queue id.                |
| `message_body.rs` | The `MessageBody` streaming source and its built-in bodies.         |
| `scram.rs`        | SCRAM-SHA-256 crypto (RFC 5802 / 7677).                             |
| `tests/`          | Unit tests against a synchronous mock transport, one file per area. |

Integration tests that exercise only the public API live in
`crates/wasm-smtp/tests/public_api.rs`.

Modules follow the Rust 2018 style: a module with submodules is a
`foo.rs` plus a `foo/` directory. Tests are isolated under `src/tests/`
so that the production modules stay free of test scaffolding.

## What the core decides, what the adapter decides

| Decision                                  | Owner    |
| ----------------------------------------- | -------- |
| SMTP command sequence, ordering, retries  | core     |
| Reply parsing and code validation         | core     |
| Dot-stuffing, CRLF terminator             | core     |
| SASL exchanges (SCRAM-SHA-256, PLAIN, LOGIN, XOAUTH2, OAUTHBEARER) | core |
| Input validation against CRLF injection   | core     |
| Fact of TLS                               | adapter  |
| Choice of TLS library / runtime API       | adapter  |
| Hostname / port / connect timeout         | adapter (and caller) |
| Concrete socket lifecycle and close       | adapter  |
| Mapping host-specific I/O errors to text  | adapter  |

The split is driven by one rule of thumb: **anything that varies per
runtime is in the adapter; anything that varies per server is in the
core.**

## Why `Transport` is `!Send` by default

Cloudflare Workers, and most modern WASM runtimes, are single-threaded
inside a request. There is no value in requiring a `Send` bound on
`Transport`'s returned futures, and doing so would make adapters
needlessly difficult to write. The trait therefore uses `async fn` in
trait without a `Send` bound, opting into the
`#[allow(async_fn_in_trait)]` warning by design.

Adapter crates that target multi-threaded runtimes can wrap their
transport in a wrapper type that adds the bound at the call site.
