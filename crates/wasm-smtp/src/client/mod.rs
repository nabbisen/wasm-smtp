//! High-level SMTP client.
//!
//! [`SmtpClient`] is the entry point of this crate. It owns a [`Transport`]
//! and orchestrates the full SMTP exchange: greeting, `EHLO`, optional
//! `AUTH LOGIN`, the mail transaction (`MAIL FROM`, `RCPT TO`, `DATA`, body,
//! end-of-data), and `QUIT`.
//!
//! ## Lifecycle
//!
//! ```text
//!   SmtpClient::connect(transport, ehlo_domain)
//!         |
//!         v
//!   [optional] login(user, pass)
//!         |
//!         v
//!   send_mail(from, &[to], body)   <-- may be called more than once
//!         |
//!         v
//!   quit()                          <-- consumes self
//! ```
//!
//! Each method advances [`SessionState`]. Misordered calls (for example,
//! `send_mail` before `connect`, or any operation after `quit`) return
//! [`InvalidInputError`] without touching the wire.

use crate::error::{AuthError, InvalidInputError, ProtocolError, SmtpError, SmtpOp};
use crate::protocol::{self, MAX_REPLY_LINE_LEN, format_command};
use crate::session::SessionState;
use crate::tracing_helpers::{smtp_debug, smtp_trace, smtp_warn};
use crate::transport::Transport;

mod auth;
mod io;
mod send;
mod starttls;

pub(super) const READ_CHUNK: usize = 1024;
pub(super) const RX_BUF_COMPACT_THRESHOLD: usize = 4096;
pub(super) const RX_BUF_HARD_LIMIT: usize = MAX_REPLY_LINE_LEN * 2;

/// SMTP client driving a single connection.
///
/// See the [module-level documentation](self) for the full lifecycle.
pub struct SmtpClient<T: Transport> {
    transport: T,
    state: SessionState,
    rx_buf: Vec<u8>,
    rx_pos: usize,
    capabilities: Vec<String>,
    /// The EHLO domain supplied to [`Self::connect`]. Stored so that
    /// [`Self::starttls`] can re-issue `EHLO` after the TLS upgrade per
    /// RFC 3207 §4.2 without forcing the caller to pass the domain again.
    ehlo_domain: String,
    /// Whether the most recent EHLO advertised `ENHANCEDSTATUSCODES`
    /// (RFC 2034). When set, every reply parsed by [`Self::read_reply`]\
    /// is annotated with an [`crate::protocol::EnhancedStatus`] (when
    /// the leading reply line carries one), and that code is propagated
    /// into [`crate::ProtocolError::UnexpectedCode`] on failure.
    enhanced_status_enabled: bool,
    /// Pre-send policy hook. Checked before each `send_mail` call.
    policy: Box<dyn crate::policy::SendPolicy>,
    /// Audit event sink. Receives events for each session milestone.
    audit: Box<dyn crate::audit::AuditSink>,
}

// Manual `Debug` implementation. We do not require `T: Debug` because typical
// transport types (raw sockets, TLS streams) do not implement it. The
// transport is therefore omitted from the formatted output; everything else
// the caller might reasonably want to inspect is included.
impl<T: Transport> core::fmt::Debug for SmtpClient<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SmtpClient")
            .field("state", &self.state)
            .field("capabilities", &self.capabilities)
            .field("ehlo_domain", &self.ehlo_domain)
            .field("enhanced_status_enabled", &self.enhanced_status_enabled)
            .field("rx_buf_len", &self.rx_buf.len())
            .field("rx_pos", &self.rx_pos)
            .finish_non_exhaustive()
    }
}

impl<T: Transport> SmtpClient<T> {
    /// Connect by reading the server greeting and performing the `EHLO`
    /// handshake.
    ///
    /// `transport` must already be connected and, if Implicit TLS is in use,
    /// already past the TLS handshake. `ehlo_domain` is the FQDN or address
    /// literal that identifies the client to the server.
    ///
    /// On success the client is in a state where [`Self::login`] or
    /// [`Self::send_mail`] may be called.
    ///
    /// Uses [`DefaultPolicy`][crate::policy::DefaultPolicy] (allow all) and
    /// [`NoopAuditSink`][crate::audit::NoopAuditSink] (no events). To attach
    /// a policy or audit sink, use [`SmtpClientOptions`] with
    /// [`Self::connect_with`].
    pub async fn connect(transport: T, ehlo_domain: &str) -> Result<Self, SmtpError> {
        Self::connect_with(transport, ehlo_domain, SmtpClientOptions::default()).await
    }

