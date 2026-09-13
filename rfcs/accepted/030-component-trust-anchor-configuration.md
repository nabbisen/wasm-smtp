# RFC 030 — Trust-anchor configuration for the Component Model interface

**Status.** Accepted (owner, 2026-09-13)
**Priority.** P2
**Tracks.** Component Model / WIT / Security / Testing
**Touches.** `crates/wasm-smtp-component/{wit/smtp.wit,src/**,Cargo.toml}`, `tools/component-smoke/`, `docs/src/adapters/component-model.md`, `CHANGELOG.md`, `rfcs/done/018-*` (amendment note)
**Handoff.** [`../handoffs/030-component-trust-anchor-configuration/implementation-handoff.md`](../handoffs/030-component-trust-anchor-configuration/implementation-handoff.md)
**Origin.** Recommended by the dev team during RFC 028 implementation; endorsed by the architect in `.git-exclude/reviewed/028-wasi-contract-alignment-review-1.md` §5.
**Target release.** 0.18.0, with the WIT package at `wasm-smtp:smtp@0.2.0`.

## Amendment — 2026-09-13, after review 1

**A1. What a PEM boundary line is.** As accepted, D3 said text outside
PEM blocks is ignored, without defining a boundary line. The
implementation followed that wording faithfully and treated any line
that was not an exact boundary as text. An architect probe then showed
that an indented private key was skipped rather than rejected, and that an
indented second certificate was dropped while `create` succeeded:
invariants 3 and 4 did not hold. It failed closed, since the trust set
only shrank, but it was silent. The grammar is now in D3 (the paragraph
headed "Boundary lines"). It was checked against a real system CA
bundle, whose 242 boundary lines are all exact, and it is enforced by a
test for each probed input. The gap was in this RFC, not in the
implementation.

## Owner decisions at acceptance

1. **Accepted.**
2. **The interface break is accepted.** The WIT package moves to `0.2.0`
   and the crates to 0.18.0.
3. **The standard is "finally clean, safe and secure, robust and
   sophisticated".** The owner asked whether the security rules were
   options. They are not: they are the invariants below. Because a break
   is being paid for anyway, the design was also revised at acceptance
   so that it is the last break this part of the interface needs.

## Summary

The Component Model interface has no way to say which certificate
authorities to trust, so a caller whose submission server sits behind a
private CA cannot use the component at all. Replace the `smtp-config`
record with a validated, immutable `smtp-config` **resource** that
carries an explicit trust-anchor choice. Validation happens once, when
the configuration is created, so a bad certificate is reported where it
was supplied and `send` can never fail because of it. As a side effect,
the component's success path becomes testable end to end, which RFC 028
could not do.

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
  component refuses a certificate it cannot chain. It cannot prove a
  successful send, because a run-generated CA cannot reach the guest.

Two alternatives were rejected during RFC 028: a cargo feature that
swaps the root store, which puts a security choice in the build instead
of the call; and a host that terminates TLS for the guest, which tests a
configuration nobody deploys.

## Goals

- A caller can state exactly which certificate authorities to trust.
- The default is the bundled set, stated explicitly, never implied by an
  empty value.
- Invalid trust input is rejected at configuration time, completely, and
  without echoing its content.
- The next connection option does not require another interface break.
- The host harness asserts a full positive transcript in both TLS modes.
- No change to the Rust API.

## Non-goals

- Client certificates or mutual TLS.
- Certificate pinning.
- Disabling verification. There is no way to do that, in any language,
  consistent with every adapter.
- Adding options beyond trust anchors. The design makes later additions
  non-breaking; it does not add them.
- `wasm-smtp-wasi`'s connect helpers: `ConnectOptions` already serves
  Rust callers.

## Design

### D1. `smtp-config` becomes an immutable resource

```wit
resource smtp-config {
    create: static func(
        host: string,
        port: u16,
        ehlo-domain: string,
        tls-mode: tls-mode,
        trust: trust-anchors,
    ) -> result<smtp-config, send-error>;
}

send: func(
    config: borrow<smtp-config>,
    credentials: smtp-credentials,
    message: smtp-message,
) -> result<send-result, send-error>;
```

**Why a resource.** A WIT record's fields are its type: adding one
breaks every generated binding, optional or not. A resource's methods
can be added in a compatible minor version of the package, so a future
option — a timeout, say — arrives as an addition, not as `0.3.0`. The
break is paid once, now.

**Why `create` and not a constructor.** Creation is fallible, and a
static function returning `result` is the form every binding generator
supports. A constructor that cannot report an error would force
validation back into `send`.

**Why immutable.** A configuration is validated once and then only
read. There are no setters, so a configuration that passed validation
cannot later become invalid, and one configuration can be shared by
concurrent sends in hosts that allow it.

**Credentials stay per call.** `smtp-credentials` remains an argument to
`send` and is still not retained. Putting secrets in a long-lived
resource would change the credential threat model in
`docs/src/adapters/component-model.md`, and that is not this RFC's to
change.

