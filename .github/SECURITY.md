# Security Policy

Thank you for taking the time to report a security issue in `wasm-smtp`.
Mail handling is a sensitive area; we treat security reports with the
priority they deserve.

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a suspected security
problem. Instead, send a private report to:

> nabbisen <nabbisen@scqr.net>

Please include:

- A description of the issue and the affected crate(s).
- The version, commit hash, or branch the report applies to.
- A reproducer if at all possible: minimal code, scripted server
  responses, or a `cargo test` invocation that demonstrates the issue.
- Your assessment of the impact.

We aim to acknowledge any report within seven days, and to publish a
fix or a clear timeline within thirty days of acknowledgement.

## Scope

The following are in scope for security reports:

- CRLF injection, command injection, or any other class of input
  smuggling into SMTP commands or the DATA payload.
- Mishandling of the `\r\n.\r\n` terminator (premature termination,
  failure to dot-stuff content lines).
- Disclosure of credentials, message bodies, or recipient addresses
  through error messages, panic messages, or logged output.
- Unbounded resource consumption triggered by hostile server replies
  (memory growth, CPU loops).
- TLS or transport-layer issues introduced by the core's contract with
  adapter crates.

The following are explicitly out of scope:

- Misuse of the library to send unsolicited or fraudulent mail. See
  [`TERMS_OF_USE.md`].
- Vulnerabilities in the SMTP server you submit to.
- Vulnerabilities in the runtime host (e.g. Cloudflare Workers itself).

[`TERMS_OF_USE.md`]: ../TERMS_OF_USE.md
