//! SMTP session audit events.
//!
//! [`AuditSink`] is an application-supplied observer that receives a
//! structured event for each meaningful milestone in the SMTP session.
//! Events carry machine-readable fields (reply codes, mechanism names)
//! but never include credentials or message body content.
//!
//! ## Attaching a sink
//!
//! Pass a boxed sink to [`SmtpClientOptions`][crate::SmtpClientOptions]:
//!
//! ```rust
//! use wasm_smtp::audit::{AuditSink, SmtpAuditEvent};
//! use wasm_smtp::SmtpClientOptions;
//! use std::sync::{Arc, Mutex};
//!
//! #[derive(Default, Clone)]
//! struct CounterSink {
//!     messages_accepted: Arc<Mutex<u32>>,
//! }
//!
//! impl AuditSink for CounterSink {
//!     fn on_event(&self, event: &SmtpAuditEvent<'_>) {
//!         if let SmtpAuditEvent::MessageAccepted { .. } = event {
//!             *self.messages_accepted.lock().unwrap() += 1;
//!         }
//!     }
//! }
//!
//! let sink = CounterSink::default();
//! let opts = SmtpClientOptions::new()
//!     .with_audit(Box::new(sink));
//! ```
//!
//! ## Event sequence — successful authenticated send
//!
//! ```text
//! Connected
//! GreetingReceived { code: 220 }
//! EhloCompleted
//! AuthCompleted { mechanism: "AUTH SCRAM-SHA-256" }
//! MailFromAccepted { code: 250 }
//! RecipientAccepted { code: 250 }   (once per recipient)
//! MessageAccepted { code: 250 }
//! QuitCompleted
//! ```
//!
//! For STARTTLS connections, `TlsUpgraded` appears between
//! `EhloCompleted` (plaintext) and the second `EhloCompleted` (post-TLS).
//!
//! ## Security
//!
//! No event carries credentials, message body content, or server reply text.
//! `MessageAccepted` carries only the reply code (not the queue ID or any
//! server-supplied message). Implementations of `AuditSink` are
//! caller-supplied; the caller is responsible for the security of the sink.

/// Observer called on each SMTP session milestone.
///
/// The method is synchronous. If you need async processing (writing to a
/// database, pushing to a metrics endpoint), spawn a task from inside
/// `on_event` and send the event over a channel.
///
/// `AuditSink` is object-safe; you can box it as `Box<dyn AuditSink>`.
pub trait AuditSink: Send + Sync {
    /// Called once for each [`SmtpAuditEvent`] emitted by the session.
    fn on_event(&self, event: &SmtpAuditEvent<'_>);
}

/// A named session milestone emitted by the SMTP state machine.
///
/// The enum is `#[non_exhaustive]` so that future milestones can be added
/// without a breaking change. Sinks should include a `_ => {}` arm in
/// their match.
#[derive(Debug)]
#[non_exhaustive]
pub enum SmtpAuditEvent<'a> {
    /// TCP connection established (before the SMTP greeting).
    Connected,
    /// Server greeting received.
    GreetingReceived {
        /// The greeting reply code (typically 220).
        code: u16,
    },
    /// `EHLO` exchange completed; capability list has been parsed.
    EhloCompleted,
    /// TLS upgrade completed (STARTTLS path only).
    TlsUpgraded,
    /// Authentication completed successfully.
    AuthCompleted {
        /// The SASL mechanism used (e.g. `"AUTH SCRAM-SHA-256"`).
        mechanism: &'a str,
    },
    /// `MAIL FROM` accepted by the server.
    MailFromAccepted {
        /// Server reply code (typically 250).
        code: u16,
    },
    /// `RCPT TO` accepted for one recipient.
    RecipientAccepted {
        /// Server reply code (250 or 251).
        code: u16,
    },
    /// `RCPT TO` rejected for one recipient (4xx or 5xx).
    RecipientRejected {
        /// Server reply code.
        code: u16,
    },
    /// `DATA` body accepted; message has been queued by the server.
    MessageAccepted {
        /// Server reply code (typically 250).
        code: u16,
    },
    /// `QUIT` exchange completed and transport closed cleanly.
    QuitCompleted,
    /// Session ended abnormally (transport error or unexpected server close).
    SessionAborted,
}

/// [`AuditSink`] that does nothing. Used when no sink is configured.
///
/// Zero overhead: the compiler can eliminate all calls to `on_event`
/// for this type.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAuditSink;

impl AuditSink for NoopAuditSink {
    #[inline]
    fn on_event(&self, _event: &SmtpAuditEvent<'_>) {}
}

/// [`AuditSink`] that collects all events into a `Vec`.
///
/// Useful in tests to verify the exact event sequence emitted by a session.
///
/// ```rust
/// use wasm_smtp::audit::{VecAuditSink, SmtpAuditEvent};
/// use wasm_smtp::SmtpClientOptions;
/// use std::sync::{Arc, Mutex};
///
/// let sink = Arc::new(VecAuditSink::default());
/// let opts = SmtpClientOptions::new()
///     .with_audit(Box::new(Arc::clone(&sink)));
///
/// // ... run the session ...
///
/// // After the session:
/// let events = sink.events();
/// assert!(matches!(events[0], SmtpAuditEvent::Connected));
/// ```
#[derive(Debug, Default)]
pub struct VecAuditSink {
    events: std::sync::Mutex<Vec<String>>,
}

impl VecAuditSink {
    /// Consume a snapshot of the event names collected so far.
    ///
    /// Returns a `Vec<String>` where each entry is the `Debug` label of the
    /// event variant (e.g. `"Connected"`, `"GreetingReceived { code: 220 }"`).
    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    /// Return the number of events collected.
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Return `true` if no events have been collected.
    pub fn is_empty(&self) -> bool {
        self.events.lock().unwrap().is_empty()
    }
}

impl AuditSink for VecAuditSink {
    fn on_event(&self, event: &SmtpAuditEvent<'_>) {
        self.events
            .lock()
            .unwrap()
            .push(format!("{event:?}"));
    }
}

impl AuditSink for Arc<VecAuditSink> {
    fn on_event(&self, event: &SmtpAuditEvent<'_>) {
        self.as_ref().on_event(event);
    }
}

use std::sync::Arc;
