# Usage

This page is a tour of the `wasm-smtp-core` API as a user would
encounter it. The Cloudflare adapter is referenced by name only; until
Phase 3 lands, the examples here use a `Transport` you supply yourself
(typically via the in-tree mock or a hand-written stub on top of
`wasmtime-wasi`, `tokio`, or whatever runtime you happen to be using
locally).

## A complete send

```rust
use wasm_smtp_core::{SmtpClient, Transport, SmtpError};

async fn send_one<T: Transport>(transport: T) -> Result<(), SmtpError> {
    let mut client = SmtpClient::connect(transport, "client.example.com").await?;
    client.login("user@example.com", "secret").await?;
    let body = "From: user@example.com\r\n\
                To: recipient@example.org\r\n\
                Subject: Hello\r\n\
                MIME-Version: 1.0\r\n\
                Content-Type: text/plain; charset=utf-8\r\n\
                \r\n\
                Hello from a Worker.\r\n";
    client.send_mail(
        "user@example.com",
        &["recipient@example.org"],
        body,
    ).await?;
    client.quit().await?;
    Ok(())
}
```

Everything in this example is the entire API. There is no separate
"begin transaction", "send headers", or "set timeout" call. The body
is a fully-formed RFC 5322 message; `send_mail` adds nothing to it
beyond dot-stuffing and the terminator.

## Multiple messages on one connection

```rust
let mut client = SmtpClient::connect(transport, "client.example.com").await?;
client.login("user@example.com", "secret").await?;

for (rcpt, body) in messages {
    client.send_mail("user@example.com", &[rcpt], body).await?;
}

client.quit().await?;
```

After `send_mail` returns, the client is back in `MailFrom` state and
is ready for another transaction. RFC 5321 §3.3 explicitly allows this,
and many submission servers will be more efficient if you batch
messages on a single connection.

## STARTTLS (port 587)

For submission servers that require an in-place TLS upgrade rather
than Implicit TLS on port 465, use the STARTTLS flow. The transport
must be one that implements `StartTlsCapable` (the Cloudflare adapter
does, when constructed via `connect_starttls`).

The convenience entry point performs the entire upgrade in one call:

```rust
use wasm_smtp_core::{SmtpClient, SmtpError, StartTlsCapable, Transport};

async fn send_via_starttls<T: StartTlsCapable>(transport: T) -> Result<(), SmtpError> {
    // Plaintext connect, EHLO, STARTTLS, transport upgrade, re-EHLO.
    let mut client = SmtpClient::connect_starttls(transport, "client.example.com").await?;
    // The client is now in Authentication state on the upgraded stream.
    client.login("user@example.com", "secret").await?;
    // ... usual send_mail / quit
    # Ok(())
}
```

If you want to inspect the pre-TLS capabilities first (or take some
runtime decision based on them) the explicit two-call form is also
available:

```rust
let mut client = SmtpClient::connect(transport, "client.example.com").await?;
// Pre-TLS capabilities are visible here.
if client.capabilities().iter().any(|c| c.eq_ignore_ascii_case("STARTTLS")) {
    client.starttls().await?;
}
// After starttls() the post-TLS capabilities have replaced the pre-TLS ones.
client.login("user@example.com", "secret").await?;
```

The state machine enforces ordering: `starttls()` may be called only
immediately after `connect()`, and is rejected with `InvalidInput`
once `login` or `send_mail` has been called. Per RFC 3207 §4.2 the
client re-issues `EHLO` after the upgrade, which `wasm-smtp-core`
does for you — the post-TLS capability list is visible via
`client.capabilities()` after `starttls()` returns.

If the server does not advertise `STARTTLS`, `starttls()` returns
`ProtocolError::ExtensionUnavailable { name: "STARTTLS" }` and moves
the client to `Closed` rather than silently falling through to
plaintext. This is deliberate: a caller that asked for STARTTLS
should never end up authenticating in cleartext.

## Skipping authentication

For a relay that does not require authentication:

