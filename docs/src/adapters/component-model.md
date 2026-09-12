# Component Model interface

`wasm-smtp-component` exports the `smtp-send` WIT interface defined in
`crates/wasm-smtp-component/wit/smtp.wit`, enabling any language with WIT
tooling to send email without writing Rust. The contract lives inside the
crate so that the published crate carries it (RFC 024 D11); the paths
below are relative to the crate directory.

## WIT interface (abbreviated)

```wit
package wasm-smtp:smtp@0.1.0;

interface smtp-send {
    send: func(
        config: smtp-config,
        credentials: smtp-credentials,
        message: smtp-message,
    ) -> result<send-result, send-error>;
}
```

See `wit/smtp.wit` for the complete interface including all record and
variant type definitions.

Two notes on reading that file:

- The envelope sender field is written `%from`. `from` is a reserved WIT
  keyword, and `%` is WIT's escape for using a keyword as an identifier.
  Binding generators still produce a field named `from` (`from` in
  TypeScript, `From` in Go, and so on).
- The world imports `wasi:io`, `wasi:sockets`, and their transitive
  `wasi:clocks` at version 0.2.12. Those packages are vendored under
  `wit/deps/` so that the contract resolves standalone, both in a
  checkout and in the published crate — see `wit/deps/README.md`.

## Host requirement

**A WASI 0.2 host.** The declared `@0.2.12` is the minor the component's
own WASI bindings come from; it is not an exact description of the
artifact, which also carries `wasi:*@0.2.3` interfaces contributed by
the Rust standard library. Hosts satisfy these imports by semver
compatibility within 0.2, not by exact match, so a host implementing any
0.2.x runs it. The project's gate proves this on every run by
instantiating the built artifact under wasmtime 36, whose `wasi:*`
packages are at **0.2.6** — neither of the two minors the artifact
imports — and calling `send` through it.

## Building the component

Prerequisites:

```sh
rustup target add wasm32-wasip2
```

Build:

```sh
cargo build --target wasm32-wasip2 -p wasm-smtp-component
# Output: target/wasm32-wasip2/debug/wasm_smtp_component.wasm
```

`cargo-component` is **not** needed. The `wasm32-wasip2` target emits a
Component Model component directly — the artifact begins with the
component preamble `00 61 73 6d 0d 00 01 00`, not the core-module
`01 00 00 00` — and `wit-bindgen`'s `export!` macro in the crate
supplies the export glue. Earlier revisions of this page said otherwise;
running the thing settled it (RFC 028).

## Running it

Any WASI 0.2 host will do. The repository carries one, used by the gate:

```sh
cargo build --target wasm32-wasip2 -p wasm-smtp-component
cargo run -p wasm-smtp-component-smoke
```

It starts a scripted TLS SMTP responder on loopback, instantiates the
artifact with `wasmtime` 38 embedded as a library, permits TCP to that
one address and nothing else, and calls `smtp-send.send`. Worth reading
before writing your own host: it is about a hundred lines, and it shows
which WASI capabilities the component actually needs — TCP, name
lookup, and no filesystem at all.

One limit of it, which is a limit of the interface: `smtp-config` has no
trust-anchor field, so a test CA cannot be handed to the guest and a
scripted responder with a self-signed certificate cannot complete a
handshake with it. What the gate therefore asserts is that the component
validates the certificate, refuses one it cannot chain, and reports that
as a `send-error` rather than trapping or hanging.

## Language bindings

### TypeScript / JavaScript (via jco)

```sh
npm install -g @bytecodealliance/jco
jco types wit/smtp.wit -o ./smtp-types
```

### Go (via wit-bindgen)

```sh
wit-bindgen go wit/smtp.wit --out-dir ./smtp_bindings
```

### Python (via componentize-py)

```sh
pip install componentize-py
componentize-py --wit-path wit/smtp.wit bindings .
```

## Calling from TypeScript

```typescript
import { send } from './smtp-types/smtp-send.js';

const result = send(
  { host: 'smtp.example.com', port: 465,
    ehloDomain: 'client.example.com', tlsMode: 'implicit' },
  { username: 'user@example.com', password: 'secret' },
  {
    from: 'user@example.com',
    to: ['recipient@example.org'],
    rawMessage:
      'From: user@example.com\r\nTo: recipient@example.org\r\n' +
      'Subject: Hello\r\n\r\nBody.\r\n',
  }
);
```

## Credential security model

Credentials are passed as plain WIT strings on each `send` call and
are **not** retained by the component between calls. The component
does not log, store, or expose credentials in any way.

The host runtime controls what network access the component has.
Ensure the runtime is configured to restrict outbound TCP to trusted
SMTP servers.

## Rust unit tests (no WASM runtime required)

```sh
cargo test -p wasm-smtp-component
```

The test suite runs on native hosts using hand-written type stubs that
mirror the WIT-generated types, so no WASM toolchain is needed for
`cargo test`.
