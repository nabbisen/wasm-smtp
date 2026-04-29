//! End-to-end behaviour: drive a full SMTP exchange against a
//! `tokio_test::io::Builder` script that stands in for `worker::Socket`.
//!
//! Both `tokio_test::io::Builder` and the real `worker::Socket`
//! implement `tokio::io::AsyncRead + AsyncWrite`, so the adapter's
//! behaviour is identical from the SMTP client's point of view.

use std::cell::Cell;
use std::rc::Rc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_test::io::Builder;
use wasm_smtp::{IoError, SmtpClient, StartTlsCapable, Transport};

/// A minimal `Transport` over any `AsyncRead + AsyncWrite + Unpin`
/// stream. This is the same shape as `CloudflareTransport`, but
/// generic so the test can plug in `tokio_test::io::Mock`.
///
/// Optionally records the number of `upgrade_to_tls` calls so the
/// STARTTLS test can assert the upgrade was performed.
struct StreamTransport<S> {
    stream: S,
    upgrades: Rc<Cell<u32>>,
}

impl<S> StreamTransport<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            upgrades: Rc::new(Cell::new(0)),
        }
    }

    fn upgrade_counter(&self) -> Rc<Cell<u32>> {
        Rc::clone(&self.upgrades)
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

/// On a real `worker::Socket` this would consume the socket and
/// return a TLS-wrapped one. The `tokio_test` mock cannot perform
/// a TLS handshake, so we just count the call and continue using
/// the same plaintext stream — which is fine: the SMTP byte
/// sequence after STARTTLS is identical regardless of whether
/// the underlying bytes are actually encrypted on the wire.
impl<S: AsyncRead + AsyncWrite + Unpin> StartTlsCapable for StreamTransport<S> {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError> {
        self.upgrades.set(self.upgrades.get() + 1);
        Ok(())
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

#[tokio::test]
async fn starttls_full_flow() {
    // RFC 3207 STARTTLS submission flow (port 587 style):
    //   plaintext greeting + EHLO + STARTTLS + 220 ready
    //   -> upgrade -> EHLO again on the (notionally) TLS channel
    //   -> AUTH PLAIN -> normal MAIL FROM/RCPT TO/DATA/QUIT.
    // The `tokio_test` mock cannot actually perform a TLS handshake,
    // but `StreamTransport::upgrade_to_tls` is a no-op counter, so
    // the byte sequence post-upgrade is identical to a real run.
    let mock = Builder::new()
        // Pre-TLS greeting and EHLO. STARTTLS is advertised; AUTH
        // is deliberately NOT advertised yet, to exercise the
        // requirement that capabilities be re-read after upgrade.
        .read(b"220 mail.example.com ESMTP\r\n")
        .write(b"EHLO client.example.com\r\n")
        .read(b"250-mail.example.com\r\n250-PIPELINING\r\n250 STARTTLS\r\n")
        // STARTTLS handshake.
        .write(b"STARTTLS\r\n")
        .read(b"220 ready to start TLS\r\n")
        // Post-TLS: re-issue EHLO; this time AUTH is advertised.
        .write(b"EHLO client.example.com\r\n")
        .read(b"250-mail.example.com\r\n250-PIPELINING\r\n250 AUTH PLAIN\r\n")
        // Normal authenticated transaction on the upgraded stream.
        .write(b"AUTH PLAIN AHVzZXIAcGFzcw==\r\n")
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
    let upgrades = transport.upgrade_counter();
    let mut client = SmtpClient::connect_starttls(transport, "client.example.com")
        .await
        .expect("connect_starttls");
    // The transport's upgrade hook must have fired exactly once.
    assert_eq!(upgrades.get(), 1);
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