    /// Connect with explicit policy and audit options.
    ///
    /// ```rust
    /// # use wasm_smtp::policy::BoundedPolicy;
    /// # use wasm_smtp::SmtpClientOptions;
    /// let opts = SmtpClientOptions::new()
    ///     .with_policy(Box::new(BoundedPolicy::new().max_recipients(50)));
    /// // let client = SmtpClient::connect_with(transport, "client.example.com", opts).await?;
    /// ```
    pub async fn connect_with(
        transport: T,
        ehlo_domain: &str,
        options: SmtpClientOptions,
    ) -> Result<Self, SmtpError> {
        protocol::validate_ehlo_domain(ehlo_domain)?;
        smtp_debug!(ehlo_domain = %ehlo_domain, "SMTP session: connect");
        options.audit.on_event(&crate::audit::SmtpAuditEvent::Connected);
        let mut client = Self {
            transport,
            state: SessionState::Greeting,
            rx_buf: Vec::with_capacity(READ_CHUNK),
            rx_pos: 0,
            capabilities: Vec::new(),
            ehlo_domain: ehlo_domain.to_owned(),
            enhanced_status_enabled: false,
            policy: options.policy,
            audit: options.audit,
        };
        client.read_greeting().await?;
        client.send_ehlo(ehlo_domain).await?;
        smtp_debug!(
            capability_count = client.capabilities.len(),
            "SMTP session: ready"
        );
        Ok(client)
    }

    /// The capability lines returned by the server in its `EHLO` reply.
    ///
    /// The first reply line (the greeting) is excluded; each remaining entry
    /// is one advertised extension, for example `"AUTH LOGIN PLAIN"`,
    /// `"PIPELINING"`, or `"8BITMIME"`.
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    /// The current session state. Mostly useful for diagnostics and tests.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Authenticate using the best `AUTH` mechanism the server advertised.
    ///
    /// `PLAIN` is preferred over `LOGIN` when both are advertised, because
    /// it completes in a single round-trip and is the IETF-standard SASL
    /// mechanism. `LOGIN` is used as a fallback for older servers that
    /// only advertise it. Callers that need to lock in a specific
    /// mechanism (for testing, or for known-broken servers) should call
    /// [`Self::login_with`] instead.
    ///
    /// Returns [`AuthError::UnsupportedMechanism`] if the server's `EHLO`
    /// reply did not advertise either `PLAIN` or `LOGIN`. Returns
    /// [`AuthError::Rejected`] if the server rejects the credentials.
    ///
    /// May only be called immediately after [`Self::connect`]. Calling it
    /// a second time, or after [`Self::send_mail`], returns
    /// [`InvalidInputError`].
    ///
    /// # Credential lifetime and zeroization
    ///
    /// `wasm-smtp` does not retain copies of `user` or `pass` after
    /// this call returns: the credentials are passed by reference, used
    /// once to build a base64-encoded SASL payload, and dropped together
    /// with that payload at the end of the call. The crate also never
    /// includes credentials in [`Debug`](core::fmt::Debug) output, error
    /// messages, or [`Display`](core::fmt::Display) text.
    ///
    /// What the crate cannot do is securely erase the bytes the caller
    /// supplied — that storage belongs to the caller. If your threat
    /// model includes memory disclosure (a process dump, a debugger
    /// attached to the running Worker, etc.), wrap the password in a
    /// type that zeroes its backing memory on drop (the `zeroize` crate
    /// is the conventional choice) and pass `&z.expose_secret()` only at
    /// the call site. Concretely, avoid pulling the password out of an
    /// environment variable into a long-lived `String`.