```rust
let mut client = SmtpClient::connect(transport, "client.example.com").await?;
// No call to login() — go straight to send_mail.
client.send_mail("user@example.com", &["recipient@example.org"], body).await?;
client.quit().await?;
```

The state machine accepts the `Authentication → MailFrom` skip
directly.

## Choosing an authentication mechanism

`SmtpClient::login(user, pass)` consults the server's `EHLO`
capabilities and picks the best supported **static-password**
mechanism: `AUTH PLAIN` when advertised (preferred — one round-trip
and the IETF-standard SASL mechanism), falling back to `AUTH LOGIN`
otherwise. This is the right behavior for almost every static-
password caller.

If you need to lock in a specific mechanism — to reproduce a
production failure that is tied to one of them, or to test against a
server whose advertisement is known to be inaccurate — call
`login_with`:

```rust
use wasm_smtp_core::AuthMechanism;

client.login_with(AuthMechanism::Plain, "user", "secret").await?;
// or:
client.login_with(AuthMechanism::Login, "user", "secret").await?;
```

`login_with` returns `AuthError::UnsupportedMechanism` if the chosen
mechanism is not in the server's advertisement, just like `login`
does when neither mechanism is advertised.

## OAuth 2.0 (XOAUTH2) for Gmail and Microsoft 365

For providers that authenticate with OAuth 2.0 access tokens rather
than static passwords — Gmail's SMTP relay and Microsoft 365's
submission endpoint are the most common — call `login_xoauth2`:

```rust
let access_token = obtain_oauth2_token().await?; // your code
client.login_xoauth2("user@example.com", &access_token).await?;
```

XOAUTH2 lives behind the `xoauth2` cargo feature, which is enabled
by default. Callers that send only via static-password SMTP relays
can drop OAuth 2.0 support with `default-features = false`, shaving
roughly 250 LOC of protocol code from the WASM bundle. Doing so
also removes the `login_xoauth2` method and the `XOAuth2` arm of
`login_with`; calling either when the feature is disabled returns
`SmtpError::InvalidInput` without performing any I/O.

`login_xoauth2` opts in explicitly to the XOAUTH2 SASL profile.
`login()` deliberately does not pick XOAUTH2 even when the server
advertises it: a static-password caller passing a stale OAuth token
under the assumption that the same auto-select logic applies would
silently fail in confusing ways.

This crate does not perform the OAuth 2.0 dance itself. Token
acquisition, refresh, scope, and storage are the caller's
responsibility. For Gmail, the relevant scope is
`https://mail.google.com/`; for Microsoft 365 it is
`https://outlook.office.com/SMTP.Send`.

When the server rejects the token, providers typically use the
RFC 7628 §3.2.3 two-step flow: a `334` with base64-encoded JSON
error detail, an empty client continuation, then a final `5.x.x`.
The crate handles this transparently and surfaces the result as
`AuthError::Rejected`. The final reply text is preserved so callers
can log the provider's diagnostic.

## Reading enhanced status codes

When the server advertises `ENHANCEDSTATUSCODES` (RFC 2034), every
reply carries a structured `class.subject.detail` code in addition
to the basic three-digit code. The crate parses these into
`EnhancedStatus` and exposes them on errors so callers can route on
the structured code instead of grepping the message text:

```rust
use wasm_smtp_core::{EnhancedStatus, ProtocolError, SmtpError};

match client.send_mail(from, &[to], body).await {
    Ok(()) => {}
    Err(SmtpError::Protocol(ProtocolError::UnexpectedCode {
        enhanced: Some(EnhancedStatus { class: 5, subject: 1, .. }),
        ..
    })) => {
        // 5.1.x — bad address. No retry.
    }
    Err(SmtpError::Protocol(ProtocolError::UnexpectedCode {
        enhanced: Some(EnhancedStatus { class: 4, .. }),
        ..
    })) => {
        // 4.x.x — transient. Retry later.
    }
    Err(other) => return Err(other),
}
```

The same structured field is present on `AuthError::Rejected`, which
is useful for distinguishing `5.7.8` (invalid credentials) from
`5.7.9` (mechanism too weak) without parsing message text.

