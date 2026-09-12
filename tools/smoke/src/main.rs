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

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use wasm_smtp_smoke::{
    Expected, Recording, check_refused, check_session, generate_cert, serve_implicit,
    serve_starttls, server_config,
};

/// The session the `smoke` guest is written to send.
const SMOKE: Expected<'static> = Expected {
    ehlo: "smoke.example.com",
    from: "smoke@example.com",
    rcpt: "rcpt@example.org",
};

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
            Ok((sock, _)) => {
                if mode.speaks_starttls() {
                    serve_starttls(sock, tls_config)
                } else {
                    serve_implicit(sock, tls_config)
                }
            }
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

    // And the transcript: nothing, or exactly the plaintext leg, and nothing
    // inside TLS. Shared with the component and tokio harnesses.
    check_refused(rec, mode.speaks_starttls(), SMOKE.ehlo)
}

/// Assert the recorded session is the one SMTP requires. The assertions
/// themselves live in the library, shared with the other harnesses.
fn check(mode: Mode, rec: &Recording) -> Result<(), String> {
    check_session(rec, mode == Mode::StartTls, &SMOKE)
}

// ---------------------------------------------------------------------------
// Responder
// ---------------------------------------------------------------------------
