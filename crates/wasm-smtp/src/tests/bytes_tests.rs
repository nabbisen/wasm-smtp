//! Tests for `send_mail_bytes` (RFC 019 Phase 2) and large-message
//! / memory-behaviour scenarios (RFC 020).

use super::harness::{MockTransport, block_on, flatten};
use crate::client::SmtpClient;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn standard_exchange(extra_rcpt_replies: usize) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = vec![
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n",
        b"250 2.1.5 OK\r\n",
    ];
    for _ in 0..extra_rcpt_replies {
        parts.push(b"250 2.1.5 OK\r\n");
    }
    parts.extend_from_slice(&[
        b"354 Start mail input\r\n",
        b"250 2.0.0 OK: queued\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    flatten(&parts)
}

fn run_bytes_send(body: &[u8]) -> String {
    let script = standard_exchange(0);
    let (transport, written, _closed) = MockTransport::new(&[&script]);
    block_on(async {
        let mut client = SmtpClient::connect(transport, "client.example.com")
            .await
            .expect("connect");
        client.login("u", "p").await.expect("login");
        client
            .send_mail_bytes(
                "from@example.com",
                &["to@example.com"],
                body,
            )
            .await
            .expect("send_mail_bytes");
        client.quit().await.expect("quit");
    });
    String::from_utf8(written.borrow().clone()).unwrap()
}

// ---------------------------------------------------------------------------
// send_mail_bytes — basic correctness
// ---------------------------------------------------------------------------

#[test]
fn send_mail_bytes_sends_same_wire_as_send_mail() {
    let body_str = "Subject: hi\r\n\r\nHello world\r\n";
    let body_bytes = body_str.as_bytes();

    // Via send_mail (str)
    let script = standard_exchange(0);
    let (t1, w1, _) = MockTransport::new(&[&script]);
    block_on(async {
        let mut c = SmtpClient::connect(t1, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"], body_str)
            .await
            .unwrap();
        c.quit().await.unwrap();
    });

    // Via send_mail_bytes
    let sent_bytes = run_bytes_send(body_bytes);
    let sent_str = String::from_utf8(w1.borrow().clone()).unwrap();

    assert_eq!(
        sent_bytes, sent_str,
        "send_mail_bytes and send_mail must produce identical wire output for the same body"
    );
}

#[test]
fn send_mail_bytes_dot_stuffs_lines_starting_with_dot() {
    let body = b"Subject: dots\r\n\r\n.line one\r\nnormal\r\n.line two\r\n";
    let wire = run_bytes_send(body);
    // Both dot-lines must be stuffed.
    let data_section = wire
        .split("DATA\r\n")
        .nth(1)
        .expect("DATA command must appear");
    assert!(
        data_section.contains("..line one\r\n"),
        "first dot-line must be stuffed"
    );
    assert!(
        data_section.contains("..line two\r\n"),
        "second dot-line must be stuffed"
    );
}

#[test]
fn send_mail_bytes_appends_crlf_dot_crlf_terminator() {
    let wire = run_bytes_send(b"Subject: test\r\n\r\nbody\r\n");
    assert!(
        wire.contains("\r\n.\r\n"),
        "end-of-data terminator must be present"
    );
}

#[test]
fn send_mail_bytes_handles_empty_body() {
    let wire = run_bytes_send(b"");
    // Empty body: should still have the terminator.
    assert!(wire.contains("\r\n.\r\n"), "terminator required even for empty body");
}

#[test]
fn send_mail_bytes_handles_body_without_trailing_crlf() {
    // Body that does not end with \r\n: dot_stuff_and_terminate adds one.
    let wire = run_bytes_send(b"Subject: test\r\n\r\nbody without newline");
    assert!(wire.contains("\r\n.\r\n"), "terminator must be appended");
}

#[test]
fn send_mail_bytes_validates_from_address() {
    let script = standard_exchange(0);
    let (transport, _written, _closed) = MockTransport::new(&[&script]);
    let err = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        // CR in address is always rejected.
        c.send_mail_bytes("from\r@example.com", &["to@example.com"], b"body")
            .await
    })
    .expect_err("address with CR must fail");
    assert!(matches!(err, crate::error::SmtpError::InvalidInput(_)));
}

#[test]
fn send_mail_bytes_rejects_empty_recipient_list() {
    let script = standard_exchange(0);
    let (transport, _written, _closed) = MockTransport::new(&[&script]);
    let err = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_bytes("from@example.com", &[], b"body").await
    })
    .expect_err("empty recipients must fail");
    assert!(matches!(err, crate::error::SmtpError::InvalidInput(_)));
}

// ---------------------------------------------------------------------------
// RFC 020: Large-message behaviour
// ---------------------------------------------------------------------------

