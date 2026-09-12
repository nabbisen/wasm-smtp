# RFC 028 — WASI contract version alignment and component execution

**Status.** Accepted
**Priority.** P1
**Tracks.** Component Model / WASI / Testing / CI
**Touches.** `crates/wasm-smtp-component/wit/`, `crates/wasm-smtp-component/src/lib.rs`, `crates/wasm-smtp-component/Cargo.toml`, `tools/smoke/` or a new `tools/component-smoke/`, `.github/workflows/ci.yml`, `rfcs/done/018-*`, `rfcs/done/024-*` (amendment notes)
**Handoff.** [`../handoffs/028-wasi-contract-alignment/implementation-handoff.md`](../handoffs/028-wasi-contract-alignment/implementation-handoff.md)
**Origin.** Escalated by the dev team during RFC 027 implementation; decided by the architect in `.git-exclude/reviewed/027-docs-publication-dependency-currency-review-1.md` §2.

## Summary

The Component Model crate declares its WASI imports at version 0.2.4
while the crate that actually provides those imports resolves, in any
current lockfile, to an implementation declaring 0.2.12. Fix the
mismatch, then make it impossible for it to recur silently, then close
the evidence gap that let it go unnoticed for three releases by
executing the component under a host that calls its export.

## Motivation

`crates/wasm-smtp-component/wit/` vendors WASI 0.2.4 and the world's
imports are annotated `@0.2.4` (RFC 024 D8). The import *bindings* are
not generated from those files: the `with:` map delegates them to the
`wasi` crate, which declares `wasip2 = "1.0.1"` as a caret range. Any
fresh lockfile resolves wasip2 1.0.4 or later, whose wasm import
modules are named `wasi:sockets/tcp@0.2.12` and siblings.

So the declared contract and the linked imports disagree, and **no
lockfile of ours can fix that for a consumer** — they resolve their own.
Every published version of this crate since 0.14.0 has had the defect
for anyone whose lockfile postdates wasip2 1.0.2.

It has never been observed because the component has never run. It is
compile-checked only; the RFC 025 smoke test exercises
`wasm-smtp-wasi`, not the component. Componentization, which is where a
declared-versus-actual import mismatch would surface, has never been
performed in this repository: `cargo component` is not installed.

Two consequences beyond the version number: a host reading the
component's type is told 0.2.4 while the guest was built against 0.2.12
bindings, and `wit/deps/README.md` justifies the 0.2.4 choice with a
claim that is no longer true (corrected in 0.17.0 to point here).

## Goals

- The declared WASI version and the linked imports agree.
- A gate check fails when they stop agreeing, naming both versions.
- The component is built as a component and its `send` export is
  invoked by a host in CI, against the scripted responder RFC 025
  already provides.
- RFC 018 and RFC 024 D8 record the amended contract.

## Non-goals

- Changing the `wasm-smtp:smtp@0.1.0` package version or any type in
  the `smtp-send` interface. The WASI import versions are the only
  contract change.
- Supporting several WASI minors at once.
- Replacing the RFC 025 smoke harness; extend it or sit beside it.

## Design sketch

Three parts, in dependency order. Details belong in the handoff.

**D1. Align the annotations.** Re-vendor `wit/deps/` at the WASI minor
the `wasi` crate's resolved `wasip2` implements, and move the world's
`@0.2.x` annotations and the `with:` map to match. Pinning `wasip2` in
our lockfile is explicitly rejected: it aligns our CI and protects no
consumer, which is worse than the drift because it looks fixed.

**D2. Guard it.** A gate command that reads the resolved `wasip2`
version from the lockfile, reads the `package wasi:…@x.y.z` lines from
`wit/deps/`, and fails with both versions named when they differ. The
alternative, dropping `with:` so wit-bindgen generates imports from our
own vendored files, makes the component self-consistent at the cost of
a second copy of the WASI bindings in the binary; the handoff should
evaluate it, because a design that cannot drift beats a check that
catches drift.

**D3. Execute it.** Install `cargo component` in CI, build the
component for `wasm32-wasip2`, and run a host that calls `smtp-send`
against the RFC 025 responder, asserting the same transcript the
adapter-level smoke test asserts. This is the check that would have
found the mismatch by running instead of by reading, and it retires the
"compile-checked only" limitation carried since 0.14.0.

## Security considerations

No credential or transport behaviour changes. D3 increases assurance:
the component's credential-passing path has never executed.

## Open questions

Superseded by the amendment below. Question 2 is answered (no
`cargo component` is needed); questions 1 and 3 rest on a premise the
amendment disproves.

## Amendment — 2026-09-13, on accepting

The owner accepted this RFC on 2026-09-13. Before writing the handoff
the architect inspected the built artifact, and two of the design's
assumptions above are wrong. They are recorded here rather than
silently corrected, because they change what the work is.

**A1. `cargo component` is not needed.** `cargo build --target
wasm32-wasip2 -p wasm-smtp-component` already emits a **component**,
not a core module — the artifact's version header is the component
encoding. The Rust target performs the componentization itself. D3's
premise that execution waits on a tool that is not installed is false,
and nothing in this work requires installing one. This makes execution
the cheapest part of the RFC rather than the most expensive.

**A2. The mismatch is real, observable, and wider than a single
version.** The import names in the built component are
`wasi:sockets/tcp@0.2.12` and siblings, against our world's declared
`@0.2.4` — confirmed in the artifact, not inferred. But the artifact
also imports `wasi:io/streams@0.2.3` and other `@0.2.3` interfaces,
contributed by the Rust standard library's own WASI support. So the
component already imports **two** WASI minors, and re-vendoring
`wit/deps/` at 0.2.12 would align one of them while leaving the other.

A2 changes the nature of the defect. The world's import list does not
describe the artifact and cannot be made to: the artifact's imports are
assembled by the linker from every Rust crate that declares one,
including `std`, while the `with:` mapping means our WIT generates no
import bindings at all. The import annotations are therefore
documentation — and documentation that is specific where it cannot be
accurate is worse than documentation that states the real requirement,
which is a WASI 0.2 host.

**Revised design intent.** The handoff investigates before it changes
anything, in this order:

1. **Execute it** (was D3, now first). Build the component, instantiate
   it in a host built on the `wasmtime` crate with WASI sockets
   provided by `wasmtime-wasi`, and call `smtp-send.send` against the
   scripted responder RFC 025 already provides. This is the check that
   would have found all of this by running, and it is now cheap. What a
   real host says about the version mismatch is evidence the rest of
   the decision should rest on.
2. **Then decide the contract's shape** with that evidence: either
   align the annotations to the WASI minor the artifact predominantly
   imports and document the `std` contribution, or stop pinning a minor
   in the world at all and state the requirement as a WASI 0.2 host.
   The second is the architect's leaning, and the handoff must confirm
   that WIT permits it before committing to it.
3. **Then guard** whatever invariant the chosen shape actually has. A
   guard on a number that cannot be accurate would be worse than none.

The `with:`-versus-duplicate-bindings question from the original
Open questions is deferred: it is a binary-size optimisation, not a
correctness matter, and bundling it here would obscure the result of
step 1.
