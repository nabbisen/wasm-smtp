# Connection reuse

For low-volume notification email — one message every few seconds or
slower — the simplest pattern is "connect, send, quit" per message.
For higher volumes, opening a new TLS handshake per message becomes
the bottleneck: the TCP three-way + TLS handshake easily costs 100ms
per message, and a 50-message burst that should take half a second
ends up taking five.

`wasm-smtp` supports SMTP's native connection-reuse pattern: a
single `SmtpClient` instance can submit any number of messages
(distinct envelopes, distinct bodies) over the same authenticated
session before quitting.

## When to reuse

| Pattern                                  | Typical scale       | Reuse?                       |
|------------------------------------------|---------------------|------------------------------|
| Per-request notification (web app form)  | 1 mail per request  | No — connect-send-quit       |
| Cron / batch processing                  | Tens-to-hundreds    | **Yes** — reuse within batch |
| Background queue worker                  | Continuous          | **Yes** — short-lived pool   |
| Per-event audit logging                  | Hundreds-to-thousands per minute | **Yes** — reuse aggressively |

Reuse is only safe within a single logical batch / request. Don't
hold an SMTP connection open across an HTTP request boundary in a
web server unless you have a real reason — most submission servers
will time the connection out after 5-10 minutes of idle time, and
the resulting "connection closed" surface is more painful than the
extra handshake.

## How it works

After a successful `send_mail` (or `send_message`, `send_mail_smtputf8`),
the session state returns to `MailFrom`. From `MailFrom` you can
issue another `send_mail` and the crate will start a fresh SMTP
transaction (`MAIL FROM:` → `RCPT TO:` → `DATA` → body) over the
same connection.

```rust,ignore
use wasm_smtp::SmtpClient;
use wasm_smtp_tokio::TokioTlsTransport;

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let transport = TokioTlsTransport::connect_implicit_tls(
    "smtp.example.com", 465, "smtp.example.com",
).await?;
let mut client = SmtpClient::connect(transport, "client.example.com").await?;
client.login("user@example.com", "secret").await?;

// First message.
client.send_mail(
    "user@example.com",
    &["alice@example.org"],
    "Subject: hi\r\n\r\nfirst\r\n",
).await?;

// Second message — same connection, no re-auth.
client.send_mail(
    "user@example.com",
    &["bob@example.org"],
    "Subject: hi\r\n\r\nsecond\r\n",
).await?;

// ...as many as you need.

client.quit().await?;
# Ok(())
# }
```

The login persists for the lifetime of the connection, so each
subsequent `send_mail` skips the authentication round-trip. The
TLS handshake is also amortized.

## What state persists

| State item                           | Persists across `send_mail`? |
|--------------------------------------|------------------------------|
| TCP / TLS connection                 | Yes                          |
| Authentication (PLAIN/LOGIN/XOAUTH2) | Yes                          |
| EHLO capabilities                    | Yes (cached)                 |
| Envelope (`MAIL FROM` / `RCPT TO`)   | No (each transaction is fresh) |
| Message body                         | No                           |

There is no need to call `EHLO` again, login again, or set up TLS
again between messages.

## Idle timeouts and graceful failure

SMTP servers idle-disconnect their clients. Common limits:

| Server                 | Typical idle timeout |
|------------------------|----------------------|
| Postfix `smtpd`        | 5 min                |
| Microsoft 365 / Gmail  | 5–10 min             |
| Many ESPs              | 30 s – 2 min         |

If your batch is short (a few seconds to a minute), this is not a
concern. For longer-lived connections, treat any `send_mail` failure
that comes with `SmtpError::Io` as a signal to drop the connection
and reconnect:

```rust,ignore
use wasm_smtp::SmtpError;
# async fn try_send(client: &mut wasm_smtp::SmtpClient<impl wasm_smtp::Transport>) -> Result<(), SmtpError> {
match client.send_mail("a@x.com", &["b@x.com"], "...").await {
    Ok(()) => Ok(()),
    Err(SmtpError::Io(_)) => {
        // Connection's gone. Drop the client, reconnect fresh,
        // and retry. Don't try to revive the existing client —
        // SmtpError::Io closes the session.
        Err(SmtpError::Io(wasm_smtp::IoError::new("reconnect needed")))
    }
    Err(e) => Err(e),
}
# }
```

Note that once `SmtpClient` returns `SmtpError::Io` from any method,
the session is moved to `Closed` and further calls will fail-fast.
This is intentional: a connection that has produced an I/O error is
not safe to reuse — you don't know what state the wire is in. Drop
it and reconnect.

## QUIT vs drop

The `Drop` impl on `SmtpClient` does **not** send `QUIT`. This is a
deliberate design choice: `Drop` is synchronous, but `QUIT` is an
async I/O operation, and the only standard way to fire it would be
to spawn a task or block in `Drop` — both of which have surprising
semantics in async runtimes.

Polite shutdown (the server logs a clean disconnect; useful for
ESPs that track reputation) is the caller's responsibility:

```rust,ignore
client.quit().await?;
// ... or, if you don't care: just let the client go out of scope.
```

If you skip `quit()`, the TCP connection is closed by the OS when
the transport is dropped. The server sees a connection-reset
rather than a clean QUIT, but no message is lost (any in-flight
transaction has already received its 250 reply by the time you
reach this point in normal code).

## Connection pooling: not built in

`wasm-smtp` does not ship a connection pool, and there are no
plans to add one. The reasoning:

- A pool's value is in cross-request reuse, but submission
  endpoints idle-disconnect aggressively, so the pool's hit rate
  is poor in practice.
- Cloudflare Workers (one of the two main runtimes) doesn't have
  a "long-lived background process" to host a pool in.
- Keeping the connection model simple lets the SMTP state machine
  stay simple. Adding pooling brings ownership / Send / lifetime
  questions that don't have a single right answer for every
  runtime.

If you do need a pool — typically for a tokio service handling
high steady-state mail volume — build one at the application layer
using whatever pool primitive matches your runtime (`bb8`, `deadpool`,
or a hand-rolled `Mutex<Option<SmtpClient>>` is often enough).
The `Transport` types from the adapter crates are the right unit
to pool around.

## Authentication and reuse: a subtle point

Most submission servers accept a single `AUTH` per connection and
reject a second one with a 503 ("Bad sequence of commands"). This
crate does not currently expose a "re-authenticate as a different
user on the same connection" path, and the SMTP state machine
treats `AUTH` as a one-shot transition out of `Authentication`.

Reuse a connection only with the same authenticated identity. If
you need to switch users — for instance, multi-tenant submission
where each tenant has their own SMTP credentials — close the
connection and reconnect with the new credentials. This is rare in
practice; most multi-tenant setups have a single shared sender
identity and use the message headers to distinguish tenants.
