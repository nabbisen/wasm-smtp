# Developer Handoff — RFC 028: WASI contract alignment and component execution

**Governing RFC.** [`../../accepted/028-wasi-contract-version-alignment.md`](../../accepted/028-wasi-contract-version-alignment.md) — read its **Amendment** section first; two of the design's original assumptions are disproven there and the work order changed because of it.
**Prepared.** 2026-09-13 by the architect. Baseline: `ff3d07b` (0.17.1 released).
**Starts.** Now. Accepted by the owner on 2026-09-13.
**Target release.** Undecided, and deliberately so: what ships depends on what step 1 finds. Propose a version in the review request. Release approval is the owner's.
**Review request goes to.** `.git-exclude/review-request/028-wasi-contract-alignment.md`

## 1. Purpose

The component crate has never executed. Run it, and let what a real
host says decide how to fix the version mismatch between its declared
WASI imports and the ones in the artifact.

## 2. What is already known, so you do not re-derive it

Established by the architect before this handoff; verify anything you
depend on, but do not spend time rediscovering it.

- `cargo build --target wasm32-wasip2 -p wasm-smtp-component` emits a
  **component** already. No `cargo component`, and nothing to install.
  Artifact: `target/wasm32-wasip2/debug/wasm_smtp_component.wasm`.
- Its import names say `wasi:sockets/tcp@0.2.12` and siblings, while
  `wit/smtp.wit`'s world declares `@0.2.4`.
- It **also** imports `@0.2.3` interfaces, from the Rust standard
  library's own WASI support. Two minors, in one artifact, today.
- The `with:` map means our WIT generates no import bindings; only the
  `smtp-send` export comes from it. The import section is assembled by
  the linker from `std` and the `wasip2` crate.
- The WASI minor appears in three places: the `package` lines in
  `wit/deps/`, seven annotations in `wit/smtp.wit`, nine keys in the
  `with:` map in `src/lib.rs`.

## 3. Change scope

`tools/component-smoke/` (new crate, `publish = false`) or an extension
of `tools/smoke/` — your call, argued in the review request;
`crates/wasm-smtp-component/{wit/**,src/lib.rs,Cargo.toml}`;
`.github/workflows/ci.yml`; `.github/CONTRIBUTING.md`;
`docs/src/adapters/component-model.md`; `rfcs/done/018-*.md` and
`rfcs/done/024-*.md` (amendment notes under the Status line only);
`CHANGELOG.md`; and the version plus pins if a release slice is agreed.

## 4. Non-change scope

- No change to the `smtp-send` interface: no type, field, function, or
  the `wasm-smtp:smtp@0.1.0` package version. The WASI **import**
  declarations are the only part of the contract in scope.
- No library source outside `crates/wasm-smtp-component/src/lib.rs`.
- No new dependency in any published crate. The host harness is an
  unpublished tool and may depend on `wasmtime` and `wasmtime-wasi`.
- No `--all-features`. Do not tag, push, or publish.
- Do not edit `rfcs/README.md`.

## 5. Slices, in order

The order matters and mirrors RFC 025: run it first, and let the run
decide. Do not touch `wit/` until S2.

### S1. Execute the component under a host

1. A host harness that instantiates the built component and calls
   `smtp-send.send`. Shape: a `wasmtime::component::bindgen!` against
   `crates/wasm-smtp-component/wit`, a `Linker` with
   `wasmtime_wasi::add_to_linker_sync` (or the async equivalent) so the
   guest's `wasi:sockets` imports are satisfied, and network access
   permitted to loopback only.
   - Pin `wasmtime` and `wasmtime-wasi` to one version and say which.
     The CLI the RFC 025 smoke test uses is 27.0.0; the crate's current
     release is 38.x. They need not match, but the choice must be
     deliberate and recorded — and if the crate version you pick cannot
     satisfy a `@0.2.12` import, that is itself the finding S2 rests on.
