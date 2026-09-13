# RFC 022 — Direct Sockets adapter for Isolated Web Apps, and its browser credential model

**Status.** Draft
**Priority.** P3
**Tracks.** Experimental / Browser / Adapter / Security
**Touches.** `crates/wasm-smtp-direct-sockets/` (future), `docs/src/adapters/direct-sockets.md` (future)
**Supersedes.** [RFC 023](../archive/023-browser-side-secret-consent-model.md), merged here at the owner's decision on 2026-09-13, as RFC 023's own open question 3 proposed. The two were one design split across two files.
**Refreshed.** 2026-09-13. The 2025 draft rested on premises that are no longer true. Every factual statement below was checked on that date against the sources listed at the end.

## Summary

Design an experimental `Transport` adapter over the Direct Sockets API
(`TCPSocket`), for Chrome Isolated Web Apps (IWAs). The adapter would
let an app delivered through the browser submit mail with no backend
server. It also defines the credential rules any such adapter must
document. This stays a draft: it is feasible, but its audience today is
small (§Audience).

## What is known (2026-09-13)

| Fact | Source |
|---|---|
| The WICG Direct Sockets spec is still "a work in progress". It defines `TCPSocket`, `UDPSocket`, `TCPServerSocket`, and `MulticastController`, with WHATWG streams for TCP | WICG spec |
| **The spec defines no TLS**: no secure socket, no `startTLS`, no upgrade | WICG spec |
| `TCPSocket` is `[IsolatedContext]` and gated by the **`direct-sockets` permissions policy** (default allowlist `'none'`), declared in the IWA manifest. **No user activation is required** | WICG spec; Chrome IWA docs |
| Chrome's Intent to Ship was approved for Chrome 130, desktop only, Isolated Web Apps only. Android is excluded | blink-dev Intent to Ship |
| **IWAs themselves are supported only on ChromeOS**, installed through admin policy, for users and browsers from ChromeOS 128 | Chrome Enterprise help |
| Standards signals: Mozilla "Closed as Harmful"; WebKit "No signal" | blink-dev Intent to Ship |
| `web-sys` 0.3.105 has `TcpSocket`, `TcpServerSocket`, and the socket events | docs.rs |
| **`rustls` 0.23.44 with `ring` compiles for `wasm32-unknown-unknown`** on the pinned 1.88 toolchain, **only** with `ring`'s `wasm32_unknown_unknown_js` feature (randomness from the browser) and `rustls-pki-types`' `web` feature (the clock). Without them, `ring::rand::SystemRandom` and `UnixTime::now` do not exist on that target. **It is not verified that a handshake runs inside an IWA** | architect probe |

What changed from the 2025 draft:
- It assumed browser-managed TLS was "the spec direction". There is no TLS in the spec, so the only path is TLS in Rust.
- It used `navigator.openTCPSocket`. The API is `new TCPSocket(…)`.
- Its security model relied on a **user gesture** that the API does not require.
- It waited for a "Candidate Recommendation", which a WICG incubation with a harmful position from one engine is not on course to reach.

## Audience

In practice, organizations running Chrome Enterprise-managed ChromeOS
fleets that ship an IWA, from one engine, behind a policy an
administrator must set. That is the whole reason this is P3 and a
draft. It is feasible, and it is narrow.

## Goals

- A `DirectSocketsTransport` implementing `Transport` and
  `StartTlsCapable` over `TCPSocket` streams.
- TLS in Rust, over the raw stream, reusing the WASI adapter's approach:
  explicit provider, explicit root store, no process default.
- Implicit TLS (465) and STARTTLS (587). With TLS in Rust, both are the
  same code path as the WASI adapter, not a browser capability question.
- Credential rules (§Credentials) documented in the crate docs and the
  book, and as far as the adapter can, enforced by its API shape.
- Marked experimental in every place a reader meets it.

## Non-goals

- Ordinary web pages, PWAs, or any non-isolated context. The API does
  not exist there, and there is no fallback.
- A WebSocket-to-TCP proxy. That is the right design for ordinary web
  apps, and it is not this RFC.
- Credential storage of any kind, encrypted or not.
- Compatibility guarantees across Chrome versions.

## Design

### Transport

```rust
pub struct DirectSocketsTransport {
    // TCPSocket's opened streams, wrapped for async byte reads and writes.
}

impl Transport for DirectSocketsTransport { /* … */ }
impl StartTlsCapable for DirectSocketsTransport { /* … */ }
```

Opening a socket is `new TCPSocket(host, port)` and awaiting `opened`,
through `web-sys`. The transport owns the reader and writer, and releases
them before closing, so a close never races a pending read.

### TLS

