# Tokio adapter

`wasm-smtp-tokio` is the production-quality adapter for tokio-based
servers — axum, actix, warp, hyper, plain tokio, anything that runs
on the standard tokio runtime. It is the sibling of
`wasm-smtp-cloudflare` for non-Cloudflare deployments.

It pairs `tokio::net::TcpStream` with `tokio-rustls` to give the
SMTP core a TLS-wrapped byte stream. Certificate validation,
hostname matching, and trust-anchor selection are handled by
`rustls`; this crate's job is the small piece of glue that maps
between tokio's `AsyncRead`/`AsyncWrite` and `wasm-smtp`'s
`Transport` contract.

## Features

| Feature        | Default | Trust-anchor source                                          |
|----------------|---------|--------------------------------------------------------------|
| `native-roots` | yes     | System trust store via [`rustls-native-certs`].              |
| `webpki-roots` | no      | Bundled Mozilla root set via [`webpki-roots`].               |
| `xoauth2`      | yes     | Pass-through to `wasm-smtp/xoauth2` (Gmail / Microsoft 365). |
| `smtputf8`     | no      | Pass-through to `wasm-smtp/smtputf8`.                        |

`native-roots` and `webpki-roots` are mutually exclusive — pick one.
`native-roots` is right for desktop and traditional server
deployments where the OS already manages CA trust. `webpki-roots`
is right for minimal containers (distroless, scratch-based images,
WASM-adjacent constrained environments) without a system CA store.

```toml
[dependencies]
wasm-smtp = "0.7"

# Default (system trust):
wasm-smtp-tokio = "0.7"

# Or explicitly bundled Mozilla roots:
# wasm-smtp-tokio = { version = "0.7", default-features = false, features = ["webpki-roots"] }
```

## Implicit TLS (port 465)

```rust,ignore
use wasm_smtp::SmtpClient;
use wasm_smtp_tokio::TokioTlsTransport;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let transport = TokioTlsTransport::connect_implicit_tls(
    "smtp.example.com",
    465,
    "smtp.example.com",  // SNI / certificate hostname
).await?;

let mut client = SmtpClient::connect(transport, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "Subject: hello\r\n\r\nhi.\r\n",
).await?;
client.quit().await?;
# Ok(())
# }
```

## STARTTLS (port 587)

```rust,ignore
use wasm_smtp::SmtpClient;
use wasm_smtp_tokio::TokioPlainTransport;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let transport = TokioPlainTransport::connect(
    "smtp.example.com",
    587,
    "smtp.example.com",  // certificate hostname for the eventual upgrade
).await?;

let mut client = SmtpClient::connect_starttls(transport, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
// ... rest is the same as implicit TLS
# Ok(())
# }
```

`SmtpClient::connect_starttls` runs the EHLO + STARTTLS dance over
the plaintext channel, then asks the transport to upgrade in place.
After the upgrade the client re-issues EHLO over the encrypted
channel, replacing its capability cache. The `wasm-smtp` core
includes a CVE-2011-1575-class injection defence at the upgrade
boundary; see the [Errors](./errors.md) chapter for details.

## Custom configuration

For private CAs, alternate SNI, or development against a self-signed
certificate, use `ConnectOptions`:

```rust,ignore
use wasm_smtp_tokio::{TokioTlsTransport, ConnectOptions};
use tokio_rustls::rustls::RootCertStore;
use rustls_pki_types::CertificateDer;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let mut roots = RootCertStore::empty();
let test_ca: CertificateDer<'static> = todo!();
roots.add(test_ca)?;

let opts = ConnectOptions::new()
    .with_server_name("internal.example.com")
    .with_root_store(roots);

let transport = TokioTlsTransport::connect_with(
    "10.0.1.20",
    465,
    opts,
).await?;
# Ok(())
# }
```

`ConnectOptions::with_root_store` **replaces** the trust set — the
default trust anchors are not also included. This is intentional:
when you need a private CA, you usually want only that CA, not "the
private CA plus every public CA Mozilla trusts."

## What the adapter does NOT do

There is intentionally **no** API to disable certificate
verification — no `dangerous_configuration`, no
`accept_invalid_certs`, no `disable_hostname_verification`. A test
mail server with a self-signed certificate is reached by
constructing a `RootCertStore` containing that certificate and
passing it through `ConnectOptions::with_root_store`. This is
slightly more verbose than a one-flag override, deliberately: those
overrides have a long history of escaping into production code.

The crate also does not own:

- **TLS protocol selection.** `tokio-rustls` 0.26 supports TLS 1.2
  and 1.3 by default. Restricting versions further is a `rustls`
  configuration concern that you can do via a custom `ClientConfig`
  in your own code if needed (and pre-`with_no_client_auth`
  builders aren't yet exposed through `ConnectOptions`; raise an
  issue if you have a concrete need).
- **Connection pooling.** A single `Transport` is one TCP
  connection. For high-volume submission, build connections per
  outgoing batch in your application code.
- **Retry logic.** SMTP errors surface as `SmtpError` and your code
  decides whether to retry. The adapter has no opinion.

## Compared to `wasm-smtp-cloudflare`

Both adapters expose the same `Transport` contract; `wasm-smtp`
itself does not know which is in use. The differences are
runtime-shaped:

| Aspect                   | `wasm-smtp-tokio`              | `wasm-smtp-cloudflare`            |
|--------------------------|--------------------------------|-----------------------------------|
| Underlying socket        | `tokio::net::TcpStream`        | `worker::Socket`                  |
| TLS implementation       | `tokio-rustls` (`rustls`)      | Cloudflare runtime's TLS          |
| Trust anchors            | OS / Mozilla root set, opt-in  | Cloudflare-managed                |
| Compile target           | `x86_64-*`, `aarch64-*`, etc.  | `wasm32-unknown-unknown`          |
| Binary size cost         | Significant (rustls + crypto)  | Minimal (wraps existing runtime)  |

If you target Cloudflare Workers you almost certainly want
`wasm-smtp-cloudflare`; it's smaller and uses the platform's
already-trusted TLS stack. For everything else, `wasm-smtp-tokio`
is the default choice.

[`rustls-native-certs`]: https://docs.rs/rustls-native-certs
[`webpki-roots`]: https://docs.rs/webpki-roots
