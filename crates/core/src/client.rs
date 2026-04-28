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
use crate::protocol::{
    self, AuthMechanism, MAX_REPLY_LINE_LEN, MAX_REPLY_LINES, Reply,
    build_auth_plain_initial_response, dot_stuff_and_terminate, ehlo_advertises_auth,
    format_command, format_command_arg, format_mail_from, format_rcpt_to, parse_reply_line,
    select_auth_mechanism,
};
use crate::session::SessionState;
use crate::transport::Transport;

const READ_CHUNK: usize = 1024;
const RX_BUF_COMPACT_THRESHOLD: usize = 4096;
const RX_BUF_HARD_LIMIT: usize = MAX_REPLY_LINE_LEN * 2;

/// SMTP client driving a single connection.
///
/// See the [module-level documentation](self) for the full lifecycle.
pub struct SmtpClient<T: Transport> {
    transport: T,
    state: SessionState,
    rx_buf: Vec<u8>,
    rx_pos: usize,
    capabilities: Vec<String>,
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
    pub async fn connect(transport: T, ehlo_domain: &str) -> Result<Self, SmtpError> {
        protocol::validate_ehlo_domain(ehlo_domain)?;
        let mut client = Self {
            transport,
            state: SessionState::Greeting,
            rx_buf: Vec::with_capacity(READ_CHUNK),
            rx_pos: 0,
            capabilities: Vec::new(),
        };
        client.read_greeting().await?;
        client.send_ehlo(ehlo_domain).await?;
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
    pub async fn login(&mut self, user: &str, pass: &str) -> Result<(), SmtpError> {
        if let Some(mech) = select_auth_mechanism(&self.capabilities) {
            self.login_with(mech, user, pass).await
        } else {
            // Validate inputs first so the caller still gets a clean
            // InvalidInputError on empty credentials, even if the
            // server would have refused us anyway.
            protocol::validate_plain_username(user)?;
            protocol::validate_plain_password(pass)?;
            self.assert_state_in(&[SessionState::Authentication])?;
            self.mark_closed_on_logical_failure();
            Err(AuthError::UnsupportedMechanism.into())
        }
    }

    /// Authenticate using a specific `AUTH` mechanism.
    ///
    /// Use this when [`Self::login`]'s auto-selection is not what you
    /// want — for example, when reproducing a production failure that
    /// is specific to one mechanism, or when testing against a server
    /// whose advertisement is known to be inaccurate.
    ///
    /// Returns [`AuthError::UnsupportedMechanism`] if `mechanism` was not
    /// advertised by the server. Returns [`AuthError::Rejected`] if the
    /// server rejects the credentials.
    pub async fn login_with(
        &mut self,
        mechanism: AuthMechanism,
        user: &str,
        pass: &str,
    ) -> Result<(), SmtpError> {
        protocol::validate_plain_username(user)?;
        protocol::validate_plain_password(pass)?;
        self.assert_state_in(&[SessionState::Authentication])?;

        if !ehlo_advertises_auth(&self.capabilities, mechanism.name()) {
            self.mark_closed_on_logical_failure();
            return Err(AuthError::UnsupportedMechanism.into());
        }

        match mechanism {
            AuthMechanism::Plain => self.run_auth_plain(user, pass).await?,
            AuthMechanism::Login => self.run_auth_login(user, pass).await?,
        }

        self.transition(SessionState::MailFrom)?;
        Ok(())
    }

    /// SASL `PLAIN` exchange (RFC 4616) using the initial-response form.
    ///
    /// One round-trip:
    /// `C: AUTH PLAIN <b64(\0user\0pass)>` → `S: 235`.
    async fn run_auth_plain(&mut self, user: &str, pass: &str) -> Result<(), SmtpError> {
        let response = build_auth_plain_initial_response(user, pass);
        let mut cmd = String::with_capacity(11 + response.len() + 2);
        cmd.push_str("AUTH PLAIN ");
        cmd.push_str(&response);
        cmd.push_str("\r\n");
        self.write_all(cmd.as_bytes()).await?;
        self.expect_code(235, SmtpOp::AuthPlain)
            .await
            .map_err(convert_auth)?;
        Ok(())
    }

    /// `AUTH LOGIN` exchange (legacy, two round-trips).
    ///
    /// `C: AUTH LOGIN` → `S: 334` → `C: b64(user)` → `S: 334` →
    /// `C: b64(pass)` → `S: 235`.
    async fn run_auth_login(&mut self, user: &str, pass: &str) -> Result<(), SmtpError> {
        self.write_all(b"AUTH LOGIN\r\n").await?;
        self.expect_code(334, SmtpOp::AuthLogin)
            .await
            .map_err(convert_auth)?;

        let mut user_b64 = protocol::base64_encode(user.as_bytes());
        user_b64.push_str("\r\n");
        self.write_all(user_b64.as_bytes()).await?;
        self.expect_code(334, SmtpOp::AuthLogin)
            .await
            .map_err(convert_auth)?;

        let mut pass_b64 = protocol::base64_encode(pass.as_bytes());
        pass_b64.push_str("\r\n");
        self.write_all(pass_b64.as_bytes()).await?;
        self.expect_code(235, SmtpOp::AuthLogin)
            .await
            .map_err(convert_auth)?;
        Ok(())
    }

    /// Send a single message.
    ///
    /// `from` is the envelope sender (RFC 5321 reverse-path), used in the
    /// `MAIL FROM:<...>` command. `to` is a non-empty slice of envelope
    /// recipients (forward-paths). `body` is the fully-formed message,
    /// including all RFC 5322 headers, separated from the body proper by a
    /// blank line, and CRLF-normalized. Any line in `body` whose first
    /// character is `.` is automatically dot-stuffed before transmission.
    ///
    /// On success the client is left in a state where another `send_mail`
    /// may be issued, or `quit` may be called to close the session.
    pub async fn send_mail(
        &mut self,
        from: &str,
        to: &[&str],
        body: &str,
    ) -> Result<(), SmtpError> {
        protocol::validate_address(from)?;
        if to.is_empty() {
            return Err(InvalidInputError::new("at least one recipient is required").into());
        }
        for &addr in to {
            protocol::validate_address(addr)?;
        }
        self.assert_state_in(&[SessionState::Authentication, SessionState::MailFrom])?;

        // Issue MAIL FROM.
        self.transition(SessionState::MailFrom)?;
        self.write_all(&format_mail_from(from)).await?;
        self.expect_class(2, SmtpOp::MailFrom).await?;

        // Issue RCPT TO for every recipient. 250 (OK) and 251 (forwarded)
        // are both acceptances; treat any 2xx as success.
        self.transition(SessionState::RcptTo)?;
        for &addr in to {
            self.write_all(&format_rcpt_to(addr)).await?;
            self.expect_class(2, SmtpOp::RcptTo).await?;
        }

        // Issue DATA, expect 354.
        self.transition(SessionState::Data)?;
        self.write_all(&format_command("DATA")).await?;
        self.expect_code(354, SmtpOp::Data).await?;

        // Send the body with dot-stuffing and terminator.
        let payload = dot_stuff_and_terminate(body.as_bytes());
        self.write_all(&payload).await?;
        self.expect_class(2, SmtpOp::Data).await?;

        // Ready for another transaction.
        self.transition(SessionState::MailFrom)?;
        Ok(())
    }

    /// Send `QUIT` and close the transport.
    ///
    /// Consumes `self` so the client cannot be reused after a clean
    /// shutdown. If the underlying transport's `close` fails, the SMTP
    /// `QUIT` may still have completed cleanly; the returned error wraps
    /// the transport-level failure.
    pub async fn quit(mut self) -> Result<(), SmtpError> {
        if self.state == SessionState::Closed {
            return Ok(());
        }
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

        send_result?;
        close_result.map_err(SmtpError::from)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    async fn read_greeting(&mut self) -> Result<(), SmtpError> {
        let reply = self.read_reply().await?;
        if reply.class() != 2 {
            self.mark_closed_on_logical_failure();
            return Err(ProtocolError::UnexpectedCode {
                during: SmtpOp::Greeting,
                expected_class: 2,
                actual: reply.code,
                message: reply.joined_text(),
            }
            .into());
        }
        self.transition(SessionState::Ehlo)?;
        Ok(())
    }

    async fn send_ehlo(&mut self, domain: &str) -> Result<(), SmtpError> {
        self.write_all(&format_command_arg("EHLO", domain)).await?;
        let reply = self.read_reply().await?;
        if reply.class() != 2 {
            self.mark_closed_on_logical_failure();
            return Err(ProtocolError::UnexpectedCode {
                during: SmtpOp::Ehlo,
                expected_class: 2,
                actual: reply.code,
                message: reply.joined_text(),
            }
            .into());
        }
        // The first line of an EHLO reply is the greeting; capability lines
        // follow. Store only the capability lines.
        let mut lines = reply.lines;
        if !lines.is_empty() {
            lines.remove(0);
        }
        self.capabilities = lines;
        self.transition(SessionState::Authentication)?;
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), SmtpError> {
        match self.transport.write_all(buf).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.mark_closed_on_logical_failure();
                Err(SmtpError::Io(e))
            }
        }
    }

