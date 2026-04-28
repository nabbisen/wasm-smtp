# Cloudflare adapter

`wasm-smtp-cloudflare` is the adapter crate that bridges Cloudflare
Workers' socket API and `wasm-smtp-core`. The implementation is small
on purpose: `worker::Socket` already implements
`tokio::io::AsyncRead + AsyncWrite`, so the adapter is essentially a
two-method translation layer plus a connect helper.

## Responsibilities

- Open a TCP connection to a host and port using Workers' `connect()`
  API, with `secureTransport: "on"` so the runtime performs the TLS
  handshake before any byte is delivered to the SMTP state machine.
- Wrap the resulting `Socket` so that it implements
  `wasm-smtp-core::Transport`.
- Translate Workers-side I/O errors into `IoError` with stable,
  human-readable messages.
- Manage the connection lifecycle: `Transport::close` calls
  `Socket::close`, which shuts both halves cleanly.

## Non-responsibilities

- The adapter does not parse SMTP, manage state, or know about
  authentication. Everything SMTP-shaped is in `wasm-smtp-core`.
- The adapter does not own MIME, attachments, or message composition.
- The adapter does not know about STARTTLS. Implicit TLS is the
  supported model.

## Public surface

```rust
pub struct CloudflareTransport { /* ... */ }

impl CloudflareTransport {
    pub fn from_socket(socket: worker::Socket) -> Self;
    pub fn into_inner(self) -> worker::Socket;
}

impl wasm_smtp_core::Transport for CloudflareTransport { /* ... */ }

pub async fn connect_implicit_tls(host: &str, port: u16)
    -> Result<CloudflareTransport, wasm_smtp_core::IoError>;

pub async fn connect_smtps(host: &str, port: u16, ehlo_domain: &str)
    -> Result<wasm_smtp_core::SmtpClient<CloudflareTransport>,
              wasm_smtp_core::SmtpError>;
```

`from_socket` exists for callers that need non-default
`SocketOptions` and want to construct the `worker::Socket` themselves.
`connect_implicit_tls` is the standard path. `connect_smtps` adds the
SMTP greeting and `EHLO` handshake on top, returning an `SmtpClient`
that is ready for `login` or `send_mail`.

## Testing strategy

Two layers:

1. **Adapter-level unit tests** (`crates/cloudflare/src/tests.rs`)
   exercise the byte-pushing helpers `read_async_io` and
   `write_all_async_io` against `tokio_test::io::Builder`. The same
   tests then drive a full SMTP transaction through `SmtpClient`
   over a generic `StreamTransport<S>` wrapper, scripting the
   complete server side of an authenticated session. These run on
   any host (`cargo test`).
2. **Workers-runtime tests** are run manually with
   `wrangler dev` against a known submission server (or a dockerized
   Postfix instance). They are not part of the `cargo test` matrix
   because they require a Workers runtime that `cargo` cannot
   provision.

## Minimum usage

```rust
use wasm_smtp_cloudflare::connect_smtps;

let mut client =
    connect_smtps("smtp.example.com", 465, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "From: user@example.com\r\nTo: recipient@example.org\r\n\
     Subject: hi\r\n\r\nbody\r\n",
).await?;
client.quit().await?;
```

The shape of this code is identical to the example in `core.md`; the
only difference is which `Transport` implementation is constructed.
That is the point.
