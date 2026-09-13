# wasm-smtp-component

WASM Component Model interface for [`wasm-smtp`](https://crates.io/crates/wasm-smtp).

Exports the `smtp-send` WIT interface defined in `wit/smtp.wit`. Any
Component Model host can call the built component to send email through
`wasm-smtp` without writing Rust, and `jco` generates TypeScript/JavaScript
types for it.

**0.18.0 breaks the component interface** (`wasm-smtp:smtp@0.2.0`); see "Migrating from 0.1.0" in `docs/src/adapters/component-model.md`.

## Quick start

```sh
# Build the .wasm component (needs only the wasm32-wasip2 target)
cargo build --target wasm32-wasip2 -p wasm-smtp-component

# Run unit tests on native (no WASM runtime required)
cargo test -p wasm-smtp-component
```

## WIT interface

```wit
// wit/smtp.wit (abbreviated)
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

See [`wit/smtp.wit`](./wit/smtp.wit) for the complete interface.

## Language bindings

To call this component from TypeScript or JavaScript, generate its types
with jco. Point jco at the `wit` directory, not at `wit/smtp.wit`, so it
also loads the WASI packages vendored under `wit/deps/`:

```sh
jco types wit -o ./types
```

`wit-bindgen go` and `componentize-py … bindings` also read this WIT, but
they generate bindings for implementing the `smtp-client` world, not for
calling this component.

## Building

Prerequisites — the target, and nothing else:

```sh
rustup target add wasm32-wasip2
```

Then:

```sh
cargo build --target wasm32-wasip2 -p wasm-smtp-component
# → target/wasm32-wasip2/debug/wasm_smtp_component.wasm
```

`cargo-component` is not required. `wasm32-wasip2` emits a Component
Model component directly, and `wit-bindgen`'s `export!` macro in this
crate supplies the export glue.

Running it needs a WASI 0.2 host. The world declares its WASI imports at
`@0.2.12` — the minor this crate's own bindings come from — but any 0.2.x
host satisfies them, which the workspace's `tools/component-smoke` proves
on every gate run under wasmtime 36.

## Security

Credentials are passed as plain strings on each `send` call and are **not**
retained between calls. See `docs/src/adapters/component-model.md` for the threat model.

Trust anchors (`trust-anchors::custom`) are public CA certificates, not
secrets, even though an `smtp-config` holds them for its lifetime; see
"Configuration and trust anchors" in `docs/src/adapters/component-model.md`.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
