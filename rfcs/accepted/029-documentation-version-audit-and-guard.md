# RFC 029 — Documentation version audit, and a guard so it stops recurring

**Status.** Accepted
**Priority.** P1
**Tracks.** Docs / CI
**Touches.** `README.md`, `docs/src/adapters/{tokio,wasi}.md`, `docs/src/concepts/protocol.md`, `docs/src/core/{composing-messages,usage}.md`, `TERMS_OF_USE.md`, `.github/workflows/ci.yml`, `.github/CONTRIBUTING.md`, `tools/` (one check script)
**Handoff.** [`../handoffs/029-documentation-version-audit/implementation-handoff.md`](../handoffs/029-documentation-version-audit/implementation-handoff.md)
**Origin.** The owner found a stale version in the README's Cargo-features example for the **second** time, and asked for an audit of all documentation, examples, and comments.

## Summary

Every "how to depend on this" snippet in the documentation carries a
hard-coded version, and seventeen of them are stale — spanning 0.4
through 0.16 against a current 0.17.0. Correct them, then remove the
class: state dependencies as `cargo add` invocations, which carry no
version and cannot rot, and add a gate check that fails on any
remaining own-crate version string that disagrees with the workspace.

## Motivation

The owner reported a stale version in the README at 0.16.1. It was
corrected. The same strings are stale again at 0.17.0, and the owner
reported it again. The recurrence is the finding: a hand-corrected
constant that no check enforces will rot at the next release, and the
first correction only touched the two lines that were pointed at.

### What the audit found

Full sweep of `README.md`, `docs/src/**`, every crate `README.md`,
every `//!` and `///` comment in `crates/*/src` and `tools/*/src`, the
policy files, and `NOTICE`.

**Stale: 17 own-crate version strings in 6 files**

| File | Lines | Says | Should be |
|---|---|---|---|
| `README.md` | 119, 126 | `0.16` | 0.17 |
| `docs/src/adapters/tokio.md` | 65, 66, 69, 72, 75 | `0.8` | 0.17 |
| `docs/src/adapters/wasi.md` | 11, 18 | `0.15` | 0.17 |
| `docs/src/concepts/protocol.md` | 375 | `0.4` | 0.17 |
| `docs/src/core/composing-messages.md` | 43, 44, 56, 316 | `0.7`, `0.8`, `0.9` | 0.17 |
| `docs/src/core/usage.md` | 279, 281, 376 | `0.4`, `0.15` | 0.17 |

**Stale: 3 third-party version strings.** `mail-builder = "0.4"` in
`composing-messages.md` at lines 45, 57, 317. The workspace moved that
dependency to 0.5 in 0.17.0 (RFC 027), so the chapter now tells readers
to pair a version we no longer build against.

**Stale: one framing sentence.** `TERMS_OF_USE.md` opens by describing
the library as for "constrained runtimes (initially Cloudflare
Workers)". Four adapters ship, two of them not WASM-constrained at all.

**Audited and clean**, stated so the scope is on record: no version
strings in any rustdoc comment; no surviving reference to the old
`wasm-smtp-core` name or the `crates/core/` and `crates/cloudflare/`
paths; the `wit/smtp.wit` references are all correctly crate-relative
after the RFC 024 D11 move; MSRV reads 1.88 everywhere it appears;
`NOTICE` lists all six crates; the audit-event chapter matches the
emitted set after RFC 025 D4; the pipelining chapter's "before 0.16.0
only `send_mail` pipelined" is correct history, not a stale claim.

## Goals

- No stale version string anywhere in documentation or comments.
- A reader copying any dependency instruction gets a working one.
- A stale own-crate version cannot reach `main` again unnoticed.

## Non-goals

- Versioning the book per release. One live book from `main`.
- Policing third-party versions we do not depend on (`mail-auth` in the
  DKIM section stays advisory prose).
- A release by itself; see D5.

## Design

### D1. State dependencies as commands, not constants

Replace each "add this to Cargo.toml" block with the `cargo add` form,
which names no version:

