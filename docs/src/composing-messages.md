# Composing messages

`wasm-smtp::SmtpClient::send_mail` takes a fully-formed,
CRLF-normalized message body as a `&str`. Building that string is
out of scope for this crate: SMTP transport and RFC 5322 / MIME
composition are different concerns, and the message you want to
send may already be in hand from upstream (a queue, a templating
engine, a serialized JSON payload from another service).

For the common case where you do need to build a message in-process
— a notification email, a password reset, a job-completion summary
— this chapter recommends a composition partner.

## Why no `wasm-smtp-message` sibling crate

The `wasm-smtp` family deliberately stops at the SMTP wire. There
is **no** `wasm-smtp-message` crate, and there are no plans to add
one. The reasoning:

- **`mail-builder` already does this well.** Maintained by
  [Stalwart Labs] (the team behind the open-source Stalwart Mail
  Server). RFC 5322, RFC 2045-2049 (full MIME), automatic optimal
  encoding selection per body part, no required dependencies (only
  optional `gethostname`). Apache-2.0 OR MIT.
- **A thin wrapper would add no value.** The proposed
  `TextMessage::builder()...into_smtp_body()` API is essentially
  isomorphic to `mail-builder`'s `MessageBuilder::new()...write_to_string()`.
  Wrapping it would be cosmetic.
- **A from-scratch implementation would duplicate effort.**
  RFC 5322 line folding, RFC 2047 encoded-word, header injection
  defenses, MIME boundary handling — these are spec-surface details
  where re-implementing carries real correctness and security risk.
- **The `wasm-smtp` philosophy is small dependency surfaces and
  one job per crate.** A sibling crate that mostly forwards to
  another well-maintained library would dilute that.

## Recommended: `mail-builder`

Add it as a peer dependency:

```toml
[dependencies]
wasm-smtp = "0.7"
wasm-smtp-tokio = "0.7"     # or wasm-smtp-cloudflare, depending on runtime
mail-builder = "0.4"
```

### Convenience: `SmtpClient::send_message` (with the `mail-builder` feature)

To skip the explicit `write_to_string()?` step, enable the
`mail-builder` cargo feature on `wasm-smtp` and use
`SmtpClient::send_message`:

```toml
[dependencies]
wasm-smtp = { version = "0.8", features = ["mail-builder"] }
mail-builder = "0.4"
```

```rust,ignore
use mail_builder::MessageBuilder;
# async fn run(client: &mut wasm_smtp::SmtpClient<impl wasm_smtp::Transport>) -> Result<(), Box<dyn std::error::Error>> {
let message = MessageBuilder::new()
    .from(("Notify", "notify@example.com"))
    .to(("Alice", "alice@example.org"))
    .subject("Update")
    .text_body("Hello.");

client.send_message(
    "notify@example.com",
    &["alice@example.org"],
    message,
).await?;
# Ok(())
# }
```

The feature is **off by default** because `mail-builder` is a real
dependency (small, but not nothing). Enabling it adds
`mail-builder` to your dependency graph and exposes the
`send_message` method; leaving it off means callers fall back to
the manual `write_to_string()? + send_mail` pattern shown earlier.

Note that `from` and `to` here are still the **SMTP envelope**, not
the message headers. The same Bcc rule applies: include Bcc
recipients in the envelope `to` argument, but `mail-builder`
handles stripping them from the headers in its serialization.

### Plain text notification

The 80% case:

```rust,ignore
use mail_builder::MessageBuilder;
use wasm_smtp::SmtpClient;
use wasm_smtp_tokio::TokioTlsTransport;

# async fn send_notification() -> Result<(), Box<dyn std::error::Error>> {
let body = MessageBuilder::new()
    .from(("Notifications", "notify@example.com"))
    .to(("Alice", "alice@example.org"))
    .subject("Build #4231 succeeded")
    .text_body("Your build finished cleanly. See logs at https://...")
    .write_to_string()?;

let transport = TokioTlsTransport::connect_implicit_tls(
    "smtp.example.com", 465, "smtp.example.com",
).await?;
let mut client = SmtpClient::connect(transport, "ci.example.com").await?;
client.login("notify@example.com", "secret").await?;
client.send_mail(
    "notify@example.com",
    &["alice@example.org"],
    &body,
).await?;
client.quit().await?;
# Ok(())
# }
```

`MessageBuilder::write_to_string()` returns CRLF-normalized text
ready for `send_mail`. No transformation step needed.

### HTML and plain text together (`multipart/alternative`)

```rust,ignore
let body = MessageBuilder::new()
    .from(("Notifications", "notify@example.com"))
    .to("alice@example.org")
    .subject("Weekly digest")
    .text_body("Plain-text fallback — your weekly digest is attached.")
    .html_body("<h1>Digest</h1><p>Click <a href=\"...\">here</a>.</p>")
    .write_to_string()?;
```

