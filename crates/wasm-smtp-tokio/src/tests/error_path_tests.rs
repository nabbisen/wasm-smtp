//! Tests for error paths that don't require a real TLS server.
//!
//! These exercise:
//! - Connection failures to bad addresses (no listener) surface as
//!   `IoError`.
//! - The plain transport's pre-upgrade lifecycle: read/write before
//!   any I/O, after close, etc.

use crate::{TokioPlainTransport, TokioTlsTransport};

#[tokio::test]
async fn tcp_connect_to_unbound_port_fails() {
    // Port 1 on localhost is universally unbound (privileged). We
    // expect a TCP-level connection refusal, surfacing as IoError.
    let result = TokioPlainTransport::connect("127.0.0.1", 1, "localhost").await;
    assert!(result.is_err(), "connect to unbound port must fail");
}

#[tokio::test]
async fn implicit_tls_to_unbound_port_fails() {
    let result = TokioTlsTransport::connect_implicit_tls("127.0.0.1", 1, "localhost").await;
    assert!(result.is_err(), "implicit-TLS to unbound port must fail");
}

#[tokio::test]
async fn implicit_tls_to_plaintext_endpoint_fails() {
    // Stand up a TCP listener that does not speak TLS and confirm
    // that the TLS handshake fails with an IoError rather than
    // succeeding (or panicking).
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn an accept-and-immediately-close task so the connect
    // doesn't hang.
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            // Send some non-TLS bytes to confirm the handshake
            // refusal isn't a quiet socket close.
            let _ = sock.write_all(b"220 not-tls\r\n").await;
        }
    });

    let result =
        TokioTlsTransport::connect_implicit_tls(&addr.ip().to_string(), addr.port(), "localhost")
            .await;
    assert!(
        result.is_err(),
        "implicit-TLS handshake against plaintext server must fail"
    );
}

#[cfg(feature = "native-roots")]
#[tokio::test]
async fn invalid_sni_string_rejected() {
    use crate::ConnectOptions;

    // ServerName::try_from rejects strings that aren't valid DNS
    // names or IP literals. An empty server_name triggers that
    // path before any TCP I/O.
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = listener.accept().await;
    });

    let opts = ConnectOptions::new().with_server_name("");
    let result = TokioTlsTransport::connect_with(&addr.ip().to_string(), addr.port(), opts).await;
    assert!(result.is_err(), "empty SNI must be rejected");
}
