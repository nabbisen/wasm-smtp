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
    ehlo_advertises_enhanced_status_codes, ehlo_advertises_starttls, format_command,
    format_command_arg, format_mail_from, format_rcpt_to, parse_reply_line, select_auth_mechanism,
};
use crate::session::SessionState;
use crate::transport::{StartTlsCapable, Transport};

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
    /// The EHLO domain supplied to [`Self::connect`]. Stored so that
    /// [`Self::starttls`] can re-issue `EHLO` after the TLS upgrade per
    /// RFC 3207 §4.2 without forcing the caller to pass the domain again.
    ehlo_domain: String,
    /// Whether the most recent EHLO advertised `ENHANCEDSTATUSCODES`
    /// (RFC 2034). When set, every reply parsed by [`Self::read_reply`]
    /// is annotated with an [`crate::protocol::EnhancedStatus`] (when
    /// the leading reply line carries one), and that code is propagated
    /// into [`crate::ProtocolError::UnexpectedCode`] on failure.
    enhanced_status_enabled: bool,
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
    pub async fn connect(transport: T, ehlo_domain: &str) -> Result<Self, SmtpError> {
        protocol::validate_ehlo_domain(ehlo_domain)?;
        let mut client = Self {
            transport,
            state: SessionState::Greeting,
            rx_buf: Vec::with_capacity(READ_CHUNK),
            rx_pos: 0,
            capabilities: Vec::new(),
            ehlo_domain: ehlo_domain.to_owned(),
            enhanced_status_enabled: false,
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
    /// `credential` is the secret material whose meaning depends on the
    /// mechanism: a static password for `Plain` and `Login`, or an
    /// OAuth 2.0 access token for `XOAuth2` (the latter requires the
    /// `xoauth2` cargo feature). The `user` parameter is validated
    /// against rules appropriate to the mechanism (NUL bytes rejected
    /// for SASL framing in `Plain` / `Login`, additional control bytes
    /// rejected for `XOAuth2`).
    ///
    /// Returns [`AuthError::UnsupportedMechanism`] if `mechanism` was not
    /// advertised by the server. Returns [`AuthError::Rejected`] if the
    /// server rejects the credentials.
    ///
    /// When the `xoauth2` feature is disabled and the caller passes
    /// [`AuthMechanism::XOAuth2`], this returns
    /// [`InvalidInputError`] without performing any I/O — the variant
    /// remains in the public enum (it is `non_exhaustive`) but the
    /// code path is removed.
    pub async fn login_with(
        &mut self,
        mechanism: AuthMechanism,
        user: &str,
        credential: &str,
    ) -> Result<(), SmtpError> {
        match mechanism {
            AuthMechanism::Plain | AuthMechanism::Login => {
                protocol::validate_plain_username(user)?;
                protocol::validate_plain_password(credential)?;
            }
            #[cfg(feature = "xoauth2")]
            AuthMechanism::XOAuth2 => {
                protocol::validate_xoauth2_user(user)?;
                protocol::validate_oauth2_token(credential)?;
            }
            #[cfg(not(feature = "xoauth2"))]
            _ => {
                return Err(InvalidInputError::new(
                    "XOAUTH2 support is not compiled in (enable the `xoauth2` feature)",
                )
                .into());
            }
        }
        self.assert_state_in(&[SessionState::Authentication])?;

        if !ehlo_advertises_auth(&self.capabilities, mechanism.name()) {
            self.mark_closed_on_logical_failure();
            return Err(AuthError::UnsupportedMechanism.into());
        }

        match mechanism {
            AuthMechanism::Plain => self.run_auth_plain(user, credential).await?,
            AuthMechanism::Login => self.run_auth_login(user, credential).await?,
            #[cfg(feature = "xoauth2")]
            AuthMechanism::XOAuth2 => self.run_auth_xoauth2(user, credential).await?,
            #[cfg(not(feature = "xoauth2"))]
            _ => unreachable!("XOAUTH2 was screened out above when the feature is disabled"),
        }

        self.transition(SessionState::MailFrom)?;
        Ok(())
    }

    /// Authenticate with `XOAUTH2`, the Google / Microsoft OAuth 2.0
    /// SASL profile.
    ///
    /// `user` is the email address of the account, `access_token` is a
    /// short-lived OAuth 2.0 bearer token obtained via the OAuth flow
    /// for that account. This crate does not perform the OAuth dance
    /// itself — token acquisition, refresh, and storage are the
    /// caller's responsibility.
    ///
    /// Convenience wrapper for
    /// `login_with(AuthMechanism::XOAuth2, user, access_token)`. Note
    /// that [`Self::login`] (the auto-selecting variant) deliberately
    /// does not pick `XOAUTH2` even when the server advertises it,
    /// because the credential semantics are different from a static
    /// password.
    ///
    /// # Errors
    ///
    /// - [`AuthError::UnsupportedMechanism`] if the server did not
    ///   advertise `AUTH XOAUTH2`.
    /// - [`AuthError::Rejected`] if the server rejected the token.
    ///   Google and Microsoft typically return a 535 with a base64-
    ///   encoded JSON `{"status":"401","schemes":"Bearer","scope":"..."}`
    ///   in the message; the parsed text is preserved in the error.
    ///
    /// Available only with the `xoauth2` cargo feature enabled
    /// (default-on).
    #[cfg(feature = "xoauth2")]
    pub async fn login_xoauth2(&mut self, user: &str, access_token: &str) -> Result<(), SmtpError> {
        self.login_with(AuthMechanism::XOAuth2, user, access_token)
            .await
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

    /// `AUTH XOAUTH2` exchange (Google / Microsoft).
    ///
    /// Wire form:
    /// `C: AUTH XOAUTH2 <b64("user="user SOH "auth=Bearer "token SOH SOH)>`
    /// → `S: 235` on success.
    ///
    /// On failure, RFC 7628-style providers send `334 <b64(json)>` first
    /// and expect the client to reply with an empty line; the server
    /// then sends the final 5xx. We follow that protocol so the JSON
    /// error detail (containing `scope`, `error`, etc.) ends up in the
    /// final reply text and is preserved in [`AuthError::Rejected`].
    #[cfg(feature = "xoauth2")]
    async fn run_auth_xoauth2(&mut self, user: &str, token: &str) -> Result<(), SmtpError> {
        let response = protocol::build_xoauth2_initial_response(user, token);
        let mut cmd = String::with_capacity(13 + response.len() + 2);
        cmd.push_str("AUTH XOAUTH2 ");
        cmd.push_str(&response);
        cmd.push_str("\r\n");
        self.write_all(cmd.as_bytes()).await?;

        // Read the first reply. 235 is direct success; 334 indicates the
        // provider is sending JSON error details and expects an empty
        // continuation line, after which a final 5xx arrives.
        let reply = self.read_reply().await?;
        match reply.code {
            235 => Ok(()),
            334 => {
                // Provider-supplied error detail. Send an empty continuation
                // line so the provider can finalize with a proper 5xx.
                self.write_all(b"\r\n").await?;
                let final_reply = self.read_reply().await?;
                self.mark_closed_on_logical_failure();
                Err(SmtpError::Auth(AuthError::Rejected {
                    code: final_reply.code,
                    enhanced: final_reply.enhanced(),
                    message: final_reply.joined_text(),
                }))
            }
            other => {
                self.mark_closed_on_logical_failure();
                Err(if (500..600).contains(&other) {
                    SmtpError::Auth(AuthError::Rejected {
                        code: other,
                        enhanced: reply.enhanced(),
                        message: reply.joined_text(),
                    })
                } else {
                    SmtpError::Protocol(ProtocolError::UnexpectedCode {
                        during: SmtpOp::AuthXOAuth2,
                        expected_class: 2,
                        actual: other,
                        enhanced: reply.enhanced(),
                        message: reply.joined_text(),
                    })
                })
            }
        }
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
    ///
    /// # Body size
    ///
    /// `wasm-smtp` does not impose an upper bound on `body.len()`;
    /// the body is dot-stuffed into a single `Vec<u8>` and written in
    /// one [`crate::Transport::write_all`] call.
    /// In practice the caller (or a layer above this crate) should
    /// enforce a sane application-specific limit, both to avoid the
    /// allocation cost on a malicious body and to stay within the
    /// `SIZE` limit (RFC 1870) the server may have advertised in its
    /// `EHLO` response. A typical safe default for transactional mail
    /// is 10 MiB; submission relays such as Gmail enforce 25-50 MiB.
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

    /// Send a single message using the SMTPUTF8 extension (RFC 6531),
    /// allowing UTF-8 characters in envelope addresses.
    ///
    /// Identical to [`Self::send_mail`] except:
    ///
    /// - Address validation uses [`protocol::validate_address_utf8`]
    ///   instead of the strict ASCII validator, so codepoints outside
    ///   the ASCII range are accepted in `from` and `to`.
    /// - The `MAIL FROM` command is suffixed with the `SMTPUTF8`
    ///   ESMTP parameter so the server knows to expect UTF-8.
    /// - The server must have advertised `SMTPUTF8` in its `EHLO`
    ///   response. If it did not, this method returns
    ///   [`ProtocolError::ExtensionUnavailable`] without sending any
    ///   bytes.
    ///
    /// The body must still be CRLF-normalized; any UTF-8 in headers
    /// (e.g. `Subject:` containing non-ASCII characters) is the
    /// caller's responsibility to format correctly. RFC 6531 §3.2
    /// permits raw UTF-8 in headers when SMTPUTF8 is in effect, but
    /// strict deployments may still expect MIME encoded-words; this
    /// crate makes no claim either way.
    ///
    /// Available only with the `smtputf8` cargo feature enabled.
    ///
    /// # Errors
    ///
    /// In addition to the error categories returned by `send_mail`:
    ///
    /// - [`ProtocolError::ExtensionUnavailable`] with `name: "SMTPUTF8"`
    ///   if the server's `EHLO` reply did not include the keyword.
    ///   The session is moved to `Closed` to prevent silent fallback
    ///   to ASCII-only delivery.
    #[cfg(feature = "smtputf8")]
    pub async fn send_mail_smtputf8(
        &mut self,
        from: &str,
        to: &[&str],
        body: &str,
    ) -> Result<(), SmtpError> {
        protocol::validate_address_utf8(from)?;
        if to.is_empty() {
            return Err(InvalidInputError::new("at least one recipient is required").into());
        }
        for &addr in to {
            protocol::validate_address_utf8(addr)?;
        }
        self.assert_state_in(&[SessionState::Authentication, SessionState::MailFrom])?;

        if !protocol::ehlo_advertises_smtputf8(&self.capabilities) {
            self.mark_closed_on_logical_failure();
            return Err(ProtocolError::ExtensionUnavailable { name: "SMTPUTF8" }.into());
        }

        // Issue MAIL FROM:<from> SMTPUTF8.
        self.transition(SessionState::MailFrom)?;
        self.write_all(&protocol::format_mail_from_smtputf8(from))
            .await?;
        self.expect_class(2, SmtpOp::MailFrom).await?;

        // RCPT TO is identical to the ASCII path: SMTPUTF8 does not
        // add a parameter to RCPT, only to MAIL FROM. Recipients can
        // be UTF-8 because the validator we ran above already
        // accepted them.
        self.transition(SessionState::RcptTo)?;
        for &addr in to {
            self.write_all(&format_rcpt_to(addr)).await?;
            self.expect_class(2, SmtpOp::RcptTo).await?;
        }

        // DATA + body identical to the ASCII path.
        self.transition(SessionState::Data)?;
        self.write_all(&format_command("DATA")).await?;
        self.expect_code(354, SmtpOp::Data).await?;

        let payload = dot_stuff_and_terminate(body.as_bytes());
        self.write_all(&payload).await?;
        self.expect_class(2, SmtpOp::Data).await?;

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
                enhanced: reply.enhanced(),
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
                enhanced: reply.enhanced(),
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
        // Refresh ENHANCEDSTATUSCODES enablement from the post-EHLO
        // capability set. Doing this BEFORE assigning self.capabilities
        // is the cleanest order; it also keeps enabledness false if the
        // capability is dropped on a re-EHLO (e.g. after STARTTLS).
        self.enhanced_status_enabled = ehlo_advertises_enhanced_status_codes(&lines);
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
                enhanced: reply.enhanced(),
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
                enhanced: reply.enhanced(),
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
                let mut reply = Reply::new(code, lines);
                if self.enhanced_status_enabled
                    && let Some(status) = reply.try_parse_enhanced()
                {
                    reply.attach_enhanced_status(status);
                }
                return Ok(reply);
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
// STARTTLS (RFC 3207) — only available on transports that can be upgraded
// to TLS in-place.
// -----------------------------------------------------------------------------

impl<T: StartTlsCapable> SmtpClient<T> {
    /// Connect, read the greeting, send `EHLO`, issue `STARTTLS`, upgrade
    /// the transport to TLS, and re-issue `EHLO` on the secure stream.
    ///
    /// This is the convenience entry point for the STARTTLS submission flow
    /// on ports 587 / 25. The returned client is in
    /// [`SessionState::Authentication`] just like one returned by
    /// [`Self::connect`] would be — meaning the caller proceeds with
    /// [`Self::login`] (or skips straight to [`Self::send_mail`] for
    /// unauthenticated submission) without observing the TLS upgrade
    /// itself.
    ///
    /// Use [`Self::connect`] for Implicit TLS on port 465 instead. STARTTLS
    /// is appropriate when the transport must remain plaintext until the
    /// server has accepted the upgrade request.
    ///
    /// # Errors
    ///
    /// Returns the same error categories as [`Self::connect`] for the
    /// pre-upgrade phase. Additionally:
    ///
    /// - [`ProtocolError::ExtensionUnavailable`] with `name: "STARTTLS"`
    ///   if the server's first `EHLO` reply did not advertise the
    ///   extension.
    /// - [`ProtocolError::UnexpectedCode`] with `during: SmtpOp::StartTls`
    ///   if the server rejected `STARTTLS` itself.
    /// - [`SmtpError::Io`] if the transport-level upgrade fails.
    pub async fn connect_starttls(transport: T, ehlo_domain: &str) -> Result<Self, SmtpError> {
        let mut client = Self::connect(transport, ehlo_domain).await?;
        client.starttls().await?;
        Ok(client)
    }

    /// Issue `STARTTLS` on an already-connected client, upgrade the
    /// transport, and re-issue `EHLO` per RFC 3207 §4.2.
    ///
    /// May only be called immediately after [`Self::connect`]. Calling it
    /// after [`Self::login`] or [`Self::send_mail`] returns
    /// [`InvalidInputError`] without touching the wire.
    ///
    /// # Errors
    ///
    /// - [`ProtocolError::ExtensionUnavailable`] with `name: "STARTTLS"`
    ///   if the server did not advertise the extension. In this case the
    ///   client is moved to [`SessionState::Closed`] so subsequent calls
    ///   fail fast — accidentally falling back to plaintext authentication
    ///   would defeat the purpose of asking for STARTTLS.
    /// - [`ProtocolError::UnexpectedCode`] with `during: SmtpOp::StartTls`
    ///   if the server rejected the command.
    /// - [`SmtpError::Io`] if the transport-level upgrade fails.
    pub async fn starttls(&mut self) -> Result<(), SmtpError> {
        self.assert_state_in(&[SessionState::Authentication])?;

        if !ehlo_advertises_starttls(&self.capabilities) {
            self.mark_closed_on_logical_failure();
            return Err(ProtocolError::ExtensionUnavailable { name: "STARTTLS" }.into());
        }

        // Send STARTTLS and require a 220 reply before touching the
        // transport. Per RFC 3207, a 4xx/5xx reply leaves the channel
        // plaintext and the client is free to try other things — but for
        // simplicity, and to avoid silently falling through to plaintext
        // AUTH, we treat any non-220 here as a fatal error.
        self.transition(SessionState::StartTls)?;
        self.write_all(&format_command("STARTTLS")).await?;
        self.expect_code(220, SmtpOp::StartTls).await?;

        // STARTTLS injection / pipelining defense (RFC 3207 §5):
        //
        // Between the `220` reply and the TLS handshake the channel is
        // still plaintext. An attacker who is willing to corrupt the
        // server's reply stream may try to pipeline additional SMTP
        // commands ("EHLO ..\r\nMAIL FROM:..\r\n") onto the buffer
        // before the TLS upgrade, hoping the client will read those
        // bytes back AFTER the upgrade and treat them as if they had
        // arrived over the secured channel. (See CVE-2011-1575 for the
        // historical Postfix case; equivalent client-side bugs exist.)
        //
        // The defense is to refuse to start TLS when there are any
        // unread bytes in the receive buffer after the 220. Honest
        // servers do not pipeline data into the STARTTLS handshake
        // window — they wait for the client to begin the TLS
        // ClientHello. Any bytes here are therefore evidence of an
        // injection or of a server bug that we want to surface
        // loudly rather than silently absorb.
        let residue = self.rx_buf.len() - self.rx_pos;
        if residue > 0 {
            self.mark_closed_on_logical_failure();
            return Err(ProtocolError::StartTlsBufferResidue {
                byte_count: residue,
            }
            .into());
        }

        // Upgrade the transport. Discard previously-advertised
        // capabilities: RFC 3207 §4.2 mandates that the server may
        // advertise a different set after the TLS upgrade.
        self.capabilities.clear();
        self.transport.upgrade_to_tls().await.map_err(|e| {
            self.mark_closed_on_logical_failure();
            SmtpError::Io(e)
        })?;

        // RFC 3207 §4.2: re-issue EHLO on the now-secure channel. We
        // reuse send_ehlo, which writes the command, parses the reply,
        // refreshes self.capabilities, and transitions to
        // SessionState::Authentication.
        self.transition(SessionState::Ehlo)?;
        // Cloning is cheap relative to a network round-trip and avoids a
        // borrow-checker conflict with the &mut self call.
        let domain = self.ehlo_domain.clone();
        self.send_ehlo(&domain).await?;
        Ok(())
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