`rustls` over the plaintext stream, as `wasm-smtp-wasi` does it. The
features the build needs are the two named in §What is known, set by
this crate only, so no other adapter's dependency graph changes. The
root store is supplied or bundled, as in the WASI adapter. There is no
plaintext mode: an SMTP session that cannot complete TLS fails.

### Capability errors

- The API absent (not an isolated context) → `IoError` naming the IWA
  requirement.
- `NotAllowedError` from the constructor (the `direct-sockets` policy not
  granted) → `IoError` naming the manifest `permissions_policy` key.
- A connection refused → `IoError`, as every adapter reports it.

None of them panics.

### Credentials (merged from RFC 023, corrected)

These rules are documented in the crate docs and the book. The adapter
cannot enforce what an application does with a string, so the
documentation is the mechanism, and the API shape does what it can.

1. **Credentials are call arguments only.** No adapter type stores a
   username, password, or token. This matches the core's `login` and the
   component's per-call credentials.
2. **No browser persistence.** Credentials must not be written to
   `localStorage`, `sessionStorage`, IndexedDB, Cache Storage, or
   cookies. Script in the same origin, and extensions with host
   permissions, can read those.
3. **Connections come from explicit user action.** Unlike the 2025 draft's
   premise, **the API does not require a user gesture**. The policy grants
   the capability for the app's lifetime. So this is an application rule
   the documentation must state plainly, not a browser guarantee the
   adapter can lean on.
4. **Audit without secrets.** Applications should record each send's
   time, recipient count, and outcome (RFC 012's model) and never the
   credential.
5. **TLS always.** There is no plaintext mode (§TLS).

Threat model, to document: a malicious extension with host permissions;
injected script inside the IWA despite its CSP; a compromised SMTP server
(prefer SCRAM-SHA-256, where the password never crosses the wire).

On avoiding repeated password entry: RFC 023 suggested OAuth 2.0.
A fully client-side IWA has no backend redirect endpoint, so this is an
open question, not a recommendation (§Open questions).

### Experimental disclaimer

In the crate-level rustdoc, the README, and the book chapter: the adapter
targets Chrome Isolated Web Apps on ChromeOS, is not available to
ordinary web pages, depends on an incubating API with a negative
standards position from one engine, and is not recommended for
production without a security review.

## Conditions for moving to Proposed

The 2025 conditions are replaced. Moving to Proposed needs:

1. **A demonstrated need**: at least one planned IWA deployment of
   `wasm-smtp`, from a real user.
2. **A proof of concept**: rustls completing a handshake over a
   `TCPSocket` inside an IWA, and one SMTP transaction, run once and
   recorded. The compile probe above does not count.
3. **A test strategy the gate can run**: Chrome with the isolated-context
   flag that WPT uses, or a stated reason the adapter is exempt from
   on-target testing, which would make it the only adapter without it.

## Security considerations

Browser-side SMTP is the highest-risk context this project could target.
The design refuses every weakening it could offer: no plaintext, no
stored credentials, no verification bypass (RFC 030's invariants apply to
any root-store input). The largest residual risk is outside the adapter:
same-origin script and extensions. The documentation has to say so rather
than imply the policy gate protects credentials. It does not.

## Alternatives considered

- **WebSocket proxy.** Works in every browser, and needs a server. The
  right answer for ordinary web apps; not for an offline-first IWA.
- **Using the Component Model (RFC 018/030) from JavaScript via `jco`.**
  The component imports `wasi:sockets`, which a browser does not provide.
  A shim onto `TCPSocket` could bridge that. It is worth comparing during
  the proof of concept, because it would reuse the component's interface
  and trust-anchor model without a new Rust adapter.
- **Not implementing it.** Still valid. This RFC records the design so
  that the decision, when a user appears, starts from correct facts.

## Open questions

1. Adapter or component shim (§Alternatives)? The proof of concept should
   try the cheaper one first.
2. How does an IWA avoid repeated password entry without a backend? Is
   OAuth 2.0 practical from an IWA at all?
3. Can on-target tests run in CI, given that IWAs need an isolated
   context?

## Sources (checked 2026-09-13)

- WICG Direct Sockets: <https://wicg.github.io/direct-sockets/>
- Chrome for Developers, Direct Sockets: <https://developer.chrome.com/docs/iwa/direct-sockets>
- blink-dev, Intent to Ship: Direct Sockets API: <https://groups.google.com/a/chromium.org/g/blink-dev/c/5R0P_aYBWQI>
- iwa-dev, updated feature: Direct Sockets API: <https://groups.google.com/a/chromium.org/g/iwa-dev/c/KZsueK7Q9YU>
- Chrome Enterprise help, installing IWAs: <https://support.google.com/chrome/a/answer/9367354?hl=en>
- `web-sys` 0.3.105 on docs.rs: <https://docs.rs/web-sys/0.3.105/web_sys/struct.TcpSocket.html>
