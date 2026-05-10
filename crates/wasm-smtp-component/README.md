# wasm-smtp-component

WASM Component Model interface for [`wasm-smtp`](https://crates.io/crates/wasm-smtp).

Exports the `smtp-send` WIT interface defined in `wit/smtp.wit`, enabling
any language with WIT bindings (TypeScript, Go, Python, C, …) to send email
through `wasm-smtp` without writing Rust.

## Quick start

```sh
# Build the .wasm component (requires cargo-component + wasm32-wasip2 target)
cargo component build --target wasm32-wasip2 -p wasm-smtp-component

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

See [`wit/smtp.wit`](../../wit/smtp.wit) for the complete interface.

## Language bindings

```sh
# TypeScript / JavaScript
jco types ../../wit/smtp.wit -o ./types

# Go
wit-bindgen go ../../wit/smtp.wit --out-dir ./smtp_bindings
```

## Building

Prerequisites:

```sh
# Install cargo-component
cargo install cargo-component

# Add the WASM target
rustup target add wasm32-wasip2
```

Then:

```sh
cargo component build --target wasm32-wasip2 -p wasm-smtp-component
# → target/wasm32-wasip2/debug/wasm_smtp_component.wasm
```

## Security

Credentials are passed as plain strings on each `send` call and are **not**
retained between calls. See `docs/src/component-model.md` for the threat model.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
