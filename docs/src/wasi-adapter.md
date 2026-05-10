# WASI adapter

`wasm-smtp-wasi` provides a [`Transport`] implementation for WASI 0.2
runtimes (wasmtime ≥ 19, WAMR) targeting `wasm32-wasip2`. It uses WASI
sockets (`wasi:sockets/tcp`) and rustls for TLS.

## Add to Cargo.toml

```toml
[dependencies]
wasm-smtp-wasi = "0.15"
```

For native-roots (platform certificate store) instead of bundled
WebPKI roots:

```toml
wasm-smtp-wasi = { version = "0.15", default-features = false, features = ["native-roots"] }
```

## Implicit TLS (port 465)

```rust
use wasm_smtp_wasi::connect_smtps;

let mut client =
    connect_smtps("smtp.example.com", 465, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "From: user@example.com\r\nTo: recipient@example.org\r\n\
     Subject: Hi\r\n\r\nHello!\r\n",
).await?;
client.quit().await?;
```

## STARTTLS (port 587)

```rust
use wasm_smtp_wasi::connect_smtp_starttls;

let mut client =
    connect_smtp_starttls("smtp.example.com", 587, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
// … send as above …
```

## Build

```sh
# Add the target
rustup target add wasm32-wasip2

# Build your component
cargo build --target wasm32-wasip2

# Run with wasmtime (requires --allow-ip-name-lookup and --inherit-network)
wasmtime run --allow-ip-name-lookup --inherit-network target/wasm32-wasip2/debug/my_app.wasm
```

## TLS certificate roots

Two mutually exclusive features control which TLS root certificates are
used:

| Feature | Default | Source |
|---------|---------|--------|
| `webpki-roots` | **on** | Bundled Mozilla root store (via `webpki-roots`) |
| `native-roots` | off | Platform certificate store (via `rustls-native-certs`) |

`webpki-roots` is preferred for reproducible builds and Wasm components.
`native-roots` is useful in environments where the platform store is
managed by the operator.

## DNS and sockets

DNS resolution uses `wasi:sockets/ip-name-lookup`. TCP connections use
`wasi:sockets/tcp`. Both require the runtime to grant the corresponding
capabilities (wasmtime: `--allow-ip-name-lookup`, `--inherit-network`).

[`Transport`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/trait.Transport.html