`mail-builder` picks `multipart/alternative` automatically when both
text and HTML are present.

### Attachments (`multipart/mixed`)

```rust,ignore
let body = MessageBuilder::new()
    .from("notify@example.com")
    .to("alice@example.org")
    .subject("Report")
    .text_body("See attached.")
    .attachment("application/pdf", "report.pdf", &pdf_bytes[..])
    .write_to_string()?;
```

### Multiple recipients

For `To`, `Cc`, `Bcc`, supply a tuple list to the builder. Note that
`Bcc` recipients **must not** appear in the message headers as sent
on the wire — `mail-builder` strips them automatically when
`write_to_string()` is called, but you still need to include them in
the SMTP envelope:

```rust,ignore
// Build the headers without Bcc:
let body = MessageBuilder::new()
    .from("notify@example.com")
    .to(vec![("Alice", "alice@example.org"),
             ("Bob",   "bob@example.org")])
    .cc(vec![("Carol", "carol@example.org")])
    .subject("Status update")
    .text_body("...")
    .write_to_string()?;

// Pass ALL recipients (including Bcc) to send_mail:
client.send_mail(
    "notify@example.com",
    &["alice@example.org", "bob@example.org",
      "carol@example.org", "dan@example.org"],  // dan is the Bcc
    &body,
).await?;
```

`send_mail`'s `to` argument is the SMTP envelope, not the message
header — every recipient (including Bcc) goes there.

## Non-ASCII subjects (RFC 2047)

`mail-builder` automatically picks the most efficient encoding when
a header contains non-ASCII bytes:

```rust,ignore
let body = MessageBuilder::new()
    .from("notify@example.com")
    .to("alice@example.org")
    .subject("ビルド成功 — Build OK")
    .text_body("...")
    .write_to_string()?;
```

The subject travels on the wire as an RFC 2047 encoded-word
(typically `=?utf-8?B?...?=` or `=?utf-8?Q?...?=`). All major MUAs
(Apple Mail, Gmail, Outlook, Thunderbird) decode this back to the
original text.

You do **not** need to enable `wasm-smtp`'s `smtputf8` feature for
this. The `smtputf8` feature is about non-ASCII characters in the
**SMTP envelope** (`MAIL FROM` / `RCPT TO` addresses, RFC 6531),
which is a separate concern from non-ASCII characters in **message
headers** (RFC 2047, 5322). If your senders and recipients all use
ASCII addresses, you do not need `smtputf8`, regardless of what
goes into the `Subject:` line.

## Dot-stuffing: not your problem

RFC 5321 §4.5.2 requires that any line beginning with `.` in the
DATA payload be transmitted as `..` so the trailing `.\r\n`
end-of-message sentinel is unambiguous.

`wasm-smtp::SmtpClient::send_mail` performs dot-stuffing internally
on the body you pass in. **`mail-builder` does not** — its output
is the unmodified message body. This is correct: dot-stuffing is
an SMTP-transport concern, not an RFC 5322 concern. Pass the
`mail-builder` output to `send_mail` and the transport layer will
handle the dot-stuffing.

## CRLF normalization

`mail-builder::MessageBuilder::write_to_string()` produces
CRLF-terminated lines throughout. `wasm-smtp::SmtpClient::send_mail`
expects CRLF-terminated lines. The two match without any
intermediate transformation.

If you build messages by hand instead of using `mail-builder`, be
careful: many text editors (and `format!` / `println!`) produce
LF-only line endings. `send_mail` will not silently convert these;
the message will be technically malformed on the wire and some
servers will reject it. Either use `mail-builder` (which always
emits CRLF) or normalize manually:

```rust,ignore
let normalized = body.replace('\n', "\r\n");
```

(Note that this naive form double-converts pre-existing CRLF. For
robust normalization, prefer a streaming approach or simply use
`mail-builder`.)

## Header injection: `mail-builder` handles it; manual builders, beware

If you build a message by string concatenation:

```rust,ignore
// DON'T do this with untrusted user input:
let body = format!(
    "From: {}\r\nSubject: {}\r\n\r\n{}\r\n",
    user_from, user_subject, user_body,
);
```

then `user_subject = "Subject\r\nBcc: attacker@example.com"` smuggles
a header. `mail-builder` does the per-field encoding to prevent this;
hand-rolled builders typically do not. Strongly prefer `mail-builder`
when any input crosses a trust boundary.

## When `mail-builder` doesn't fit

A few reasons you might still build messages yourself:

- **You're forwarding pre-formed messages.** A queue producer hands
  you a complete RFC 5322 message; you only have to verify it's
  CRLF-normalized and pass it through. No builder needed.
- **You target a sub-RFC subset.** Some text-only notification
  channels (machine-to-machine reports) only need a 4-header
  envelope and a plain body. Hand-rolling is reasonable, but apply
  the safety advice above.
