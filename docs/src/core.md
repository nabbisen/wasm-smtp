# Core crate

`wasm-smtp-core` is the part of the project that does the actual SMTP.
This document describes its public surface and the design constraints
that shaped it.

## Public surface

```rust
pub use client::SmtpClient;
pub use error::{
    AuthError, InvalidInputError, IoError, ProtocolError, SmtpError, SmtpOp,
};
pub use protocol::{AuthMechanism, EnhancedStatus};
pub use session::SessionState;
pub use transport::{StartTlsCapable, Transport};
```

These types together constitute the entire public API of the crate.
There is no separate "builder", no separate "config", no separate
"low-level" interface. The intent is that anyone reading `lib.rs` can
hold the entire surface in their head.

`EnhancedStatus` (RFC 3463) is the parsed `class.subject.detail`
code that the crate populates on replies and errors when the server
has advertised `ENHANCEDSTATUSCODES`. `AuthMechanism` enumerates the
SASL mechanisms this client knows: `Plain`, `Login`, `XOAuth2`.

## `Transport` and `StartTlsCapable`

```rust
pub trait Transport {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;
    async fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError>;
    async fn close(&mut self) -> Result<(), IoError>;
}

pub trait StartTlsCapable: Transport {
    async fn upgrade_to_tls(&mut self) -> Result<(), IoError>;
}
```

`Transport` is the single I/O contract between the core and the host
runtime. `read` returning `Ok(0)` means the peer cleanly closed the
connection; the state machine treats this as
`ProtocolError::UnexpectedClose` if a reply was still being assembled.
`write_all` must perform any short-write retries internally — the core
calls it once per command. `close` is independent of the SMTP-level
`QUIT`: `QUIT` says "I'm done with the SMTP session"; `close` says "I'm
done with the underlying socket".

`StartTlsCapable` is an opt-in extension of `Transport` for transports
that can be upgraded to TLS in-place. Implementing it unlocks
`SmtpClient::starttls()` and `SmtpClient::connect_starttls()` at
compile time. Implicit-TLS-only transports need not implement it.

## `SmtpClient`

```rust
SmtpClient::connect(transport, ehlo_domain).await?;          // greeting + EHLO
client.login(user, pass).await?;                             // optional, AUTH LOGIN
client.send_mail(from, &[to_a, to_b], body).await?;          // 0..N times
client.quit().await?;                                        // consumes self
```

`connect` reads the server greeting, validates that it is a 2xx code,
and immediately issues `EHLO`. The capability lines from the EHLO reply
are stored on the client and exposed via `capabilities()`.

`login` requires that the server advertised `AUTH LOGIN`. If it did not,
the call fails with `AuthError::UnsupportedMechanism` without sending
any bytes. Credentials are base64-encoded with the crate's own small
encoder; there is no `base64` dependency.

`send_mail` runs the full transaction: `MAIL FROM`, one `RCPT TO` per
recipient (where 250 and 251 are both treated as success), `DATA`, the
dot-stuffed body, the terminator, and the final acknowledgement reply.
After it returns, the client is in a state where another `send_mail`
may be issued: RFC 5321 §3.3 explicitly permits multiple transactions
on a single session.

`quit` consumes the client. Even if the server's `221` response is
missing or malformed, the underlying transport is closed.

## `SessionState`

Every operation that mutates the client checks `SessionState` first.
Misordered API calls are rejected as `InvalidInputError` before any
byte is sent, which converts ordering bugs in caller code into clean
errors instead of corrupted SMTP exchanges. The transition table is
declared as a single `match` in `session.rs::can_transition_to` so that
the protocol's ordering rules are visible in one place.

## Buffering and limits

The client uses an internal receive buffer that grows as needed and is
periodically compacted to keep memory bounded. The crate enforces two
defensive limits, exposed as constants in `protocol.rs`:

- `MAX_REPLY_LINE_LEN` (998 octets) — refuses oversized server lines.
- `MAX_REPLY_LINES` (128) — refuses pathologically long multi-line
  replies.

Both exist to prevent a hostile or buggy server from causing unbounded
allocation. They are considerably larger than RFC requirements, so
they should never trip on conforming servers.

## What is intentionally absent

- **No global state.** The client owns everything it needs.
- **No external dependencies.** The crate has no `[dependencies]`
  outside `core` and `std`.
- **No `unsafe`.** The workspace lints set `unsafe_code = "forbid"`.
- **No `mod.rs`.** Each module is a single `.rs` file.
- **No executor dependency.** The crate calls only `await`; choosing an
  executor is the adapter's and the application's job.
