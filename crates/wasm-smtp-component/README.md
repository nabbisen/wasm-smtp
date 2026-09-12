# wasm-smtp-component

WASM Component Model interface for [`wasm-smtp`](https://crates.io/crates/wasm-smtp).

Exports the `smtp-send` WIT interface defined in `wit/smtp.wit`, enabling
any language with WIT bindings (TypeScript, Go, Python, C, …) to send email
through `wasm-smtp` without writing Rust.

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
    send: func(
        config: smtp-config,
        credentials: smtp-credentials,
        message: smtp-message,
    ) -> result<send-result, send-error>;
}
```

See [`wit/smtp.wit`](./wit/smtp.wit) for the complete interface.

## Language bindings

```sh
# TypeScript / JavaScript
jco types wit/smtp.wit -o ./types

# Go
wit-bindgen go wit/smtp.wit --out-dir ./smtp_bindings
```

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

## License

Apache-2.0. See [LICENSE](../../LICENSE).
