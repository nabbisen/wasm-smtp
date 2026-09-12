# RFC 028 — WASI contract version alignment and component execution

**Status.** Proposed
**Priority.** P1
**Tracks.** Component Model / WASI / Testing / CI
**Touches.** `crates/wasm-smtp-component/wit/`, `crates/wasm-smtp-component/src/lib.rs`, `crates/wasm-smtp-component/Cargo.toml`, `tools/smoke/` or a new `tools/component-smoke/`, `.github/workflows/ci.yml`, `rfcs/done/018-*`, `rfcs/done/024-*` (amendment notes)
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

1. D2: guard the `with:` map, or remove it and accept duplicated WASI
   bindings? The handoff should decide on measured binary size.
2. Does `cargo component` in CI need a pinned version, as wasmtime does?
3. Should the WASI minor be tracked in one place both the WIT and the
   guard read, rather than in three files?
