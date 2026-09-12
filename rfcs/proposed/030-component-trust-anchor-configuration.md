# RFC 030 — Trust-anchor configuration for the Component Model interface

**Status.** Proposed
**Priority.** P2
**Tracks.** Component Model / WIT / Testing
**Touches.** `crates/wasm-smtp-component/{wit/smtp.wit,src/lib.rs}`, `tools/component-smoke/`, `docs/src/adapters/component-model.md`, `rfcs/done/018-*` (amendment note)
**Origin.** Recommended by the dev team during RFC 028 implementation; endorsed by the architect in `.git-exclude/reviewed/028-wasi-contract-alignment-review-1.md` §5.

## Summary

The Component Model interface offers no way to say which certificate
authorities to trust, so a caller submitting through a server behind a
private CA cannot use the component at all. Add an optional
trust-anchor field to `smtp-config`. As a side effect it makes a
positive end-to-end transcript through the component testable, which is
the one assertion RFC 028's host harness could not make.

## Motivation

`smtp-config` carries host, port, EHLO domain, and TLS mode. The
component builds its root store from the WASI adapter's default, the
bundled Mozilla set. Two consequences:

- **A real caller is excluded.** Private-CA submission servers are
  ordinary in corporate deployments, and the Rust API has served this
  case since 0.12.0 through `ConnectOptions::with_root_store`. The
  component, which exists so non-Rust callers get the same library, is
  the only surface where that is impossible.
- **The test can only assert a negative.** RFC 028's harness proves the
  component refuses a certificate it cannot chain and speaks no SMTP.
  It cannot prove a successful send, because a run-generated CA cannot
  reach the guest. So the component's happy path — authenticate, send,
  dot-stuff, quit — is still only exercised through the Rust adapter.

Two alternatives were considered and rejected during RFC 028: a cargo
feature on the component that swaps the root store, which puts a
security-relevant choice in the build rather than the call and cannot
vary per request; and a host that terminates TLS for the guest, which
tests a configuration nobody deploys.

## Goals

- A caller can supply PEM trust anchors per `send` call.
- Omitting them keeps today's behaviour exactly.
- The host harness asserts a full positive transcript.
- No change to any existing field, and no change to the Rust API.

## Non-goals

- Client certificates or mutual TLS.
- Certificate pinning.
- Disabling verification. There must remain no way to do that, in any
  language, consistent with every adapter.
- Changing the Rust `ConnectOptions` surface, which already does this.

## Design sketch

Details belong in the handoff; the shape is the decision.

**The field.** An optional list of PEM-encoded certificates on
`smtp-config` — `trust-anchors: option<list<string>>` or a `list<u8>`
per certificate, the handoff to choose on what binding generators
produce in TypeScript, Go, and Python. Absent or empty means the
default store, so every existing caller is unaffected.

**Semantics.** Supplied anchors **replace** the default set rather than
adding to it, matching `ConnectOptions::with_root_store` in the Rust
API. A caller wanting both must pass both, and the documentation must
say so, since the opposite assumption fails closed in a way that is
hard to diagnose.

**A rejected anchor is an input error.** PEM that does not parse is
`send-error::invalid-input`, not an I/O failure. It is the caller's
data, and the distinction is what tells them where to look.

**Version.** This adds a field to a record in `wasm-smtp:smtp@0.1.0`.
Unlike RFC 028's correction, this does change what a consumer writes,
so the WIT package version moves — the handoff determines whether WIT
record evolution permits an additive optional field without breaking
existing bindings, and if it does not, the cost of the break is the
central question for the owner rather than for the implementer.

## Security considerations

The only surface that could weaken TLS, so the boundaries are firm:
replacement rather than addition, no verification-disabling path, and
parse failure as a caller error rather than a silent fallback to the
default store — a silent fallback would turn a typo into an
unnoticed downgrade of who is trusted.

The trust anchors are public certificates, not secrets, so they do not
join the credential-handling rules; but the documentation should say
that explicitly, because a reader who sees "passed on every call" may
assume otherwise.

## Open questions

1. Does WIT permit adding an optional field to an existing record
   without breaking generated bindings? If not, what does the break
   cost, and is a `0.2.0` package version acceptable?
2. `list<string>` of PEM or `list<list<u8>>` of DER? PEM is what
   operators have; DER is what rustls wants. The handoff should decide
   on what the three binding generators make idiomatic.
3. Should the same field reach `wasm-smtp-wasi`'s own connect helpers
   for symmetry, or is `ConnectOptions` already that?
