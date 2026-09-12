# Examples

This page collects worked end-to-end examples for the use cases this
crate is designed for. Each example is a complete program in the sense
that, given a `Transport` (in production: from
`wasm-smtp-cloudflare`), it would run as written.

## Contact-form delivery

A common pattern: a Cloudflare Worker receives a form submission from
a website's "Contact us" page, then forwards the message to a fixed
mailbox.

```rust,no_run
use wasm_smtp_cloudflare::connect_smtps;
use wasm_smtp::SmtpError;

# struct ContactForm { name: String, email: String, message: String }
async fn deliver_contact_form(form: ContactForm) -> Result<(), SmtpError> {
    // Connect, greet, EHLO.
    let mut client = connect_smtps(
        "smtp.example.com",
        465,
        "worker.example.com",
    )
    .await?;

    // login() picks PLAIN when the server advertises it, falling back
    // to LOGIN. For the typical contact-form scenario, you don't care
    // which: you just want to authenticate and send.
    client
        .login("forms@example.com", &smtp_password())
        .await?;

    // Compose a fully-formed RFC 5322 message. The library does not
    // build MIME for you; for plain text this is just a few headers
    // followed by a blank line and the body.
    let body = format!(
        "From: forms@example.com\r\n\
         To: support@example.com\r\n\
         Reply-To: {sender_email}\r\n\
         Subject: Contact form: {sender_name}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         From: {sender_name} <{sender_email}>\r\n\
         \r\n\
         {body}\r\n",
        sender_name = form.name,
        sender_email = form.email,
        body = form.message,
    );

    client
        .send_mail(
            "forms@example.com",
            &["support@example.com"],
            &body,
        )
        .await?;

    client.quit().await?;
    Ok(())
}

# fn smtp_password() -> String { String::new() }
```

A few notes on the headers in this example. `Reply-To` is set to the
form submitter's address while `From` stays as the authenticated
sending mailbox — this is the well-known pattern for forwarding mail
on someone's behalf without spoofing the envelope sender, which would
violate SPF/DKIM at the receiving server. The `\r\n` line endings are
not optional: SMTP requires CRLF.

## Contact form with a bot challenge

The example above sends whatever arrives. A public form needs one more
thing first: some evidence that a human asked. Without it the endpoint is
a relay, and it will be found.

