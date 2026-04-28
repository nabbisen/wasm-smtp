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
variants without breaking source compatibility. As of v0.2.0, the
crate uses this to add `ProtocolError::ExtensionUnavailable` and
`SessionState::StartTls` without forcing a major bump.

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
