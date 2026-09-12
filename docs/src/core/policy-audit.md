# Policy and audit hooks

`wasm-smtp` provides two extension points that let callers intercept
every send operation without modifying the core library.

## SendPolicy

`SendPolicy` is called before any SMTP command is sent. It can veto a
transaction on the basis of the sender address, recipient list, or
estimated message size.

```rust
use wasm_smtp::policy::{SendPolicy, PolicyError};

struct BlockBigMessages;

impl SendPolicy for BlockBigMessages {
    fn check_sender(&self, _from: &str) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_recipients(&self, _to: &[&str]) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_message_size(&self, bytes: usize) -> Result<(), PolicyError> {
        if bytes > 10 * 1024 * 1024 {
            return Err(PolicyError::new("message exceeds 10 MiB limit"));
        }
        Ok(())
    }
}
```

Attach the policy when building the client:

```rust
use wasm_smtp::client::SmtpClientOptions;

let opts = SmtpClientOptions::new()
    .with_policy(Box::new(BlockBigMessages));

let mut client = SmtpClient::connect_with(transport, "client.example.com", opts).await?;
```

When `check_message_size` is called from `send_mail_stream`, the
size argument is `usize::MAX` because the total body size is not
known in advance. Callers that need precise size enforcement should
use `send_mail_bytes` instead.

## AuditSink

`AuditSink` receives a `SmtpAuditEvent` for each significant protocol
event. Use it for logging, metrics, or tracing.

The full set, in the order a successful authenticated send produces
them:

| Event | When |
|---|---|
| `Connected` | the transport is up, before the greeting is read |
| `GreetingReceived { code }` | the server's greeting was accepted |
| `EhloCompleted` | `EHLO` accepted; capabilities recorded |
| `TlsUpgraded` | a STARTTLS upgrade completed (between the two `EhloCompleted`s) |
| `AuthCompleted { mechanism }` | authentication succeeded; the mechanism is the wire keyword, e.g. `"SCRAM-SHA-256"` |
| `MailFromAccepted { code }` | `MAIL FROM` accepted |
| `RecipientAccepted { code }` | one `RCPT TO` accepted (250 or 251) |
| `RecipientRejected { code }` | one `RCPT TO` refused (4xx or 5xx), emitted before the error propagates |
| `MessageAccepted { code }` | the server accepted the message body |
| `QuitCompleted` | `QUIT` and the transport close both succeeded |
| `SessionAborted` | the session failed and will accept no further commands |

`SessionAborted` is emitted exactly once per session, from the single
place that closes the state machine on failure — an unexpected reply, a
rejected recipient, or a transport error all reach it, and a `quit` on an
already-aborted session does not add a second. A clean session never
emits it. When a recipient rejection is what ended the session, the
`RecipientRejected` event precedes the abort, so a log reader sees the
cause before the effect.

No event carries credentials, message content, or envelope addresses;
codes and mechanism names only.

```rust
use wasm_smtp::audit::{AuditSink, SmtpAuditEvent};

struct MetricsSink;

impl AuditSink for MetricsSink {
    fn on_event(&self, event: &SmtpAuditEvent) {
        match event {
            SmtpAuditEvent::MessageAccepted { code } => {
                // increment "smtp.messages.accepted" counter
                println!("Message accepted with code {code}");
            }
            SmtpAuditEvent::AuthCompleted { mechanism } => {
                println!("Authenticated via {mechanism}");
            }
            _ => {}
        }
    }
}
```

```rust
let opts = SmtpClientOptions::new()
    .with_audit(Box::new(MetricsSink));
```

## VecAuditSink

`VecAuditSink` is a built-in sink for testing. It stores every event
as a `String` and exposes them via `.events()`:

```rust
use wasm_smtp::audit::VecAuditSink;
use std::sync::Arc;

let sink = Arc::new(VecAuditSink::default());
let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));

// … run the client …

let events = sink.events();
assert!(events.iter().any(|e| e.starts_with("MessageAccepted")));
```
