# Protocol

This page describes the slice of SMTP that `wasm-smtp-core` actually
implements. The goal is not to restate the RFCs; it is to document the
specific choices the crate makes.

## Reference

- RFC 5321 — Simple Mail Transfer Protocol.
- RFC 5322 — Internet Message Format (callers are responsible for
  building bodies that conform).
- RFC 4954 — SMTP Service Extension for Authentication.
- RFC 4648 — Base16, Base32, and Base64 encodings.

## Reply parsing

A reply is one or more lines, each formatted as:

```text
NNN<sep><text>
```

where `NNN` is a three-digit code, `<sep>` is `' '` (final line) or
`'-'` (continuation), and `<text>` is arbitrary text up to CRLF.

`parse_reply_line` decodes one line. `SmtpClient::read_reply` accumulates
lines until a final-separator line is seen. RFC 5321 requires every
line of a multi-line reply to share the same code; if a server breaks
this rule, the crate raises `ProtocolError::InconsistentMultiline`.

Code-only lines (three digits with no separator) are accepted as final
lines with empty text. Any separator other than `' '` or `'-'` is
rejected as `ProtocolError::Malformed`.

## Command formatting

Commands are written as `VERB[ ARG]\r\n`. Two helpers in `protocol.rs`
do this directly (`format_command`, `format_command_arg`); two more
write the slightly nonstandard `MAIL FROM:<addr>` and `RCPT TO:<addr>`
forms (`format_mail_from`, `format_rcpt_to`). All four return owned
byte vectors.

The validators in `protocol.rs` reject CRLF, NUL, angle brackets, and
whitespace in addresses, and reject anything but printable ASCII in
the EHLO domain. These checks happen *before* anything is written to
the transport, which is what allows `InvalidInputError` to be raised
without disrupting the SMTP session.

## State machine

The states tracked by the client are:

```text
Greeting → Ehlo → Authentication → MailFrom ⇄ RcptTo → Data → MailFrom ...
                                                                    ↓
                                                                   Quit → Closed
```

- `Ehlo → Authentication` is automatic after a successful `EHLO`.
- `Authentication → MailFrom` happens either after a successful `login`
  or directly when the caller skips authentication.
- `MailFrom → MailFrom` is allowed because RFC 5321 §3.3 permits
  multiple transactions on one session.
- Any active state may transition to `Quit` (and then `Closed`).
- A fatal error transitions the client to `Closed` directly.

## DATA and dot-stuffing

The DATA payload is constructed by `dot_stuff_and_terminate(body)`:

1. Any `.` that occurs at the start of a line in the body is doubled,
   so that the literal `.` is preserved on the wire.
2. The body is guaranteed to end with `\r\n` (a CRLF is appended if it
   does not already end with one).
3. The end-of-data marker `.\r\n` is appended.

The line-start tracking treats only CRLF as a line terminator. The
crate assumes the body has been CRLF-normalized before it is passed in.
Lone LF or lone CR bytes inside the body are passed through verbatim:
the SMTP server is then free to accept or reject the message according
to its own policy.

## Authentication

The crate implements two SASL mechanisms: `PLAIN` (RFC 4616) and the
historical `LOGIN` mechanism. `PLAIN` is preferred because it is the
IETF-standard SASL mechanism and completes in a single round-trip.
`LOGIN` is retained because many older submission servers still
advertise only it.

### Mechanism selection

The high-level [`SmtpClient::login`] method consults the server's
`EHLO` capabilities and picks the best supported mechanism: `PLAIN`
if advertised, otherwise `LOGIN`, otherwise
[`AuthError::UnsupportedMechanism`]. Callers that need a specific
mechanism — for example, to reproduce a failure tied to one mechanism
— should use [`SmtpClient::login_with`] instead.

### AUTH PLAIN (RFC 4616)

The crate uses the **initial-response** form (RFC 4954 §4), which is
one round trip:

```text
C: AUTH PLAIN <base64(authzid \0 authcid \0 password)>
S: 235 <message>
```

The authorization identity is empty (the client does not act on behalf
of a third party); the on-wire payload is therefore
`\0 user \0 password` base64-encoded. RFC 4616 mandates UTF-8 for the
authcid and password fields, which matches Rust's `String` invariant.

A 5xx response at this step is mapped to
[`AuthError::Rejected { code, message }`]. Any other unexpected code
is mapped to [`ProtocolError::UnexpectedCode`].

### AUTH LOGIN

`AUTH LOGIN` is two round trips:

```text
C: AUTH LOGIN
S: 334 <base64 prompt>          # typically VXNlcm5hbWU6 = "Username:"
C: <base64(user)>
S: 334 <base64 prompt>          # typically UGFzc3dvcmQ6 = "Password:"
C: <base64(pass)>
S: 235 <message>
```

The crate does not parse the server's prompt; it expects a 334 and
sends the next credential. Most server implementations treat the
prompt content as decorative.

A 5xx response at any AUTH step is mapped to
[`AuthError::Rejected`]. Any other unexpected code is mapped to
[`ProtocolError::UnexpectedCode`].

[`SmtpClient::login`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.SmtpClient.html#method.login
[`SmtpClient::login_with`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.SmtpClient.html#method.login_with
[`AuthError::Rejected`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.Rejected
[`AuthError::Rejected { code, message }`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.Rejected
[`AuthError::UnsupportedMechanism`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.UnsupportedMechanism
[`ProtocolError::UnexpectedCode`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.ProtocolError.html#variant.UnexpectedCode

## Implicit TLS

The crate operates exclusively on a byte stream that is *already*
secure. The TLS handshake is the transport's job. The Implicit TLS
contract — port 465, immediately TLS, immediately SMTP-on-TLS — keeps
the state machine free of mid-session protocol switches. STARTTLS is
out of scope for the initial release; it can be added later without
disrupting the current state diagram.
