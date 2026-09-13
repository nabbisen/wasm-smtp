# Component Model interface

`wasm-smtp-component` exports the `smtp-send` WIT interface defined in
`crates/wasm-smtp-component/wit/smtp.wit`, enabling any language with WIT
tooling to send email without writing Rust. The contract lives inside the
crate so that the published crate carries it (RFC 024 D11); the paths
below are relative to the crate directory.

## WIT interface (abbreviated)

```wit
package wasm-smtp:smtp@0.2.0;

interface smtp-send {
    variant trust-anchors {
        bundled,
        custom(string),
    }

    resource smtp-config {
        create: static func(
            host: string,
            port: u16,
            ehlo-domain: string,
            tls-mode: tls-mode,
            trust: trust-anchors,
        ) -> result<smtp-config, send-error>;
    }

    send: func(
        config: borrow<smtp-config>,
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

## Configuration and trust anchors

A connection is described by an `smtp-config`, which is a **resource**,
not a record. `smtp-config.create` checks every argument once and either
returns a configuration or fails with `send-error::invalid-input`. The
result is immutable: it has no setters, `send` borrows it rather than
consuming it, and one configuration can serve any number of sends. So a
configuration that was accepted can never make `send` fail, and a bad
certificate is reported where it was supplied rather than at connection
time.

`trust` states which certificate authorities the TLS connection trusts,
and it is always explicit: there is no empty value that silently means
"the default".

- **`bundled`** trusts the bundled Mozilla root set. It is the right
  choice for public submission servers.
- **`custom(pem)`** trusts exactly the certificate authorities in `pem`,
  and no others. Pass the text of a CA bundle file: one or more
  `-----BEGIN CERTIFICATE-----` blocks, with any comments between them.

Three things to know about `custom`:

1. **It replaces the bundled roots; it does not add to them.** A caller
   who also needs public roots must put them in the bundle.
2. **It is checked completely, and it never falls back.** Every block must
   be a `CERTIFICATE` that the root store accepts. A private key pasted by
   mistake, a block that does not parse, a bundle with no certificates,
   more than 64 blocks, or more than 256 KiB of text makes `create` fail.
   A rejected bundle is an error, never a quiet switch to `bundled`. The
   error names the block by position and the kind of problem, such as
   `trust anchors: block 2: not a certificate`, and never repeats any of
   the text supplied.
3. **Trust anchors are public certificates, not secrets.** They are not
   covered by the credential rules below, even though an `smtp-config`
   holds them for as long as it lives.

There is no option, in any form, that disables certificate or hostname
verification.

Further connection options will arrive as additional functions on the
`smtp-config` resource, in compatible versions of the package.

## Migrating from 0.1.0

`wasm-smtp:smtp@0.2.0` (crates 0.18.0) is a breaking change for component
consumers. Nothing changes for Rust callers of `wasm-smtp` or its
adapters.

In 0.1.0 the configuration was a record passed by value to every
`send`:

```text
// 0.1.0
let result = smtp-send.send(
  smtp-config { host: "smtp.example.com", port: 465,
                ehlo-domain: "client.example.com",
                tls-mode: implicit },
  credentials,
  message,
);
```

In 0.2.0 it is created once, with an explicit trust choice, and borrowed
by each send:

```text
// 0.2.0
let config = smtp-config.create(
  "smtp.example.com", 465, "client.example.com", implicit,
  trust-anchors::bundled,          // 0.1.0's only behaviour, now stated
)?;
let result = smtp-send.send(config, credentials, message);
```

To keep 0.1.0's behaviour exactly, pass `trust-anchors::bundled`. For a
server behind a private certificate authority, pass
`trust-anchors::custom` with that authority's certificate, which 0.1.0
had no way to express.

`create` can now fail where 0.1.0's record could not, with
`invalid-input`: an EHLO domain the component would have rejected at
connection time is now rejected at creation, and `custom` input is
checked as above.

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

Point each generator at the `wit` **directory**, not at `wit/smtp.wit`.
The world imports WASI packages vendored under `wit/deps/`, and given the
single file the generators do not load them: they stop with
`package 'wasi:sockets@0.2.12' not found`.

### TypeScript / JavaScript (via jco)

```sh
npm install -g @bytecodealliance/jco
jco types wit -o ./smtp-types
```

### Go (via wit-bindgen)

```sh
wit-bindgen go wit --out-dir ./smtp_bindings
```

### Python (via componentize-py)

```sh
pip install componentize-py
componentize-py --wit-path wit bindings .
```

## Calling from TypeScript

The types below are what `jco types` generates for 0.2.0: an
`SmtpConfig` class with a static `create`, and `send` taking that class.

```typescript
import { SmtpConfig, send } from './smtp-types/interfaces/wasm-smtp-smtp-smtp-send.js';

// Created once and reused. `using` disposes of the resource at the end of
// the scope; without it, call `config[Symbol.dispose]()` when finished.
using config = SmtpConfig.create(
  'smtp.example.com', 465, 'client.example.com', 'implicit',
  { tag: 'bundled' },
  // For a private certificate authority instead:
  //   { tag: 'custom', val: readFileSync('corporate-root-ca.pem', 'utf8') }
);

const result = send(
  config,
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
does not log, store, or expose credentials in any way. They are
deliberately not part of `smtp-config`, which lives as long as the caller
keeps it.

Trust anchors are the exception to "nothing is retained", and they are
public data: see [Configuration and trust anchors](#configuration-and-trust-anchors).

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
