//! STARTTLS support (RFC 3207) for [`super::SmtpClient`].
//!
//! Only available on transports that implement [`crate::transport::StartTlsCapable`].
//! For Implicit TLS (port 465) use [`super::SmtpClient::connect`] instead.

use crate::error::{ProtocolError, SmtpError, SmtpOp};
use crate::protocol::{
    format_command,
    ehlo_advertises_starttls,
};
use crate::session::SessionState;
use crate::tracing_helpers::{smtp_debug, smtp_error};
use crate::transport::StartTlsCapable;
use super::SmtpClient;

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
        smtp_debug!("STARTTLS: requesting upgrade");

        if !ehlo_advertises_starttls(&self.capabilities) {
            smtp_error!("STARTTLS: extension not advertised; refusing to fall back to plaintext");
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
            smtp_error!(
                byte_count = residue,
                "STARTTLS: refusing to upgrade due to non-empty rx buffer (injection defense)"
            );
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
            smtp_error!("STARTTLS: TLS upgrade failed at transport layer");
            self.mark_closed_on_logical_failure();
            SmtpError::Io(e)
        })?;
        self.audit.on_event(&crate::audit::SmtpAuditEvent::TlsUpgraded);

        // RFC 3207 §4.2: re-issue EHLO on the now-secure channel. We
        // reuse send_ehlo, which writes the command, parses the reply,
        // refreshes self.capabilities, and transitions to
        // SessionState::Authentication.
        self.transition(SessionState::Ehlo)?;
        // Cloning is cheap relative to a network round-trip and avoids a
        // borrow-checker conflict with the &mut self call.
        let domain = self.ehlo_domain.clone();
        self.send_ehlo(&domain).await?;
        smtp_debug!(
            capability_count = self.capabilities.len(),
            "STARTTLS: upgrade complete; re-EHLO done"
        );
        Ok(())
    }
}

