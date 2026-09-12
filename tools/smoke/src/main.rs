//! On-target smoke test for the WASI adapter (RFC 025 D1).
//!
//! Starts a scripted TLS-terminated SMTP responder on loopback, runs the
//! `smoke` guest under wasmtime against it, and asserts that the session
//! that actually crossed the wire is the one SMTP requires. Both modes are
//! exercised: implicit TLS, and STARTTLS with its in-place upgrade.
//!
//! ```text
//! cargo build --target wasm32-wasip2 -p wasm-smtp-wasi --example smoke
//! cargo run -p wasm-smtp-smoke
//! ```
//!
//! Environment overrides:
//!
//! - `SMOKE_GUEST` — path to the guest `.wasm` (default:
//!   `target/wasm32-wasip2/debug/examples/smoke.wasm`).
//! - `WASMTIME` — wasmtime binary (default: `wasmtime` from `PATH`).
//!
//! The guest connects to `127.0.0.1` rather than `localhost`, and the
//! generated certificate carries an IP SAN for it. Resolving `localhost`
//! inside the guest can yield `::1` first, which a v4-only listener would
//! refuse; pinning the literal keeps the test about SMTP rather than about
//! the host's name resolution order.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// Which leg of the session a recorded line arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    Plain,
    Tls,
}

/// What the responder saw.
#[derive(Debug, Default)]
struct Recording {
    /// Command lines, in arrival order, tagged with the leg they arrived on.
    lines: Vec<(Leg, String)>,
    /// The DATA payload, verbatim, exactly as it arrived (still
    /// dot-stuffed, terminator excluded).
    body: String,
}

impl Recording {
    fn commands(&self) -> Vec<&str> {
        self.lines.iter().map(|(_, l)| l.as_str()).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Implicit,
    StartTls,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Implicit => "implicit",
            Mode::StartTls => "starttls",
        }
    }
}

fn main() {
    let guest = std::env::var("SMOKE_GUEST").map_or_else(
        |_| PathBuf::from("target/wasm32-wasip2/debug/examples/smoke.wasm"),
        PathBuf::from,
    );
    if !guest.exists() {
        eprintln!(
            "smoke: guest not found at {}\n\
             build it first: cargo build --target wasm32-wasip2 -p wasm-smtp-wasi --example smoke",
            guest.display()
        );
        std::process::exit(2);
    }

    let (cert_pem, key_pem) = match generate_cert() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("smoke: certificate generation failed: {e}");
            std::process::exit(2);
        }
    };
    let ca_path =
        std::env::temp_dir().join(format!("wasm-smtp-smoke-ca-{}.pem", std::process::id()));
    if let Err(e) = std::fs::write(&ca_path, &cert_pem) {
        eprintln!("smoke: writing {} failed: {e}", ca_path.display());
        std::process::exit(2);
    }

    let mut failures = 0;
    for mode in [Mode::Implicit, Mode::StartTls] {
        match run_mode(mode, &guest, &ca_path, &cert_pem, &key_pem) {
            Ok(()) => println!("PASS  {}", mode.as_str()),
            Err(e) => {
                println!("FAIL  {}: {e}", mode.as_str());
                failures += 1;
            }
        }
    }

    let _ = std::fs::remove_file(&ca_path);
    if failures > 0 {
        eprintln!("smoke: {failures} mode(s) failed");
        std::process::exit(1);
    }
}

/// Run one mode end to end and check what crossed the wire.
fn run_mode(
    mode: Mode,
    guest: &Path,
    ca_path: &Path,
    cert_pem: &str,
    key_pem: &str,
) -> Result<(), String> {
    let tls_config = server_config(cert_pem, key_pem)?;

    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .map_err(|e| format!("bind failed: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr failed: {e}"))?
        .port();

    // One connection, one scripted session, on its own thread so the guest
    // can run while the responder talks.
    let (tx, rx) = mpsc::channel();
    let responder = thread::spawn(move || {
        let result = listener
            .accept()
            .map_err(|e| format!("accept failed: {e}"))
            .and_then(|(sock, _)| serve(sock, mode, tls_config));
        let _ = tx.send(());
        result
    });

    let wasmtime = std::env::var("WASMTIME").unwrap_or_else(|_| "wasmtime".to_owned());
    let output = Command::new(&wasmtime)
        .arg("run")
        .arg("--wasi")
        .arg("inherit-network")
        .arg("--wasi")
        .arg("allow-ip-name-lookup")
        .arg("--dir")
        .arg(format!(
            "{}::{}",
            ca_path.parent().unwrap_or(Path::new("/")).display(),
            ca_path.parent().unwrap_or(Path::new("/")).display()
        ))
        .arg(guest)
        .arg(mode.as_str())
        .arg("127.0.0.1")
        .arg(port.to_string())
        .arg(ca_path)
        .output()
        .map_err(|e| format!("running {wasmtime} failed: {e} (set WASMTIME to override)"))?;

    // Let the responder finish before judging the recording.
    let _ = rx.recv_timeout(std::time::Duration::from_secs(30));
    let responder_result = responder
        .join()
        .map_err(|_| "responder thread panicked".to_owned())?;

    // The guest's stderr is the useful half of almost every failure here, so
    // it goes into the message whichever side reported the problem first. A
    // responder-side error usually means the guest hung up on it.
    let guest_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let context = |what: &str| {
        format!(
            "{what}\n--- guest exit ---\n{}\n--- guest stderr ---\n{}",
            output.status,
            if guest_stderr.is_empty() {
                "(empty)"
            } else {
                &guest_stderr
            }
        )
    };

    let recording = match responder_result {
        Ok(r) => r,
        Err(e) => return Err(context(&format!("responder: {e}"))),
    };
    if !output.status.success() {
        return Err(context(&format!(
            "guest failed; recorded so far: {:?}",
            recording.commands()
        )));
    }

    check(mode, &recording).map_err(|e| context(&e))
}

