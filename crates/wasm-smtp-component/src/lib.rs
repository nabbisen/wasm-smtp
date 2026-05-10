//! # wasm-smtp-component
//!
//! WASM Component Model interface for [`wasm-smtp`].
//!
//! This crate exports the [`smtp-send`][`crate`] WIT interface defined in
//! `wit/smtp.wit`, allowing any language with WIT bindings to send email
//! through `wasm-smtp` without writing Rust.
//!
//! ## Building
//!
//! ```sh
//! # Requires cargo-component and the wasm32-wasip2 target.
//! cargo component build --target wasm32-wasip2 -p wasm-smtp-component
//! # Output: target/wasm32-wasip2/debug/wasm_smtp_component.wasm
//! ```
//!
//! ## Running tests on native
//!
//! ```sh
//! cargo test -p wasm-smtp-component
//! ```
//!
//! ## Credential security
//!
//! Credentials cross the host-component boundary as plain WIT strings on
//! every `send` call. They are not retained by the component between calls.
//! See `docs/src/adapters/component-model.md` for the full threat model.
//!
//! [`wasm-smtp`]: https://docs.rs/wasm-smtp

// ── WIT bindings (wasm32 only) ────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "smtp-client",
        // Disable the default std export so the component doesn't pull in
        // the standard library's panic handler unconditionally.
        exports: {
            "wasm-smtp:smtp/smtp-send": super::SmtpSendImpl,
        },
    });
}

// ── Type aliases (shared between wasm32 and native stubs) ─────────────────

#[cfg(target_arch = "wasm32")]
use bindings::exports::wasm_smtp::smtp::smtp_send::{
    Guest, SendError, SendResult, SmtpConfig, SmtpCredentials, SmtpMessage, TlsMode,
};

/// Native-only stubs so the crate compiles and tests on non-WASM hosts.
#[cfg(not(target_arch = "wasm32"))]
mod stubs;

#[cfg(not(target_arch = "wasm32"))]
use stubs::{SendError, SendResult, SmtpConfig, SmtpCredentials, SmtpMessage};

// ── Core implementation ───────────────────────────────────────────────────

/// The struct that implements the `smtp-send` WIT interface.
pub struct SmtpSendImpl;

impl SmtpSendImpl {
    /// Core send logic, shared between the WIT export and native tests.
    #[allow(unused_variables)]
    pub fn send_impl(
        config: SmtpConfig,
        credentials: SmtpCredentials,
        message: SmtpMessage,
    ) -> Result<SendResult, SendError> {
        use wasm_smtp::SmtpError;

        // Build the transport and SmtpClient using wasm-smtp-wasi.
        #[cfg(target_arch = "wasm32")]
        let client_result = {
            let opts = ConnectOptions::default();
            let fut = match config.tls_mode {
                TlsMode::Implicit => wasm_smtp_wasi::connect_smtps(
                    &config.host,
                    config.port,
                    &config.ehlo_domain,
                ),
                TlsMode::Starttls => wasm_smtp_wasi::connect_smtp_starttls(
                    &config.host,
                    config.port,
                    &config.ehlo_domain,
                ),
            };
            // Drive the async future synchronously via WASI blocking poll.
            // On wasm32-wasip2, the executor is provided by the runtime.
            wasm_smtp_component_rt::block_on(fut)
        };

        #[cfg(not(target_arch = "wasm32"))]
        let client_result: Result<_, SmtpError> =
            Err(SmtpError::Io(wasm_smtp::IoError::new(
                "wasm-smtp-component only runs on wasm32-wasip2; \
                 use cargo test for native unit tests",
            )));

        let mut _client = client_result.map_err(smtp_error_to_wit)?;

        // Authenticate.
        #[cfg(target_arch = "wasm32")]
        wasm_smtp_component_rt::block_on(
            client.login(&credentials.username, &credentials.password),
        )
        .map_err(smtp_error_to_wit)?;

        // Send the message.
        let to_refs: Vec<&str> = message.to.iter().map(String::as_str).collect();

        #[cfg(target_arch = "wasm32")]
        let outcome = wasm_smtp_component_rt::block_on(client.send_mail(
            &message.from,
            &to_refs,
            &message.raw_message,
        ))
        .map_err(smtp_error_to_wit)?;

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(unreachable_code)]
        let _outcome: wasm_smtp::SendOutcome = {
            let _ = (&message.from, &to_refs, &message.raw_message);
            return Err(SendError::Io("unreachable on native host".into()));
            unreachable!()
        };

        // QUIT (best-effort; ignore errors to not mask a successful send).
        #[cfg(target_arch = "wasm32")]
        { wasm_smtp_component_rt::block_on(client.quit()).ok(); }

        #[cfg(target_arch = "wasm32")]
        return Ok(SendResult { reply_code: outcome.code });
        #[cfg(not(target_arch = "wasm32"))]
        Ok(SendResult { reply_code: _outcome.code })
    }
}

// ── WIT Guest impl (wasm32 only) ──────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
impl Guest for SmtpSendImpl {
    fn send(
        config: SmtpConfig,
        credentials: SmtpCredentials,
        message: SmtpMessage,
    ) -> Result<SendResult, SendError> {
        Self::send_impl(config, credentials, message)
    }
}

// ── Error mapping ─────────────────────────────────────────────────────────

fn smtp_error_to_wit(e: wasm_smtp::SmtpError) -> SendError {
    match e {
        wasm_smtp::SmtpError::Io(io) => SendError::Io(io.to_string()),
        wasm_smtp::SmtpError::Protocol(p) => SendError::Protocol(p.to_string()),
        wasm_smtp::SmtpError::Auth(_) => SendError::AuthRejected,
        wasm_smtp::SmtpError::InvalidInput(i) => SendError::InvalidInput(i.to_string()),
        wasm_smtp::SmtpError::Policy(p) => SendError::PolicyRejected(p.to_string()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_error_io_variant_carries_message() {
        let e = SendError::Io("connection refused".into());
        match e {
            SendError::Io(msg) => assert_eq!(msg, "connection refused"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn send_error_auth_rejected_has_no_payload() {
        let e = SendError::AuthRejected;
        assert!(matches!(e, SendError::AuthRejected));
    }

    #[test]
    fn smtp_config_fields_accessible() {
        let cfg = SmtpConfig {
            host: "smtp.example.com".into(),
            port: 465,
            ehlo_domain: "client.example.com".into(),
            tls_mode: TlsMode::Implicit,
        };
        assert_eq!(cfg.host, "smtp.example.com");
        assert_eq!(cfg.port, 465);
    }

    #[test]
    fn smtp_message_to_is_vec() {
        let msg = SmtpMessage {
            from: "a@example.com".into(),
            to: vec!["b@example.com".into(), "c@example.com".into()],
            raw_message: "Subject: hi\r\n\r\nbody\r\n".into(),
        };
        assert_eq!(msg.to.len(), 2);
    }

    #[test]
    fn credentials_fields_accessible() {
        let cred = SmtpCredentials {
            username: "user@example.com".into(),
            password: "secret".into(),
        };
        assert_eq!(cred.username, "user@example.com");
        // password is accessible (it's a plain struct field) but must never
        // appear in any log, error, or debug output.
        assert!(!format!("{cred:?}").contains("secret"),
                "credentials must not appear in Debug output");
    }
}
