# Architecture

## The two-crate split

```text
   ┌─────────────────────────┐         ┌─────────────────────────────┐
   │ application code        │         │ Cloudflare Worker entry     │
   │ (Worker, server, etc.)  │         │ (or any other adapter user) │
   └────────────┬────────────┘         └──────────────┬──────────────┘
                │                                     │
                │  uses SmtpClient API                │  builds a Transport
                ▼                                     ▼
   ┌─────────────────────────────────────────────────────────────┐
   │                       wasm-smtp-core                        │
   │   client.rs · session.rs · protocol.rs · error.rs · ...     │
   │                  (no I/O, no host APIs)                     │
   └─────────────────────────────────────────────────────────────┘
                ▲
                │  implements `trait Transport`
                │
   ┌─────────────────────────────────────────────────────────────┐
   │  wasm-smtp-cloudflare        (planned)                      │
   │  Cloudflare Socket API   ─→   Transport                     │
   └─────────────────────────────────────────────────────────────┘
```

`wasm-smtp-core` is a library of pure protocol logic. The only contract
it has with the outside world is the `Transport` trait, which exposes
three async methods: `read`, `write_all`, and `close`. The trait is
intentionally minimal so that any runtime, real or mocked, can satisfy
it.

Transports that need to support STARTTLS (RFC 3207) additionally
implement the `StartTlsCapable` sub-trait, whose single method
`upgrade_to_tls` is invoked by `SmtpClient::starttls()` after the
server has accepted the `STARTTLS` command. Keeping this on a
separate trait means: (a) Implicit-TLS-only transports compile
without any STARTTLS scaffolding, (b) calling `starttls()` on an
incompatible transport is a compile-time error, and (c) the core
state machine is the same regardless of which TLS model the caller
chose — the transport handles all of the bytes-on-the-wire details.

`wasm-smtp-cloudflare` is the first concrete adapter. It will translate
between Cloudflare Workers' `Socket` (and its `ReadableStream` /
`WritableStream` halves) and the `Transport` trait. It does no SMTP
bookkeeping of its own.

## Module layout in `wasm-smtp-core`

| File           | Responsibility                                                      |
| -------------- | ------------------------------------------------------------------- |
| `lib.rs`       | Public re-exports. Module declarations.                             |
| `transport.rs` | The `Transport` trait. The only I/O contract.                       |
| `protocol.rs`  | Reply parsing, command formatting, dot-stuffing, base64, validators. |
| `session.rs`   | The `SessionState` enum and the explicit transition table.          |
| `client.rs`    | `SmtpClient` — orchestrates the full SMTP exchange.                  |
| `error.rs`     | `SmtpError`, `IoError`, `ProtocolError`, `AuthError`, `InvalidInputError`. |
| `tests.rs`     | Unit and integration tests against a synchronous mock transport.     |

This crate does not use `mod.rs`; each module is a single `.rs` file at
the same level as its parent. Tests are isolated in `tests.rs` so that
the production modules stay free of test scaffolding.

## What the core decides, what the adapter decides

| Decision                                  | Owner    |
| ----------------------------------------- | -------- |
| SMTP command sequence, ordering, retries  | core     |
| Reply parsing and code validation         | core     |
| Dot-stuffing, CRLF terminator             | core     |
| `AUTH LOGIN` exchange                     | core     |
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
