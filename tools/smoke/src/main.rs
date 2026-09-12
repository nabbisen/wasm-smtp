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
    /// Implicit TLS, but the guest is handed a CA that did not sign the
    /// certificate the responder serves. The session must not happen.
    ///
    /// Without this, a client that skipped certificate validation entirely
    /// would produce a transcript identical to `Implicit` and pass every
    /// other assertion here — the one wrong client the positive modes
    /// cannot see. It also discharges RFC 017's acceptance criterion that
    /// validation fails against an untrusted certificate, which had never
    /// been checked on a host.
    Untrusted,
    /// STARTTLS with an untrusted CA. The plaintext leg happens, the
    /// upgrade fails, and — the property under test — nothing is spoken in
    /// plaintext afterwards. A client that fell back to the unprotected
    /// channel on a failed upgrade would be caught here (DEC-015).
    UntrustedStartTls,
}

impl Mode {
    /// The mode name, and the argument the guest is invoked with. The
    /// negative mode runs the guest's implicit-TLS path: the difference is
    /// entirely in which CA it is given.
    fn guest_arg(self) -> &'static str {
        match self {
            Mode::Implicit | Mode::Untrusted => "implicit",
            Mode::StartTls | Mode::UntrustedStartTls => "starttls",
        }
    }

    /// Whether this mode expects the session to be refused.
    fn is_negative(self) -> bool {
        matches!(self, Mode::Untrusted | Mode::UntrustedStartTls)
    }

    /// Whether the guest talks plaintext first and upgrades in place.
    fn speaks_starttls(self) -> bool {
        matches!(self, Mode::StartTls | Mode::UntrustedStartTls)
    }

    fn as_str(self) -> &'static str {
        match self {
            Mode::Implicit => "implicit",
            Mode::StartTls => "starttls",
            Mode::Untrusted => "untrusted",
            Mode::UntrustedStartTls => "untrusted-starttls",
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

    // A second, unrelated certificate. Its PEM is handed to the guest in
    // the negative mode while the responder keeps serving the first one, so
    // the only thing wrong with the connection is the trust chain.
    let (other_cert_pem, _other_key_pem) = match generate_cert() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("smoke: second certificate generation failed: {e}");
            std::process::exit(2);
        }
    };
    let other_ca_path = std::env::temp_dir().join(format!(
        "wasm-smtp-smoke-other-ca-{}.pem",
        std::process::id()
    ));
    if let Err(e) = std::fs::write(&other_ca_path, &other_cert_pem) {
        eprintln!("smoke: writing {} failed: {e}", other_ca_path.display());
        std::process::exit(2);
    }

    let mut failures = 0;
    for mode in [
        Mode::Implicit,
        Mode::StartTls,
        Mode::Untrusted,
        Mode::UntrustedStartTls,
    ] {
        // The negative modes differ only in which CA the guest trusts.
        let guest_ca = if mode.is_negative() {
            &other_ca_path
        } else {
            &ca_path
        };
        match run_mode(mode, &guest, guest_ca, &cert_pem, &key_pem) {
            Ok(()) => println!("PASS  {}", mode.as_str()),
            Err(e) => {
                println!("FAIL  {}: {e}", mode.as_str());
                failures += 1;
            }
        }
    }

    let _ = std::fs::remove_file(&ca_path);
    let _ = std::fs::remove_file(&other_ca_path);
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
        let outcome = match listener.accept() {
            Ok((sock, _)) => serve(sock, mode, tls_config),
            Err(e) => (Recording::default(), Err(format!("accept failed: {e}"))),
        };
        let _ = tx.send(());
        outcome
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
        .arg(mode.guest_arg())
        .arg("127.0.0.1")
        .arg(port.to_string())
        .arg(ca_path)
        .output()
        .map_err(|e| format!("running {wasmtime} failed: {e} (set WASMTIME to override)"))?;

    // Let the responder finish before judging the recording.
    let _ = rx.recv_timeout(std::time::Duration::from_secs(30));
    let (recording, responder_result) = responder
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

    // The negative mode expects the opposite of everything below: the guest
    // must fail, and nothing may reach the responder. A responder-side error
    // is the normal outcome there (the handshake dies under it), so it is
    // not treated as a failure of the test.
    if mode.is_negative() {
        // A responder-side error is the normal outcome here: the handshake
        // dies underneath it. The recording is what matters.
        return check_untrusted(mode, output.status, &guest_stderr, &recording)
            .map_err(|e| context(&e));
    }

    if let Err(e) = responder_result {
        return Err(context(&format!("responder: {e}")));
    }
    if !output.status.success() {
        return Err(context(&format!(
            "guest failed; recorded so far: {:?}",
            recording.commands()
        )));
    }

    check(mode, &recording).map_err(|e| context(&e))
}

