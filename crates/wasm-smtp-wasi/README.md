# wasm-smtp-wasi

WASI sockets adapter for [`wasm-smtp`](https://crates.io/crates/wasm-smtp),
targeting `wasm32-wasip2` (WASI 0.2 Component Model) runtimes.

## Quick start

```rust
use wasm_smtp_wasi::connect_smtps;

let mut client = connect_smtps("smtp.example.com", 465, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "Subject: Hello\r\n\r\nBody.\r\n",
).await?;
client.quit().await?;
```

## Building

```sh
cargo build --target wasm32-wasip2 -p wasm-smtp-wasi
```

Running tests on a native host (no WASI runtime required):

```sh
cargo test -p wasm-smtp-wasi
```

## TLS

TLS is handled by [rustls](https://docs.rs/rustls) on top of WASI byte
streams. Certificate validation is enforced and cannot be disabled.
Trust anchors use the bundled Mozilla root CA set (`webpki-roots` feature,
default) or the OS trust store (`native-roots` feature).

## Feature flags

| Flag | Default | Description |
|---|---|---|
| `webpki-roots` | ✅ | Bundle Mozilla root CA set |
| `native-roots` | ❌ | Use OS trust store |
| `plaintext-only` | ❌ | **Test / proxy-offload only.** No TLS. |

## Limitations

- `wasm32-wasip2` target stdlib required (`rustup target add wasm32-wasip2`).
- No connection pooling.
- No built-in retry or timeout logic.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