```sh
cargo add wasm-smtp-wasi
cargo add wasm-smtp --no-default-features
cargo add wasm-smtp --features smtputf8
cargo add wasm-smtp-tokio --no-default-features --features webpki-roots,ring
```

This is the modern idiom, it is shorter than the TOML it replaces, and
it is immune to the defect. Where a TOML block genuinely reads better —
the tokio chapter's side-by-side comparison of four feature
configurations is the one real case — keep TOML and let D2 guard it.

### D2. Guard the remainder

A check script, run in the `gate` job and listed in CONTRIBUTING:
read `major.minor` from `[workspace.package] version`; scan `README.md`,
`docs/src/**`, and `crates/*/README.md` for `wasm-smtp*` dependency
version strings; fail, naming file, line, found and expected, on any
mismatch. Extend it to `mail-builder`, compared against the workspace
manifest rather than a literal, since that one is our dependency and
the chapter's advice depends on it.

The check must be able to fail. The handoff requires demonstrating that
by perturbing one string and showing a red run, the way RFC 025's
negative smoke mode was demonstrated.

### D3. The sweep

Correct all 20 strings, convert what D1 covers, and reword the
`TERMS_OF_USE.md` opening to describe the library as it is: SMTP from
WebAssembly and other constrained runtimes, with adapters for Workers,
WASI, tokio, and the Component Model.

### D4. Why a guard rather than more care

Two hand-corrections have already failed. The gate is the project's
only mechanism that has reliably held a documentation property — the
same argument RFC 024 made for the release gate and RFC 025 for the
on-target test. A constant that must match a release is exactly the
kind of thing a machine should check.

### D5. Release

Ships as **0.17.1**, a patch, authorized by the owner on 2026-09-13.
Documentation changes nothing in the crates, but crates.io renders the
README attached to each published version, so the version a reader sees
there stays wrong until a release carries the fix. Same trade as the
0.16.1 decision. Release approval at release time; tag, push, and
publish are the owner's and the architect's.

## Security considerations

None. No behaviour, dependency, or credential path changes; the
`mail-builder` correction aligns advice with what we already build.

## Simplicity and maintainability considerations

The sweep removes text rather than adding it: `cargo add` lines are
shorter than the TOML blocks they replace. The guard is one script and
one gate line.

## Alternatives considered

**Correct the strings and rely on care.** Tried twice; failed twice.

**Drop versions from prose without a guard.** Better, but the tokio
chapter's comparison block is worth keeping in TOML, and any future
author may reintroduce one. The guard costs little and covers both.

**Generate the snippets from the manifest at book-build time.** mdBook
preprocessors could do it. Rejected as more machinery than a 20-string
problem justifies.

## Implementation plan

Two slices plus a release commit; see the handoff. Guard first, so it
fails and proves it can, then the sweep turns it green, then 0.17.1.

## Acceptance criteria

- The guard exists, runs in the gate, and was demonstrated failing.
- No own-crate or `mail-builder` version string in documentation
  disagrees with the manifest.
- Every dependency instruction a reader can copy is either a `cargo
  add` command or a guarded TOML block.
- `mdbook build docs` clean; full gate green.

## Open questions

None. The owner authorized the 0.17.1 patch on 2026-09-13.

## Amendment log

- 2026-09-13: accepted by the owner, who opened 0.17.1 as the release
  for it (D5).
- 2026-09-13, after review 1: approved at `d13a212`. D2 confirmed as
  major.minor after the guard was seen to behave; the handoff's §S4.1
  expectation that a patch bump would invalidate the documentation was
  the architect's error and contradicted D2. The `tokio.md` TOML block
  is retained deliberately: it is the only content the guard exercises
  for own-crate versions, so converting it would leave the check
  permanently green with nothing to compare.
  **Follow-up, with the RFC 025 smoke-driver item:** the guard needs a
  self-test (its `mail-builder` branch has no live coverage) and its
  header should name its deliberate exclusions, `CHANGELOG.md` and
  `rfcs/`, so an exclusion cannot be mistaken for an oversight.
