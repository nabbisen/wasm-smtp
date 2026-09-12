# RFC 024 — Release gate integrity, toolchain baseline, and MSRV correction

**Status.** Implemented (0.15.2)
**Priority.** P0
**Tracks.** Release / Governance / CI / Workspace
**Touches.** `Cargo.toml`, `rust-toolchain.toml` (new), `.github/workflows/` (new), `crates/*`, `docs/src/`, `CHANGELOG.md`, `ROADMAP.md`, `rfcs/done/001-*` (amendment note)
**Handoff.** [`../handoffs/024-release-gate-integrity/implementation-handoff.md`](../handoffs/024-release-gate-integrity/implementation-handoff.md)

## Summary

Make the project's release gate trustworthy. This RFC records the
decisions needed to turn "the tests pass" from a claim into evidence:
a correct and enforced minimum supported Rust version, a pinned
toolchain for every gate, an enumerated set of feature-combination and
cross-target checks, and a CI workflow that runs all of it on every
change. It also settles how the Component Model crate drives async
code on `wasm32-wasip2`, a gap that has prevented that crate from ever
building for its real target.

The implementation ships as the v0.15.2 patch release, subject to the
owner's release approval.

## Motivation

An independent verification on 2026-09-12 at commit `04696b1`
(workspace version 0.15.1) found that the repository does not pass its
own gate:

| Check | Result |
|---|---|
| `cargo test --workspace` | fails: component crate native test build, two core doctests, one WASI doctest |
| `cargo check -p wasm-smtp --features smtputf8` | fails: missing import in `client/send.rs` |
| `cargo check -p wasm-smtp-wasi --no-default-features --features native-roots` | fails: `rustls-native-certs` 0.8 API mismatch |
| `cargo +1.85 check -p wasm-smtp` (declared MSRV) | fails: `let` chains require Rust 1.88 |
| `cargo fmt --all -- --check` | 129 diffs, identical on 1.88, 1.91 and 1.98 |
| `wasm-smtp-component` on `wasm32-wasip2` | cannot build: references a non-existent runtime crate, an unimported type, and a `wit-bindgen` macro form that 0.57 does not accept |

A June 2026 handoff bundle reported three of these as "fixed"; the
fixes were never committed. The published 0.15.x crates therefore
advertise an MSRV they do not honor, and the release process has no
mechanism that would have caught any of this.

RFC 001 and RFC 010 both state that CI enforces crate-boundary and
security invariants. No CI has ever existed. This RFC closes that gap.

## Goals

- Declare a truthful MSRV and prove it on every change.
- Pin one toolchain so formatting, linting, building and testing are
  reproducible for every contributor and in CI.
- Define the release gate as an explicit, enumerated command list.
- Add a CI workflow that runs the gate, with a blocking MSRV job and a
  non-blocking latest-stable job.
- Get every crate building for its real target, including
  `wasm-smtp-component` on `wasm32-wasip2`.
- Ship the accumulated fixes as v0.15.2 and bring the documentation
  back in line with the code.

## Non-goals

- New protocol features or API additions.
- Property-based or fuzz testing of the parser (a later RFC).
- On-target execution of WASI or Component tests under a real runtime
  (`cargo check` for the target is the bar here; a wasmtime smoke test
  is future work).
- Release automation (publishing from CI). Releases remain manual and
  owner-approved.

## Design

### D1. Minimum supported Rust version: 1.88

`rust-version` in `[workspace.package]` becomes `1.88`. This is the
lowest version on which the workspace compiles, because the core uses
`let` chains (stable since 1.88 in edition 2024). Rewriting that code
to preserve the previously declared 1.85 was rejected: no consumer has
requested 1.85, and the rewrite would churn correct code for no user
benefit.

Raising the MSRV is a compatibility-relevant change and is recorded in
the changelog as such. It is not a breaking change under this
project's 0.x policy because the previous declaration was already
false.

### D2. Pinned toolchain

