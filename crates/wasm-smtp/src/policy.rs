//! Pre-send policy hook.
//!
//! [`SendPolicy`] is an application-defined gate that runs before any SMTP
//! command is sent for a given message. Use it to enforce:
//!
//! - Sender allowlists / domain restrictions.
//! - Per-send recipient count limits.
//! - Message size caps.
//! - Rate limits (backed by a counter you supply).
//!
//! ## Attaching a policy
//!
//! Pass a boxed policy to [`SmtpClientOptions`][crate::SmtpClientOptions]:
//!
//! ```rust
//! # use wasm_smtp::policy::{SendPolicy, PolicyError, DefaultPolicy};
//! # use wasm_smtp::SmtpClientOptions;
//! struct DomainPolicy { allowed: &'static str }
//!
//! impl SendPolicy for DomainPolicy {
//!     fn check_sender(&self, from: &str) -> Result<(), PolicyError> {
//!         if from.ends_with(self.allowed) {
//!             Ok(())
//!         } else {
//!             Err(PolicyError::new(format!("sender domain not allowed: {from}")))
//!         }
//!     }
//!     fn check_recipients(&self, _to: &[&str]) -> Result<(), PolicyError> { Ok(()) }
//!     fn check_message_size(&self, _bytes: usize) -> Result<(), PolicyError> { Ok(()) }
//! }
//!
//! let opts = SmtpClientOptions::new()
//!     .with_policy(Box::new(DomainPolicy { allowed: "@example.com" }));
//! ```
//!
//! ## Execution order
//!
//! For each call to [`SmtpClient::send_mail`][crate::SmtpClient::send_mail]:
//!
//! 1. [`SendPolicy::check_sender`] — before `MAIL FROM`.
//! 2. [`SendPolicy::check_recipients`] — before any `RCPT TO`.
//! 3. [`SendPolicy::check_message_size`] — before `DATA`.
//!
//! If any check fails, the send is aborted with
//! [`SmtpError::Policy`][crate::SmtpError::Policy] and no SMTP commands
//! are sent for that message.
//!
//! ## Security
//!
//! Policy implementations must not include credential or message-body data
//! in [`PolicyError`] messages, as those messages may appear in logs.

use crate::error::PolicyError;

/// Application-defined pre-send validation.
///
/// Each method is called synchronously before the corresponding SMTP
/// command. Return `Err(PolicyError::new("reason"))` to abort the send.
///
/// Implement only the methods that matter to you; the rest can return `Ok(())`.
///
/// See the [module-level documentation](self) for the execution order and
/// a worked example.
pub trait SendPolicy: Send + Sync {
    /// Validate the envelope sender address before `MAIL FROM`.
    fn check_sender(&self, from: &str) -> Result<(), PolicyError>;

    /// Validate the complete recipient list before any `RCPT TO`.
    ///
    /// All recipients are supplied at once so that cross-recipient policies
    /// (e.g. maximum recipient count) can be enforced in a single check.
    fn check_recipients(&self, recipients: &[&str]) -> Result<(), PolicyError>;

    /// Validate the message size in bytes before `DATA`.
    ///
    /// The `bytes` argument is the byte length of the raw message body passed
    /// to `send_mail`. Dot-stuffing and CRLF normalisation may slightly
    /// increase the on-wire size; this check uses the pre-normalisation length.
    fn check_message_size(&self, bytes: usize) -> Result<(), PolicyError>;
}

/// Default policy that permits every send operation.
///
/// Used when no policy is configured. Has zero overhead.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicy;

impl SendPolicy for DefaultPolicy {
    fn check_sender(&self, _from: &str) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_recipients(&self, _recipients: &[&str]) -> Result<(), PolicyError> {
        Ok(())
    }
    fn check_message_size(&self, _bytes: usize) -> Result<(), PolicyError> {
        Ok(())
    }
}

/// A [`SendPolicy`] that enforces configurable limits.
///
/// Useful as a hardening layer without requiring a custom implementation.
///
/// ```rust
/// use wasm_smtp::policy::BoundedPolicy;
/// use wasm_smtp::SmtpClientOptions;
///
/// let opts = SmtpClientOptions::new()
///     .with_policy(Box::new(BoundedPolicy::new()
///         .max_recipients(10)
///         .max_message_bytes(5 * 1024 * 1024))); // 5 MB
/// ```
#[derive(Debug, Clone)]
pub struct BoundedPolicy {
    max_recipients: Option<usize>,
    max_message_bytes: Option<usize>,
}

impl BoundedPolicy {
    /// Construct with no limits. Equivalent to [`DefaultPolicy`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_recipients: None,
            max_message_bytes: None,
        }
    }

    /// Set the maximum number of recipients per message.
    #[must_use]
    pub const fn max_recipients(mut self, n: usize) -> Self {
        self.max_recipients = Some(n);
        self
    }

    /// Set the maximum message body size in bytes.
    #[must_use]
    pub const fn max_message_bytes(mut self, bytes: usize) -> Self {
        self.max_message_bytes = Some(bytes);
        self
    }
}

impl Default for BoundedPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl SendPolicy for BoundedPolicy {
    fn check_sender(&self, _from: &str) -> Result<(), PolicyError> {
        Ok(())
    }

    fn check_recipients(&self, recipients: &[&str]) -> Result<(), PolicyError> {
        if let Some(max) = self.max_recipients {
            if recipients.len() > max {
                return Err(PolicyError::new(format!(
                    "too many recipients: {actual} exceeds limit of {max}",
                    actual = recipients.len(),
                )));
            }
        }
        Ok(())
    }

    fn check_message_size(&self, bytes: usize) -> Result<(), PolicyError> {
        if let Some(max) = self.max_message_bytes {
            if bytes > max {
                return Err(PolicyError::new(format!(
                    "message too large: {bytes} bytes exceeds limit of {max} bytes",
                )));
            }
        }
        Ok(())
    }
}
