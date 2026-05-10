//! Streaming message body source for [`SmtpClient::send_mail_stream`].
//!
//! [`MessageBody`] is a project-defined async read abstraction that keeps
//! `wasm-smtp` runtime-independent. Unlike `tokio::io::AsyncRead`, it has
//! no executor dependency and can be implemented for any byte source.
//!
//! ## Built-in implementations
//!
//! | Type | Source | Notes |
//! |---|---|---|
//! | [`SliceBody`] | `&[u8]` | Zero-copy; useful for pre-serialised byte payloads |
//! | [`StrBody`] | `&str` | Zero-copy; same as `SliceBody` but from a string slice |
//!
//! ## Custom implementation
//!
//! Implement `MessageBody` for any async byte source:
//!
//! ```rust
//! use wasm_smtp::message_body::MessageBody;
//! use wasm_smtp::IoError;
//!
//! struct MyBodySource {
//!     data: Vec<u8>,
//!     pos: usize,
//! }
//!
//! impl MessageBody for MyBodySource {
//!     async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
//!         if self.pos >= self.data.len() {
//!             return Ok(0); // EOF
//!         }
//!         let n = buf.len().min(self.data.len() - self.pos);
//!         buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
//!         self.pos += n;
//!         Ok(n)
//!     }
//! }
//! ```
//!
//! ## Body requirements
//!
//! The bytes supplied by [`MessageBody::read_chunk`] must be a fully composed
//! RFC 5322 message with **CRLF line endings**. Dot-stuffing and the
//! end-of-data terminator (`\r\n.\r\n`) are applied automatically by
//! [`SmtpClient::send_mail_stream`]; the caller must not add them.

use crate::error::IoError;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// A streaming source of message body bytes.
///
/// `read_chunk` is called repeatedly until it returns `Ok(0)` (end of body).
/// Each call may fill any number of bytes into `buf[..n]` where `0 < n <=
/// buf.len()`. Returning `Ok(0)` signals end-of-stream and must not occur
/// before the full body has been supplied.
///
/// ## Body contract
///
/// - The bytes must form a valid RFC 5322 message (headers + blank line +
///   content) with **CRLF** (`\r\n`) line endings.
/// - Do **not** include the end-of-data terminator (`\r\n.\r\n`);
///   it is added automatically.
/// - Do **not** pre-dot-stuff lines starting with `.`; that is also handled
///   automatically.
///
/// ## Error handling
///
/// Returning `Err(IoError)` from `read_chunk` aborts the DATA phase and
/// surfaces the error to the caller of `send_mail_stream`. The SMTP session
/// is moved to `Closed`.
#[allow(async_fn_in_trait)]
pub trait MessageBody {
    /// Read the next chunk of bytes into `buf`.
    ///
    /// Returns `Ok(n)` where `n > 0` means `n` bytes were written to `buf`,
    /// and `Ok(0)` signals end-of-body. Never returns `Ok(0)` before all
    /// bytes have been supplied.
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;
}

// ---------------------------------------------------------------------------
// SliceBody
// ---------------------------------------------------------------------------

/// A [`MessageBody`] that reads from a `&[u8]` slice.
///
/// The slice is read in chunks of `buf.len()` bytes. No allocation occurs.
///
/// ```rust
/// use wasm_smtp::message_body::SliceBody;
///
/// let body = b"Subject: hi\r\n\r\nhello\r\n";
/// let mut source = SliceBody::new(body);
/// ```
pub struct SliceBody<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceBody<'a> {
    /// Create a [`SliceBody`] from a byte slice.
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl MessageBody for SliceBody<'_> {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// StrBody
// ---------------------------------------------------------------------------

/// A [`MessageBody`] that reads from a `&str` string slice.
///
/// Equivalent to `SliceBody::new(s.as_bytes())`.
///
/// ```rust
/// use wasm_smtp::message_body::StrBody;
///
/// let body = "Subject: hi\r\n\r\nhello\r\n";
/// let mut source = StrBody::new(body);
/// ```
pub struct StrBody<'a>(SliceBody<'a>);

impl<'a> StrBody<'a> {
    /// Create a [`StrBody`] from a string slice.
    #[must_use]
    pub fn new(s: &'a str) -> Self {
        Self(SliceBody::new(s.as_bytes()))
    }
}

impl MessageBody for StrBody<'_> {
    async fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.0.read_chunk(buf).await
    }
}