A `rust-toolchain.toml` at the workspace root pins:

```toml
[toolchain]
channel = "1.88"
components = ["rustfmt", "clippy"]
targets = ["wasm32-unknown-unknown", "wasm32-wasip2"]
```

The pin equals the MSRV so that one version answers every question:
what must compile, what rustfmt output is canonical, and which clippy
lint set is authoritative. Contributors on newer toolchains use
`cargo +stable` explicitly if they want to.

### D3. The release gate

The gate is the following command list, run on the pinned toolchain
from the workspace root. All must pass.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # lib + integration + doctests

# Feature combinations. Never use --all-features (DEC-009).
cargo check -p wasm-smtp --no-default-features
cargo check -p wasm-smtp --features smtputf8,mail-builder,tracing
cargo check -p wasm-smtp-wasi --no-default-features --features native-roots
cargo check -p wasm-smtp-wasi --features plaintext-only
cargo check -p wasm-smtp-tokio --no-default-features --features webpki-roots,ring

# Real targets.
cargo check -p wasm-smtp -p wasm-smtp-cloudflare --target wasm32-unknown-unknown
cargo check -p wasm-smtp -p wasm-smtp-wasi -p wasm-smtp-component --target wasm32-wasip2
```

Clippy runs with `-D warnings`. The manifest keeps `pedantic = warn`
so that local builds stay informative; CI denies. Each existing
pedantic warning is either fixed or suppressed with a targeted
`#[allow]` carrying a one-line justification. Blanket
`#![allow(clippy::pedantic)]` is not acceptable.

### D4. Continuous integration

`.github/workflows/ci.yml` with two jobs:

- **`gate`** (blocking): the pinned toolchain from
  `rust-toolchain.toml`, runs D3 verbatim.
- **`stable`** (non-blocking, `continue-on-error: true`): latest
  stable, runs `cargo test --workspace` and the clippy step only. Its
  purpose is early warning about upcoming breakage. Lower priority;
  may land after `gate`.

Triggers: pull requests and pushes to `main`. No secrets are used or
needed.

### D5. Driving async code in `wasm-smtp-component`

The WIT `send` export is synchronous, but `SmtpClient` is async. The
WASI transport's operations are themselves blocking (they poll WASI
pollables inline), so every future the component awaits resolves on
first poll. The component therefore drives futures with a private
no-op-waker executor identical in shape to `wasm_smtp_test::block_on`,
panicking with a clear message if a future ever returns `Pending`.
This replaces the phantom `wasm_smtp_component_rt` reference.

The binding generation uses `wit_bindgen::generate!` with the
`export!` macro form that wit-bindgen 0.57 requires. Adapting the
macro invocation to the pinned wit-bindgen version is implementation
detail; changing the WIT contract is not permitted.

### D6. `wasm-smtp-test` publication

Recommended: publish `wasm-smtp-test` to crates.io as a regular member
of the family. It is already documented as a downstream tool, it is
the reference implementation of the `Transport` contract for
third-party adapter authors, and the WASI and Component crates already
depend on it as a dev-dependency.

Conditions: pin its `wasm-smtp` dependency to the workspace version
instead of `"*"`, publish core before it, keep the "not for
production" wording, and add an amendment note to RFC 001, which
currently states the crate is not published.

**Confirmed by the owner on 2026-09-12.**

### D8. WIT contract amendment for `wasm-smtp-component`

Implementation found two defects in `wit/smtp.wit` that have kept the
component from ever building for `wasm32-wasip2`:

1. The `smtp-message` record uses the reserved WIT keyword `from` as a
   field name. Corrected to `%from`, WIT's escape for keywords used as
   identifiers. Binding generators still see a field named `from`, so
   no consumer-visible name changes. The package stays at `0.1.0`.
