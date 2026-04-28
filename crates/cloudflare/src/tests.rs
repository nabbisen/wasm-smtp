//! Internal test suite for `wasm-smtp-cloudflare`.
//!
//! These tests run on the host (`cargo test`); they do **not**
//! exercise `worker::Socket`, which requires the Cloudflare Workers
//! runtime. Instead, they cover the byte-pushing helpers
//! [`crate::adapter::read_async_io`] and
//! [`crate::adapter::write_all_async_io`] using
//! `tokio_test::io::Builder` as a stand-in for the real socket.
//!
//! Coverage for the full SMTP exchange — greeting, `EHLO`, `AUTH
//! LOGIN`, `MAIL FROM`/`RCPT TO`/`DATA`/`QUIT` — lives in
//! `wasm-smtp-core`'s in-tree mock-driven integration tests. The
//! adapter does not duplicate that work; it only verifies that the
//! `tokio::io` ↔ `wasm-smtp-core::Transport` translation is correct.
//!
//! End-to-end tests against a real submission server require a
//! Cloudflare Workers runtime (`wrangler dev`) and are not run by
//! `cargo test`.

#![allow(
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]

use crate::adapter::{read_async_io, write_all_async_io};
use tokio_test::io::Builder;

// -- read_async_io ----------------------------------------------------------

#[tokio::test]
async fn read_async_io_returns_zero_on_eof() {
    // No `read()` calls scripted: the mock immediately reports EOF.
    let mut mock = Builder::new().build();
    let mut buf = [0u8; 16];
    let n = read_async_io(&mut mock, &mut buf).await.expect("ok");
    assert_eq!(n, 0);
}

#[tokio::test]
async fn read_async_io_returns_bytes_when_available() {
    let mut mock = Builder::new().read(b"hello").build();
    let mut buf = [0u8; 16];
    let n = read_async_io(&mut mock, &mut buf).await.expect("ok");
    assert_eq!(n, 5);
    assert_eq!(&buf[..n], b"hello");
}

#[tokio::test]
async fn read_async_io_propagates_errors_as_io_error() {
    let err = std::io::Error::other("simulated transport failure");
    let mut mock = Builder::new().read_error(err).build();
    let mut buf = [0u8; 16];
    let result = read_async_io(&mut mock, &mut buf).await;
    let err = result.expect_err("must error");
    let s = format!("{err}");
    assert!(
        s.contains("read failed") && s.contains("simulated transport failure"),
        "unexpected error message: {s}",
    );
}

// -- write_all_async_io -----------------------------------------------------

#[tokio::test]
async fn write_all_async_io_writes_full_buffer() {
    let mut mock = Builder::new().write(b"EHLO client.example.com\r\n").build();
    write_all_async_io(&mut mock, b"EHLO client.example.com\r\n")
        .await
        .expect("write_all ok");
}

#[tokio::test]
async fn write_all_async_io_handles_short_writes() {
    // Split one buffer across two scheduled `write` calls; the helper
    // must still complete because `AsyncWriteExt::write_all` loops
    // internally.
    let mut mock = Builder::new()
        .write(b"DATA\r\n")
        .write(b"Subject: t\r\n\r\nbody\r\n.\r\n")
        .build();
    write_all_async_io(&mut mock, b"DATA\r\n")
        .await
        .expect("first ok");
    write_all_async_io(&mut mock, b"Subject: t\r\n\r\nbody\r\n.\r\n")
        .await
        .expect("second ok");
}

#[tokio::test]
async fn write_all_async_io_propagates_errors_as_io_error() {
    let err = std::io::Error::other("simulated write failure");
    let mut mock = Builder::new().write_error(err).build();
    let result = write_all_async_io(&mut mock, b"X").await;
    let err = result.expect_err("must error");
    let s = format!("{err}");
    assert!(
        s.contains("write failed") && s.contains("simulated write failure"),
        "unexpected error message: {s}",
    );
}

// -- end-to-end SMTP behavior over a tokio mock -----------------------------