- **Hard size constraints.** `mail-builder` is small (~2K SLoC, no
  required deps), but a hand-rolled minimum can be smaller still.

## Other crates worth knowing about

- **`lettre::message`**: the message builder from the `lettre`
  family. Functionally similar to `mail-builder`. Pulls in the rest
  of `lettre` (an SMTP client + connection pool) as a transitive,
  which is undesirable next to `wasm-smtp`.
- **`mail-send`**: Stalwart Labs's own SMTP client, sibling to
  `mail-builder`. Pairs the two together, but does its own
  transport plumbing — there's no path to use it with
  `wasm-smtp-tokio` or `wasm-smtp-cloudflare`. Mention here for
  completeness; pick one family or the other for transport.
- **`mail-parser`**: Stalwart Labs's parser. Useful if your
  application also needs to read MIME messages (e.g. to extract
  attachments before re-sending). Independent of composition.

[Stalwart Labs]: https://stalw.art/

## DKIM signing

Mailers that want a higher chance of landing in inboxes — rather
than spam folders — sign their outgoing messages with DKIM
(RFC 6376). The signature attests that a specific domain
authorised the message and lets receiving servers verify the
message has not been tampered with in transit.

DKIM signing is a **message-layer concern**, not an SMTP-layer
concern: the signature is a header (`DKIM-Signature: ...`)
prepended to the RFC 5322 message before submission. `wasm-smtp`
does not sign messages; the `body` argument you pass to
[`SmtpClient::send_mail`] is whatever you have already prepared,
including any `DKIM-Signature` header. This keeps the SMTP layer
focused and lets you choose any DKIM implementation.

The recommended pairing is [Stalwart Labs]'s [`mail-auth`] crate,
which is no-deps-required and supports both signing and
verification.

### Minimum signing example

```toml
[dependencies]
wasm-smtp = "0.9"
mail-builder = "0.4"
mail-auth = "0.7"
```

```rust,ignore
use mail_auth::common::crypto::{Ed25519Key, SigningKey};
use mail_auth::dkim::DkimSigner;
use mail_builder::MessageBuilder;
use wasm_smtp::SmtpClient;

# async fn run<T: wasm_smtp::Transport>(
#     mut client: SmtpClient<T>,
#     signing_key_pem: &[u8],
# ) -> Result<(), Box<dyn std::error::Error>> {
// 1. Compose the message body in the usual way.
let unsigned = MessageBuilder::new()
    .from(("Notify", "notify@example.com"))
    .to("alice@example.org")
    .subject("Hello")
    .text_body("Hi there.")
    .write_to_string()?;

// 2. Sign it. The selector ("notify._domainkey.example.com" via
//    the s= and d= attributes) and the key must already be
//    published in DNS.
let key = Ed25519Key::from_pkcs8_der(signing_key_pem)?;
let signature = DkimSigner::from_key(key)
    .domain("example.com")
    .selector("notify")
    .headers(["From", "To", "Subject", "Date", "Message-ID"])
    .sign(unsigned.as_bytes())?;

// 3. Prepend the signature header to the message body.
let signed = format!("{signature}\r\n{unsigned}");

// 4. Submit through wasm-smtp as usual.
client.send_mail(
    "notify@example.com",
    &["alice@example.org"],
    &signed,
).await?;
# Ok(())
# }
```

### What the signing layer does (and does not) protect

- ✅ **Tamper-evidence in transit**: a forwarder that rewrites
  any signed header invalidates the signature. Receiving servers
  detect this and adjust the spam score accordingly.
- ✅ **Domain accountability**: the receiving side can confirm
  that the signing domain actually permitted the message (the
  domain's DNS record holds the public key).
- ❌ **Confidentiality**: DKIM is signing, not encryption. The
  message body is still visible on the wire and at every hop.
  TLS (Implicit TLS or STARTTLS) is what keeps it confidential.
- ❌ **Sender authentication**: DKIM does not bind the
  signature to the *envelope* sender (`MAIL FROM`); SPF and
  DMARC do. A complete deliverability story usually combines
  all three. Those are also message-layer concerns and
  configured at the DNS / sending domain layer; the SMTP
  client does not need to know about them.

### Operational notes

- Generate keys with `mail-auth`'s helpers, OpenSSL, or
  `opendkim-genkey`. Ed25519 (RFC 8463) is the modern choice;
  RSA is the universal fallback.
- The `DKIM-Signature` header lists which other headers it
  signs (`h=From:To:Subject:Date:Message-ID` in the example
  above). Receivers verify exactly that list, in that order;
  do not change it after signing.
- Deploying DKIM also means publishing a TXT record at
  `<selector>._domainkey.<domain>` containing the public key.
  This is outside `wasm-smtp`'s scope but is a hard prerequisite
  for the signature to be useful.

[`mail-auth`]: https://crates.io/crates/mail-auth
