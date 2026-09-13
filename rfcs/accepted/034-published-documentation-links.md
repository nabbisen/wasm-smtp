# RFC 034 — Links that work where the documentation is published

**Status.** Accepted (owner, 2026-09-13)
**Handoff.** [`../handoffs/034-published-documentation-links/implementation-handoff.md`](../handoffs/034-published-documentation-links/implementation-handoff.md)
**Priority.** P1
**Tracks.** Documentation / CI
**Touches.** `README.md`, `crates/wasm-smtp-cloudflare/src/lib.rs` (one rustdoc link), `docs/src/{adapters/wasi,concepts/protocol,concepts/security,core/usage}.md`, `CHANGELOG.md` (two link targets), `crates/wasm-smtp/src/{client/send.rs,client/starttls.rs,message_body.rs}` (four intra-doc links), `crates/{wasm-smtp,wasm-smtp-wasi}/Cargo.toml` (`[package.metadata.docs.rs]`), a new check under `tools/` with fixtures, `.github/workflows/ci.yml`, `.github/CONTRIBUTING.md`
**Origin.** The owner found four broken links on <https://crates.io/crates/wasm-smtp> on 2026-09-13 and suspected relative paths. The architect confirmed the cause and found the same class of defect in more places.

## Decisions at acceptance

The owner accepted the RFC on 2026-09-13 and asked for the handoff.
Neither open question was answered separately, so the architect's stated
recommendations stand as the defaults, recorded here so they can be
overridden:

1. **Fixed before 0.18.0.** The release commit is redone after the fix.
2. **docs.rs documents the core with `smtputf8`, `mail-builder`, and
   `tracing`.**

## Amendment — 2026-09-13, at acceptance

Preparing the handoff, the architect ran each premise and found the same
class of defect in two more places. They are added as D5 and D6.

- **D5.** Rustdoc reports **four unresolved intra-doc links** in the core's
  own documentation, which docs.rs renders as dead text:
  `client/send.rs:348` (`MessageBody`), `client/starttls.rs:55`
  (`InvalidInputError`), and `message_body.rs:1` and `:45`
  (`SmtpClient::send_mail_stream`). They are present at default features
  too, so docs.rs shows them today.
- **D6.** **docs.rs documents no functions at all for `wasm-smtp-wasi`**
  0.17.2. Its connect API is `cfg(target_arch = "wasm32")`, and docs.rs's
  default landing page is built for `x86_64-unknown-linux-gnu`, where those
  functions do not exist. Built locally for `wasm32-wasip2`, the crate
  documents all four with no warnings. On a host build the same crate
  produces three unresolved-link warnings, which are symptoms of this, not
  separate defects.

## Summary

Documentation is read in four places: GitHub, crates.io, docs.rs, and
the published book. Links are written for one of them and checked in
none. Make every link resolve where it is read, publish the core's
feature-gated API on docs.rs, and add an offline gate check so a broken
link cannot reach a reader again.

## Motivation

This is the third time a documentation defect of this kind has reached a reader
by hand: stale versions twice (RFC 029), and now links. RFC 029
concluded that a constant a human has to keep in sync is a check the
gate should run. Links are the same kind of constant.

Three distinct defects, each verified:

**1. crates.io resolves relative links from the crate's directory.**
`wasm-smtp`, `wasm-smtp-tokio`, and `wasm-smtp-cloudflare` publish the
repository's root `README.md` (`readme = "../../README.md"`). crates.io
rewrites a relative link to
`https://github.com/nabbisen/wasm-smtp/blob/HEAD/crates/<crate>/<link>`.
That is verified in the README crates.io serves for 0.17.2. The root
README's `LICENSE` badge, `./docs/src`, and `./TERMS_OF_USE.md` therefore
point at paths that do not exist, on all three crate pages. They work on
GitHub. The three crates with their own READMEs (`-component`, `-test`,
`-wasi`) use `../../LICENSE` and resolve correctly.

**2. docs.rs item URLs name paths that do not exist.** `wasm_smtp`'s
modules are public, so rustdoc documents each item at its **module** path
(`transport/trait.Transport.html`) and lists the crate-root re-export
without a page of its own. Links written as `wasm_smtp/trait.Transport.html`
return 404 on both `latest` and `0.17.2`; the module path returns 200.
Ten links make this mistake:

- `README.md:156`: `Transport`, used twice in the text. These are two of the owner's four.
- `crates/wasm-smtp-cloudflare/src/lib.rs:74`: `Transport`, broken on docs.rs itself.
- `docs/src/adapters/wasi.md:137`, `docs/src/concepts/security.md:120`: `Transport`.
- `docs/src/concepts/protocol.md:322–324`: `SmtpClient::login`, `login_with`, `login_xoauth2`.
- `docs/src/concepts/protocol.md:329`: `EnhancedStatus`.
- `docs/src/core/usage.md:250`: `SendOutcome`.
- `CHANGELOG.md:943–944`: `SendOutcome`, `SmtpClient::send_mail`.

