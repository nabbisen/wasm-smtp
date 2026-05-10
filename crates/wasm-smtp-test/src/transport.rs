//! [`MockTransport`] implementation.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_smtp::{IoError, StartTlsCapable, Transport};

/// Behavior of [`MockTransport`]'s STARTTLS upgrade.
#[derive(Debug, Clone)]
pub enum UpgradeBehavior {
    /// `upgrade_to_tls()` succeeds.
    Succeed,
    /// `upgrade_to_tls()` fails with the given message.
    Fail(&'static str),
}

/// `(transport, written_bytes, close_flag)` returned by [`MockTransport::new`].
pub type MockHandles = (MockTransport, Rc<RefCell<Vec<u8>>>, Rc<RefCell<bool>>);

/// `(transport, written_bytes, close_flag, upgrade_count)` returned by
/// [`MockTransport::with_starttls`].
pub type MockStartTlsHandles = (
    MockTransport,
    Rc<RefCell<Vec<u8>>>,
    Rc<RefCell<bool>>,
    Rc<RefCell<u32>>,
);

/// Synchronous mock [`Transport`] for testing.
///
/// Replays pre-scripted byte chunks on `read` and records all `write_all`
/// calls for later assertion. Drives the SMTP state machine without any
/// real network I/O.
///
/// ## Usage
///
/// Pass server response chunks to [`MockTransport::new`]. The transport
/// delivers them in order; when exhausted, `read` returns `Ok(0)` (clean
/// EOF). Use the returned `Rc<RefCell<Vec<u8>>>` handle to inspect what the
/// client sent.
///
/// ```rust
/// use wasm_smtp_test::{MockTransport, block_on};
/// use wasm_smtp::SmtpClient;
///
/// let (transport, written, _closed) = MockTransport::new(&[
///     b"220 mx.example.com ESMTP\r\n",
///     b"250 mx.example.com\r\n",
///     b"221 Bye\r\n",
/// ]);
///
/// // connect() reads the greeting and sends EHLO.
/// block_on(async {
///     let client = SmtpClient::connect(transport, "client.example.com").await.unwrap();
///     client.quit().await.unwrap();
/// });
///
/// let sent = String::from_utf8(written.borrow().clone()).unwrap();
/// assert!(sent.contains("EHLO client.example.com\r\n"));
/// assert!(sent.contains("QUIT\r\n"));
/// ```
pub struct MockTransport {
    incoming: VecDeque<Vec<u8>>,
    /// Chunks held back until `upgrade_to_tls()` succeeds.
    pending_post: VecDeque<Vec<u8>>,
    written: Rc<RefCell<Vec<u8>>>,
    closed: Rc<RefCell<bool>>,
    upgrades: Rc<RefCell<u32>>,
    upgrade_behavior: UpgradeBehavior,
}

impl MockTransport {
    /// Build a transport from a list of server response chunks.
    ///
    /// Returns `(transport, written_handle, close_flag)`.
    /// `written_handle` accumulates everything the client writes.
    /// `close_flag` becomes `true` when [`Transport::close`] is called.
    pub fn new(chunks: &[&[u8]]) -> MockHandles {
        let (t, w, c, _u) = Self::build(chunks, &[], UpgradeBehavior::Succeed);
        (t, w, c)
    }

    /// Build a STARTTLS-aware transport.
    ///
    /// `pre_chunks` are delivered before the TLS upgrade; `post_chunks` are
    /// delivered only after `upgrade_to_tls()` succeeds. This matches the
    /// behaviour of a real SMTP server that withholds the post-TLS EHLO
    /// response until the handshake completes, and prevents spurious
    /// STARTTLS injection-detection errors in tests.
    ///
    /// Returns `(transport, written_handle, close_flag, upgrade_count)`.
    pub fn with_starttls(
        pre_chunks: &[&[u8]],
        post_chunks: &[&[u8]],
        behavior: UpgradeBehavior,
    ) -> MockStartTlsHandles {
        Self::build(pre_chunks, post_chunks, behavior)
    }

    fn build(
        pre_chunks: &[&[u8]],
        post_chunks: &[&[u8]],
        behavior: UpgradeBehavior,
    ) -> MockStartTlsHandles {
        let written = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(RefCell::new(false));
        let upgrades = Rc::new(RefCell::new(0u32));
        let incoming: VecDeque<Vec<u8>> = pre_chunks.iter().map(|c| c.to_vec()).collect();
        let pending_post: VecDeque<Vec<u8>> = post_chunks.iter().map(|c| c.to_vec()).collect();
        (
            Self {
                incoming,
                pending_post,
                written: Rc::clone(&written),
                closed: Rc::clone(&closed),
                upgrades: Rc::clone(&upgrades),
                upgrade_behavior: behavior,
            },
            written,
            closed,
            upgrades,
        )
    }
}

impl Transport for MockTransport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let Some(chunk) = self.incoming.front_mut() else {
            return Ok(0); // EOF — no more scripted responses
        };
        let n = buf.len().min(chunk.len());
        buf[..n].copy_from_slice(&chunk[..n]);
        chunk.drain(..n);
        if chunk.is_empty() {
            self.incoming.pop_front();
        }
        Ok(n)
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.written.borrow_mut().extend_from_slice(buf);
        Ok(())
    }

    async fn close(&mut self) -> Result<(), IoError> {
        *self.closed.borrow_mut() = true;
        Ok(())
    }
}

impl StartTlsCapable for MockTransport {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError> {
        *self.upgrades.borrow_mut() += 1;
        match &self.upgrade_behavior {
            UpgradeBehavior::Succeed => {
                // Move post-upgrade chunks into the live queue.
                while let Some(chunk) = self.pending_post.pop_front() {
                    self.incoming.push_back(chunk);
                }
                Ok(())
            }
            UpgradeBehavior::Fail(msg) => Err(IoError::new(*msg)),
        }
    }
}
