# Errors

`wasm-smtp-core` exposes a single top-level error type, `SmtpError`,
with four variants. The taxonomy is intentionally coarse so that the
match arms in caller code are stable.

```rust
pub enum SmtpError {
    Io(IoError),
    Protocol(ProtocolError),
    Auth(AuthError),
    InvalidInput(InvalidInputError),
}
```

## When each variant appears

| Variant         | Cause                                                                  |
| --------------- | ---------------------------------------------------------------------- |
| `Io`            | The transport failed: socket dead, TLS error, runtime cancellation.    |
| `Protocol`      | The server replied in a way SMTP does not allow, or did not reply.     |
| `Auth`          | Authentication did not succeed (rejection, no compatible mechanism).   |
| `InvalidInput`  | The caller supplied input that violates SMTP grammar (CRLF, brackets). |

The state machine sets the client to `SessionState::Closed` after any
`Io` or `Protocol` failure, and after a `Auth::UnsupportedMechanism`
failure. After such a failure, all further calls to the client return
`InvalidInput` ("session is already closed") — this prevents accidental
reuse of a poisoned connection.

## Operation context (`SmtpOp`)

Every `ProtocolError::UnexpectedCode` records which SMTP operation was
in progress when the error occurred. The `SmtpOp` enum has one variant
per user-visible step: `Greeting`, `Ehlo`, `AuthPlain`, `AuthLogin`,
`MailFrom`, `RcptTo`, `Data`, `Quit`. The Display output of an
`SmtpError::Protocol(...)` reads, for example:

```text
smtp protocol error: during MAIL FROM, expected 2xx response but
received 550: sender domain refused
```

This is the granularity an operator wants in a log line: knowing
"`MAIL FROM` was rejected with 550" is more useful than the bare
"server returned 550 for *something*". Programmatic callers can read
the field directly:

```rust
match err {
    SmtpError::Protocol(ProtocolError::UnexpectedCode { during, actual, .. })
        if during == SmtpOp::AuthPlain && actual == 535 => {
        // PLAIN-specific recovery, e.g. fall back to AUTH LOGIN.
    }
    _ => {}
}
```

The enum is `non_exhaustive`, so future SMTP extensions can add
variants without breaking source compatibility. As of v0.3.0, the
enum carries the new `ExtensionUnavailable` variant (added in
v0.2.0 alongside STARTTLS) and the `enhanced` field on
`UnexpectedCode` (added in v0.3.0 alongside ENHANCEDSTATUSCODES).

## Enhanced status codes (RFC 3463)

When the server advertises `ENHANCEDSTATUSCODES`, the crate parses
the `class.subject.detail` prefix off every reply line and exposes
it as the `enhanced` field on:

- `ProtocolError::UnexpectedCode { enhanced: Option<EnhancedStatus>, .. }`
- `AuthError::Rejected { enhanced: Option<EnhancedStatus>, .. }`

`EnhancedStatus` has three numeric fields: `class` (always 2, 4, or
5 per RFC 3463), `subject`, and `detail`. The Display
representation is `class.subject.detail`; the `to_dotted()` method
returns the same. The Display impl of `ProtocolError::UnexpectedCode`
includes the enhanced code in square brackets between the basic code
and the message:

```text
during MAIL FROM, expected 2xx response but received 550 [5.7.1]:
relay access denied
```

When the extension is not advertised, `enhanced` is always `None`
— a stray `5.7.1`-shaped substring in a reply is left unparsed,
preventing accidental misclassification.

Common enhanced codes worth handling:

| Code  | Meaning |
|-------|---------|
| 2.0.0 | Generic success |
| 4.4.x | Network / DNS issue (retryable) |
| 4.7.x | Transient policy / security (sometimes retryable elsewhere) |
| 5.1.1 | User unknown (permanent address failure) |
| 5.1.2 | Bad sender system address |
| 5.7.1 | Relay access denied |
| 5.7.8 | Authentication credentials invalid |
| 5.7.9 | Authentication mechanism too weak |

## STARTTLS-specific errors

Two `ProtocolError` variants are observable only on the STARTTLS
flow:

- `ProtocolError::ExtensionUnavailable { name: "STARTTLS" }` — the
  caller asked to upgrade with `starttls()` (or via
  `connect_starttls`), but the server's `EHLO` reply did not list
  `STARTTLS` among its capabilities. The session is moved to
  `Closed` to prevent accidental fallback to plaintext.
- `ProtocolError::UnexpectedCode { during: SmtpOp::StartTls, .. }`
  — the server rejected the `STARTTLS` command with a non-220 reply.

Transport-level upgrade failures (e.g. the TLS handshake itself
fails, or `worker::Socket::start_tls` returns an error) surface as
`SmtpError::Io`, just like any other transport failure.

## XOAUTH2-specific errors

`AuthError` is `non_exhaustive`. The `Rejected` variant carries the
final 5xx reply from the server, even when the provider used the
RFC 7628 §3.2.3 two-step error flow (334 with base64 JSON, then
final 5xx). The base64 JSON error detail is **not** decoded by the
crate; it is the caller's responsibility to extract the JSON payload
from the message field and parse it if structured detail is needed.

## Handling

A typical caller distinguishes only between transient and permanent
failures. The standard pattern is:

```rust
match client.send_mail(from, &[to], body).await {
    Ok(()) => {}
    Err(SmtpError::Io(_))                           => /* retry later */,
    Err(SmtpError::Protocol(ProtocolError::UnexpectedCode { actual, .. }))
        if (400..500).contains(&actual)             => /* retry later */,
    Err(SmtpError::Protocol(_))                     => /* permanent: log + skip */,
    Err(SmtpError::Auth(_))                         => /* fix credentials */,
    Err(SmtpError::InvalidInput(_))                 => /* programmer error */,
}
```

The reply code is preserved on `ProtocolError::UnexpectedCode` and
`AuthError::Rejected`, so callers can implement more elaborate retry
policies if they want.

## Sensitivity

The error types do not carry credentials, message bodies, or recipient
addresses, even via debug formatting:

- `IoError` carries a transport-supplied message string only.
- `ProtocolError::UnexpectedCode` carries the *server's* reply text;
  the client never includes its own input in the message.
- `AuthError::Rejected` carries the server's reply code and text; the
  username and password are never embedded.
- `InvalidInputError::new` takes `&'static str`, which makes it
  *statically impossible* to embed runtime user input in its message.

This is enforced in tests: `error_tests::invalid_input_takes_only_static_strings`
exists specifically to guard against a future refactor that loosens
the constructor signature.

## Source chain

`SmtpError` implements `std::error::Error`, including `source()`, so
that callers using `anyhow`, `eyre`, or any other error-handling crate
get the full chain when they print with `{:#}`.