**3. docs.rs does not document the core's feature-gated API.** No crate
sets `[package.metadata.docs.rs]`, so docs.rs builds default features.
`protocol::validate_address_utf8`, `send_mail_smtputf8`, and the
`mail-builder` integration are absent from docs.rs entirely.
`docs/src/concepts/protocol.md:381` links to a page that has never
existed. A reader told by the book to use SMTPUTF8 cannot find its API
documentation.

Two dead reference definitions, `[LICENSE]` and `[NOTICE]`, sit unused in
`README.md`.

## Design

### D1. The root README uses absolute links for repository files

It is published in two places whose relative bases differ, so relative
links cannot serve both. `LICENSE`, `TERMS_OF_USE.md`, and `docs/src` become
absolute GitHub URLs on the default branch. The two unused reference
definitions are removed. Crate-local READMEs keep their relative links:
their base is the same in both places, and they already resolve.

### D2. docs.rs links use the path rustdoc generates

Every `https://docs.rs/<crate>/latest/<ident>/…` link names the page
rustdoc actually produces: the module path for items in public modules.
Anchors (`#method.login`) are kept and checked too (D4).

### D3. docs.rs documents the core with its features

`crates/wasm-smtp/Cargo.toml` gains `[package.metadata.docs.rs]` with
`features = ["smtputf8", "mail-builder", "tracing"]`, the same set gate
command 5 tests. The adapters keep default features: `wasm-smtp-tokio`'s
provider and root-store features are mutually exclusive, and its defaults
are what readers use.

Marking feature-gated items with badges on docs.rs needs nightly-only
rustdoc attributes whose names have changed across releases. That is left
out until it can be verified. The feature names are already stated in each
item's documentation.

### D4. A gate check, offline

`tools/check-doc-links.sh`, or a Rust tool if the handoff argues for
one, runs against local artifacts and never the network:

1. **Published READMEs.** For each published crate, take the file its
   manifest's `readme` names. Resolve every relative link from **the
   crate's directory**, as crates.io does. Fail naming the file, line, and
   target when the target does not exist.
2. **docs.rs links.** Build `cargo doc --no-deps` for the published
   crates, with D3's features for the core. For every
   `https://docs.rs/<crate>/latest/<ident>/<path>.html[#anchor]` in the
   READMEs, `docs/src`, crate sources, and `CHANGELOG.md`, require
   `target/doc/<ident>/<path>.html` to exist and, when there is an anchor,
   to contain `id="<anchor>"`.

The architect verified the premise: a local `cargo doc` emits exactly the
paths docs.rs serves (`transport/trait.Transport.html`,
`client/struct.SmtpClient.html`, `outcome/struct.SendOutcome.html`,
`protocol/struct.EnhancedStatus.html`, and, with `smtputf8`,
`protocol/fn.validate_address_utf8.html`).

It needs fixture tests like the other two guards (RFC 032 D5), and a
demonstration that each half fails.

### D5. Rustdoc's own broken-link warnings fail the gate

Fix the four links with paths that resolve from their module, such as
`crate::MessageBody`, and add two gate commands that build
documentation with warnings denied, for the targets docs.rs uses:

```
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps -p wasm-smtp -p wasm-smtp-tokio -p wasm-smtp-cloudflare -p wasm-smtp-test -p wasm-smtp-component --features wasm-smtp/smtputf8,wasm-smtp/mail-builder,wasm-smtp/tracing
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps -p wasm-smtp-wasi --target wasm32-wasip2
```

Verified: the first fails today with exactly the four errors, and the
second passes. They also produce the `target/doc` trees D4's check reads.

### D6. docs.rs builds the WASI adapter for its target

`crates/wasm-smtp-wasi/Cargo.toml` gains `[package.metadata.docs.rs]`
with `targets = ["wasm32-wasip2"]`. docs.rs's metadata documentation
says any rustup-supported target may be used, and that the first entry
of `targets` becomes the default landing page when `default-target` is
unset. The crate's own `wasm32-wasip2` build is already in the gate.

**Verifiable only after publishing**, because docs.rs builds on publish. The
architect checks the landing page and `fn.connect_smtps.html` for
0.18.0 as part of the release. If docs.rs cannot cross-compile the
crate, that becomes a follow-up, not a revert: the metadata is correct
either way.

### Not in scope

- External links other than docs.rs. They need the network, and a gate
  that fails when a third-party site is down is not a gate.
- Links inside `rfcs/`, which are historical records.

## Release

The READMEs crates.io shows and the documentation docs.rs builds are both
**per published version**. A fix that lands after 0.18.0 is tagged
leaves 0.18.0's pages broken for good. That is the version whose
component interface breaks, and its pages will be the most read. The
architect recommends fixing this **before 0.18.0**, which means redoing
the release commit once more. The alternative is 0.18.1 immediately after.
The owner's call.

## Open questions

None. Both were settled at acceptance; see *Decisions at acceptance*.
