//! On-target smoke guest (RFC 025 D1).
//!
//! Built for `wasm32-wasip2` and run under wasmtime by the
//! `wasm-smtp-smoke` driver, which starts the SMTP responder this talks
//! to. It is not a library example in the usual sense: it exists so that
//! the adapter executes on a real host on every CI run.
//!
//! ```text
//! smoke <implicit|starttls> <host> <port> <ca.pem>
//! ```
//!
//! Exit code 0 on a completed session; on failure the error `Display`
//! goes to stderr and the exit code is 1.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    // The adapter's connect helpers exist only on wasm32, so there is
    // nothing to run here. A stub keeps `cargo check`/`clippy` over
    // `--all-targets` clean on the host.
    eprintln!("smoke: this example only runs on wasm32-wasip2");
    std::process::exit(2);
}

#[cfg(target_arch = "wasm32")]
fn main() {
    match run() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("smoke: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use rustls::RootCertStore;
    use rustls_pki_types::CertificateDer;
    use rustls_pki_types::pem::PemObject;
    use wasm_smtp::AuthMechanism;
    use wasm_smtp_wasi::ConnectOptions;

    let args: Vec<String> = std::env::args().collect();
    let [_, mode, host, port, ca_path] = args.as_slice() else {
        return Err("usage: smoke <implicit|starttls> <host> <port> <ca.pem>".into());
    };
    let port: u16 = port.parse()?;

    // Trust only the CA the driver generated for this run.
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(ca_path)? {
        roots.add(cert?)?;
    }
    let options = ConnectOptions::default().with_root_store(roots);

    // The adapter is synchronous underneath (RFC 024 D5): every future it
    // returns resolves on the first poll, so a no-op waker drives it.
    block_on(async {
        let mut client = match mode.as_str() {
            "implicit" => {
                wasm_smtp_wasi::connect_smtps_with(host, port, "smoke.example.com", options).await?
            }
            "starttls" => {
                wasm_smtp_wasi::connect_smtp_starttls_with(host, port, "smoke.example.com", options)
                    .await?
            }
            other => return Err(format!("unknown mode '{other}'").into()),
        };

        client
            .login_with(AuthMechanism::Plain, "smoke@example.com", "secret")
            .await?;

        // The body carries a line starting with `.` so the driver can
        // confirm dot-stuffing on the wire.
        client
            .send_mail(
                "smoke@example.com",
                &["rcpt@example.org"],
                "From: smoke@example.com\r\n\
                 To: rcpt@example.org\r\n\
                 Subject: smoke\r\n\
                 \r\n\
                 first line\r\n\
                 .leading-dot\r\n\
                 last line\r\n",
            )
            .await?;

        client.quit().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}

/// Drive a future to completion with a no-op waker.
///
/// Sound for the same reason as the component crate's copy: the WASI
/// transport polls its pollables inline, so nothing ever returns
/// `Pending`.
#[cfg(target_arch = "wasm32")]
fn block_on<F: core::future::Future>(fut: F) -> F::Output {
    use core::pin::pin;
    use core::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut fut = pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("smoke: future returned Pending; the WASI transport never yields"),
    }
}
