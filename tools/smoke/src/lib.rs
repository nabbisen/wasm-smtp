//! The scripted SMTP responder shared by the project's two on-target
//! harnesses.
//!
//! `tools/smoke` drives the WASI **adapter** through an example guest;
//! `tools/component-smoke` drives the **component** through a host that
//! calls its `smtp-send` export. Both need the same thing underneath: a
//! TLS-terminated SMTP server on loopback that follows a fixed script and
//! records every line it receives, so a test can assert what actually
//! crossed the wire rather than that something returned `Ok`.
//!
//! Kept here rather than copied so the two harnesses cannot drift into
//! asserting against different servers.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// Which leg of the session a recorded line arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    /// Before any TLS handshake.
    Plain,
    /// Inside TLS.
    Tls,
}

/// What the responder saw.
#[derive(Debug, Default)]
pub struct Recording {
    /// Command lines, in arrival order, tagged with the leg they arrived on.
    pub lines: Vec<(Leg, String)>,
    /// The DATA payload, verbatim, exactly as it arrived (still
    /// dot-stuffed, terminator excluded).
    pub body: String,
}

impl Recording {
    /// The command lines in arrival order, without their leg tags.
    #[must_use]
    pub fn commands(&self) -> Vec<&str> {
        self.lines.iter().map(|(_, l)| l.as_str()).collect()
    }
}

/// Serve one implicit-TLS session: TLS first, then the SMTP script.
///
/// The recording is returned even when the session fails, because a failure
/// can be the expected outcome — an untrusted certificate, say — and the
/// transcript up to that point is the evidence.
pub fn serve_implicit(sock: TcpStream, cfg: Arc<ServerConfig>) -> (Recording, Result<(), String>) {
    serve(sock, false, cfg)
}

/// Serve one STARTTLS session: a plaintext leg offering `STARTTLS`, then
/// the upgrade, then the SMTP script.
pub fn serve_starttls(sock: TcpStream, cfg: Arc<ServerConfig>) -> (Recording, Result<(), String>) {
    serve(sock, true, cfg)
}

fn serve(
    sock: TcpStream,
    starttls: bool,
    cfg: Arc<ServerConfig>,
) -> (Recording, Result<(), String>) {
    let mut rec = Recording::default();
    let result = serve_inner(sock, starttls, cfg, &mut rec);
    (rec, result)
}

fn serve_inner(
    sock: TcpStream,
    starttls: bool,
    cfg: Arc<ServerConfig>,
    rec: &mut Recording,
) -> Result<(), String> {
    sock.set_nodelay(true).ok();
    let mut sock = sock;
    if starttls {
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
    if !starttls {
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

/// Generate a throwaway self-signed certificate valid for `localhost` and
/// `127.0.0.1`. Returns `(cert PEM, key PEM)`.
///
/// # Errors
///
/// Returns the `rcgen` error as a string.
pub fn generate_cert() -> Result<(String, String), String> {
    let subject_alt_names = vec!["localhost".to_owned(), "127.0.0.1".to_owned()];
    let cert =
        rcgen::generate_simple_self_signed(subject_alt_names).map_err(|e| format!("rcgen: {e}"))?;
    Ok((cert.cert.pem(), cert.signing_key.serialize_pem()))
}

/// Build a rustls server configuration from a PEM certificate and key.
///
/// # Errors
///
/// Returns the rustls or PEM parse error as a string.
pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<Arc<ServerConfig>, String> {
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
