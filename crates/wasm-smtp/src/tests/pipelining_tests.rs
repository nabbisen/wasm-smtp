//! Tests for SMTP PIPELINING (RFC 2920).

use super::harness::{MockTransport, block_on, flatten};
use crate::client::SmtpClient;
use crate::protocol::ehlo_advertises_pipelining;

// ── Protocol helper ───────────────────────────────────────────────────────

#[test]
fn ehlo_advertises_pipelining_detects_capability() {
    let caps = vec!["PIPELINING".into(), "8BITMIME".into()];
    assert!(ehlo_advertises_pipelining(&caps));
}

#[test]
fn ehlo_advertises_pipelining_case_insensitive() {
    let caps = vec!["pipelining".into()];
    assert!(ehlo_advertises_pipelining(&caps));
    let caps2 = vec!["Pipelining".into()];
    assert!(ehlo_advertises_pipelining(&caps2));
}

#[test]
fn ehlo_advertises_pipelining_absent_returns_false() {
    let caps = vec!["AUTH PLAIN".into(), "8BITMIME".into()];
    assert!(!ehlo_advertises_pipelining(&caps));
}

// ── Client integration ────────────────────────────────────────────────────

/// Server exchange advertising PIPELINING.
fn pipelining_exchange(num_recipients: usize) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = vec![
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250-AUTH PLAIN LOGIN\r\n250 PIPELINING\r\n",
        b"235 2.7.0 OK\r\n",   // AUTH
        b"250 2.1.0 OK\r\n",   // MAIL FROM
    ];
    for _ in 0..num_recipients {
        parts.push(b"250 2.1.5 OK\r\n"); // RCPT TO
    }
    parts.extend_from_slice(&[
        b"354 Start mail\r\n",       // DATA
        b"250 2.0.0 OK: queued as A1B2C3\r\n",  // DATA body
        b"221 2.0.0 Bye\r\n",        // QUIT
    ]);
    flatten(&parts)
}

/// Server exchange WITHOUT PIPELINING.
fn no_pipelining_exchange(num_recipients: usize) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = vec![
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"250 2.1.0 OK\r\n",
    ];
    for _ in 0..num_recipients {
        parts.push(b"250 2.1.5 OK\r\n");
    }
    parts.extend_from_slice(&[
        b"354 Start mail\r\n",
        b"250 2.0.0 OK queued\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    flatten(&parts)
}

/// Capture the commands sent before DATA (MAIL FROM, RCPT TO lines, DATA).
fn extract_pre_data_commands(wire: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(wire);
    s.lines()
        .filter(|l| l.starts_with("MAIL FROM:") || l.starts_with("RCPT TO:") || *l == "DATA")
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn pipelining_send_mail_succeeds_single_recipient() {
    let (transport, written, _) = MockTransport::new(&[&pipelining_exchange(1)]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"],
            "Subject: test\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow().clone();
    let cmds = extract_pre_data_commands(&wire);
    assert_eq!(cmds.len(), 3, "MAIL FROM + RCPT TO + DATA: {cmds:?}");
}

#[test]
fn pipelining_send_mail_succeeds_multiple_recipients() {
    let recipients = ["a@e.com", "b@e.com", "c@e.com"];
    let (transport, written, _) = MockTransport::new(&[&pipelining_exchange(3)]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &recipients,
            "Subject: multi\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow().clone();
    let cmds = extract_pre_data_commands(&wire);
    // 1 MAIL FROM + 3 RCPT TO + 1 DATA = 5
    assert_eq!(cmds.len(), 5, "expected 5 pre-DATA commands: {cmds:?}");
}

#[test]
fn no_pipelining_send_mail_still_succeeds() {
    // Ensure the sequential path works correctly after the pipelining refactor.
    let (transport, written, _) = MockTransport::new(&[&no_pipelining_exchange(2)]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["a@e.com", "b@e.com"],
            "Subject: seq\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow().clone();
    let cmds = extract_pre_data_commands(&wire);
    assert_eq!(cmds.len(), 4, "1 MAIL FROM + 2 RCPT TO + 1 DATA: {cmds:?}");
}

#[test]
fn pipelining_wire_contains_all_commands_before_body() {
    // Verify the wire bytes contain MAIL FROM, RCPT TO, and DATA all
    // before the first body bytes appear.
    let (transport, written, _) = MockTransport::new(&[&pipelining_exchange(2)]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        c.send_mail("from@example.com", &["x@e.com", "y@e.com"],
            "Subject: order\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = written.borrow().clone();
    let s = String::from_utf8_lossy(&wire);
    let mail_pos  = s.find("MAIL FROM:").unwrap();
    let rcpt1_pos = s.find("RCPT TO:<x@e.com>").unwrap();
    let rcpt2_pos = s.find("RCPT TO:<y@e.com>").unwrap();
    let data_pos  = s.find("DATA\r\n").unwrap();
    let body_pos  = s.find("Subject: order").unwrap();

    assert!(mail_pos  < rcpt1_pos, "MAIL FROM before first RCPT TO");
    assert!(rcpt1_pos < rcpt2_pos, "first RCPT TO before second");
    assert!(rcpt2_pos < data_pos,  "RCPT TOs before DATA");
    assert!(data_pos  < body_pos,  "DATA before body");
}

#[test]
fn pipelining_result_carries_queue_id() {
    let (transport, _, _) = MockTransport::new(&[&pipelining_exchange(1)]);
    let outcome = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login("u", "p").await.unwrap();
        let o = c.send_mail("from@example.com", &["to@example.com"],
            "Subject: queue\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
        o
    });

    assert_eq!(outcome.code, 250, "pipelining outcome must be 250");
    assert!(outcome.queue_id.is_some(), "queue id should be parsed from '250 2.0.0 OK: queued as A1B2C3'");
}
