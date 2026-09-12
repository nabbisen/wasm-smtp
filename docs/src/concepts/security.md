# Security

This page summarises the threat model, the mitigations built into
`wasm-smtp`, and the responsibilities that remain with the caller.

## Credential handling

Credentials (passwords, Bearer tokens) are validated before being sent
on the wire and are never stored by `SmtpClient`. The `Debug`
implementation of internal types that carry credentials redacts them.

### Transport security

`wasm-smtp` does not implement TLS itself; that is the responsibility of
the [`Transport`] adapter. All four shipping adapters (Cloudflare,
Tokio, WASI, Component) establish TLS before sending any credentials:

- **Implicit TLS** (port 465): TLS before the first SMTP byte.
- **STARTTLS** (port 587): plaintext connect → `STARTTLS` command → TLS
  upgrade. `wasm-smtp` refuses to proceed if the server does not
  advertise `STARTTLS` and the adapter is in STARTTLS mode.

### Authentication mechanism selection

`SmtpClient::login()` (auto-select) prioritises in this order:

1. `SCRAM-SHA-256` — challenge-response; password never leaves the client.
2. `PLAIN` — password is base64-encoded but not encrypted; safe only
   over TLS.
3. `LOGIN` — legacy two-step; same security as PLAIN.

`XOAUTH2` and `OAUTHBEARER` are deliberately excluded from auto-selection
because their credential is an OAuth Bearer token, not a static password.
Use `login_xoauth2` / `login_oauthbearer` (or `login_with`) explicitly.

## STARTTLS injection defence

`wasm-smtp` defends against STARTTLS injection attacks (CVE-2021-3618)
by rejecting any server reply to the initial `EHLO` that contains
`250-STARTTLS` in the pre-TLS plaintext. If a line-buffering MitM
injects an early `220 Go ahead`, the subsequent `EHLO` response after
upgrade is checked to detect the attack.

## Dot-stuffing

All message bodies are passed through the RFC 5321 §4.5.2 dot-stuffer
before being written to the wire, preventing a body that begins a line
with `.` from terminating the DATA session early. The streaming
(`send_mail_stream`) path uses `DotStufferState` which preserves
correctness across chunk boundaries.

## Anti-abuse at the request boundary

An endpoint that sends mail on behalf of an anonymous visitor — a contact
form, a "share this page" button, a notification webhook — is a relay
waiting to be found. The controls that stop that are not SMTP controls,
and they do not live in this library.

### Two layers, both needed

**The request boundary.** Bot challenges (Turnstile, hCaptcha,
reCAPTCHA), rate limits, and honeypot fields. HTTP-layer and
vendor-specific. They decide whether a request may cause a send *at all*,
and they run before any SMTP session exists — which is the point, since a
session that never opens costs nothing and cannot be abused.

**The SMTP layer.** [`SendPolicy`](../core/policy-audit.md),
vendor-neutral and part of this library. It decides whether a given
envelope may go out: who may appear as the sender, how many recipients one
message may carry, how large it may be.

Neither substitutes for the other. A challenge does not stop a verified
human from submitting a thousand-recipient message; `BoundedPolicy` does
not stop a script from calling your endpoint a thousand times.

### Why the library stops at the SMTP layer

The core does no I/O and speaks only SMTP; the adapters are transport and
nothing else; `SendPolicy` is synchronous and I/O-free by design.
Verifying a challenge needs an HTTP client, a JSON parser, and a vendor's
secret. Putting that in the crate family would bind every runtime to one
vendor's endpoint and add a maintenance surface with no SMTP content in
it, so the project deliberately does not (RFC 026).

What this means in practice: the verification is a dozen lines in your
handler, and you keep the choice of vendor. See
[Contact form with a bot challenge](../reference/examples.md#contact-form-with-a-bot-challenge)
for a worked Worker.

### Rate limiting

Keep per-IP or per-sender counters in the platform's own store — Workers
KV or a Durable Object, or the platform's built-in rate-limiting rules —
and check them *before* the challenge, so that a flood of bot traffic does
not consume challenge verifications (which are themselves a metered
resource). `BoundedPolicy` bounds recipients and bytes per message; it
knows nothing about requests per minute, and cannot.

### Failing closed

Whatever you verify, decide what happens when the verifier is
unreachable. The safe answer for a mail endpoint is to refuse: an outage
at the challenge provider should stop mail, not wave it through. The
worked example returns `503` and never opens a session.

## Acceptable use

`wasm-smtp` must not be used to deliver unsolicited bulk mail, to
impersonate other senders, or to deliver mail that violates the policy
of any SMTP server. See [`TERMS_OF_USE.md`].

[`Transport`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/trait.Transport.html
[`TERMS_OF_USE.md`]: https://github.com/nabbisen/wasm-smtp/blob/main/TERMS_OF_USE.md