The controls that provide that evidence belong at the HTTP boundary,
before any SMTP session exists — see
[Anti-abuse at the request boundary](../concepts/security.md#anti-abuse-at-the-request-boundary)
for why they are not in the library. The complete Worker is
`crates/wasm-smtp-cloudflare/examples/contact_form_turnstile.rs`; the
shape is:

1. **Method.** Only `POST` with a form body. Anything else is `405` or
   `400`.
2. **Honeypot.** A `website` field, hidden by CSS in the page. Humans
   leave it empty; naive bots fill in everything. Non-empty means `400`,
   with no network call spent.
3. **Challenge verification.** `POST` the submitted token to the vendor's
   verification endpoint with your secret key. A refusal is `403`; an
   unreachable verifier is `503`.
4. **Send.** Only now open the SMTP session.

The middle of it, with the boilerplate elided:

```rust,no_run
# use serde::Deserialize;
# use worker::{console_log, Env, Request, Response, Result, Method};
# #[derive(Deserialize)]
# struct SiteVerify { success: bool, #[serde(rename = "error-codes", default)] error_codes: Vec<String> }
# async fn verify(_s: &str, _t: &str, _ip: &str) -> Result<SiteVerify> { unimplemented!() }
# fn field(_f: &worker::FormData, _n: &str) -> String { String::new() }
# async fn deliver(_h: &str, _p: u16, _u: &str, _pw: &str, _n: &str, _e: &str, _m: &str)
#     -> std::result::Result<(), wasm_smtp::SmtpError> { Ok(()) }
# async fn handle(mut req: Request, env: Env) -> Result<Response> {
# let Ok(form) = req.form_data().await else { return Response::error("Expected a form body", 400) };
// Honeypot: no network call, no log of what was submitted.
if !field(&form, "website").is_empty() {
    console_log!("contact form: honeypot triggered");
    return Response::error("Bad Request", 400);
}

// Challenge: the last gate before we spend a round trip on SMTP.
let token = field(&form, "cf-turnstile-response");
if token.is_empty() {
    return Response::error("Missing challenge response", 400);
}
let secret = env.secret("TURNSTILE_SECRET")?.to_string();
let remote_ip = req.headers().get("CF-Connecting-IP").ok().flatten().unwrap_or_default();

match verify(&secret, &token, &remote_ip).await {
    Ok(v) if v.success => {}
    Ok(v) => {
        // The verdict and its reason codes only. Never the token.
        console_log!("contact form: challenge rejected: {:?}", v.error_codes);
        return Response::error("Forbidden", 403);
    }
    Err(_) => {
        // Fail closed: an outage at the verifier must not open the relay.
        console_log!("contact form: challenge verification unavailable");
        return Response::error("Service Unavailable", 503);
    }
}

// Only past this point does an SMTP session exist.
let host = env.var("SMTP_HOST")?.to_string();
let user = env.secret("SMTP_USER")?.to_string();
let pass = env.secret("SMTP_PASS")?.to_string();
deliver(&host, 465, &user, &pass,
        &field(&form, "name"), &field(&form, "email"), &field(&form, "message"))
    .await
    .map_err(|e| { console_log!("contact form: send failed: {e}"); worker::Error::RustError("send failed".into()) })?;

Response::ok("Thanks — your message is on its way.")
# }
```

The handler itself carries `#[event(fetch)]`:

```rust,ignore
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    handle(req, env).await
}
```

### Configuration

| Binding | Kind | Notes |
|---|---|---|
| `TURNSTILE_SECRET` | secret | `wrangler secret put TURNSTILE_SECRET` |
| `SMTP_USER`, `SMTP_PASS` | secret | the submission mailbox |
| `SMTP_HOST`, `SMTP_PORT` | var | `wrangler.toml` `[vars]` |

The Turnstile **site key** is not in this table on purpose: it is public
and belongs in the page's HTML widget. Only the secret key goes near the
Worker, and it never appears in a log line.

Two notes on why the verification is here rather than in the library. It
needs an HTTP client, a JSON parser, and a vendor's secret — none of which
the SMTP core or its adapters have any business carrying. And keeping it
in your handler is what lets you swap Turnstile for hCaptcha, reCAPTCHA,
or your own scheme by changing one function.

## Transactional alert

A scheduled Worker emits an alert when a metric crosses a threshold.
The mailbox-of-record is fixed; there is one recipient, no user input
in the body, and the message must succeed or fail visibly.

```rust,no_run
use wasm_smtp_cloudflare::connect_smtps;
use wasm_smtp::SmtpError;

async fn emit_alert(metric: &str, value: f64, threshold: f64) -> Result<(), SmtpError> {
    let mut client =
        connect_smtps("smtp.example.com", 465, "alerts.example.com").await?;
    client
        .login("alerts@example.com", &smtp_password())
        .await?;

    let body = format!(
        "From: alerts@example.com\r\n\
         To: oncall@example.com\r\n\
         Subject: ALERT: {metric}\r\n\
         \r\n\
         Metric `{metric}` is {value}, threshold is {threshold}.\r\n",
    );

    client
        .send_mail("alerts@example.com", &["oncall@example.com"], &body)
        .await?;
    client.quit().await?;
    Ok(())
}
# fn smtp_password() -> String { String::new() }
```

This example shows the simplest happy-path code. Production callers
will want to wrap the whole sequence in a retry loop keyed on
`SmtpError::Io` and 4xx `ProtocolError::UnexpectedCode` — see
[Errors](../concepts/errors.md) for the recommended pattern.

## Multiple recipients on one connection

Sending to several recipients of the same message requires no extra
round-trips beyond one extra `RCPT TO` per address. `send_mail`'s
`to:` argument is a slice of recipients:

```rust,no_run
# use wasm_smtp::{SmtpClient, Transport, SmtpError};
# async fn run<T: Transport>(transport: T) -> Result<(), SmtpError> {
let mut client = SmtpClient::connect(transport, "client.example.com").await?;
client.send_mail(
    "newsletter@example.com",
    &[
        "alice@example.org",
        "bob@example.org",
        "carol@example.org",
    ],
    "From: newsletter@example.com\r\n\
     Subject: Weekly digest\r\n\r\nbody...\r\n",
).await?;
client.quit().await?;
# Ok(())
# }
```

The library accepts both `250` and `251 User not local; will forward`
as success on each `RCPT TO`. If any single recipient is refused with
a 5xx, the whole transaction is aborted and the connection is closed:
SMTP does not provide a way to recover an in-progress transaction
after a `RCPT TO` rejection.

## Multiple messages on one connection

Distinct messages — e.g. one alert per metric in the same Worker
invocation — share a single connection and a single login:

```rust,no_run
# use wasm_smtp_cloudflare::connect_smtps;
# use wasm_smtp::SmtpError;
# struct Alert { recipient: String, body: String }
# fn smtp_password() -> String { String::new() }
async fn drain_alert_queue(alerts: &[Alert]) -> Result<(), SmtpError> {
    let mut client =
        connect_smtps("smtp.example.com", 465, "alerts.example.com").await?;
    client.login("alerts@example.com", &smtp_password()).await?;

    for alert in alerts {
        client
            .send_mail(
                "alerts@example.com",
                &[alert.recipient.as_str()],
                &alert.body,
            )
            .await?;
    }

    client.quit().await?;
    Ok(())
}
```

After `send_mail` returns, the client is back in the `MailFrom` state
and ready for another transaction. RFC 5321 §3.3 explicitly permits
this, and many submission servers process subsequent transactions on
an open connection more efficiently than on fresh ones.

## STARTTLS submission (port 587)

For relays that listen on port 587 with the STARTTLS upgrade flow
rather than Implicit TLS on 465, swap `connect_smtps` for
`connect_smtp_starttls`. Everything else is identical.

```rust
use wasm_smtp_cloudflare::connect_smtp_starttls;
use wasm_smtp::SmtpError;

# async fn send_via_starttls() -> Result<(), SmtpError> {
let mut client = connect_smtp_starttls(
    "smtp.example.com",
    587,
    "client.example.com",
).await?;

client.login("user@example.com", "secret").await?;
client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "From: user@example.com\r\n\
     To: recipient@example.org\r\n\
     Subject: Hello over 587\r\n\
     \r\n\
     Sent with STARTTLS.\r\n",
).await?;
client.quit().await?;
# Ok(())
# }
```

`connect_smtp_starttls` performs the entire upgrade dance — plaintext
greeting, plaintext `EHLO`, `STARTTLS`, transport-level TLS upgrade,
re-`EHLO` on the secure channel — before returning. The client is
delivered in the same `Authentication` state as the Implicit-TLS
path, so the `login` and `send_mail` calls do not change.

If the server fails to advertise `STARTTLS`, the connect call
returns `SmtpError::Protocol(ProtocolError::ExtensionUnavailable {
name: "STARTTLS" })` and closes the session — there is no silent
fallback to plaintext authentication.

## OAuth 2.0 submission via Gmail

Gmail's submission service authenticates with short-lived OAuth 2.0
access tokens via the XOAUTH2 SASL profile. This example assumes
the caller has already obtained a fresh access token (token
acquisition and refresh are out of scope for this crate).

```rust
use wasm_smtp_cloudflare::connect_smtp_starttls;
use wasm_smtp::{AuthError, SmtpError};

# async fn obtain_oauth2_token() -> Result<String, Box<dyn std::error::Error>> {
#     unimplemented!("call out to your OAuth provider")
# }
# async fn send_via_gmail() -> Result<(), Box<dyn std::error::Error>> {
let access_token = obtain_oauth2_token().await?;

// Gmail's submission endpoint listens on 587 with STARTTLS.
let mut client = connect_smtp_starttls(
    "smtp.gmail.com",
    587,
    "client.example.com",
).await?;

match client.login_xoauth2("user@example.com", &access_token).await {
    Ok(()) => {}
    Err(SmtpError::Auth(AuthError::Rejected { code, message, .. })) => {
        // Token expired, scope wrong, or account doesn't allow SMTP.
        // The provider's diagnostic JSON is in `message`.
        eprintln!("XOAUTH2 rejected ({code}): {message}");
        return Err("auth failed".into());
    }
    Err(other) => return Err(other.into()),
}

client.send_mail(
    "user@example.com",
    &["recipient@example.org"],
    "From: user@example.com\r\n\
     To: recipient@example.org\r\n\
     Subject: Sent via Gmail OAuth\r\n\
     \r\n\
     Token-authenticated submission.\r\n",
).await?;
client.quit().await?;
# Ok(())
# }
```

The same pattern applies to Microsoft 365 (host
`smtp.office365.com`, port 587), with appropriate scope and tenant
configuration on the OAuth side. The crate does not differentiate
between providers — XOAUTH2 is XOAUTH2.

## Acceptable use, again

Every example here is a small-volume, transactional pattern: a message
generated by a clear application event, addressed to a specific
recipient, on behalf of a domain whose operator has consented. None of
these patterns is a marketing blast or a scrape-and-spam loop. See
[`TERMS_OF_USE.md`] at the repository root for the full statement.

[`TERMS_OF_USE.md`]: ../../TERMS_OF_USE.md
