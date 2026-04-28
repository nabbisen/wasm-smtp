# wasm-smtp

[![License](https://img.shields.io/github/license/nabbisen/wasm-smtp)](https://github.com/nabbisen/wasm-smtp/blob/main/LICENSE)

[![crates.io](https://img.shields.io/crates/v/wasm-smtp-core?label=core)](https://crates.io/crates/wasm-smtp-core)
[![crates.io](https://img.shields.io/crates/v/wasm-smtp-cloudflare?label=cloudflare)](https://crates.io/crates/wasm-smtp-cloudflare)
[![Rust Documentation](https://docs.rs/wasm-smtp-core/badge.svg?version=latest)](https://docs.rs/wasm-smtp-core)
[![Rust Documentation](https://docs.rs/wasm-smtp-cloudflare/badge.svg?version=latest)](https://docs.rs/wasm-smtp-cloudflare)
[![Dependency Status](https://deps.rs/crate/wasm-smtp-core/latest/status.svg)](https://deps.rs/crate/wasm-smtp-core)
[![Dependency Status](https://deps.rs/crate/wasm-smtp-cloudflare/latest/status.svg)](https://deps.rs/crate/wasm-smtp-cloudflare)

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

## Cargo features

`wasm-smtp-core` exposes two cargo features that allow size-sensitive
deployments (Cloudflare Workers' 3 MiB cap, in particular) to opt out
of functionality they will not use:

| Feature    | Default | What it adds                                                                                              |
|------------|---------|-----------------------------------------------------------------------------------------------------------|
| `xoauth2`  | **on**  | `SmtpClient::login_xoauth2`, `AuthMechanism::XOAuth2` code paths, OAuth 2.0 token validation helpers      |
| `smtputf8` | off     | `SmtpClient::send_mail_smtputf8`, `validate_address_utf8`, `format_mail_from_smtputf8`, capability check |

Defaults are chosen so that v0.3.x users see no behavior change on
upgrade. To strip OAuth 2.0 support entirely (typical for transactional
senders against a self-hosted Postfix or commercial relay using static
passwords):

```toml
wasm-smtp-core = { version = "0.4", default-features = false }
```

To opt into international addresses while keeping the OAuth 2.0
support:

```toml
wasm-smtp-core = { version = "0.4", features = ["smtputf8"] }
```

The `wasm-smtp-cloudflare` adapter exposes a matching `smtputf8`
feature that pass-through-enables it on the core crate, so adapter-
only callers do not need a direct dependency on `wasm-smtp-core` to
opt in.

## Acceptable use

This library must not be used to deliver unsolicited bulk mail, to
impersonate other senders, or to deliver mail that violates the
operating policy of any SMTP server. See [`TERMS_OF_USE.md`].

## Documentation

Long-form documentation lives in [`docs/src`]. The mdBook structure
covers project architecture, the SMTP protocol surface, the error
taxonomy, and end-to-end usage.

[`Transport`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/trait.Transport.html
[`docs/src`]: ./docs/src
[`LICENSE`]: ./LICENSE
[`NOTICE`]: ./NOTICE
[`TERMS_OF_USE.md`]: ./TERMS_OF_USE.md
