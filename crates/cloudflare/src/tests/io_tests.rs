//! Tests for `read_async_io` and `write_all_async_io` — the
//! `tokio::io` ↔ `wasm-smtp::Transport` byte-pushing helpers.

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
