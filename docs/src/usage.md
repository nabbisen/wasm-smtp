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
capabilities and picks the best supported mechanism: `AUTH PLAIN`
when advertised (preferred — one round-trip and the IETF-standard
SASL mechanism), falling back to `AUTH LOGIN` otherwise. This is the
right behavior for almost every caller.

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
