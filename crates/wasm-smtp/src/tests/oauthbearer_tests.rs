//! Tests for AUTH OAUTHBEARER (RFC 7628).

use super::harness::{MockTransport, block_on, flatten};
use crate::client::{SmtpClient, SmtpClientOptions};
use crate::error::SmtpError;
use crate::protocol::{AuthMechanism, build_oauthbearer_initial_response};

// ── Protocol helpers ──────────────────────────────────────────────────────

#[test]
fn oauthbearer_initial_response_has_gs2_header() {
    let resp = build_oauthbearer_initial_response("user@example.com", "token123");
    let decoded = base64_decode(&resp);
    assert!(decoded.starts_with("n,a="), "must start with GS2 header n,a=");
    assert!(decoded.contains("\x01auth=Bearer token123\x01\x01"),
        "must contain auth=Bearer key");
}

#[test]
fn oauthbearer_empty_user_gives_empty_authzid() {
    let resp = build_oauthbearer_initial_response("", "tok");
    let decoded = base64_decode(&resp);
    // Empty authzid: n,,\x01auth=Bearer tok\x01\x01
    assert!(decoded.starts_with("n,a=,") || decoded.starts_with("n,,"),
        "empty user should produce empty authzid: {decoded:?}");
}

#[test]
fn oauthbearer_differs_from_xoauth2() {
    let bearer = build_oauthbearer_initial_response("u@e.com", "tok");
    let xoauth2 = crate::protocol::build_xoauth2_initial_response("u@e.com", "tok");
    // Different SASL payload format despite same inputs.
    assert_ne!(bearer, xoauth2, "OAUTHBEARER and XOAUTH2 must produce different responses");
    // OAUTHBEARER payload starts with 'n,' (GS2 header)
    let bearer_decoded = base64_decode(&bearer);
    assert!(bearer_decoded.starts_with("n,"), "OAUTHBEARER must have GS2 header");
    // XOAUTH2 payload starts with 'user='
    let xoauth2_decoded = base64_decode(&xoauth2);
    assert!(xoauth2_decoded.starts_with("user="), "XOAUTH2 must start with user=");
}

// ── AUTH mechanism name ───────────────────────────────────────────────────

#[test]
fn oauthbearer_mechanism_name_is_correct() {
    assert_eq!(AuthMechanism::OAuthBearer.name(), "OAUTHBEARER");
}

// ── Client integration ────────────────────────────────────────────────────

fn ehlo_with_oauthbearer() -> Vec<u8> {
    flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH OAUTHBEARER PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",   // AUTH accepted
        b"250 2.1.0 OK\r\n",   // MAIL FROM
        b"250 2.1.5 OK\r\n",   // RCPT TO
        b"354 Start mail\r\n", // DATA
        b"250 2.0.0 OK queued\r\n", // DATA body
        b"221 2.0.0 Bye\r\n",  // QUIT
    ])
}

#[test]
fn login_oauthbearer_success() {
    let (transport, written, _) = MockTransport::new(&[&ehlo_with_oauthbearer()]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login_oauthbearer("user@example.com", "access_token_abc").await.unwrap();
        c.send_mail("from@example.com", &["to@example.com"],
            "Subject: test\r\n\r\nbody\r\n").await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(wire.contains("AUTH OAUTHBEARER "), "must send AUTH OAUTHBEARER command");
}

#[test]
fn login_with_oauthbearer_sends_correct_mechanism() {
    // Minimal exchange: greeting + EHLO + AUTH + QUIT only.
    let exchange = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH OAUTHBEARER PLAIN LOGIN\r\n",
        b"235 2.7.0 OK\r\n",
        b"221 2.0.0 Bye\r\n",
    ]);
    let (transport, written, _) = MockTransport::new(&[&exchange]);
    block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login_with(AuthMechanism::OAuthBearer, "user@example.com", "token_xyz")
            .await.unwrap();
        c.quit().await.unwrap();
    });

    let wire = String::from_utf8(written.borrow().clone()).unwrap();
    assert!(wire.contains("AUTH OAUTHBEARER "), "login_with must send AUTH OAUTHBEARER");
}

#[test]
fn login_oauthbearer_server_challenge_returns_auth_rejected() {
    let exchanges = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        b"250-mail.example.com\r\n250 AUTH OAUTHBEARER\r\n",
        // Server sends 334 JSON error challenge
        b"334 eyJzdGF0dXMiOiI0MDEiLCJzY2hlbWVzIjoiQmVhcmVyIn0=\r\n",
        // After client sends \x01, server sends 535
        b"535 5.7.8 Authentication credentials invalid\r\n",
    ]);
    let (transport, _, _) = MockTransport::new(&[&exchanges]);
    let err = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login_oauthbearer("user@example.com", "bad_token").await
    }).expect_err("should fail");

    assert!(matches!(err, SmtpError::Auth(_)), "must return Auth error: {err:?}");
}

#[test]
fn login_oauthbearer_unsupported_returns_error() {
    let exchanges = flatten(&[
        b"220 mail.example.com ESMTP\r\n",
        // Server does NOT advertise OAUTHBEARER
        b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
    ]);
    let (transport, _, _) = MockTransport::new(&[&exchanges]);
    let err = block_on(async {
        let mut c = SmtpClient::connect(transport, "client.example.com").await.unwrap();
        c.login_oauthbearer("user@example.com", "token").await
    }).expect_err("should fail");

    assert!(matches!(err, SmtpError::Auth(_)), "must return Auth error: {err:?}");
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn base64_decode(s: &str) -> String {
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let bytes: Vec<u8> = s.chars()
        .filter(|&c| c != '=')
        .map(|c| alphabet.find(c).unwrap() as u8)
        .collect();
    for chunk in bytes.chunks(4) {
        if chunk.len() >= 2 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
        }
        if chunk.len() >= 3 {
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if chunk.len() == 4 {
            out.push((chunk[2] << 6) | chunk[3]);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
