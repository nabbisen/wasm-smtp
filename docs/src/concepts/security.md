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

## Acceptable use

`wasm-smtp` must not be used to deliver unsolicited bulk mail, to
impersonate other senders, or to deliver mail that violates the policy
of any SMTP server. See [`TERMS_OF_USE.md`].

[`Transport`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/trait.Transport.html
[`TERMS_OF_USE.md`]: https://github.com/nabbisen/wasm-smtp/blob/main/TERMS_OF_USE.md
