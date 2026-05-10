//! Message-sending methods for [`super::SmtpClient`].
//!
//! `send_mail`, `send_mail_bytes`, `send_mail_stream`,
//! `send_mail_smtputf8`, and `send_message` (mail-builder integration)
//! live here.

#[cfg(feature = "mail-builder")]
use crate::error::IoError;
use crate::error::{InvalidInputError, SmtpError, SmtpOp};
use crate::outcome::SendOutcome;
use crate::protocol::{
    self, DotStufferState, dot_stuff_and_terminate, format_command,
    format_mail_from, format_rcpt_to,
};
#[cfg(feature = "smtputf8")]
use crate::protocol::{
    ehlo_advertises_smtputf8, format_mail_from_smtputf8, validate_address_utf8,
};
use crate::session::SessionState;
use crate::tracing_helpers::smtp_debug;
use crate::transport::Transport;
use super::SmtpClient;

impl<T: Transport> SmtpClient<T> {

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
    ) -> Result<SendOutcome, SmtpError> {
        protocol::validate_address(from)?;
        if to.is_empty() {
            return Err(InvalidInputError::new("at least one recipient is required").into());
        }
        for &addr in to {
            protocol::validate_address(addr)?;
        }
        self.assert_state_in(&[SessionState::Authentication, SessionState::MailFrom])?;

        // Policy checks — run before any SMTP command is sent.
        self.policy.check_sender(from).map_err(crate::error::SmtpError::Policy)?;
        self.policy.check_recipients(to).map_err(crate::error::SmtpError::Policy)?;
        self.policy
            .check_message_size(body.len())
            .map_err(crate::error::SmtpError::Policy)?;

        smtp_debug!(
            from = %from,
            recipient_count = to.len(),
            body_bytes = body.len(),
            "send_mail: starting transaction"
        );

        // Issue MAIL FROM, RCPT TO, and DATA — with pipelining if the
        // server advertised it (RFC 2920). Pipelining sends all three
        // command types in a single write, then reads all responses,
        // reducing RTTs from 3+N (one per command) to 2 (one flush + one
        // DATA-body exchange) regardless of recipient count.
        self.transition(SessionState::MailFrom)?;

        #[cfg(feature = "pipelining")]
        let pipelining = protocol::ehlo_advertises_pipelining(&self.capabilities);
        #[cfg(not(feature = "pipelining"))]
        let pipelining = false;

        if pipelining {
            // ── Pipelined path ────────────────────────────────────────
            // Collect MAIL FROM + all RCPT TO + DATA into one buffer,
            // write once, flush, then read all responses in order.
            let mut pipeline: Vec<u8> = Vec::with_capacity(
                64 + to.iter().map(|a| 12 + a.len()).sum::<usize>(),
            );
            pipeline.extend_from_slice(&format_mail_from(from));
            self.transition(SessionState::RcptTo)?;
            for &addr in to {
                pipeline.extend_from_slice(&format_rcpt_to(addr));
            }
            self.transition(SessionState::Data)?;
            pipeline.extend_from_slice(&format_command("DATA"));
            self.write_all(&pipeline).await?;
            self.flush().await?;

            // Read MAIL FROM response.
            let mail_reply = self.expect_class(2, SmtpOp::MailFrom).await?;
            self.audit.on_event(&crate::audit::SmtpAuditEvent::MailFromAccepted {
                code: mail_reply.code,
            });
            smtp_debug!(from = %from, pipelining = true, "MAIL FROM accepted");

            // Read one RCPT TO response per recipient.
            for _ in to {
                let rcpt_reply = self.expect_class(2, SmtpOp::RcptTo).await?;
                self.audit.on_event(&crate::audit::SmtpAuditEvent::RecipientAccepted {
                    code: rcpt_reply.code,
                });
            }

            // Read DATA 354 response.
            self.expect_code(354, SmtpOp::Data).await?;
        } else {
            // ── Sequential path (original) ────────────────────────────
            self.write_all(&format_mail_from(from)).await?;
            let mail_reply = self.expect_class(2, SmtpOp::MailFrom).await?;
            self.audit.on_event(&crate::audit::SmtpAuditEvent::MailFromAccepted {
                code: mail_reply.code,
            });
            smtp_debug!(from = %from, pipelining = false, "MAIL FROM accepted");

            self.transition(SessionState::RcptTo)?;
            for &addr in to {
                self.write_all(&format_rcpt_to(addr)).await?;
                let rcpt_reply = self.expect_class(2, SmtpOp::RcptTo).await?;
                self.audit.on_event(&crate::audit::SmtpAuditEvent::RecipientAccepted {
                    code: rcpt_reply.code,
                });
                smtp_debug!(rcpt = %addr, "RCPT TO accepted");
            }

            self.transition(SessionState::Data)?;
            self.write_all(&format_command("DATA")).await?;
            self.expect_code(354, SmtpOp::Data).await?;
        }

        // Send the body with dot-stuffing and terminator. The
        // post-terminator reply carries the queue id (if the server
        // assigns one) — capture it and return it to the caller.
        let payload = dot_stuff_and_terminate(body.as_bytes());
        self.write_all(&payload).await?;
        let final_reply = self.expect_class(2, SmtpOp::Data).await?;
        let outcome = SendOutcome::new(final_reply.code, final_reply.joined_text());
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MessageAccepted {
            code: outcome.code,
        });
        smtp_debug!(
            body_bytes = body.len(),
            code = outcome.code,
            queue_id = outcome.queue_id.as_deref().unwrap_or("<none>"),
            "DATA accepted; transaction complete"
        );

        // Ready for another transaction.
        self.transition(SessionState::MailFrom)?;
        Ok(outcome)
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
    /// Convenience: serialize a `mail-builder` `MessageBuilder` to a
    /// CRLF-normalized string and submit it.
    ///
    /// Equivalent to:
    ///
    /// ```ignore
    /// let body = message.write_to_string()?;
    /// client.send_mail(from, to, &body).await?;
    /// ```
    ///
    /// `from` is the SMTP envelope sender (`MAIL FROM:`); `to` is the
    /// envelope recipient list (`RCPT TO:`). These are **separate** from
    /// the `From:` and `To:` headers that `MessageBuilder` writes into
    /// the message body — they often coincide in practice, but the
    /// envelope is what the SMTP server uses for routing, while the
    /// headers are what the recipient's MUA displays. `Bcc` recipients
    /// must appear in `to` (the envelope) but **not** in any
    /// `MessageBuilder::bcc(...)` call (or, if they do, `MessageBuilder`
    /// strips them from the headers when serializing — verify against
    /// your `mail-builder` version).
    ///
    /// Available only with the `mail-builder` cargo feature enabled.
    ///
    /// # Errors
    ///
    /// All the categories returned by [`Self::send_mail`], plus:
    ///
    /// - [`SmtpError::Io`] with the underlying `mail_builder` error
    ///   preserved as the source chain if `MessageBuilder::write_to_string`
    ///   fails (effectively only on out-of-memory in current
    ///   `mail-builder` versions).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use mail_builder::MessageBuilder;
    /// let message = MessageBuilder::new()
    ///     .from(("Notify", "notify@example.com"))
    ///     .to("alice@example.org")
    ///     .subject("Status update")
    ///     .text_body("Hello.");
    ///
    /// client.send_message(
    ///     "notify@example.com",
    ///     &["alice@example.org"],
    ///     message,
    /// ).await?;
    /// ```
    #[cfg(feature = "mail-builder")]
    pub async fn send_message(
        &mut self,
        from: &str,
        to: &[&str],
        message: ::mail_builder::MessageBuilder<'_>,
    ) -> Result<SendOutcome, SmtpError> {
        let body = message
            .write_to_string()
            .map_err(|e| SmtpError::Io(IoError::with_source("failed to serialize message", e)))?;
        self.send_mail(from, to, &body).await
    }

    /// Send a single message supplied as a raw byte slice.
    ///
    /// Identical to [`Self::send_mail`] except that `body` is `&[u8]`
    /// rather than `&str`. Use this when the message has already been
    /// serialised to bytes by a builder such as `mail-builder` or when
    /// the body may contain non-UTF-8 octets (e.g. binary attachments
    /// encoded as base64 within a MIME part that uses a legacy charset).
    ///
    /// # Body requirements
    ///
    /// `body` must be a fully composed RFC 5322 message — headers, a blank
    /// line, and content — **with CRLF line endings**. [`Self::send_mail`]
    /// has the same requirement; the difference is that `send_mail_bytes`
    /// skips the UTF-8 validity check on the input slice.
    ///
    /// Dot-stuffing and the end-of-data terminator (`\r\n.\r\n`) are applied
    /// automatically, exactly as in `send_mail`.
    ///
    /// # Policy and audit
    ///
    /// The pre-send policy checks and audit events are identical to those
    /// fired by `send_mail`. `check_message_size` receives
    /// `body.len()` (the raw byte length before dot-stuffing).
    ///
    /// # Errors
    ///
    /// Same categories as [`Self::send_mail`].
    pub async fn send_mail_bytes(
        &mut self,
        from: &str,
        to: &[&str],
        body: &[u8],
    ) -> Result<SendOutcome, SmtpError> {
        protocol::validate_address(from)?;
        if to.is_empty() {
            return Err(InvalidInputError::new("at least one recipient is required").into());
        }
        for &addr in to {
            protocol::validate_address(addr)?;
        }
        self.assert_state_in(&[SessionState::Authentication, SessionState::MailFrom])?;

        // Policy checks — run before any SMTP command.
        self.policy.check_sender(from).map_err(crate::error::SmtpError::Policy)?;
        self.policy.check_recipients(to).map_err(crate::error::SmtpError::Policy)?;
        self.policy
            .check_message_size(body.len())
            .map_err(crate::error::SmtpError::Policy)?;

        smtp_debug!(
            from = %from,
            recipient_count = to.len(),
            body_bytes = body.len(),
            "send_mail_bytes: starting transaction"
        );

        // Issue MAIL FROM.
        self.transition(SessionState::MailFrom)?;
        self.write_all(&format_mail_from(from)).await?;
        let mail_reply = self.expect_class(2, SmtpOp::MailFrom).await?;
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MailFromAccepted {
            code: mail_reply.code,
        });

        // Issue RCPT TO for every recipient.
        self.transition(SessionState::RcptTo)?;
        for &addr in to {
            self.write_all(&format_rcpt_to(addr)).await?;
            let rcpt_reply = self.expect_class(2, SmtpOp::RcptTo).await?;
            self.audit.on_event(&crate::audit::SmtpAuditEvent::RecipientAccepted {
                code: rcpt_reply.code,
            });
        }

        // Issue DATA, expect 354.
        self.transition(SessionState::Data)?;
        self.write_all(&format_command("DATA")).await?;
        self.expect_code(354, SmtpOp::Data).await?;

        // Send the body with dot-stuffing and terminator.
        let payload = dot_stuff_and_terminate(body);
        self.write_all(&payload).await?;
        let final_reply = self.expect_class(2, SmtpOp::Data).await?;
        let outcome = SendOutcome::new(final_reply.code, final_reply.joined_text());
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MessageAccepted {
            code: outcome.code,
        });
        smtp_debug!(
            body_bytes = body.len(),
            code = outcome.code,
            queue_id = outcome.queue_id.as_deref().unwrap_or("<none>"),
            "DATA accepted; transaction complete"
        );

        self.transition(SessionState::MailFrom)?;
        Ok(outcome)
    }

    /// Send a message supplied as a [`MessageBody`] stream.
    ///
    /// This is the streaming variant of [`Self::send_mail_bytes`]. The body
    /// is read in chunks of `chunk_size` bytes (default: 8 KB), dot-stuffed,
    /// and written to the transport incrementally. Peak memory usage is
    /// O(`chunk_size`) rather than O(body size), making this suitable for
    /// large messages and memory-constrained runtimes.
    ///
    /// # Body requirements
    ///
    /// The body must be a fully composed RFC 5322 message (headers + blank
    /// line + content) with **CRLF line endings**. Dot-stuffing and the
    /// end-of-data terminator are applied automatically.
    ///
    /// # Policy and audit
    ///
    /// `check_sender` and `check_recipients` run before any SMTP command.
    /// `check_message_size` is called with `usize::MAX` because the total
    /// body size is unknown in advance; callers that need accurate size
    /// enforcement should use [`Self::send_mail_bytes`] instead.
    ///
    /// Audit events are identical to [`Self::send_mail`].
    ///
    /// # Errors
    ///
    /// Same as [`Self::send_mail`], plus:
    ///
    /// - [`SmtpError::Io`] if `body.read_chunk` returns an error. The
    ///   session is moved to `Closed`.
    pub async fn send_mail_stream<B>(
        &mut self,
        from: &str,
        to: &[&str],
        body: &mut B,
    ) -> Result<SendOutcome, SmtpError>
    where
        B: crate::message_body::MessageBody,
    {
        protocol::validate_address(from)?;
        if to.is_empty() {
            return Err(InvalidInputError::new("at least one recipient is required").into());
        }
        for &addr in to {
            protocol::validate_address(addr)?;
        }
        self.assert_state_in(&[SessionState::Authentication, SessionState::MailFrom])?;

        // Policy checks. check_message_size receives usize::MAX because the
        // total size is unknown; callers needing precise limits should use
        // send_mail_bytes instead.
        self.policy.check_sender(from).map_err(crate::error::SmtpError::Policy)?;
        self.policy.check_recipients(to).map_err(crate::error::SmtpError::Policy)?;
        self.policy
            .check_message_size(usize::MAX)
            .map_err(crate::error::SmtpError::Policy)?;

        smtp_debug!(
            from = %from,
            recipient_count = to.len(),
            "send_mail_stream: starting transaction"
        );

        // MAIL FROM.
        self.transition(SessionState::MailFrom)?;
        self.write_all(&format_mail_from(from)).await?;
        let mail_reply = self.expect_class(2, SmtpOp::MailFrom).await?;
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MailFromAccepted {
            code: mail_reply.code,
        });

        // RCPT TO.
        self.transition(SessionState::RcptTo)?;
        for &addr in to {
            self.write_all(&format_rcpt_to(addr)).await?;
            let rcpt_reply = self.expect_class(2, SmtpOp::RcptTo).await?;
            self.audit.on_event(&crate::audit::SmtpAuditEvent::RecipientAccepted {
                code: rcpt_reply.code,
            });
        }

        // DATA.
        self.transition(SessionState::Data)?;
        self.write_all(&format_command("DATA")).await?;
        self.expect_code(354, SmtpOp::Data).await?;

        // Stream the body through the dot-stuffer in 8 KB chunks.
        let mut stuffer = DotStufferState::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = match body.read_chunk(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    self.mark_closed_on_logical_failure();
                    return Err(SmtpError::Io(e));
                }
            };
            let stuffed = stuffer.process_chunk(&buf[..n]);
            self.write_all(&stuffed).await?;
        }
        // Terminator: ensures the body ends with \r\n then appends .\r\n
        let terminator = stuffer.finish();
        self.write_all(&terminator).await?;

        let final_reply = self.expect_class(2, SmtpOp::Data).await?;
        let outcome = SendOutcome::new(final_reply.code, final_reply.joined_text());
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MessageAccepted {
            code: outcome.code,
        });
        smtp_debug!(
            code = outcome.code,
            queue_id = outcome.queue_id.as_deref().unwrap_or("<none>"),
            "send_mail_stream: DATA accepted"
        );

        self.transition(SessionState::MailFrom)?;
        Ok(outcome)
    }

    /// Submit a UTF-8 (RFC 6531) message and recipient set.
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
    ) -> Result<SendOutcome, SmtpError> {
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

        // Policy checks — run before any SMTP command.
        self.policy.check_sender(from).map_err(crate::error::SmtpError::Policy)?;
        self.policy.check_recipients(to).map_err(crate::error::SmtpError::Policy)?;
        self.policy
            .check_message_size(body.len())
            .map_err(crate::error::SmtpError::Policy)?;

        // Issue MAIL FROM:<from> SMTPUTF8.
        self.transition(SessionState::MailFrom)?;
        self.write_all(&protocol::format_mail_from_smtputf8(from))
            .await?;
        let mail_reply = self.expect_class(2, SmtpOp::MailFrom).await?;
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MailFromAccepted {
            code: mail_reply.code,
        });

        // RCPT TO is identical to the ASCII path.
        self.transition(SessionState::RcptTo)?;
        for &addr in to {
            self.write_all(&format_rcpt_to(addr)).await?;
            let rcpt_reply = self.expect_class(2, SmtpOp::RcptTo).await?;
            self.audit.on_event(&crate::audit::SmtpAuditEvent::RecipientAccepted {
                code: rcpt_reply.code,
            });
        }

        // DATA + body identical to the ASCII path.
        self.transition(SessionState::Data)?;
        self.write_all(&format_command("DATA")).await?;
        self.expect_code(354, SmtpOp::Data).await?;

        let payload = dot_stuff_and_terminate(body.as_bytes());
        self.write_all(&payload).await?;
        let final_reply = self.expect_class(2, SmtpOp::Data).await?;
        let outcome = SendOutcome::new(final_reply.code, final_reply.joined_text());
        self.audit.on_event(&crate::audit::SmtpAuditEvent::MessageAccepted {
            code: outcome.code,
        });

        self.transition(SessionState::MailFrom)?;
        Ok(outcome)
    }
}
