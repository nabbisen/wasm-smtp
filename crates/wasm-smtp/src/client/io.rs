//! Low-level I/O helpers for [`super::SmtpClient`].
//!
//! Line-reading, buffered I/O, SMTP reply parsing, and the greeting /
//! EHLO exchange live here. All methods are `pub(super)` — they are not
//! part of the public API but are called by sibling modules (`auth`,
//! `send`, `starttls`) and by `mod.rs`.

use crate::error::{ProtocolError, SmtpError, SmtpOp};
use crate::protocol::{
    MAX_REPLY_LINE_LEN, MAX_REPLY_LINES, Reply,
    ehlo_advertises_enhanced_status_codes, format_command_arg, parse_reply_line,
};
use crate::session::SessionState;
use crate::transport::Transport;
use super::{SmtpClient, find_crlf, READ_CHUNK, RX_BUF_COMPACT_THRESHOLD, RX_BUF_HARD_LIMIT};

impl<T: Transport> SmtpClient<T> {
    pub(super) async fn read_greeting(&mut self) -> Result<(), SmtpError> {
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
        self.audit.on_event(&crate::audit::SmtpAuditEvent::GreetingReceived { code: reply.code });
        self.transition(SessionState::Ehlo)?;
        Ok(())
    }

    pub(super) async fn send_ehlo(&mut self, domain: &str) -> Result<(), SmtpError> {
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
        self.audit.on_event(&crate::audit::SmtpAuditEvent::EhloCompleted);
        self.transition(SessionState::Authentication)?;
        Ok(())
    }

    pub(super) async fn write_all(&mut self, buf: &[u8]) -> Result<(), SmtpError> {
        match self.transport.write_all(buf).await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.mark_closed_on_logical_failure();
                Err(SmtpError::Io(e))
            }
        }
    }

    /// Flush the transport's write buffer. Used by the pipelining path to
    /// ensure all buffered commands are sent before reading responses.
    pub(super) async fn flush(&mut self) -> Result<(), SmtpError> {
        match self.transport.flush().await {
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
    pub(super) async fn expect_code(&mut self, expected: u16, during: SmtpOp) -> Result<Reply, SmtpError> {
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
    pub(super) async fn expect_class(
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

    pub(super) async fn read_reply(&mut self) -> Result<Reply, SmtpError> {
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

    pub(super) async fn read_line(&mut self) -> Result<Vec<u8>, SmtpError> {
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

    pub(super) async fn fill_buf(&mut self) -> Result<usize, SmtpError> {
        let mut tmp = [0u8; READ_CHUNK];
        let n = self.transport.read(&mut tmp).await.map_err(|e| {
            // I/O failure is fatal; transition to Closed.
            self.state = SessionState::Closed;
            SmtpError::Io(e)
        })?;
        self.rx_buf.extend_from_slice(&tmp[..n]);
        Ok(n)
    }

    pub(super) fn compact_rx(&mut self) {
        if self.rx_pos >= RX_BUF_COMPACT_THRESHOLD {
            self.rx_buf.drain(..self.rx_pos);
            self.rx_pos = 0;
        }
    }

}
