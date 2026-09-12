//! The tokio adapter, end to end, over a real loopback socket (RFC 032 D1).
//!
//! Until this test the adapter a native Rust user picks had never completed
//! a send in a test. Its ten unit tests cover options and error paths; the
//! success path — a TLS handshake against a real certificate, a STARTTLS
//! upgrade on the same socket, authentication, dot-stuffed DATA, QUIT — was
//! exercised only through the core's mock transport, which by construction
//! cannot see what the adapter does with a socket.
//!
//! Each case serves the scripted responder from this crate's library on a
//! `std::thread`, drives `wasm-smtp-tokio` against it, and holds the
//! recorded transcript to the same assertions the WASI adapter smoke test
//! and the component harness use.
//!
//! Trust goes through [`ConnectOptions::with_root_store`] holding only the
//! run's CA: the public API a private-CA user calls, so that path is covered
//! too. The certificate is generated per run; no key material is committed.

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::thread;
use std::time::{Duration, Instant};

use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;
use wasm_smtp::SmtpClient;
use wasm_smtp_smoke::{
    Expected, Recording, check_refused, check_session, generate_cert, serve_implicit,
    serve_starttls, server_config,
};
use wasm_smtp_tokio::{ConnectOptions, TokioPlainTransport, TokioTlsTransport};

const EXP: Expected<'static> = Expected {
    ehlo: "tokio.example.com",
    from: "tokio@example.com",
    rcpt: "rcpt@example.org",
};

/// A leading-dot line, so the responder can prove the adapter's output was
/// dot-stuffed on the wire rather than trusting that it was.
const BODY: &str = "From: tokio@example.com\r\n\
                    To: rcpt@example.org\r\n\
                    Subject: tokio adapter\r\n\
                    \r\n\
                    first line\r\n\
                    .leading-dot\r\n\
                    last line\r\n";

/// How long the responder waits for a connection, and each client step for
/// the responder, before the test fails rather than hangs.
const TIMEOUT: Duration = Duration::from_secs(30);

type Responder = thread::JoinHandle<(Recording, Result<(), String>)>;

/// Bind loopback, and serve one scripted session on a thread. Returns the
/// port and the certificate the responder presents.
fn start(starttls: bool) -> (u16, String, Responder) {
    let (cert_pem, key_pem) = generate_cert().expect("certificate");
    let cfg = server_config(&cert_pem, &key_pem).expect("server config");
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("bind");
    let port = listener.local_addr().expect("local_addr").port();
    // Never block forever on accept: a client that fails before connecting
    // must fail the test, not hang it.
    listener.set_nonblocking(true).expect("set_nonblocking");

    let responder = thread::spawn(move || {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match listener.accept() {
                Ok((sock, _)) => {
                    if let Err(e) = sock.set_nonblocking(false) {
                        return (Recording::default(), Err(format!("set_blocking: {e}")));
                    }
                    return if starttls {
                        serve_starttls(sock, cfg)
                    } else {
                        serve_implicit(sock, cfg)
                    };
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return (
                            Recording::default(),
                            Err("the client never connected".to_owned()),
                        );
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return (Recording::default(), Err(format!("accept: {e}"))),
            }
        }
    });
    (port, cert_pem, responder)
}

/// A root store holding exactly one certificate: the entire trust set.
fn trusting(cert_pem: &str) -> ConnectOptions {
    let mut store = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(cert_pem.as_bytes()) {
        store.add(cert.expect("CA PEM")).expect("add CA");
    }
    // The certificate carries an IP SAN for 127.0.0.1, which is what the
    // client connects to, for the reason tools/smoke gives: `localhost` can
    // resolve to ::1 first and turn this into a test of resolution order.
    ConnectOptions::new()
        .with_root_store(store)
        .with_server_name("127.0.0.1")
}

async fn within<F: Future>(what: &str, fut: F) -> F::Output {
    tokio::time::timeout(TIMEOUT, fut)
        .await
        .unwrap_or_else(|_| panic!("{what} did not finish within {TIMEOUT:?}"))
}

fn join(responder: Responder) -> (Recording, Result<(), String>) {
    responder.join().expect("responder thread panicked")
}

#[tokio::test]
async fn implicit_tls_sends_the_session_smtp_requires() {
    let (port, cert_pem, responder) = start(false);

    let transport = within(
        "TLS connect",
        TokioTlsTransport::connect_with("127.0.0.1", port, trusting(&cert_pem)),
    )
    .await
    .expect("implicit TLS connect");
    let mut client = within("EHLO", SmtpClient::connect(transport, EXP.ehlo))
        .await
        .expect("SMTP connect");
    within("AUTH", client.login(EXP.from, "secret"))
        .await
        .expect("login");
    let outcome = within("send", client.send_mail(EXP.from, &[EXP.rcpt], BODY))
        .await
        .expect("send_mail");
    within("QUIT", client.quit()).await.expect("quit");

    let (rec, served) = join(responder);
    served.expect("responder");
    // The reply code the responder gave to the end of DATA, as the adapter
    // reports it.
    assert_eq!(outcome.code, 250, "reply code");
    check_session(&rec, false, &EXP).unwrap();
}

#[tokio::test]
async fn starttls_upgrades_the_same_socket_and_sends() {
    let (port, cert_pem, responder) = start(true);

    let transport = within(
        "TCP connect",
        TokioPlainTransport::connect_with("127.0.0.1", port, "127.0.0.1", trusting(&cert_pem)),
    )
    .await
    .expect("plaintext connect");
    let mut client = within(
        "EHLO + STARTTLS",
        SmtpClient::connect_starttls(transport, EXP.ehlo),
    )
    .await
    .expect("STARTTLS connect");
    within("AUTH", client.login(EXP.from, "secret"))
        .await
        .expect("login");
    let outcome = within("send", client.send_mail(EXP.from, &[EXP.rcpt], BODY))
        .await
        .expect("send_mail");
    within("QUIT", client.quit()).await.expect("quit");

    let (rec, served) = join(responder);
    served.expect("responder");
    assert_eq!(outcome.code, 250, "reply code");
    // Includes: EHLO and STARTTLS in plaintext, everything after inside TLS
    // on the one connection the responder accepted.
    check_session(&rec, true, &EXP).unwrap();
}

#[tokio::test]
async fn implicit_tls_refuses_a_certificate_it_cannot_chain() {
    let (port, _served_cert, responder) = start(false);
    // A second, unrelated certificate as the entire trust set. The responder
    // keeps presenting the first, so the only thing wrong is the chain.
    let (other_cert, _other_key) = generate_cert().expect("second certificate");

    // The property is that nothing was sent, not that an error came back.
    // So if the connect ever succeeds, carry on as a caller would: an adapter
    // that skipped validation then speaks SMTP, and `check_refused` below
    // sees every command it spoke.
    let connected = within(
        "TLS connect",
        TokioTlsTransport::connect_with("127.0.0.1", port, trusting(&other_cert)),
    )
    .await;
    if let Ok(transport) = connected {
        if let Ok(mut client) = within("EHLO", SmtpClient::connect(transport, EXP.ehlo)).await {
            let _ = within("AUTH", client.login(EXP.from, "secret")).await;
            let _ = within("send", client.send_mail(EXP.from, &[EXP.rcpt], BODY)).await;
            let _ = within("QUIT", client.quit()).await;
        }
    }

    // A responder-side error is the expected outcome: the handshake dies
    // underneath it. The recording is what decides.
    let (rec, _served) = join(responder);
    check_refused(&rec, false, EXP.ehlo).unwrap();
}