2. The world imports `wasi:io` and `wasi:sockets` interfaces but the
   repository carries no WIT packages to resolve them. The WASI 0.2.4
   packages `wasi:io`, `wasi:sockets`, and `wasi:clocks` are vendored
   under `wit/deps/` (0.2.4 is the version implemented by the `wasi`
   0.14 crate the WASI adapter already uses), and the world's import
   annotations move from `@0.2.0` to `@0.2.4`. The imports are mapped
   onto the `wasi` crate with a `with:` block so wit-bindgen does not
   generate a second copy of the WASI bindings; for that purpose
   `wasi` becomes a direct `wasm32`-only dependency of the component
   crate. It was already in the wasm32 dependency tree.

Narrowing the world to exports only was rejected: the import list is
the contract's statement of required host capabilities. RFC 018
receives an amendment note pointing here.

### D9. Adapters select their rustls crypto provider explicitly

When `wasm-smtp-wasi` (ring) and `wasm-smtp-tokio` (aws-lc-rs by
default) are compiled into one process, rustls has two providers
enabled and `ClientConfig::builder()` panics because it cannot choose.
`cargo test --workspace` hits this, and so would any application that
depends on both adapters. DEC-009's guard does not cover it, because
ordinary feature unification triggers it.

Decision: a library must not depend on, or install, the process-wide
default provider. Each adapter builds its `ClientConfig` with
`builder_with_provider(...)` using the provider its own features
selected (tokio: aws-lc-rs or ring per feature; WASI: ring). Test-side
`install_default()` calls are removed; the workspace test run passing
without them is the proof. No public API change.

### D10. `unsafe_code` level in `wasm-smtp-component` — **accepted by the owner on 2026-09-12**

Building the component for `wasm32-wasip2` (D5, D8) revealed that
wit-bindgen's generated canonical-ABI glue contains `unsafe` by
construction (`#[unsafe(export_name)]` shims, `unsafe fn` lift and
lower helpers). A crate that expands those macros cannot inherit the
workspace's `unsafe_code = "forbid"`, because `forbid` cannot be lifted
by any attribute inside the crate.

Implemented shape: the component crate declares its own lint tables,
identical to the workspace's except `unsafe_code = "deny"`, and exactly
two modules containing only generated code carry
`#[allow(unsafe_code)]`. Hand-written `unsafe` in that crate is still
refused; every other crate keeps `forbid`.

Security review: `.git-exclude/reviewed/024-release-gate-integrity-review-2.md` §3.
Recommendation: accept, and reword the baseline rule (RFC 010, and
DEC-002 / NF-2 in the June 2026 handoff) to "`forbid` in every crate,
except generated Component Model glue in `wasm-smtp-component`, which
is `deny` with allowances scoped to the generated modules only." The
separate-bindings-crate alternative was rejected: it relocates the same
unsafe surface without reducing it.

This is a change to a documented security-baseline rule; the owner
accepted it on 2026-09-12. RFC 010 receives the amendment note in the
release commit (handoff S11).

### D11. The WIT contract lives inside the component crate

Verifying the release candidate showed that
`cargo package -p wasm-smtp-component` produces a package with no WIT
files: the contract lived at the workspace root and the crate reached
it through `path: "../../wit"`, which cargo cannot include. Every
previously published version of the crate had the same hole; it went
unnoticed because the crate never built for its target. A consumer
building the published crate for `wasm32-wasip2` would fail at the
binding macro.

Decision: the canonical location of the contract is
`crates/wasm-smtp-component/wit/` (`smtp.wit` plus `deps/`). The
workspace-root `wit/` directory is removed rather than symlinked, so
that packaging needs no assumptions about how cargo treats symlinks
and so that the repository has one copy. All references are updated.
RFC 018's "Touches" location is amended accordingly.

The release gate (D3) gains a packaging check:

```bash
# The published component must carry its contract.
cargo package --list -p wasm-smtp-component | grep -q '^wit/smtp.wit$'
cargo package --list -p wasm-smtp-component | grep -q '^wit/deps/sockets/tcp.wit$'
```