    /// Read one full reply (possibly multi-line) and require the given
    /// exact code. Any deviation is reported as
    /// [`ProtocolError::UnexpectedCode`] tagged with `during` so the
    /// caller knows which SMTP step the failure refers to.
    async fn expect_code(&mut self, expected: u16, during: SmtpOp) -> Result<Reply, SmtpError> {
        let reply = self.read_reply().await?;
        if reply.code == expected {
            Ok(reply)
        } else {
            let class = u8::try_from(expected / 100).expect("expected code is in valid SMTP range");
            self.mark_closed_on_logical_failure();
            Err(ProtocolError::UnexpectedCode {
                during,
                expected_class: class,
                actual: reply.code,
                message: reply.joined_text(),
            }
            .into())
        }
    }

    /// Read one full reply (possibly multi-line) and require the given
    /// leading-digit class. Errors are tagged with `during` for the
    /// same reason as [`Self::expect_code`].
    async fn expect_class(
        &mut self,
        expected_class: u8,
        during: SmtpOp,
    ) -> Result<Reply, SmtpError> {
        let reply = self.read_reply().await?;
        if reply.class() == expected_class {
            Ok(reply)
        } else {
            self.mark_closed_on_logical_failure();
            Err(ProtocolError::UnexpectedCode {
                during,
                expected_class,
                actual: reply.code,
                message: reply.joined_text(),
            }
            .into())
        }
    }