/// Assert that an untrusted certificate stopped the session dead.
///
/// Three things must hold, and the third is the one that matters: a client
/// that validated nothing would still exit 0 and still talk SMTP, so the
/// proof is that no command was ever spoken.
fn check_untrusted(
    mode: Mode,
    status: std::process::ExitStatus,
    guest_stderr: &str,
    rec: &Recording,
) -> Result<(), String> {
    if status.success() {
        return Err(format!(
            "guest accepted an untrusted certificate: exited {status} \
             after speaking {:?}",
            rec.commands()
        ));
    }

    // The failure has to be about the certificate, not about, say, a port
    // that was never opened.
    let lowered = guest_stderr.to_lowercase();
    let names_tls_failure = [
        "certificate",
        "handshake",
        "tls",
        "unknownissuer",
        "invalidcertificate",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    if !names_tls_failure {
        return Err(format!(
            "guest failed, but not with a TLS/certificate error: {guest_stderr:?}"
        ));
    }

    // What may legitimately have been spoken before the handshake failed:
    // nothing at all for implicit TLS, and exactly the plaintext EHLO and
    // STARTTLS for the upgrade path.
    let allowed: &[&str] = if mode.speaks_starttls() {
        &["EHLO smoke.example.com", "STARTTLS"]
    } else {
        &[]
    };
    if rec.commands() != allowed {
        return Err(format!(
            "after a refused certificate the responder should have seen \
             {allowed:?}, but it received {:?}",
            rec.commands()
        ));
    }
    // Nothing may arrive over the channel that was never protected.
    if rec.lines.iter().any(|(leg, _)| *leg == Leg::Tls) {
        return Err("no command may arrive after a failed handshake".to_owned());
    }

    Ok(())
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

/// Serve one scripted session, returning what was received either way.
///
/// The recording is returned even when the session fails, because in the
/// negative modes the failure is the expected outcome and the transcript up
/// to that point is the evidence.
fn serve(sock: TcpStream, mode: Mode, cfg: Arc<ServerConfig>) -> (Recording, Result<(), String>) {
    let mut rec = Recording::default();
    let result = serve_inner(sock, mode, cfg, &mut rec);
    (rec, result)
}

fn serve_inner(
    sock: TcpStream,
    mode: Mode,
    cfg: Arc<ServerConfig>,
    rec: &mut Recording,
) -> Result<(), String> {
    sock.set_nodelay(true).ok();
    let mut sock = sock;
    if mode.speaks_starttls() {
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
    // Both implicit modes greet once TLS is up; the STARTTLS leg already
    // greeted in plaintext. The negative mode gets the same treatment as
    // Implicit so that, if validation ever stopped happening, the session
    // would proceed and the assertions would catch it — rather than both
    // sides waiting on each other and the test hanging.
    if !mode.speaks_starttls() {
        greet(&mut tls).map_err(|e| format!("greeting failed: {e}"))?;
    }
    smtp_phase(&mut tls, rec).map_err(|e| format!("TLS leg failed: {e}"))?;
    Ok(())
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
