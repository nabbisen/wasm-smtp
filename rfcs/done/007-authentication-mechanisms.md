# RFC 007 — Authentication mechanisms

**Status.** Implemented (v0.9.0)
**Priority.** P1
**Tracks.** Core auth / Security
**Touches.** `crates/wasm-smtp/src/session.rs`, `crates/wasm-smtp/src/scram.rs`, `crates/wasm-smtp/src/error.rs`

## Summary

Define the SMTP authentication mechanisms supported by `wasm-smtp`:
AUTH LOGIN, AUTH PLAIN, AUTH SCRAM-SHA-256, and XOAUTH2. Specify the
selection logic, the credential handling rules, and the error surface.

## Motivation

SMTP submission requires authentication against a real mail server.
Multiple AUTH mechanisms exist with different security properties:
LOGIN and PLAIN transmit credentials in base64 (they are only safe
over TLS); SCRAM-SHA-256 never transmits the password; XOAUTH2 uses a
bearer token. Supporting all four lets `wasm-smtp` work with the full
range of real-world SMTP submission servers without forcing the caller
to implement the mechanism themselves.

## Goals

- Implement AUTH LOGIN: two-step challenge-response with base64-encoded
  username and password.
- Implement AUTH PLAIN: single-step with a null-separated base64
  credential bundle.
- Implement AUTH SCRAM-SHA-256: RFC 5802 / RFC 7677 challenge-response
  with PBKDF2 key derivation. Password never crosses the wire.
- Implement XOAUTH2: single step with a formatted bearer token string.
- Auto-select the best available mechanism: SCRAM-SHA-256 > PLAIN > LOGIN.
- Allow explicit mechanism selection via `login_with`.
- Map server failures (535, wrong code) to `AuthError`.
- Never include credentials in `Debug` output, error messages, or logs.

## Non-goals

- OAuth 2.0 token acquisition (the caller supplies a ready token).
- SASL negotiation beyond the four mechanisms above.
- `SCRAM-SHA-256-PLUS` (channel binding; requires TLS binding tokens).
- `SASLprep` normalisation for non-ASCII credentials.
- Credential storage or key management.

## Design

### Mechanism selection

`login(username, password)` queries the EHLO capability list for
`AUTH` and selects:

1. SCRAM-SHA-256 if advertised (most secure).
2. PLAIN if advertised (simpler than LOGIN; equivalent security over TLS).
3. LOGIN as last resort.
4. `AuthError::UnsupportedMechanism` if none of the above are offered,
   with a message listing what the server advertised and what this
   client supports.

### AUTH LOGIN sequence

```
C: AUTH LOGIN
S: 334 VXNlcm5hbWU6     (base64 "Username:")
C: <base64(username)>
S: 334 UGFzc3dvcmQ6     (base64 "Password:")
C: <base64(password)>
S: 235 2.7.0 Authentication successful
```

### AUTH PLAIN sequence

```
C: AUTH PLAIN <base64("\0username\0password")>
S: 235 2.7.0 Authentication successful
```

### AUTH SCRAM-SHA-256

RFC 5802 / RFC 7677 four-message exchange:

1. `AUTH SCRAM-SHA-256 <base64(client-first-message)>`
2. Server: `334 <base64(server-first-message)>` — contains nonce, salt,
   iteration count.
3. `<base64(client-final-message)>` — HMAC proof derived from the
   password, PBKDF2, and the server challenge.
4. Server: `235 2.7.0 <base64(server-final-message)>` — server
   signature for mutual authentication.

PBKDF2-HMAC-SHA-256 key derivation with iteration count clamped to
`[4096, 1_000_000]` to resist trivial DoS. Nonce prefix check ensures
the server-first nonce starts with the client nonce (replay defence).
Server signature verification uses constant-time comparison (`subtle`).

The implementation lives in `crates/wasm-smtp/src/scram.rs` and is
gated behind the `scram-sha-256` cargo feature (enabled by default).

### XOAUTH2

```
C: AUTH XOAUTH2 <base64("user=<email>\x01auth=Bearer <token>\x01\x01")>
S: 235 2.7.0 Authentication successful
```

The caller supplies the bearer token. `wasm-smtp` formats the SASL
string and base64-encodes it. Gated behind the `xoauth2` cargo feature
(enabled by default).

### Credential security

- Credentials are `&str` arguments passed for the duration of the
  authentication step. They are not stored in the `SmtpClient` struct.
- No credential appears in `Debug` output of any type.
- `AuthError` variants carry no credential data.
- SCRAM: the client proof and server signature are computed in a local
  scope and dropped after the exchange.

## Security considerations

- LOGIN and PLAIN are safe only over TLS. Both the Cloudflare and tokio
  adapters establish TLS before authentication.
- SCRAM-SHA-256 provides mutual authentication: the client verifies the
  server's signature, detecting a MITM that accepted the client's
  credentials without completing the handshake.
- Iteration count clamping prevents a malicious server from forcing an
  expensive PBKDF2 computation.
- Nonce validation prevents replay attacks.

## Simplicity and maintainability considerations

Each mechanism is implemented as a self-contained function in the
session module (or `scram.rs`), not as a trait hierarchy. This keeps
the control flow readable and avoids dynamic dispatch.

## Alternatives considered

**Runtime-loadable SASL plugins:** rejected. The fixed set of four
mechanisms covers > 99% of real-world SMTP submission servers. Dynamic
loading would add substantial complexity for minimal gain.

**Always preferring PLAIN over SCRAM:** rejected. SCRAM provides
password-never-on-wire security; defaulting to it when available is the
correct security posture even though both are safe over TLS.

## Implementation plan

*AUTH LOGIN implemented v0.2.x, AUTH PLAIN v0.4.x, XOAUTH2 v0.6.x, SCRAM-SHA-256 v0.9.0.*

## Acceptance criteria

- `login()` selects SCRAM-SHA-256 when advertised.
- `login_with(AuthMechanism::Plain, ...)` sends AUTH PLAIN regardless of
  preference.
- `AuthError::UnsupportedMechanism` lists both what the server offered
  and what this client understands.
- SCRAM passes the RFC 7677 §3 official test vector.
- No credential appears in any error message or `Debug` output.

## Open questions

None. Implemented and stable.