    async fn read_reply(&mut self) -> Result<Reply, SmtpError> {
        let mut lines: Vec<String> = Vec::new();
        let mut code: Option<u16> = None;
        loop {
            if lines.len() >= MAX_REPLY_LINES {
                self.mark_closed_on_logical_failure();
                return Err(ProtocolError::Malformed(format!(
                    "reply exceeded {MAX_REPLY_LINES} lines",
                ))
                .into());
            }
            let line = self.read_line().await?;
            let parsed = match parse_reply_line(&line) {
                Ok(p) => p,
                Err(e) => {
                    self.mark_closed_on_logical_failure();
                    return Err(e.into());
                }
            };
            match code {
                None => code = Some(parsed.code),
                Some(prev) if prev != parsed.code => {
                    self.mark_closed_on_logical_failure();
                    return Err(ProtocolError::InconsistentMultiline {
                        first: prev,
                        later: parsed.code,
                    }
                    .into());
                }
                _ => {}
            }
            lines.push(String::from_utf8_lossy(parsed.text).into_owned());
            if parsed.is_last {
                let code = code.expect("at least one line was read so code has been initialised");
                return Ok(Reply { code, lines });
            }
        }
    }

    async fn read_line(&mut self) -> Result<Vec<u8>, SmtpError> {
        loop {
            // Search for CRLF in the unread portion of the buffer.
            if let Some(pos) = find_crlf(&self.rx_buf[self.rx_pos..]) {
                let abs_end = self.rx_pos + pos;
                let line = self.rx_buf[self.rx_pos..abs_end].to_vec();
                self.rx_pos = abs_end + 2;
                self.compact_rx();
                if line.len() > MAX_REPLY_LINE_LEN {
                    self.mark_closed_on_logical_failure();
                    return Err(ProtocolError::LineTooLong.into());
                }
                return Ok(line);
            }
            // No CRLF yet. Refuse to grow without bound.
            if self.rx_buf.len() - self.rx_pos > RX_BUF_HARD_LIMIT {
                self.mark_closed_on_logical_failure();
                return Err(ProtocolError::LineTooLong.into());
            }
            let n = self.fill_buf().await?;
            if n == 0 {
                self.mark_closed_on_logical_failure();
                return Err(ProtocolError::UnexpectedClose.into());
            }
        }
    }

    async fn fill_buf(&mut self) -> Result<usize, SmtpError> {
        let mut tmp = [0u8; READ_CHUNK];
        let n = self.transport.read(&mut tmp).await.map_err(|e| {
            // I/O failure is fatal; transition to Closed.
            self.state = SessionState::Closed;
            SmtpError::Io(e)
        })?;
        self.rx_buf.extend_from_slice(&tmp[..n]);
        Ok(n)
    }

    fn compact_rx(&mut self) {
        if self.rx_pos >= RX_BUF_COMPACT_THRESHOLD {
            self.rx_buf.drain(..self.rx_pos);
            self.rx_pos = 0;
        }
    }

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
        self.state = SessionState::Closed;
    }
}

// -----------------------------------------------------------------------------
// Free helpers
// -----------------------------------------------------------------------------

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

/// Convert a generic protocol error from an AUTH-phase reply into a more
/// specific [`AuthError::Rejected`] when the server returned a 5xx code.
fn convert_auth(err: SmtpError) -> SmtpError {
    match err {
        SmtpError::Protocol(ProtocolError::UnexpectedCode {
            actual, message, ..
        }) if (500..600).contains(&actual) => SmtpError::Auth(AuthError::Rejected {
            code: actual,
            message,
        }),
        other => other,
    }
}
