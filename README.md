# wasm-smtp

Rust crates for sending mail by SMTP from WebAssembly runtimes. The
project separates the protocol implementation from the runtime-specific
socket code so that the same SMTP engine can be reused on every host.

## Crates

| Crate                   | Role                                                       | Status         |
| ----------------------- | ---------------------------------------------------------- | -------------- |
| `wasm-smtp-core`        | Environment-independent SMTP state machine and parser.     | Implemented    |
| `wasm-smtp-cloudflare`  | Cloudflare Workers socket adapter for `wasm-smtp-core`.    | Implemented    |

`wasm-smtp-core` is the foundation: it implements the SMTP state
machine, response parsing, command formatting, dot-stuffing, and error
classification, but does no I/O of its own. Each runtime gets its own
adapter crate that provides a [`Transport`] implementation; today, only
the Cloudflare Workers adapter is on the roadmap.

## Minimum usage

From a Cloudflare Worker (the production target):

```rust
use wasm_smtp_cloudflare::connect_smtps;

# async fn run() -> Result<(), wasm_smtp_cloudflare::SmtpError> {
let mut client =
    connect_smtps("smtp.example.com", 465, "client.example.com").await?;
client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "From: user@example.com\r\n\
     To: recipient@example.org\r\n\
     Subject: Hello\r\n\
     \r\n\
     Body text.\r\n",
).await?;
client.quit().await?;
# Ok(())
# }
```

Or directly against `wasm-smtp-core` with any `Transport` you supply:

```rust
use wasm_smtp_core::{SmtpClient, Transport};

async fn send<T: Transport>(transport: T) -> Result<(), wasm_smtp_core::SmtpError> {
    let mut client = SmtpClient::connect(transport, "client.example.com").await?;
    client.login("user@example.com", "secret").await?;
    client.send_mail(
        "user@example.com",
        &["recipient@example.org"],
        "From: user@example.com\r\n\
         To: recipient@example.org\r\n\
         Subject: Hello\r\n\
         \r\n\
         Body text.\r\n",
    ).await?;
    client.quit().await?;
    Ok(())
}
```

The `body` argument is a fully-formed RFC 5322 message: headers, a blank
line, then the body, with CRLF line endings. The library does not build
MIME, attach files, or compose multipart bodies.

## Connection model

Two TLS models are supported:

- **Implicit TLS** on port 465 — the runtime negotiates TLS before any
  SMTP byte is exchanged. Use `connect_smtps`.
- **STARTTLS** on port 587 — the connection starts plaintext and is
  upgraded to TLS in-place after the SMTP greeting. Use
  `connect_smtp_starttls`.

In both cases the TLS handshake is the responsibility of the
[`Transport`] implementation; `wasm-smtp-core` sees an opaque byte
stream and (for STARTTLS) a single `upgrade_to_tls()` signal.

## Acceptable use

This library must not be used to deliver unsolicited bulk mail, to
impersonate other senders, or to deliver mail that violates the
operating policy of any SMTP server. See [`TERMS_OF_USE.md`].

## Documentation

Long-form documentation lives in [`docs/src`]. The mdBook structure
covers project architecture, the SMTP protocol surface, the error
taxonomy, and end-to-end usage.

## License

Apache-2.0. See [`LICENSE`] and [`NOTICE`].

[`Transport`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/trait.Transport.html
[`docs/src`]: ./docs/src
[`LICENSE`]: ./LICENSE
[`NOTICE`]: ./NOTICE
[`TERMS_OF_USE.md`]: ./TERMS_OF_USE.md