`cargo package --list` needs a clean tree, which CI has; locally add
`--allow-dirty` when checking uncommitted work.

### D7. Release

The implementation ships as **v0.15.2**, a patch release, after the
architect's review and the owner's approval. The changelog gets an
English 0.15.2 entry, the 0.15.1 entry is translated to English per
project rules, the duplicated `[0.9.4]` headings are merged into one
section without altering their content, and the comparison links are
completed for 0.10.0 onward using the project's tag format (no `v`
prefix).

## Security considerations

Nothing in this RFC changes a trust boundary. Two points are worth
stating explicitly:

- `-D warnings` and full doctest execution reduce the chance that a
  security-relevant example or code path silently rots.
- CI must never use `--all-features`; the tokio adapter's
  `compile_error!` on conflicting crypto providers is a deliberate
  guard (DEC-009), and enumerated feature sets preserve it.
- No CI secrets, no publishing from CI.

## Simplicity and maintainability considerations

One pinned version for everything is the simplest policy that makes
the gate deterministic. The enumerated feature list in D3 is short and
lives in one place (the workflow file), which is also where a future
feature must be added.

## Alternatives considered

**Keep MSRV 1.85 and remove `let` chains.** Rejected; see D1.

**Pin `channel = "stable"` and test MSRV only in CI.** Common practice,
but it lets rustfmt and clippy drift under contributors' feet, which is
exactly what produced the 129 formatting diffs and the unaudited
pedantic warnings. Rejected for now; can be revisited when CI has
been stable for a few releases.

**Add a real async executor to the component crate.** Unnecessary
complexity: the WASI transport never yields. Rejected.

**Skip the wasm32-wasip2 check because the target must be installed.**
Rejected. The Component crate's only purpose is that target; a gate
that never compiles it is not a gate.

## Implementation plan

See the Developer Handoff linked in the header. Slices are ordered so
that each is independently reviewable: toolchain baseline → compile
and doctest fixes → component target build → format and lint →
CI → documentation sync → (conditional) test-crate publication →
release candidate.

## Acceptance criteria

- `rust-toolchain.toml` exists and pins 1.88; `rust-version = "1.88"`.
- Every command in D3 passes on the pinned toolchain, with output
  captured as evidence.
- `.github/workflows/ci.yml` exists; the `gate` job runs D3; the
  `stable` job is non-blocking.
- `wasm-smtp-component` and `wasm-smtp-wasi` pass `cargo check
  --target wasm32-wasip2`.
- Documentation no longer describes the Cloudflare adapter as planned,
  PLAIN as the preferred mechanism, or four `SmtpError` variants, and
  names the current crate set in NOTICE and CONTRIBUTING.
- CHANGELOG has a single `[0.9.4]` section and an English 0.15.2
  entry; comparison links resolve to real tags.
- RFC 001 carries an amendment note for D6; RFC 018 carries one for D8.
- `wit/smtp.wit` parses; `wit/deps/` holds the WASI 0.2.4 packages with
  a README naming the source; the component's `generate!` maps imports
  onto the `wasi` crate.
- Both adapters build their `ClientConfig` with an explicit provider and
  no test installs a process default.

## Open questions

None. D6 was confirmed by the owner on 2026-09-12; the changelog's
merged `[0.9.4]` section with its one-line note is the permanent record
of the version offset.

## Amendment log

- 2026-09-12: D6 confirmed; D8 and D9 added after review 1 of the
  implementation (see `.git-exclude/reviewed/024-release-gate-integrity-review-1.md`).
- 2026-09-12: D10 added after review 2 and accepted by the owner; the
  owner approved the v0.15.2 release on the same day (D7).
- 2026-09-12: released as 0.15.2 (tag at `8a46d22`; six crates published).
- 2026-09-12: D11 added after review 3 of the release commit: the
  component crate packaged without its WIT; the contract moves inside
  the crate and a packaging check joins the gate. Release held until
  a corrected release commit is verified.
