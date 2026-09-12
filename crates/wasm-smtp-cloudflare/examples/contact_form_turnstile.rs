//! Contact form with a bot challenge, in a Cloudflare Worker.
//!
//! Demonstrates where anti-abuse controls belong when you build on
//! `wasm-smtp`: at the HTTP request boundary, before any SMTP session
//! exists. The handler refuses cheaply and in order — method, honeypot,
//! then challenge verification — and opens a connection to the submission
//! server only once the request has earned it.
//!
//! Cloudflare Turnstile is the worked example because this adapter targets
//! Workers. It is one vendor's challenge, not a requirement: hCaptcha,
//! reCAPTCHA, and friends slot into step 3 unchanged — a `POST` to the
//! vendor's verification endpoint with the secret and the client's token,
//! and a boolean out.
//!
//! This is the request-boundary layer. It is not a substitute for
//! `wasm_smtp::policy::SendPolicy`, which runs inside the SMTP session and
//! bounds what a single message may do, nor for a rate limiter, which
//! bounds how often anyone may ask. See `docs/src/concepts/security.md`.
//!
//! ## Bindings this expects
//!
//! ```toml
//! # wrangler.toml
//! [vars]
//! SMTP_HOST = "smtp.example.com"
//! SMTP_PORT = "465"
//!
//! # wrangler secret put TURNSTILE_SECRET
//! # wrangler secret put SMTP_USER
//! # wrangler secret put SMTP_PASS
//! ```
//!
//! The Turnstile *site* key is public and belongs in the page's HTML
//! widget; only the secret key comes anywhere near the Worker.

use serde::Deserialize;
use wasm_smtp_cloudflare::connect_smtps;
use worker::{
    Env, Fetch, Headers, Method, Request, RequestInit, Response, Result, console_log, event,
};

/// Cloudflare's challenge verification endpoint.
const SITEVERIFY: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// The part of `siteverify`'s JSON reply this handler acts on.
#[derive(Deserialize)]
struct SiteVerify {
    success: bool,
    /// Machine-readable reasons for a refusal. Safe to log: these name the
    /// failure mode (`invalid-input-response`, `timeout-or-duplicate`, …)
    /// and never echo the token.
    #[serde(rename = "error-codes", default)]
    error_codes: Vec<String>,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    handle(req, env).await
}

/// The handler proper, separated from the macro so it reads as ordinary
/// code and can be called from a test.
pub async fn handle(mut req: Request, env: Env) -> Result<Response> {
    // 1. Method. A form post is the only thing this endpoint does.
    if req.method() != Method::Post {
        return Response::error("Method Not Allowed", 405);
    }
    let Ok(form) = req.form_data().await else {
        return Response::error("Expected a form body", 400);
    };

    // 2. Honeypot. `website` is hidden by CSS in the page; a human never
    //    fills it in and most naive bots fill in everything. This costs
    //    nothing and runs before any network call.
    if !field(&form, "website").is_empty() {
        // Note the rejection without echoing what was submitted.
        console_log!("contact form: honeypot triggered");
        return Response::error("Bad Request", 400);
    }

    // 3. Challenge. Everything past here costs a round trip, so this is
    //    the last gate before we spend one on SMTP.
    let token = field(&form, "cf-turnstile-response");
    if token.is_empty() {
        return Response::error("Missing challenge response", 400);
    }
    let secret = env.secret("TURNSTILE_SECRET")?.to_string();
    let remote_ip = req
        .headers()
        .get("CF-Connecting-IP")
        .ok()
        .flatten()
        .unwrap_or_default();

    match verify(&secret, &token, &remote_ip).await {
        Ok(v) if v.success => {}
        Ok(v) => {
            // Only the verdict and its reason codes. Never the token.
            console_log!("contact form: challenge rejected: {:?}", v.error_codes);
            return Response::error("Forbidden", 403);
        }
        Err(_) => {
            // Fail closed. If we cannot tell whether the caller is a bot,
            // we do not send. An open failure mode here would turn every
            // Turnstile outage into an open relay.
            console_log!("contact form: challenge verification unavailable");
            return Response::error("Service Unavailable", 503);
        }
    }

    // 5. Only now does an SMTP session exist.
    let host = env.var("SMTP_HOST")?.to_string();
    let port: u16 = env.var("SMTP_PORT")?.to_string().parse().unwrap_or(465);
    let user = env.secret("SMTP_USER")?.to_string();
    let pass = env.secret("SMTP_PASS")?.to_string();

    let name = field(&form, "name");
    let email = field(&form, "email");
    let message = field(&form, "message");

    // 4. Header safety. `name` and `email` land in the header block, which
    //    this handler builds as text. A value containing CR or LF would
    //    inject headers of the submitter's choosing — `Bcc:` being the
    //    useful one — and turn this endpoint into the relay the steps above
    //    exist to prevent. `send_mail` validates the *envelope* addresses
    //    and refuses CR/LF there, but the header block is caller-supplied
    //    and passes through untouched, so the check belongs here.
    if has_line_break(&name) || has_line_break(&email) {
        console_log!("contact form: rejected a field containing a line break");
        return Response::error("Bad Request", 400);
    }
    if wasm_smtp::protocol::validate_address(&email).is_err() {
        console_log!("contact form: rejected an invalid submitter address");
        return Response::error("Bad Request", 400);
    }

    match deliver(&host, port, &user, &pass, &name, &email, &message).await {
        Ok(()) => Response::ok("Thanks — your message is on its way."),
        Err(e) => {
            // The Display text carries the SMTP step and reply code and no
            // credentials or body content (RFC 010).
            console_log!("contact form: send failed: {e}");
            Response::error("Bad Gateway", 502)
        }
    }
}

