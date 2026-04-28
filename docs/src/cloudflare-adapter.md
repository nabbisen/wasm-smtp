# Cloudflare adapter

`wasm-smtp-cloudflare` is the adapter crate that bridges Cloudflare
Workers' socket API and `wasm-smtp-core`. The implementation is small
on purpose: `worker::Socket` already implements
`tokio::io::AsyncRead + AsyncWrite`, so the adapter is essentially a
two-method translation layer plus a connect helper.

## Responsibilities

- Open a TCP connection to a host and port using Workers' `connect()`
  API, choosing between two TLS models:
  - **Implicit TLS** (`secureTransport: "on"`): the runtime performs
    the TLS handshake before any byte reaches the SMTP state machine.
    This is port 465.
  - **STARTTLS** (`secureTransport: "starttls"`): the connection
    starts plaintext and is upgraded to TLS in-place after the SMTP
    `STARTTLS` command. This is port 587 (and, for legacy relays,
    port 25).
- Wrap the resulting `Socket` so that it implements
  `wasm-smtp-core::Transport` for both flows, and additionally
  `wasm-smtp-core::StartTlsCapable` so the core state machine can
  drive the upgrade.
- Translate Workers-side I/O errors into `IoError` with stable,
  human-readable messages.
- Manage the connection lifecycle: `Transport::close` calls
  `Socket::close`, which shuts both halves cleanly.

## Non-responsibilities

- The adapter does not parse SMTP, manage state, or know about
  authentication. Everything SMTP-shaped is in `wasm-smtp-core`.
- The adapter does not own MIME, attachments, or message composition.
- The adapter does not negotiate which TLS model to use. The caller
  picks the entry point that matches the server's listener.

## Public surface

```rust
pub struct CloudflareTransport { /* ... */ }

impl CloudflareTransport {
    pub fn from_socket(socket: worker::Socket) -> Self;
    pub fn into_inner(self) -> Option<worker::Socket>;
}

impl wasm_smtp_core::Transport for CloudflareTransport { /* ... */ }
impl wasm_smtp_core::StartTlsCapable for CloudflareTransport { /* ... */ }

// Implicit TLS (port 465).
pub async fn connect_implicit_tls(host: &str, port: u16)
    -> Result<CloudflareTransport, wasm_smtp_core::IoError>;
pub async fn connect_smtps(host: &str, port: u16, ehlo_domain: &str)
    -> Result<wasm_smtp_core::SmtpClient<CloudflareTransport>,
              wasm_smtp_core::SmtpError>;

// STARTTLS (port 587).
pub async fn connect_starttls(host: &str, port: u16)
    -> Result<CloudflareTransport, wasm_smtp_core::IoError>;
pub async fn connect_smtp_starttls(host: &str, port: u16, ehlo_domain: &str)
    -> Result<wasm_smtp_core::SmtpClient<CloudflareTransport>,
              wasm_smtp_core::SmtpError>;
```

`from_socket` exists for callers that need non-default
`SocketOptions` and want to construct the `worker::Socket`
themselves. The `connect_*` helpers are the standard path.
`connect_smtps` and `connect_smtp_starttls` add the SMTP greeting
and `EHLO` handshake on top of the corresponding socket factory,
returning an `SmtpClient` that is ready for `login` or `send_mail`.

## STARTTLS internals

`Socket::start_tls(self) -> Socket` consumes the socket, so the
`StartTlsCapable::upgrade_to_tls` impl temporarily takes the inner
socket out of the transport (`Option::take`), invokes
`start_tls()`, and puts the upgraded socket back. From the SMTP
state machine's point of view, nothing has changed: the same
`Transport` reference keeps working, only now the bytes underneath
are TLS-secured. The original socket must have been opened with
`SecureTransport::StartTls`; sockets opened with
`SecureTransport::Off` cannot be upgraded.

## Testing strategy

Two layers:

1. **Adapter-level unit tests** (`crates/cloudflare/src/tests.rs`)
   exercise the byte-pushing helpers `read_async_io` and
   `write_all_async_io` against `tokio_test::io::Builder`. The same
   tests then drive a full SMTP transaction through `SmtpClient`
   over a generic `StreamTransport<S>` wrapper, scripting the
   complete server side of an authenticated session. A separate
   `starttls_full_flow` test exercises the post-upgrade re-`EHLO`
   path using a no-op upgrade hook (the mock cannot actually
   perform a TLS handshake, but the SMTP byte sequence is
   identical regardless). These run on any host (`cargo test`).
2. **Workers-runtime tests** are run manually with
   `wrangler dev` against a known submission server (or a dockerized
   Postfix instance). They are not part of the `cargo test` matrix
   because they require a Workers runtime that `cargo` cannot
   provision.

## Minimum usage

Implicit TLS (port 465):

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

STARTTLS (port 587):

```rust
use wasm_smtp_cloudflare::connect_smtp_starttls;

let mut client =
    connect_smtp_starttls("smtp.example.com", 587, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
// ...same body and send_mail/quit calls as above
```

The shape of this code is identical to the example in `core.md`; the
only difference is which `Transport` implementation is constructed
and whether the upgrade happens at connect time (Implicit TLS) or
mid-session (STARTTLS). That is the point.
