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

The crate implements three SASL mechanisms: `PLAIN` (RFC 4616), the
historical `LOGIN` mechanism, and `XOAUTH2` (the Google / Microsoft
OAuth 2.0 SASL profile). `PLAIN` is the default for static-password
flows because it is the IETF-standard SASL mechanism and completes
in a single round-trip; `LOGIN` is retained for older submission
servers; `XOAUTH2` is opt-in for OAuth 2.0 bearer-token flows.

### Mechanism selection

The high-level [`SmtpClient::login`] method consults the server's
`EHLO` capabilities and picks the best **static-password** mechanism:
`PLAIN` if advertised, otherwise `LOGIN`, otherwise
[`AuthError::UnsupportedMechanism`]. It deliberately does not pick
`XOAUTH2` even when advertised, because the credential semantics are
different (a short-lived bearer token rather than a static password).
Callers that want OAuth 2.0 use [`SmtpClient::login_xoauth2`]
explicitly.

Callers that need a specific mechanism — for example, to reproduce a
failure tied to one mechanism — should use [`SmtpClient::login_with`]
instead.

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
[`AuthError::Rejected { code, enhanced, message }`]. Any other
unexpected code is mapped to [`ProtocolError::UnexpectedCode`].

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

### AUTH XOAUTH2

`XOAUTH2` is the SASL profile defined by Google and adopted by
Microsoft for OAuth 2.0 access-token authentication. The wire format
is one round trip with an initial response, like `PLAIN`:

```text
C: AUTH XOAUTH2 <base64(payload)>
S: 235 <message>                # success path
```

where `payload`, before base64, is:

```text
user={user}\x01auth=Bearer {token}\x01\x01
```

The `\x01` byte (SOH) separates fields. The token is a short-lived
OAuth 2.0 bearer token; obtaining and refreshing it is the caller's
responsibility — `wasm-smtp-core` does not perform the OAuth dance.

When credentials are wrong, providers commonly use a two-step error
flow defined by RFC 7628 §3.2.3:

```text
C: AUTH XOAUTH2 <base64(payload)>
S: 334 <base64(json error detail)>
C: <empty line>
S: 535 <final error text>
```

The crate handles this transparently: on a 334 reply, it sends an
empty continuation line, reads the final 5xx, and surfaces the
result as [`AuthError::Rejected`] with the final reply text in the
message field.

The user field is validated to reject NUL, CR, LF, and SOH bytes
(any of which would corrupt the SASL framing). The token field is
validated to be ASCII printable without whitespace (RFC 6750 §2.1).

## ENHANCEDSTATUSCODES (RFC 2034 / 3463)

When the server's `EHLO` reply advertises `ENHANCEDSTATUSCODES`,
every reply line carries a structured status code in the form
`class.subject.detail` immediately after the basic three-digit code:

```text
S: 550 5.7.1 relay access denied
        ^^^^^
       enhanced
```

The crate parses the prefix into [`EnhancedStatus`] and exposes it on
both successful replies (`Reply::enhanced()`) and on errors
(`ProtocolError::UnexpectedCode { enhanced, .. }`,
`AuthError::Rejected { enhanced, .. }`). RFC 3463 specifies:

- `class` is one of 2 (success), 4 (persistent transient), or 5
  (permanent). Codes outside this set are not parsed.
- `subject` is the broad category (e.g. `1` = address, `7` =
  security/policy).
- `detail` is the specific failure within the subject.

Knowing the enhanced code lets callers distinguish failure modes that
share a basic code: `5.1.1` (user unknown) and `5.7.1` (relay
denied) both arrive with `550`, but they require very different
handling — one is a permanent address failure, the other is a policy
issue that may be retryable from a different relay.

When the server does not advertise the extension, the crate does
**not** parse the prefix even if a stray `5.7.1`-shaped string appears
in a reply: the wire format is preserved verbatim in the reply text,
and `enhanced` is `None`.

[`SmtpClient::login`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.SmtpClient.html#method.login
[`SmtpClient::login_with`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.SmtpClient.html#method.login_with
[`SmtpClient::login_xoauth2`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.SmtpClient.html#method.login_xoauth2
[`AuthError::Rejected`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.Rejected
[`AuthError::Rejected { code, enhanced, message }`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.Rejected
[`AuthError::UnsupportedMechanism`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.AuthError.html#variant.UnsupportedMechanism
[`ProtocolError::UnexpectedCode`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/error/enum.ProtocolError.html#variant.UnexpectedCode
[`EnhancedStatus`]: https://docs.rs/wasm-smtp-core/latest/wasm_smtp_core/struct.EnhancedStatus.html

## TLS models

The crate supports two TLS models at the transport layer:

### Implicit TLS

```text
client opens TCP --[TLS handshake]--> server
                ↓
              SMTP greeting (already encrypted)
              EHLO ...
              AUTH ...
              ...
```

The TLS handshake completes before any SMTP byte is exchanged. This is
the standard model on port 465. From the state machine's perspective
the byte stream is encrypted from the start; nothing in `SmtpClient`
or `SessionState` differs between Implicit TLS and a hypothetical
plaintext run.

### STARTTLS (RFC 3207)

```text
client opens TCP                                       (plaintext)
              ↓
              SMTP greeting (220)                       (plaintext)
              EHLO domain                               (plaintext)
              250-... STARTTLS ...                      (plaintext)
              ↓
              C: STARTTLS                               (plaintext)
              S: 220 ready                              (plaintext)
              ↓
              [TLS handshake]                           (handshake)
              ↓
              EHLO domain                               (encrypted)
              250-... AUTH PLAIN ...                    (encrypted)
              AUTH ...                                  (encrypted)
              ...
```

Two protocol-level details deserve attention:

1. **Re-EHLO is mandatory.** Per RFC 3207 §4.2, after the TLS
   handshake the client must re-issue `EHLO` and discard the
   pre-handshake capability list. Servers may legitimately advertise
   different extensions before and after the upgrade — most commonly,
   submission servers refuse to advertise `AUTH` until the channel is
   secure. `wasm-smtp-core` clears `client.capabilities()` on the
   transport upgrade and re-populates it from the second EHLO reply,
   so callers always observe the post-TLS capability set.

2. **No fallback to plaintext.** If the caller asked for STARTTLS and
   the server did not advertise the extension, the crate returns
   `ProtocolError::ExtensionUnavailable { name: "STARTTLS" }` and
   moves the session to `Closed` rather than continuing in cleartext.
   Likewise, a 5xx from the server in response to the `STARTTLS`
   command is reported as `ProtocolError::UnexpectedCode { during:
   SmtpOp::StartTls, .. }` and ends the session.

The TLS handshake itself, in either model, is the transport's job.
The state machine sees only an opaque byte stream and a single
upgrade signal.