/// `POST` the token to Turnstile and read the verdict.
async fn verify(secret: &str, token: &str, remote_ip: &str) -> Result<SiteVerify> {
    let body = format!(
        "secret={}&response={}&remoteip={}",
        urlencode(secret),
        urlencode(token),
        urlencode(remote_ip)
    );

    let headers = Headers::new();
    headers.set("Content-Type", "application/x-www-form-urlencoded")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(body.into()));

    let request = Request::new_with_init(SITEVERIFY, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    response.json::<SiteVerify>().await
}

/// Open the session, authenticate, send one message, quit.
async fn deliver(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    name: &str,
    email: &str,
    message: &str,
) -> std::result::Result<(), wasm_smtp::SmtpError> {
    let mut client = connect_smtps(host, port, "worker.example.com").await?;
    client.login(user, pass).await?;

    // `From` is the authenticated mailbox; the submitter goes in
    // `Reply-To`. Putting an unverified address in `From` is how a contact
    // form becomes a spoofing tool, and it fails SPF besides.
    // CR/LF is legitimate inside a body, but SMTP wants CRLF specifically
    // and the library's dot-stuffing keys off line starts. Browsers
    // normally send CRLF from a textarea; normalizing means not relying on
    // that.
    let message = normalize_crlf(message);

    let body = format!(
        "From: {user}\r\n\
         To: support@example.com\r\n\
         Reply-To: {email}\r\n\
         Subject: Contact form: {name}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         From: {name} <{email}>\r\n\
         \r\n\
         {message}\r\n"
    );

    client
        .send_mail(user, &["support@example.com"], &body)
        .await?;
    client.quit().await
}

/// Whether a value carries a line break, and so cannot go into a header.
fn has_line_break(value: &str) -> bool {
    value.contains('\r') || value.contains('\n')
}

/// Normalize any mix of CR, LF, and CRLF to the CRLF that SMTP requires.
fn normalize_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                // Consume a following LF so CRLF does not become CRCRLF.
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            other => out.push(other),
        }
    }
    out
}

/// Read a form field as a string, treating a file upload or a missing
/// field as empty.
fn field(form: &worker::FormData, name: &str) -> String {
    match form.get(name) {
        Some(worker::FormEntry::Field(value)) => value,
        _ => String::new(),
    }
}

/// Percent-encode a value for an `application/x-www-form-urlencoded` body.
///
/// Hand-rolled to keep the example free of dependencies that the adapter
/// itself does not need; a real Worker would reach for `form_urlencoded`.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

// Examples are built as binaries on the host, where the `#[event]` export
// is inert. A real Worker crate is a `cdylib` and has no `main`.
fn main() {}

#[cfg(test)]
mod tests {
    use super::{has_line_break, normalize_crlf};

    #[test]
    fn line_breaks_in_header_fields_are_detected() {
        // The injection this guard exists to stop: a submitted name that
        // continues into a header of the submitter's choosing.
        assert!(has_line_break("Mallory\r\nBcc: victim@example.org"));
        assert!(has_line_break("Mallory\nBcc: victim@example.org"));
        assert!(has_line_break("Mallory\r"));
        assert!(!has_line_break("Mallory"));
        assert!(!has_line_break(""));
    }

    #[test]
    fn injected_address_is_rejected_by_the_validator_too() {
        // Belt and braces: even if the line-break check were removed, the
        // address validator refuses the same payload.
        assert!(wasm_smtp::protocol::validate_address("a@e.com\r\nBcc: v@e.org").is_err());
        assert!(wasm_smtp::protocol::validate_address("a@e.com").is_ok());
    }

    #[test]
    fn bodies_are_normalized_to_crlf() {
        assert_eq!(normalize_crlf("a\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\rb"), "a\r\nb");
        // Already-CRLF input must not double up.
        assert_eq!(normalize_crlf("a\r\nb"), "a\r\nb");
        assert_eq!(normalize_crlf("a\r\n\r\nb"), "a\r\n\r\nb");
        assert_eq!(normalize_crlf("plain"), "plain");
    }
}
