//! Authentication methods for [`super::SmtpClient`].
//!
//! All `login_*` and `run_auth_*` methods live here. This is a private
//! child module of `client`; child modules may access private fields of
//! the parent's types (Rust visibility rules for descendant modules).

use crate::error::{AuthError, InvalidInputError, ProtocolError, SmtpError, SmtpOp};
use crate::protocol::{
    self, AuthMechanism,
    build_auth_plain_initial_response,
    ehlo_advertises_auth,
    select_auth_mechanism,
};
use crate::session::SessionState;
use crate::tracing_helpers::{smtp_debug, smtp_warn};
use crate::transport::Transport;
use super::{SmtpClient, convert_auth};

impl<T: Transport> SmtpClient<T> {

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
            smtp_debug!(mechanism = mech.name(), "AUTH: auto-selected mechanism");
            self.login_with(mech, user, pass).await
        } else {
            smtp_warn!(
                "AUTH: no supported mechanism advertised; failing with UnsupportedMechanism"
            );
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
            #[cfg(feature = "oauthbearer")]
            AuthMechanism::OAuthBearer => {
                // user is the authzid (may be empty); credential is the Bearer token.
                protocol::validate_oauth2_token(credential)?;
            }
            #[cfg(feature = "scram-sha-256")]
            AuthMechanism::ScramSha256 => {
                protocol::validate_plain_username(user)?;
                protocol::validate_plain_password(credential)?;
            }
            #[cfg(not(any(feature = "xoauth2", feature = "oauthbearer", feature = "scram-sha-256")))]
            _ => {
                return Err(InvalidInputError::new(
                    "the requested AUTH mechanism is not compiled in",
                )
                .into());
            }
            #[allow(unreachable_patterns)]
            _ => {
                return Err(InvalidInputError::new(
                    "the requested AUTH mechanism is not compiled in",
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
            #[cfg(feature = "oauthbearer")]
            AuthMechanism::OAuthBearer => self.run_auth_oauthbearer(user, credential).await?,
            #[cfg(feature = "scram-sha-256")]
            AuthMechanism::ScramSha256 => self.run_auth_scram_sha256(user, credential).await?,
            #[allow(unreachable_patterns)]
            _ => unreachable!("variants screened out above when feature is disabled"),
        }

        self.transition(SessionState::MailFrom)?;
        smtp_debug!(mechanism = mechanism.name(), "AUTH: succeeded");
        self.audit.on_event(&crate::audit::SmtpAuditEvent::AuthCompleted {
            mechanism: mechanism.name(),
        });
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

    /// Authenticate with `OAUTHBEARER` (RFC 7628), the IETF-standard
    /// OAuth 2.0 SASL mechanism.
    ///
    /// `user` is the authorization identity (typically the account email
    /// address); `access_token` is a short-lived OAuth 2.0 bearer token.
    ///
    /// Unlike `XOAUTH2`, `OAUTHBEARER` follows the GS2 framing from RFC
    /// 5801, making it interoperable with any compliant SASL library.
    ///
    /// Convenience wrapper for
    /// `login_with(AuthMechanism::OAuthBearer, user, access_token)`.
    ///
    /// # Errors
    ///
    /// - [`AuthError::UnsupportedMechanism`] if the server did not
    ///   advertise `AUTH OAUTHBEARER`.
    /// - [`AuthError::Rejected`] if the server rejected the token with a
    ///   `334` error challenge followed by a `535`.
    ///
    /// Available only with the `oauthbearer` cargo feature (default-on).
    #[cfg(feature = "oauthbearer")]
    pub async fn login_oauthbearer(
        &mut self,
        user: &str,
        access_token: &str,
    ) -> Result<(), SmtpError> {
        self.login_with(AuthMechanism::OAuthBearer, user, access_token)
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

    /// `AUTH OAUTHBEARER` exchange (RFC 7628).
    ///
    /// Wire form:
    /// `C: AUTH OAUTHBEARER <b64("n,a="user","SOH"auth=Bearer "token SOH SOH)>`
    /// → `S: 235` on success.
    ///
    /// On failure, the server sends `334 <b64(json-error)>`, and the
    /// client must reply `\x01` to abort. The server then responds with
    /// a final `535`. The JSON error detail is preserved in
    /// [`AuthError::Rejected`].
    #[cfg(feature = "oauthbearer")]
    async fn run_auth_oauthbearer(
        &mut self,
        user: &str,
        token: &str,
    ) -> Result<(), SmtpError> {
        let response = protocol::build_oauthbearer_initial_response(user, token);
        let mut cmd = String::with_capacity(17 + response.len() + 2);
        cmd.push_str("AUTH OAUTHBEARER ");
        cmd.push_str(&response);
        cmd.push_str("\r\n");
        self.write_all(cmd.as_bytes()).await?;

        let reply = self.read_reply().await?;
        match reply.code {
            235 => Ok(()),
            334 => {
                // Server sent a JSON error challenge (RFC 7628 §3.2.2).
                // Client must reply with a single \x01 to abort; the
                // server then closes with a 5xx.
                self.write_all(b"\x01\r\n").await?;
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
                        during: SmtpOp::AuthOAuthBearer,
                        expected_class: 2,
                        actual: other,
                        enhanced: reply.enhanced(),
                        message: reply.joined_text(),
                    })
                })
            }
        }
    }

    /// `AUTH SCRAM-SHA-256` exchange (RFC 5802 / RFC 7677).
    ///
    /// Wire form:
    /// 1. `C: AUTH SCRAM-SHA-256 <b64(client-first)>`
    /// 2. `S: 334 <b64(server-first)>`
    /// 3. `C: <b64(client-final-with-proof)>`
    /// 4. `S: 334 <b64(server-final)>` then `S: 235 <ok>`
    ///    (or `S: 535` if the proof failed verification on the
    ///    server side).
    ///
    /// Note that step 4 has the server returning `334` *with* the
    /// signature, not directly `235`. The client must verify the
    /// server's signature locally (mutual authentication) and then
    /// reply with an empty continuation. The `235` confirms the
    /// session is authenticated.
    #[cfg(feature = "scram-sha-256")]
    async fn run_auth_scram_sha256(&mut self, user: &str, password: &str) -> Result<(), SmtpError> {
        // Step 1: client-first.
        let client_nonce = crate::scram::generate_client_nonce().map_err(SmtpError::Auth)?;
        let client_first = crate::scram::build_client_first(user, &client_nonce);
        let client_first_b64 = protocol::base64_encode(client_first.as_bytes());

        let mut cmd = String::with_capacity(20 + client_first_b64.len() + 2);
        cmd.push_str("AUTH SCRAM-SHA-256 ");
        cmd.push_str(&client_first_b64);
        cmd.push_str("\r\n");
        self.write_all(cmd.as_bytes()).await?;

        // Step 2: read 334 with server-first.
        let reply = self.read_reply().await?;
        if reply.code != 334 {
            self.mark_closed_on_logical_failure();
            return Err(if (500..600).contains(&reply.code) {
                SmtpError::Auth(AuthError::Rejected {
                    code: reply.code,
                    enhanced: reply.enhanced(),
                    message: reply.joined_text(),
                })
            } else {
                SmtpError::Protocol(ProtocolError::UnexpectedCode {
                    during: SmtpOp::AuthScramSha256,
                    expected_class: 3,
                    actual: reply.code,
                    enhanced: reply.enhanced(),
                    message: reply.joined_text(),
                })
            });
        }

        // The 334 reply text is the base64 of the server-first message.
        let server_first_b64 = reply.joined_text();
        let server_first_bytes = protocol::base64_decode(&server_first_b64).map_err(|_| {
            self.mark_closed_on_logical_failure();
            SmtpError::Auth(AuthError::MalformedChallenge(
                "SCRAM server-first not valid base64".into(),
            ))
        })?;
        let server_first_str = std::str::from_utf8(&server_first_bytes).map_err(|_| {
            self.mark_closed_on_logical_failure();
            SmtpError::Auth(AuthError::MalformedChallenge(
                "SCRAM server-first not valid UTF-8".into(),
            ))
        })?;

        let server_first = crate::scram::parse_server_first(server_first_str, &client_nonce)
            .map_err(|e| {
                self.mark_closed_on_logical_failure();
                SmtpError::Auth(e)
            })?;

        // Step 3: compute and send client-final.
        let cf = crate::scram::compute_client_final(
            user,
            password,
            &client_nonce,
            &server_first,
            server_first_str,
        );
        let client_final_b64 = protocol::base64_encode(cf.message.as_bytes());
        let mut cmd = String::with_capacity(client_final_b64.len() + 2);
        cmd.push_str(&client_final_b64);
        cmd.push_str("\r\n");
        self.write_all(cmd.as_bytes()).await?;

        // Step 4: server-final + confirmation.
        let reply = self.read_reply().await?;
        match reply.code {
            334 => {
                // Server is sending its signature as a challenge; verify
                // and continue with empty response.
                self.scram_verify_server_final(
                    &reply.joined_text(),
                    &cf.expected_server_signature,
                )?;
                // Send empty continuation.
                self.write_all(b"\r\n").await?;
                // Now expect 235.
                self.expect_code(235, SmtpOp::AuthScramSha256)
                    .await
                    .map_err(convert_auth)?;
                Ok(())
            }
            235 => {
                // Some servers (Stalwart in some configurations) return
                // the server-final embedded in the 235 line directly,
                // skipping the 334-then-235 dance. RFC 5802 §5.1 allows
                // this. We still verify the signature.
                self.scram_verify_server_final(
                    &reply.joined_text(),
                    &cf.expected_server_signature,
                )?;

                Ok(())
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
                        during: SmtpOp::AuthScramSha256,
                        expected_class: 2,
                        actual: other,
                        enhanced: reply.enhanced(),
                        message: reply.joined_text(),
                    })
                })
            }
        }
    }

    /// Helper for [`Self::run_auth_scram_sha256`]: base64-decode and
    /// UTF-8-decode a `server-final` payload, then verify it against
    /// the expected `ServerSignature`. Marks the session closed and
    /// returns an [`AuthError`] on any failure.
    #[cfg(feature = "scram-sha-256")]
    fn scram_verify_server_final(
        &mut self,
        server_final_b64: &str,
        expected_signature: &[u8; 32],
    ) -> Result<(), SmtpError> {
        let server_final_bytes = protocol::base64_decode(server_final_b64).map_err(|_| {
            self.mark_closed_on_logical_failure();
            SmtpError::Auth(AuthError::MalformedChallenge(
                "SCRAM server-final not valid base64".into(),
            ))
        })?;
        let server_final_str = std::str::from_utf8(&server_final_bytes).map_err(|_| {
            self.mark_closed_on_logical_failure();
            SmtpError::Auth(AuthError::MalformedChallenge(
                "SCRAM server-final not valid UTF-8".into(),
            ))
        })?;
        crate::scram::verify_server_final(server_final_str, expected_signature).map_err(|e| {
            self.mark_closed_on_logical_failure();
            SmtpError::Auth(e)
        })?;
        Ok(())
    }
}