2. Reuse the scripted responder from `tools/smoke`. Factor it out if
   that is cleaner than duplicating it; a shared module is preferable to
   a second copy of the SMTP script.
3. Assert the same things the adapter-level smoke test asserts: the
   command sequence in order, `AUTH PLAIN` present, the dot-stuffed
   body on the wire, and a `send-result` with the reply code the
   responder gave. Implicit TLS is sufficient for S1; STARTTLS only if
   it is free.
4. **Report what happens before changing anything.** Three outcomes are
   plausible and each points somewhere different:
   - It instantiates and the call succeeds. Then the version mismatch
     is inert at runtime, and S2 is a documentation decision only.
   - It fails to instantiate on an import the host cannot satisfy.
     Then the mismatch is a real defect and S2 must fix the artifact,
     not the prose. Paste the error verbatim.
   - It instantiates but `send` misbehaves. Then you have found a
     second defect, in the component's own logic, which has never run.
     Stop and report before attributing it to versions.

### S2. Decide and fix the contract's shape

With S1's evidence. Two candidates, and the RFC's amendment leans to
the second:

- **Align the annotations** to the minor the artifact predominantly
  imports: re-vendor `wit/deps/`, move the seven annotations and the
  nine `with:` keys, and document that `std` contributes its own
  interfaces at a different minor so the world still does not describe
  the artifact exactly.
- **Stop pinning a minor in the world.** Determine first whether WIT
  permits an unversioned `import wasi:sockets/tcp;` when `wit/deps/`
  holds exactly one version of the package — if it does, the world
  states "a WASI 0.2 host" and is accurate for every 0.2.x, which is
  what the WASI compatibility promise actually offers. If WIT requires
  the version, say so and fall back to the first candidate.

Whichever you choose, argue it from S1's result and say what the other
would have cost. Then amend RFC 018 and RFC 024 D8 with a note naming
the new shape.

### S3. Guard the invariant that actually exists

Only after S2, and only if the chosen shape has an invariant worth
checking. If the world stops pinning a minor there may be nothing to
guard, and the honest outcome is a gate step that builds and runs the
component — S1's harness — rather than a version comparison. If the
annotations stay pinned, the guard compares them against the resolved
`wasip2` minor and fails naming both.

Either way: wire S1's harness into the `gate` job. That is the durable
part of this RFC. Demonstrate whatever check you add can fail, the way
RFC 025's negative smoke mode and RFC 029's version guard were.

### S4. Documentation and, if agreed, release

`docs/src/adapters/component-model.md`: correct the build instructions
to drop `cargo component` and say the plain target emits a component;
state the host requirement in whatever terms S2 settled on; keep the
`jco` and `wit-bindgen` binding-generation commands, which are
unaffected. `CHANGELOG.md` under `[Unreleased]`. Propose a version in
the review request and stop at a release commit only if the owner has
approved one by then; otherwise stop before it.

## 6. Acceptance criteria

- The component is instantiated and `smtp-send.send` invoked by a host
  in CI, asserting the transcript the responder recorded.
- S1's outcome is reported verbatim, whatever it was.
- The world's WASI imports and the artifact's are either consistent or
  the discrepancy is documented as inherent, with the reason.
- Whatever check S3 adds was demonstrated failing.
- RFC 018 and RFC 024 D8 carry amendment notes.
- No change to the `smtp-send` interface or the package version.

## 7. Prohibited shortcuts

- Touching `wit/` before S1 has run and been reported.
- Installing `cargo component` and building through it to avoid
  understanding why the plain target already works.
- Asserting less in the host harness than the adapter-level smoke test
  asserts, so that "it ran" stands in for "it did the right thing".
- Guarding a version number that S2 concluded cannot be accurate.

## 8. Review request contents

S1's verbatim outcome first, before any summary. Then: the harness's
shape and where it lives, the pinned `wasmtime` versions and why, S2's
decision with the rejected alternative and its cost, what S3 guards and
its demonstrated failure, changed files, gate results, and a proposed
release version with the reasoning.