**Validation moves with the configuration.** Every check the component
already makes on host, port, and EHLO domain runs in `create`. No new
rules are invented for those fields.

### D2. The trust choice is explicit

```wit
variant trust-anchors {
    /// The bundled Mozilla root set. The right choice for public
    /// submission servers.
    bundled,
    /// Exactly these certificate authorities, and no others.
    custom(string),
}
```

`custom` carries PEM text: what operators have, one file, often a
bundle. There is no empty-means-default value to misread, and no
"bundled plus custom" case: widening trust is something a caller does
knowingly, by putting the public roots they want in the bundle.

### D3. Strict parsing of `custom` — the security invariants

These are fixed. An implementation that relaxes any of them does not
meet this RFC.

1. **Replacement, never merge.** `custom` anchors replace the bundled
   set.
2. **No fallback.** If `custom` is rejected, `create` fails. It never
   falls back to `bundled`: a silent fallback would turn a typo into an
   unnoticed change in who is trusted.
3. **No partial acceptance.** Every certificate block must be accepted
   into the root store, or `create` fails. An API that skips
   certificates it cannot parse is prohibited, because it turns "trust
   these three" into "trust whichever of these three happened to parse".
4. **Only certificates.** Text outside PEM blocks is ignored, since real
   bundles carry comments. Any block whose label is not `CERTIFICATE`
   fails `create`: a private key pasted by mistake is rejected, not
   skipped.
5. **Never echo the input.** Errors name the block's ordinal and the
   class of problem — `block 2: not a certificate`, `block 3: rejected
   by the root store` — and never contain PEM text, labels copied from
   the input, or decoded bytes. The input might be a private key.
6. **At least one certificate.** `custom` with no certificate blocks
   fails; a trust store that trusts nothing is a misconfiguration, and
   saying so at `create` beats failing every handshake.
7. **Bounded.** At most **64** certificates and **256 KiB** of text.
   Input beyond either fails before parsing. These are far above any
   real private-CA bundle and far below a size worth worrying about.
8. **No verification bypass.** There is no variant, flag, or value that
   disables certificate or hostname verification.

**Boundary lines (amendment A1).** A line containing `-----BEGIN` or
`-----END` anywhere is a boundary line, and it is never skipped as text.
Outside a block it must be exactly `-----BEGIN <label>-----`; inside a
certificate block, the only boundary allowed is exactly
`-----END CERTIFICATE-----`. Anything else — indented, with trailing
text, mid-line, a stray END, or an END for another label — fails
`create` with its own fixed reason. Lines are compared after removing
trailing whitespace, `\r` included.

All rejections are `send-error::invalid-input`: the data is the caller's,
and that tells them where to look.

### D4. Semantics that must be documented

- `custom` replaces the bundled roots. A caller who wants public roots
  as well must include them.
- Trust anchors are public certificates, not secrets, so they are not
  covered by the credential-handling rules. The documentation says so,
  because a reader who sees them held in a resource may assume
  otherwise.
- The migration from `0.1.0`: before and after, in the pseudocode style
  `smtp.wit` already uses.

### D5. Testing

Through the component, under the host harness:

- **Positive transcripts**, implicit and STARTTLS, with `custom` holding
  the run's CA. Same assertions as the adapter smoke test.
- **Refusal:** `bundled` against the run's certificate is still refused
  before any SMTP command.
- **Every D3 rule** has a test that fails `create`, and the tests for
  rules 4 and 5 use a real PEM private-key block and assert that no part
  of it appears in the error.

### D6. Version

- WIT package: `wasm-smtp:smtp@0.2.0`.
- Crates: 0.18.0, a minor release, because a published interface
  breaks. The Rust crates' APIs do not change.
- RFC 018 gets an amendment note naming the new shape.

## Security considerations

This is the only part of the interface that can change who is trusted,
so D3 is written as invariants rather than guidance. The two failure
modes it rules out are the ones that would go unnoticed in production: a
silent fallback to the default store, and silent skipping of a
certificate that did not parse. Errors are written on the assumption
that the input is a secret, because sometimes it will be one by mistake.

Validating in `create` rather than `send` also keeps an attacker-
influenced message path — `send` handles caller message content — apart
from the trust decision, which is made once, before any connection.

## Open questions

None remaining. The questions as proposed were settled at acceptance:

1. *Does WIT allow an additive optional field?* No: record fields are
   the type. Resolved by D1, which moves to a resource and accepts the
   break once, as the owner decided.
2. *PEM or DER?* PEM text in one string (D2).
3. *The same field on `wasm-smtp-wasi`'s helpers?* No (Non-goals).

One claim in D1 is **unverified**: that a resource with a static
`create` and a `borrow` parameter generates idiomatic bindings with every
generator the documentation names, and that adding a method in a later
minor version does not break a host built against `0.2.0`. The
handoff's first slice proves both before anything is implemented; if a
documented generator cannot handle it, work stops and the owner is
asked.