/// Generate a body of approximately `target_bytes` bytes using repeating
/// 72-character ASCII lines with CRLF endings (74 bytes per line).
fn large_body(target_bytes: usize) -> Vec<u8> {
    let line = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n";
    let reps = target_bytes.div_ceil(line.len());
    let mut body = Vec::with_capacity(reps * line.len());
    for _ in 0..reps {
        body.extend_from_slice(line);
    }
    body
}

/// Generate a body where every line starts with `.` to maximise dot-stuffing.
fn dot_heavy_body(lines: usize) -> Vec<u8> {
    let mut body = Vec::with_capacity(lines * 12);
    for i in 0..lines {
        body.extend_from_slice(format!(".line {i}\r\n").as_bytes());
    }
    body
}

fn run_large_send(body: Vec<u8>) {
    let script = standard_exchange(0);
    let (transport, written, _closed) = MockTransport::new(&[&script]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .expect("connect");
        c.login("u", "p").await.expect("login");
        c.send_mail_bytes("from@example.com", &["to@example.com"], &body)
            .await
            .expect("send_mail_bytes with large body");
        c.quit().await.expect("quit");
    });
    // Memory-growth sanity check: the on-wire payload (after dot-stuffing)
    // must be at most 3× the input size.
    let wire_len = written.borrow().len();
    assert!(
        wire_len <= body.len() * 3 + 4096,
        "on-wire bytes ({wire_len}) must not exceed 3× input ({}) + overhead",
        body.len()
    );
}

/// 100 KB body — should complete quickly.
#[test]
fn large_body_100kb_sends_correctly() {
    run_large_send(large_body(100_000));
}

/// 1 MB body.
#[test]
fn large_body_1mb_sends_correctly() {
    run_large_send(large_body(1_000_000));
}

/// 10 MB body — gated with `#[ignore]` to keep CI fast.
/// Run with: `cargo test large_body_10mb -- --include-ignored`
#[test]
#[ignore]
fn large_body_10mb_sends_correctly() {
    run_large_send(large_body(10_000_000));
}

/// 10 000 consecutive dot-leading lines.
#[test]
fn dot_heavy_body_10k_lines_all_stuffed() {
    let body = dot_heavy_body(10_000);
    let script = standard_exchange(0);
    let (transport, written, _closed) = MockTransport::new(&[&script]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_bytes("from@example.com", &["to@example.com"], &body)
            .await
            .unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow();
    // Find the DATA payload section and verify stuffing.
    let data_idx = wire
        .windows(6)
        .position(|w| w == b"DATA\r\n")
        .expect("DATA must appear")
        + 6; // skip "DATA\r\n"
    let after_data = &wire[data_idx..];

    // Count "..line " occurrences (stuffed) vs ".line " (unstuffed).
    // There must be exactly 10_000 stuffed and 0 unstuffed (mid-payload).
    let stuffed_count = after_data
        .windows(7)
        .filter(|w| *w == b"..line ")
        .count();
    // Each ".line N\r\n" becomes "..line N\r\n"; 10 000 lines.
    assert_eq!(stuffed_count, 10_000, "all 10 000 dot-lines must be stuffed");
}

/// Many recipients: 10 RCPT TO commands.
#[test]
fn many_recipients_sends_one_rcpt_per_address() {
    const N: usize = 10;
    let script = standard_exchange(N - 1); // standard_exchange already has one rcpt reply
    let (transport, written, _closed) = MockTransport::new(&[&script]);
    let recipients: Vec<String> = (0..N).map(|i| format!("r{i}@example.com")).collect();
    let recipient_refs: Vec<&str> = recipients.iter().map(String::as_str).collect();

    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com")
            .await
            .unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail_bytes(
            "from@example.com",
            &recipient_refs,
            b"Subject: bulk\r\n\r\nbody\r\n",
        )
        .await
        .unwrap();
        c.quit().await.unwrap();
    });

    let wire = String::from_utf8(written.borrow().clone()).unwrap();
    let rcpt_count = wire.matches("RCPT TO:").count();
    assert_eq!(rcpt_count, N, "must send exactly {N} RCPT TO commands");
}

/// Body of exactly 998 bytes per line (RFC 5321 maximum recommended).
#[test]
fn body_with_max_length_lines_sends_correctly() {
    // 998 bytes of 'A' + CRLF = 1000 bytes per line (RFC 5321 limit is
    // 1000 bytes including CRLF).
    let line: Vec<u8> = std::iter::repeat(b'A').take(998).chain(*b"\r\n").collect();
    let body: Vec<u8> = line.repeat(5);
    run_large_send(body);
}
