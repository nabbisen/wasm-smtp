//! Test harness shared across the in-tree test modules.
//!
//! `MockTransport` simulates a remote SMTP server by replaying
//! pre-scripted byte chunks and capturing client writes for
//! later assertion. `block_on` and `flatten` are simple
//! conveniences. None of this is exposed as part of the public
//! API.

use crate::error::IoError;
use crate::transport::{StartTlsCapable, Transport};
use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Behavior of [`MockTransport`]'s STARTTLS upgrade. Tests configure
/// this when building the transport.
#[derive(Debug, Clone)]
pub enum UpgradeBehavior {
    /// `upgrade_to_tls()` returns `Ok(())`.
    Succeed,
    /// `upgrade_to_tls()` returns `Err` with this message.
    Fail(&'static str),
}

/// Drive a future to completion using a no-op waker.
///
/// This is sound only for futures whose `Pending` state would never be
/// observed by a real executor: the mock transport in this module
/// always resolves its `read` and `write_all` futures synchronously,
/// so the very first `poll` will return `Ready`.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("mock-driven future returned Pending"),
    }
}

/// Triple of (mock transport, captured outgoing bytes, close flag),
/// returned by [`MockTransport::new`].
pub type MockHandles = (MockTransport, Rc<RefCell<Vec<u8>>>, Rc<RefCell<bool>>);

/// Quadruple returned by [`MockTransport::with_starttls`]: the
/// transport, the captured-bytes handle, the close flag, and a
/// counter that is incremented each time `upgrade_to_tls()` is
/// invoked.
pub type MockStartTlsHandles = (
    MockTransport,
    Rc<RefCell<Vec<u8>>>,
    Rc<RefCell<bool>>,
    Rc<RefCell<u32>>,
);

/// Synchronous mock transport.
///
/// `incoming` is a queue of byte chunks; each chunk is one "wire
/// delivery" and may be split across multiple `read` calls depending
/// on the caller's buffer size. When the queue is exhausted, further
/// `read`s return `Ok(0)`, which the SMTP state machine interprets as
/// a clean close from the peer.
///
/// `written` is held behind `Rc<RefCell<_>>` so the test can keep a
/// handle to it after the transport has been moved into the client.
pub struct MockTransport {
    incoming: VecDeque<Vec<u8>>,
    /// Chunks queued to be revealed only after `upgrade_to_tls()`
    /// has been called. Empty for non-STARTTLS tests.
    pending_post: VecDeque<Vec<u8>>,
    written: Rc<RefCell<Vec<u8>>>,
    closed: Rc<RefCell<bool>>,
    /// Number of times `upgrade_to_tls()` has been called. Incremented
    /// whether the call succeeds or fails.
    upgrades: Rc<RefCell<u32>>,
    /// Configured behavior for `upgrade_to_tls()`.
    upgrade_behavior: UpgradeBehavior,
}

impl MockTransport {
    /// Construct a mock transport from a list of byte chunks. Each
    /// chunk corresponds to one "wire packet". Returns the transport
    /// together with shared handles to the captured outgoing bytes
    /// and the close flag.
    ///
    /// The transport's `upgrade_to_tls()` succeeds by default but is
    /// not exposed; tests that exercise STARTTLS should use
    /// [`Self::with_starttls`] instead.
    pub fn new(chunks: &[&[u8]]) -> MockHandles {
        let (t, w, c, _u) = Self::build(chunks, &[], UpgradeBehavior::Succeed);
        (t, w, c)
    }

    /// Construct a mock transport that exposes its STARTTLS upgrade
    /// counter and that models a real STARTTLS-aware server: the
    /// `pre_chunks` are delivered before any `upgrade_to_tls()`
    /// call, and the `post_chunks` are revealed only after the
    /// upgrade has been performed. This mirrors the behaviour of a
    /// real submission server, which does not pipeline the post-TLS
    /// EHLO reply onto the plaintext channel — and lets us
    /// exercise the v0.5.0 STARTTLS-injection defence without
    /// false positives caused by the older "all bytes in one
    /// chunk" mock layout.
    ///
    /// Tests that want to deliberately simulate a STARTTLS
    /// injection (bytes pipelined onto the plaintext channel after
    /// the `220`) should pass those bytes as part of `pre_chunks`
    /// and verify that the upgrade is rejected.
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
        let mut q: VecDeque<Vec<u8>> = VecDeque::new();
        for c in pre_chunks {
            q.push_back((*c).to_vec());
        }
        let mut pending_post: VecDeque<Vec<u8>> = VecDeque::new();
        for c in post_chunks {
            pending_post.push_back((*c).to_vec());
        }
        (
            Self {
                incoming: q,
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
            return Ok(0);
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
                // Real servers withhold the post-TLS EHLO reply until
                // the TLS handshake has completed. Move the queued
                // post-upgrade chunks into the live read queue now.
                while let Some(chunk) = self.pending_post.pop_front() {
                    self.incoming.push_back(chunk);
                }
                Ok(())
            }
            UpgradeBehavior::Fail(msg) => Err(IoError::new(*msg)),
        }
    }
}

/// Concatenate several byte slices into one. Useful for assembling a
/// scripted server reply that must be delivered in a single chunk.
pub fn flatten(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}
