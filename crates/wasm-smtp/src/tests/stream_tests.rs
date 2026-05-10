//! Tests for `DotStufferState` (streaming dot-stuffer) and
//! `send_mail_stream` (RFC 019 Phase 3).

use super::harness::{MockTransport, block_on, flatten};
use crate::client::{SmtpClient, SmtpClientOptions};
use crate::message_body::{MessageBody, SliceBody, StrBody};
use crate::protocol::DotStufferState;

// ---------------------------------------------------------------------------
// DotStufferState unit tests
// ---------------------------------------------------------------------------

/// Helper: feed `body` through the stuffer one byte at a time (worst-case
/// for cross-chunk boundary handling) and collect the output.
fn stuff_byte_by_byte(body: &[u8]) -> Vec<u8> {
    let mut stuffer = DotStufferState::new();
    let mut out = Vec::new();
    for &b in body {
        out.extend_from_slice(&stuffer.process_chunk(&[b]));
    }
    out.extend_from_slice(&stuffer.finish());
    out
}

/// Helper: feed the whole body in one chunk (equivalent to the batch path).
fn stuff_single_chunk(body: &[u8]) -> Vec<u8> {
    let mut stuffer = DotStufferState::new();
    let mut out = stuffer.process_chunk(body).to_vec();
    out.extend_from_slice(&stuffer.finish());
    out
}

#[test]
fn streaming_matches_batch_for_normal_body() {
    use crate::protocol::dot_stuff_and_terminate;
    let body = b"Subject: test\r\n\r\nHello world\r\n";
    let batch = dot_stuff_and_terminate(body);
    let stream = stuff_single_chunk(body);
    assert_eq!(batch, stream, "single-chunk streaming must match batch");
}

#[test]
fn streaming_byte_by_byte_matches_batch() {
    use crate::protocol::dot_stuff_and_terminate;
    let body = b"Subject: test\r\n\r\n.dot line\r\nnormal\r\n..double\r\n";
    let batch = dot_stuff_and_terminate(body);
    let stream = stuff_byte_by_byte(body);
    assert_eq!(batch, stream, "byte-by-byte streaming must match batch");
}

#[test]
fn dot_at_line_start_spanning_chunk_boundary_is_stuffed() {
    // Chunk 1 ends with \r\n; chunk 2 starts with .
    let c1 = b"line one\r\n";
    let c2 = b".dotted\r\n";
    let mut stuffer = DotStufferState::new();
    let out1 = stuffer.process_chunk(c1);
    let out2 = stuffer.process_chunk(c2);
    let term = stuffer.finish();
    let wire: Vec<u8> = [out1, out2, term].concat();
    let pos = wire.windows(9).position(|w| w == b"..dotted\r");
    assert!(pos.is_some(), "dot at chunk boundary must be stuffed: {wire:?}");
}

#[test]
fn empty_body_produces_crlf_terminator() {
    use crate::protocol::dot_stuff_and_terminate;
    let batch = dot_stuff_and_terminate(b"");
    let stream = stuff_single_chunk(b"");
    assert_eq!(batch, stream);
    assert_eq!(stream, b"\r\n.\r\n");
}

#[test]
fn body_already_ending_with_crlf_not_doubled() {
    let body = b"last line\r\n";
    let out = stuff_single_chunk(body);
    // Should end with exactly one \r\n then .\r\n
    assert!(out.ends_with(b"\r\n.\r\n"), "must end with \\r\\n.\\r\\n");
    let crlf_count = out.windows(2).filter(|w| *w == b"\r\n").count();
    // "last line\r\n" = 1 CRLF; then ".\r\n" = 1 CRLF → total 2
    assert_eq!(crlf_count, 2, "exactly two CRLFs for a one-line body");
}

#[test]
fn multiple_consecutive_dot_lines_all_stuffed() {
    let body = b".a\r\n.b\r\n.c\r\n";
    let out = stuff_single_chunk(body);
    assert!(out.windows(3).any(|w| w == b"..a"), "first dot line must be stuffed");
    assert!(out.windows(3).any(|w| w == b"..b"), "second dot line must be stuffed");
    assert!(out.windows(3).any(|w| w == b"..c"), "third dot line must be stuffed");
}

#[test]
fn process_chunk_empty_input_is_noop() {
    let mut s = DotStufferState::new();
    let out = s.process_chunk(b"");
    assert!(out.is_empty());
    // finish() on a stuffer that never saw any bytes:
    let term = s.finish();
    assert_eq!(term, b"\r\n.\r\n");
}

// ---------------------------------------------------------------------------
// SliceBody and StrBody unit tests
// ---------------------------------------------------------------------------

#[test]
fn slice_body_reads_all_bytes_in_order() {
    let data = b"Hello\r\nWorld\r\n";
    let mut body = SliceBody::new(data);
    let mut buf = [0u8; 5];
    let mut collected = Vec::new();
    loop {
        let n = block_on(body.read_chunk(&mut buf)).expect("read");
        if n == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..n]);
    }
    assert_eq!(collected.as_slice(), data);
}