    /// Send `QUIT` and close the transport.
    ///
    /// Consumes `self` so the client cannot be reused after a clean
    /// shutdown. If the underlying transport's `close` fails, the SMTP
    /// `QUIT` may still have completed cleanly; the returned error wraps
    /// the transport-level failure.
    pub async fn quit(mut self) -> Result<(), SmtpError> {
        if self.state == SessionState::Closed {
            smtp_trace!("quit: already closed; nothing to do");
            return Ok(());
        }
        smtp_debug!("QUIT: closing session");
        // Best-effort QUIT: if the server has already closed, we still want
        // to release the transport.
        let send_result: Result<(), SmtpError> = async {
            self.transition(SessionState::Quit)?;
            self.write_all(&format_command("QUIT")).await?;
            self.expect_code(221, SmtpOp::Quit).await?;
            Ok(())
        }
        .await;

        let close_result = self.transport.close().await;
        self.state = SessionState::Closed;

        if send_result.is_ok() && close_result.is_ok() {
            self.audit.on_event(&crate::audit::SmtpAuditEvent::QuitCompleted);
        } else {
            self.audit.on_event(&crate::audit::SmtpAuditEvent::SessionAborted);
        }

        send_result?;
        close_result.map_err(SmtpError::from)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    fn assert_state_in(&self, allowed: &[SessionState]) -> Result<(), InvalidInputError> {
        if allowed.contains(&self.state) {
            Ok(())
        } else if self.state == SessionState::Closed {
            Err(InvalidInputError::new(
                "operation not allowed: SMTP session is already closed",
            ))
        } else {
            Err(InvalidInputError::new(
                "operation not allowed in the current SMTP session state",
            ))
        }
    }

    fn transition(&mut self, next: SessionState) -> Result<(), InvalidInputError> {
        if self.state.can_transition_to(next) {
            self.state = next;
            Ok(())
        } else {
            Err(InvalidInputError::new(
                "internal session-state transition rejected",
            ))
        }
    }

    fn mark_closed_on_logical_failure(&mut self) {
        // After any unrecoverable error, the connection is poisoned. Move to
        // Closed so subsequent calls fail fast with InvalidInput.
        if self.state != SessionState::Closed {
            smtp_warn!(
                state = ?self.state,
                "session closed on logical failure; further calls will fail fast"
            );
        }
        self.state = SessionState::Closed;
    }
}


// -----------------------------------------------------------------------------
// SmtpClientOptions
// -----------------------------------------------------------------------------

/// Options for configuring an [`SmtpClient`] before connecting.
///
/// Provides a send-policy hook (RFC 011) and an audit-event sink (RFC 012).
/// All settings are optional; the defaults are permissive and silent.
///
/// ## Example
///
/// ```rust
/// use wasm_smtp::policy::BoundedPolicy;
/// use wasm_smtp::audit::VecAuditSink;
/// use wasm_smtp::SmtpClientOptions;
/// use std::sync::Arc;
///
/// let sink = Arc::new(VecAuditSink::default());
/// let opts = SmtpClientOptions::new()
///     .with_policy(Box::new(BoundedPolicy::new().max_recipients(25)))
///     .with_audit(Box::new(Arc::clone(&sink)));
/// ```
pub struct SmtpClientOptions {
    /// Pre-send validation hook.
    pub(crate) policy: Box<dyn crate::policy::SendPolicy>,
    /// Audit event observer.
    pub(crate) audit: Box<dyn crate::audit::AuditSink>,
}

impl SmtpClientOptions {
    /// Create options with the default policy (allow all) and no-op audit sink.
    #[must_use]
    pub fn new() -> Self {
        Self {
            policy: Box::new(crate::policy::DefaultPolicy),
            audit: Box::new(crate::audit::NoopAuditSink),
        }
    }

    /// Attach a [`SendPolicy`][crate::policy::SendPolicy].
    #[must_use]
    pub fn with_policy(mut self, policy: Box<dyn crate::policy::SendPolicy>) -> Self {
        self.policy = policy;
        self
    }

    /// Attach an [`AuditSink`][crate::audit::AuditSink].
    #[must_use]
    pub fn with_audit(mut self, audit: Box<dyn crate::audit::AuditSink>) -> Self {
        self.audit = audit;
        self
    }
}

impl Default for SmtpClientOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for SmtpClientOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SmtpClientOptions")
            .finish_non_exhaustive()
    }
}

// -----------------------------------------------------------------------------
// Free helpers
// -----------------------------------------------------------------------------

pub(super) fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

/// Convert a generic protocol error from an AUTH-phase reply into a more
/// specific [`AuthError::Rejected`] when the server returned a 5xx code.
pub(super) fn convert_auth(err: SmtpError) -> SmtpError {
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode {
            actual,
            enhanced,
            message,
            ..
        }) if (500..600).contains(&actual) => SmtpError::Auth(AuthError::Rejected {
            code: actual,
            enhanced,
            message,
        }),
        other => other,
    }
}
