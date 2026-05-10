# RFC 010 — Security baseline and secret leakage prevention

**Status.** Implemented (v0.5.0)
**Priority.** P0
**Tracks.** Security
**Touches.** All crates, `TERMS_OF_USE.md`, `.github/SECURITY.md`, `docs/src/security.md`

## Summary

Define the security baseline for `wasm-smtp`: what the project commits
to preventing, the concrete rules that enforce those commitments, and
the disclosure policy for vulnerabilities.

## Motivation

`wasm-smtp` is an SMTP client library. It handles credentials
(username, password, OAuth tokens), message content, and recipient
lists. All of these are sensitive. A library that leaks any of them
through `Debug` output, error messages, or log calls fails its users
even if the SMTP protocol implementation is correct.

The project also ships as open-source software usable by anyone. A
small number of misuse vectors (spam, phishing) must be addressed in
the terms of use and, where possible, in the design.

## Goals

- No credential may appear in any `Debug` output, `Display` output,
  error message, panic message, or tracing event.
- Message bodies may not appear in error messages or tracing events
  at the `info` level or below.
- Recipient lists are not logged at any level by default.
- `unsafe_code = "forbid"` is a workspace-wide Clippy lint.
- Dependency versions are floored to known-patched releases; RUSTSEC
  advisories are tracked.
- The `TERMS_OF_USE.md` explicitly prohibits spam and abusive use.
- A `SECURITY.md` defines the vulnerability disclosure path.

## Non-goals

- Server-side SMTP hardening.
- SPF / DKIM / DMARC configuration instructions.
- Spam filter implementation.
- Rate limiting (see RFC 011).

## Design

### Credential non-leakage rules

1. **`Debug` impls:** any type that holds a credential field must
   implement `Debug` manually (or with a helper) and redact the field.
   Example: a hypothetical `Credentials { username, password }` struct
   would print `Credentials { username: "user@example.com", password: [REDACTED] }`.
2. **Error variants:** `AuthError` variants carry no credential data
   (see RFC 009). `IoError` messages are author-supplied strings, not
   derived from credentials.
3. **`tracing` events:** the `tracing` feature emits events at the
   `DEBUG` level for SMTP command names (e.g., `AUTH LOGIN`, `MAIL FROM`)
   but never emits the parameters (username, password, from address,
   to address).
4. **Panic messages:** no `unwrap()` or `expect()` call uses a format
   string that could include a credential. `clippy::expect_used` and
   `clippy::unwrap_used` are allowed but audited; in production paths
   they are replaced by `?`-propagated errors.

### Message body rules

Message bodies are passed as `&str` to `send_mail`. They are:

- Not stored in `SmtpClient`.
- Not included in any `tracing` event.
- Not included in any error message, even on DATA failure.

On DATA failure, the error includes the server response code and the
`SmtpOp::Data` tag; the body is not recorded.

### `unsafe_code = "forbid"`

Declared in `[workspace.lints.rust]`. This prevents any crate in the
workspace from using `unsafe` blocks, including adapter crates. All
unsafe operations are pushed to transitive dependencies (tokio, rustls)
that have their own audits.

### Dependency policy

- Workspace-level version floors are maintained for every dependency
  that has had a RUSTSEC advisory.
- `cargo deny` is run in CI to check for known advisories and
  license compatibility.
- New dependencies require justification in the PR; transitive
  dependency counts are monitored.

### `TERMS_OF_USE.md`

The project terms explicitly prohibit:

- Sending unsolicited bulk email (spam).
- Sending email that impersonates another sender without authorisation.
- Evading anti-spam measures.
- Using the library in violation of applicable law or the policies of
  the SMTP server being used.

These prohibitions do not have runtime enforcement in the core (see
RFC 011 for the policy hook), but they establish the project's legal
and ethical posture.

### `SECURITY.md`

Located at `.github/SECURITY.md`. Defines:

- Supported versions (current stable release).
- Private vulnerability disclosure path (email to the maintainer).
- Expected response SLA (acknowledgement within 5 business days).
- No public disclosure before a coordinated fix is available.

## Security considerations

This RFC is itself a security document. Its requirements create the
foundation that other RFCs build on. Future RFCs that relax any of
these rules (e.g., "add a debug mode that logs message bodies") require
an explicit security review and must update this RFC.

## Simplicity and maintainability considerations

The credential non-leakage rules are enforced by code review and CI
(lint checks), not by runtime guards. This is appropriate because the
cost of a false negative (credential leakage) is higher than the cost
of a false positive (blocked PR).

## Alternatives considered

**Runtime guards that zero memory after use:** desirable in principle
but complex in Rust's ownership model. The current approach (never
store credentials beyond the scope of the auth call) achieves the same
effect without explicit zeroing.

**Separate `SecretString` type:** could enforce the redaction rules
automatically. Deferred because the current manual implementation is
small and the additional dependency would need justification.

## Implementation plan

*Security baseline established as part of Phase 9 (v0.5.0).*
*`unsafe_code = "forbid"` has been in place since the workspace was created.*

## Acceptance criteria

- `format!("{:?}", err)` for any `SmtpError` value contains no
  password, token, or message body content.
- `unsafe_code = "forbid"` is present in `[workspace.lints.rust]` and
  CI fails if removed.
- `TERMS_OF_USE.md` explicitly names spam and impersonation as prohibited.
- `SECURITY.md` exists and defines the disclosure path.
- `cargo deny check` passes in CI.

## Open questions

None. Implemented and stable.