#[test]
fn str_body_reads_correct_utf8_bytes() {
    let s = "Subject: hi\r\n\r\nbody\r\n";
    let mut body = StrBody::new(s);
    let mut buf = [0u8; 64];
    let n = block_on(body.read_chunk(&mut buf)).expect("read");
    assert_eq!(&buf[..n], s.as_bytes());
    let eof = block_on(body.read_chunk(&mut buf)).expect("read");
    assert_eq!(eof, 0, "second read must be EOF");
}

// ---------------------------------------------------------------------------
// send_mail_stream integration tests
// ---------------------------------------------------------------------------

fn standard_exchange() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n",
        b"250 2.1.5 OK\r\n",
        b"354 Start mail input\r\n",
        b"250 2.0.0 OK: queued\r\n",
        b"221 2.0.0 Bye\r\n",
    ])
}

#[test]
fn send_mail_stream_produces_same_wire_as_send_mail() {
    let body_str = "Subject: stream test\r\n\r\nHello streaming world\r\n";

    // Via send_mail (reference)
    let (t1, w1, _) = MockTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect(t1, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"], body_str)
            .await
            .unwrap();
        c.quit().await.unwrap();
    });

    // Via send_mail_stream
    let (t2, w2, _) = MockTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect(t2, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_stream(
            "from@example.com",
            &["to@example.com"],
            &mut StrBody::new(body_str),
        )
        .await
        .unwrap();
        c.quit().await.unwrap();
    });

    assert_eq!(
        w1.borrow().as_slice(),
        w2.borrow().as_slice(),
        "send_mail_stream must produce identical wire bytes to send_mail"
    );
}

#[test]
fn send_mail_stream_dot_stuffs_correctly() {
    let body = "Subject: dots\r\n\r\n.line one\r\nnormal\r\n.line two\r\n";
    let (transport, written, _) = MockTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_stream(
            "from@example.com",
            &["to@example.com"],
            &mut StrBody::new(body),
        )
        .await
        .unwrap();
        c.quit().await.unwrap();
    });

    let wire = String::from_utf8(written.borrow().clone()).unwrap();
    let data_section = wire.split("DATA\r\n").nth(1).unwrap();
    assert!(data_section.contains("..line one\r\n"), "first dot-line must be stuffed");
    assert!(data_section.contains("..line two\r\n"), "second dot-line must be stuffed");
}

#[test]
fn send_mail_stream_with_slice_body() {
    let body = b"Subject: bytes\r\n\r\nbody bytes\r\n";
    let (transport, written, _) = MockTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_stream(
            "from@example.com",
            &["to@example.com"],
            &mut SliceBody::new(body),
        )
        .await
        .unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow();
    assert!(wire.windows(6).any(|w| w == b"DATA\r\n"), "DATA command must be sent");
    assert!(wire.windows(3).any(|w| w == b".\r\n"), "terminator must be present");
}

#[test]
fn send_mail_stream_with_policy_rejection_sends_no_smtp_commands() {
    use crate::error::{PolicyError, SmtpError};
    use crate::policy::SendPolicy;

    struct BlockAll;
    impl SendPolicy for BlockAll {
        fn check_sender(&self, _: &str) -> Result<(), PolicyError> {
            Err(PolicyError::new("blocked"))
        }
        fn check_recipients(&self, _: &[&str]) -> Result<(), PolicyError> { Ok(()) }
        fn check_message_size(&self, _: usize) -> Result<(), PolicyError> { Ok(()) }
    }

    let opts = SmtpClientOptions::new().with_policy(Box::new(BlockAll));
    let (transport, written, _) = MockTransport::new(&[&standard_exchange()]);
    let err = block_on(async {
        let mut c = SmtpClient::connect_with(transport, "client.example.com", opts)
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_stream(
            "from@example.com",
            &["to@example.com"],
            &mut SliceBody::new(b"Subject: test\r\n\r\nbody\r\n"),
        )
        .await
    })
    .expect_err("policy must reject");

    assert!(matches!(err, SmtpError::Policy(_)));
    let sent = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(!sent.contains("MAIL FROM"), "MAIL FROM must not be sent after policy rejection");
}

#[test]
fn send_mail_stream_audit_events_are_emitted() {
    use crate::audit::{AuditSink, SmtpAuditEvent, VecAuditSink};
    use std::sync::Arc;

    let sink = Arc::new(VecAuditSink::default());
    let opts = SmtpClientOptions::new().with_audit(Box::new(Arc::clone(&sink)));

    let (transport, _, _) = MockTransport::new(&[&standard_exchange()]);
    block_on(async {
        let mut c = SmtpClient::connect_with(transport, "client.example.com", opts)
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_stream(
            "from@example.com",
            &["to@example.com"],
            &mut StrBody::new("Subject: audit\r\n\r\nbody\r\n"),
        )
        .await
        .unwrap();
        c.quit().await.unwrap();
    });

    let events = sink.events();
    assert!(events.iter().any(|e| e.starts_with("MailFromAccepted")));
    assert!(events.iter().any(|e| e.starts_with("RecipientAccepted")));
    assert!(events.iter().any(|e| e.starts_with("MessageAccepted")));
    assert!(events.iter().any(|e| e.starts_with("QuitCompleted")));
}