mod e2e_via_tokio_mock {
    //! End-to-end-style tests that drive a full SMTP transaction
    //! through `wasm-smtp-core::SmtpClient` using a `tokio_test`
    //! mock as the underlying byte stream. The mock plays the role
    //! of a `worker::Socket` — both implement `tokio::io::AsyncRead`
    //! and `tokio::io::AsyncWrite`, so the adapter's behavior is
    //! identical.

    use super::Builder;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use wasm_smtp_core::{IoError, SmtpClient, Transport};

    /// A minimal `Transport` over any `AsyncRead + AsyncWrite + Unpin`
    /// stream. This is the same shape as `CloudflareTransport`, but
    /// generic so the test can plug in `tokio_test::io::Mock`.
    struct StreamTransport<S> {
        stream: S,
    }

    impl<S> StreamTransport<S> {
        fn new(stream: S) -> Self {
            Self { stream }
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> Transport for StreamTransport<S> {
        async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
            AsyncReadExt::read(&mut self.stream, buf)
                .await
                .map_err(|e| IoError::new(format!("read failed: {e}")))
        }

        async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
            AsyncWriteExt::write_all(&mut self.stream, buf)
                .await
                .map_err(|e| IoError::new(format!("write failed: {e}")))
        }

        async fn close(&mut self) -> Result<(), IoError> {
            AsyncWriteExt::shutdown(&mut self.stream)
                .await
                .map_err(|e| IoError::new(format!("shutdown failed: {e}")))
        }
    }

    #[tokio::test]
    async fn full_authenticated_transaction() {
        // Script the server's side of the conversation.
        let mock = Builder::new()
            .read(b"220 mail.example.com ESMTP\r\n")
            .write(b"EHLO client.example.com\r\n")
            .read(b"250-mail.example.com\r\n250 AUTH LOGIN\r\n")
            .write(b"AUTH LOGIN\r\n")
            .read(b"334 VXNlcm5hbWU6\r\n")
            .write(b"dXNlcg==\r\n") // "user"
            .read(b"334 UGFzc3dvcmQ6\r\n")
            .write(b"cGFzcw==\r\n") // "pass"
            .read(b"235 OK\r\n")
            .write(b"MAIL FROM:<a@example.com>\r\n")
            .read(b"250 OK\r\n")
            .write(b"RCPT TO:<b@example.org>\r\n")
            .read(b"250 OK\r\n")
            .write(b"DATA\r\n")
            .read(b"354 go ahead\r\n")
            .write(b"Subject: t\r\n\r\nhi\r\n.\r\n")
            .read(b"250 Queued\r\n")
            .write(b"QUIT\r\n")
            .read(b"221 Bye\r\n")
            .build();

        let transport = StreamTransport::new(mock);
        let mut client = SmtpClient::connect(transport, "client.example.com")
            .await
            .expect("connect");
        client.login("user", "pass").await.expect("login");
        client
            .send_mail(
                "a@example.com",
                &["b@example.org"],
                "Subject: t\r\n\r\nhi\r\n",
            )
            .await
            .expect("send");
        client.quit().await.expect("quit");
    }

    #[tokio::test]
    async fn unauthenticated_transaction() {
        let mock = Builder::new()
            .read(b"220 mail.example.com ESMTP\r\n")
            .write(b"EHLO client.example.com\r\n")
            .read(b"250-mail.example.com\r\n250 8BITMIME\r\n")
            .write(b"MAIL FROM:<a@example.com>\r\n")
            .read(b"250 OK\r\n")
            .write(b"RCPT TO:<b@example.org>\r\n")
            .read(b"250 OK\r\n")
            .write(b"DATA\r\n")
            .read(b"354 go ahead\r\n")
            .write(b"Subject: t\r\n\r\nhi\r\n.\r\n")
            .read(b"250 Queued\r\n")
            .write(b"QUIT\r\n")
            .read(b"221 Bye\r\n")
            .build();

        let transport = StreamTransport::new(mock);
        let mut client = SmtpClient::connect(transport, "client.example.com")
            .await
            .expect("connect");
        client
            .send_mail(
                "a@example.com",
                &["b@example.org"],
                "Subject: t\r\n\r\nhi\r\n",
            )
            .await
            .expect("send");
        client.quit().await.expect("quit");
    }
}