/// Assert the recorded session is the one SMTP requires.
fn check(mode: Mode, rec: &Recording) -> Result<(), String> {
    let cmds = rec.commands();
    let mut expected: Vec<&str> = Vec::new();
    if mode == Mode::StartTls {
        expected.push("EHLO smoke.example.com");
        expected.push("STARTTLS");
    }
    expected.extend([
        "EHLO smoke.example.com",
        "AUTH PLAIN",
        "MAIL FROM:<smoke@example.com>",
        "RCPT TO:<rcpt@example.org>",
        "DATA",
        "QUIT",
    ]);

    if cmds.len() != expected.len() {
        return Err(format!(
            "expected {} commands, saw {}: {:?}",
            expected.len(),
            cmds.len(),
            cmds
        ));
    }
    for (i, (want, got)) in expected.iter().zip(cmds.iter()).enumerate() {
        // AUTH PLAIN carries a base64 payload; match the command only.
        let ok = if *want == "AUTH PLAIN" {
            got.starts_with("AUTH PLAIN ")
        } else {
            got == want
        };
        if !ok {
            return Err(format!(
                "command {i}: expected {want:?}, saw {got:?} (full: {cmds:?})"
            ));
        }
    }

    // Dot-stuffing must have happened on the client side: the body line the
    // guest wrote as `.leading-dot` has to arrive as `..leading-dot`.
    if !rec.body.contains("..leading-dot\r\n") {
        return Err(format!(
            "DATA body is not dot-stuffed; expected a `..leading-dot` line, body was:\n{:?}",
            rec.body
        ));
    }
    // And the terminator must not have been swallowed into the body.
    if rec.body.contains("\r\n.\r\n") {
        return Err("DATA body contains the end-of-data terminator".to_owned());
    }

    if mode == Mode::StartTls {
        // The upgrade must be real: everything from the second EHLO on has
        // to have arrived inside TLS.
        let legs: Vec<Leg> = rec.lines.iter().map(|(leg, _)| *leg).collect();
        if legs[0] != Leg::Plain || legs[1] != Leg::Plain {
            return Err(format!(
                "expected EHLO and STARTTLS in plaintext, legs: {legs:?}"
            ));
        }
        if legs[2..].iter().any(|l| *l != Leg::Tls) {
            return Err(format!(
                "everything after STARTTLS must arrive over TLS, legs: {legs:?}"
            ));
        }
    } else if rec.lines.iter().any(|(leg, _)| *leg != Leg::Tls) {
        return Err("implicit-TLS mode saw plaintext commands".to_owned());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Responder
// ---------------------------------------------------------------------------

/// Serve one scripted session and return what was received.
fn serve(sock: TcpStream, mode: Mode, cfg: Arc<ServerConfig>) -> Result<Recording, String> {
    sock.set_nodelay(true).ok();
    let mut rec = Recording::default();

    let mut sock = sock;
    if mode == Mode::StartTls {
        // Plaintext leg: greeting, EHLO, STARTTLS.
        greet(&mut sock).map_err(|e| format!("greeting failed: {e}"))?;
        loop {
            let line = read_line(&mut sock).map_err(|e| format!("plaintext read failed: {e}"))?;
            let Some(line) = line else {
                return Err("client closed before STARTTLS".to_owned());
            };
            rec.lines.push((Leg::Plain, line.clone()));
            if line.starts_with("EHLO") {
                // Advertise STARTTLS but no AUTH: credentials must not be
                // offered before the channel is protected.
                write_all(
                    &mut sock,
                    "250-smoke.test\r\n250-PIPELINING\r\n250-STARTTLS\r\n250 SMTPUTF8\r\n",
                )
                .map_err(|e| format!("EHLO reply failed: {e}"))?;
            } else if line == "STARTTLS" {
                write_all(&mut sock, "220 2.0.0 Ready to start TLS\r\n")
                    .map_err(|e| format!("STARTTLS reply failed: {e}"))?;
                break;
            } else {
                return Err(format!("unexpected plaintext command: {line:?}"));
            }
        }
    }

    // TLS leg.
    let conn = ServerConnection::new(cfg).map_err(|e| format!("ServerConnection::new: {e}"))?;
    let mut tls = StreamOwned::new(conn, sock);
    if mode == Mode::Implicit {
        greet(&mut tls).map_err(|e| format!("greeting failed: {e}"))?;
    }
    smtp_phase(&mut tls, &mut rec).map_err(|e| format!("TLS leg failed: {e}"))?;
    Ok(rec)
}

fn greet<S: Write>(io: &mut S) -> io::Result<()> {
    write_all(io, "220 smoke.test ESMTP ready\r\n")
}

/// The command/response loop, from `EHLO` to `QUIT`.
fn smtp_phase<S: Read + Write>(io: &mut S, rec: &mut Recording) -> io::Result<()> {
    loop {
        let Some(line) = read_line(io)? else {
            return Ok(()); // client hung up
        };
        rec.lines.push((Leg::Tls, line.clone()));

        if line.starts_with("EHLO") {
            write_all(
                io,
                "250-smoke.test\r\n250-PIPELINING\r\n250-AUTH PLAIN LOGIN\r\n250 SMTPUTF8\r\n",
            )?;
        } else if line.starts_with("AUTH PLAIN") {
            write_all(io, "235 2.7.0 Authentication successful\r\n")?;
        } else if line.starts_with("MAIL FROM") {
            write_all(io, "250 2.1.0 Ok\r\n")?;
        } else if line.starts_with("RCPT TO") {
            write_all(io, "250 2.1.5 Ok\r\n")?;
        } else if line == "DATA" {
            write_all(io, "354 End data with <CR><LF>.<CR><LF>\r\n")?;
            rec.body = read_data(io)?;
            write_all(io, "250 2.0.0 OK: queued as SMOKE0001\r\n")?;
        } else if line == "QUIT" {
            write_all(io, "221 2.0.0 Bye\r\n")?;
            return Ok(());
        } else {
            write_all(io, "500 5.5.2 Command unrecognized\r\n")?;
            return Ok(());
        }
    }
}

/// Read the DATA payload up to, but not including, the `\r\n.\r\n`
/// terminator. The payload is returned exactly as received, dot-stuffing
/// included, so the driver can prove the client stuffed it.
fn read_data<S: Read>(io: &mut S) -> io::Result<String> {
    let mut body = String::new();
    loop {
        let Some(line) = read_line(io)? else {
            return Ok(body);
        };
        if line == "." {
            return Ok(body);
        }
        body.push_str(&line);
        body.push_str("\r\n");
    }
}

/// Read one CRLF-terminated line, returning it without the terminator.
/// `Ok(None)` means the peer closed cleanly.
fn read_line<S: Read>(io: &mut S) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match io.read(&mut byte) {
            Ok(0) => {
                return Ok(if line.is_empty() {
                    None
                } else {
                    Some(String::from_utf8_lossy(&line).into_owned())
                });
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
                }
                line.push(byte[0]);
                if line.len() > 64 * 1024 {
                    return Err(io::Error::other("line too long"));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn write_all<S: Write>(io: &mut S, s: &str) -> io::Result<()> {
    io.write_all(s.as_bytes())?;
    io.flush()
}

// ---------------------------------------------------------------------------
// Certificate
// ---------------------------------------------------------------------------

/// Generate a throwaway self-signed certificate valid for `localhost` and
/// `127.0.0.1`. Returns `(cert PEM, key PEM)`.
fn generate_cert() -> Result<(String, String), String> {
    let subject_alt_names = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
    let cert =
        rcgen::generate_simple_self_signed(subject_alt_names).map_err(|e| format!("rcgen: {e}"))?;
    Ok((cert.cert.pem(), cert.signing_key.serialize_pem()))
}

fn server_config(cert_pem: &str, key_pem: &str) -> Result<Arc<ServerConfig>, String> {
    use rustls_pki_types::pem::PemObject;

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("parsing certificate PEM: {e}"))?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())
        .map_err(|e| format!("parsing key PEM: {e}"))?;

    // Name the provider explicitly, for the same reason the adapters do
    // (RFC 024 D9): never depend on a process-wide default.
    let cfg =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("rustls protocol versions: {e}"))?
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("rustls server config: {e}"))?;
    Ok(Arc::new(cfg))
}
