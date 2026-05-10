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
event (EHLO, AUTH, MAIL FROM, RCPT TO, DATA, QUIT). Use it for logging,
metrics, or tracing.

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