If the server does not advertise the extension, `enhanced` is `None`
even when the reply text happens to contain a `class.subject.detail`-
shaped string — the parse is gated on advertisement, by design.

## International addresses (SMTPUTF8)

For envelope addresses outside the ASCII repertoire — Japanese
mailbox names, IDN U-label domains, and so on — enable the
`smtputf8` cargo feature and use `send_mail_smtputf8` instead of
`send_mail`:

```toml
[dependencies]
wasm-smtp-core = { version = "0.4", features = ["smtputf8"] }
# or, via the cloudflare adapter (which re-exports the feature):
wasm-smtp-cloudflare = { version = "0.4", features = ["smtputf8"] }
```

```rust
client.send_mail_smtputf8(
    "\u{9001}\u{4FE1}@example.jp",
    &["\u{53D7}\u{4FE1}@\u{4F8B}\u{3048}.jp"],
    "Subject: hello\r\n\r\nbody\r\n",
).await?;
```

The method validates addresses with a UTF-8-permissive validator
(rejecting only structural hazards like CR/LF/NUL/`<>`/whitespace
and ASCII / C1 control characters), and emits the SMTPUTF8 ESMTP
parameter on `MAIL FROM`. If the server did not advertise
`SMTPUTF8` in its `EHLO` reply, the call returns
`ProtocolError::ExtensionUnavailable { name: "SMTPUTF8" }` without
sending any bytes — there is no silent fallback to ASCII.

The feature is gated because the relevant code is dead weight for
most callers (typical transactional submission uses ASCII-only
addresses) and adding ~5 KB to a Cloudflare Workers bundle for an
unused feature is a real cost there. When the feature is disabled,
the normal `send_mail` continues to enforce strict ASCII as before.

## Inspecting capabilities

After `connect`, the EHLO capability lines are exposed as a slice:

```rust
let client = SmtpClient::connect(transport, "client.example.com").await?;
for line in client.capabilities() {
    println!("server advertises: {line}");
}
```

The greeting line is excluded. Each remaining entry is one extension as
the server reported it (e.g. `"AUTH LOGIN PLAIN"`, `"PIPELINING"`,
`"8BITMIME"`).

## Body construction

The library does not build MIME for you. If you need anything beyond
plain text, you have two options:

1. Build the body yourself, with `\r\n` line endings, headers separated
   from body by a blank line. This is straightforward for plain text
   and basic HTML mail.
2. Use a separate MIME crate (e.g. `lettre`'s message builder) to
   produce the byte string, then pass it as `body` here.

Either way, `send_mail` will dot-stuff and terminate the bytes you
give it; nothing else.

## Errors and retries

```rust
match client.send_mail(from, recipients, body).await {
    Ok(()) => log::info!("delivered"),
    Err(wasm_smtp_core::SmtpError::Io(e)) => {
        log::warn!("transport failure, will retry: {e}");
        // Re-establish the connection on the next attempt.
    }
    Err(wasm_smtp_core::SmtpError::Protocol(p)) => {
        log::error!("server protocol violation: {p}");
        // Usually permanent. Log the full error for diagnostics.
    }
    Err(wasm_smtp_core::SmtpError::Auth(a)) => {
        log::error!("auth failed: {a}");
        // Almost always a credentials problem.
    }
    Err(wasm_smtp_core::SmtpError::InvalidInput(i)) => {
        // The library refused to send what we asked it to send. This
        // is a programmer error in the calling code: a malformed
        // address or an out-of-order call.
        unreachable!("client bug: {i}");
    }
}
```

See [Errors](./errors.md) for the full taxonomy and which states the
client moves to after each kind of failure.

## Testing your code

Because `SmtpClient` is generic over `Transport`, you can drive it
against any synchronous mock you like. The crate's own
`tests::harness::MockTransport` is private, but the *pattern* is
straightforward to reproduce: a struct with a `VecDeque<Vec<u8>>` of
scripted server replies and a `Vec<u8>` for captured outgoing bytes.
This lets you write SMTP-flow tests with no executor at all.
