//! Tests for the WASI adapter.
//!
//! All tests in this module run on native hosts using [`MockTransport`] from
//! `wasm-smtp-test`. They verify that the `Transport` contract is respected
//! and that the TLS configuration helpers work correctly. WASI-specific
//! socket integration is covered only by manual testing with a real wasmtime
//! runtime.

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use rustls;
    use wasm_smtp::SmtpClient;
    use wasm_smtp_test::{MockTransport, block_on, flatten};

    use crate::tls::ConnectOptions;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn full_exchange() -> Vec<u8> {
        flatten(&[
            b"220 mail.example.com ESMTP\r\n",
            b"250-mail.example.com\r\n250 AUTH PLAIN LOGIN\r\n",
            b"235 2.7.0 OK\r\n",
            b"250 2.1.0 OK\r\n",
            b"250 2.1.5 OK\r\n",
            b"354 Start mail input\r\n",
            b"250 2.0.0 OK: queued\r\n",
            b"221 2.0.0 Bye\r\n",
        ])
    }

    // -----------------------------------------------------------------------
    // ConnectOptions unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn connect_options_default_has_no_overrides() {
        let opts = ConnectOptions::default();
        assert!(opts.server_name.is_none());
        assert!(opts.root_store.is_none());
        assert!(opts.alpn.is_empty());
    }

    #[test]
    fn connect_options_builder_sets_server_name() {
        let opts = ConnectOptions::default()
            .with_server_name("alt.example.com");
        assert_eq!(opts.server_name.as_deref(), Some("alt.example.com"));
    }

    #[test]
    fn connect_options_builder_sets_alpn() {
        let opts = ConnectOptions::default()
            .with_alpn(&[b"smtp"]);
        assert_eq!(opts.alpn, vec![b"smtp".to_vec()]);
    }

    // -----------------------------------------------------------------------
    // MockTransport: verify the full SMTP session with wasm-smtp-test
    //
    // These tests confirm that the Transport contract works for any
    // implementation — the actual WasiTlsTransport included — since
    // SmtpClient is generic over Transport.
    // -----------------------------------------------------------------------

    #[test]
    fn mock_transport_full_session_succeeds() {
        let script = full_exchange();
        let (transport, written, _closed) = MockTransport::new(&[&script]);

        block_on(async {
            let mut client = SmtpClient::connect(transport, "client.example.com")
                .await
                .expect("connect");
            client.login("user@example.com", "pass").await.expect("login");
            client
                .send_mail(
                    "user@example.com",
                    &["to@example.com"],
                    "Subject: wasi test\r\n\r\nbody\r\n",
                )
                .await
                .expect("send_mail");
            client.quit().await.expect("quit");
        });

        let sent = String::from_utf8(written.borrow().clone()).unwrap();
        assert!(sent.contains("EHLO client.example.com\r\n"));
        assert!(sent.contains("MAIL FROM:<user@example.com>\r\n"));
        assert!(sent.contains("RCPT TO:<to@example.com>\r\n"));
        assert!(sent.contains("DATA\r\n"));
        assert!(sent.contains("QUIT\r\n"));
    }

    #[test]
    fn mock_transport_starttls_flow_succeeds() {
        use wasm_smtp_test::UpgradeBehavior;

        let pre_script = flatten(&[
            b"220 mail.example.com ESMTP\r\n",
            b"250-mail.example.com\r\n250-STARTTLS\r\n250 AUTH PLAIN\r\n",
            b"220 2.0.0 Ready to start TLS\r\n",
        ]);
        let post_script = flatten(&[
            // Post-TLS EHLO.
            b"250-mail.example.com\r\n250 AUTH PLAIN\r\n",
            b"235 2.7.0 OK\r\n",
            b"250 2.1.0 OK\r\n",
            b"250 2.1.5 OK\r\n",
            b"354 Start mail input\r\n",
            b"250 2.0.0 OK: queued\r\n",
            b"221 2.0.0 Bye\r\n",
        ]);

        let (transport, written, _closed, upgrades) = MockTransport::with_starttls(
            &[&pre_script],
            &[&post_script],
            UpgradeBehavior::Succeed,
        );

        block_on(async {
            let mut client = SmtpClient::connect_starttls(transport, "client.example.com")
                .await
                .expect("connect_starttls");
            client.login("user@example.com", "pass").await.expect("login");
            client
                .send_mail(
                    "user@example.com",
                    &["to@example.com"],
                    "Subject: starttls test\r\n\r\nbody\r\n",
                )
                .await
                .expect("send_mail");
            client.quit().await.expect("quit");
        });

        assert_eq!(*upgrades.borrow(), 1, "exactly one TLS upgrade must occur");
        let sent = String::from_utf8(written.borrow().clone()).unwrap();
        assert!(sent.contains("STARTTLS\r\n"), "STARTTLS command must be sent");
    }

    // -----------------------------------------------------------------------
    // TLS config: make_tls_config does not panic with default options
    // -----------------------------------------------------------------------

    #[test]
    fn make_tls_config_succeeds_with_default_options() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let opts = ConnectOptions::default();
        let result = crate::tls::make_tls_config(&opts);
        assert!(
            result.is_ok(),
            "default TLS config must succeed: {:?}",
            result.err()
        );
    }

    #[test]
    fn make_tls_config_accepts_alpn_override() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let opts = ConnectOptions::default().with_alpn(&[b"smtp"]);
        let config = crate::tls::make_tls_config(&opts).expect("config");
        assert_eq!(config.alpn_protocols, vec![b"smtp".to_vec()]);
    }

    #[test]
    fn server_name_helper_accepts_valid_hostname() {
        assert!(crate::tls::server_name("smtp.example.com").is_ok());
    }

    #[test]
    fn server_name_helper_rejects_empty_string() {
        // rustls rejects empty SNI.
        assert!(crate::tls::server_name("").is_err());
    }
}
