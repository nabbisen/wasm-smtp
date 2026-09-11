# Developer Handoff — RFC 024: Release gate integrity, toolchain baseline, and MSRV correction

**Governing RFC.** [`../../accepted/024-release-gate-integrity-toolchain-baseline.md`](../../accepted/024-release-gate-integrity-toolchain-baseline.md)
**Target release.** v0.15.2 (patch). Release approval is the owner's; this handoff produces the release candidate.
**Prepared.** 2026-09-12 by the architect. Baseline commit `04696b1`.
**Review request goes to.** `.git-exclude/review-request/024-release-gate-integrity.md`

---

## 1. Purpose

Make `cargo test --workspace` and the rest of the release gate pass on
a truthful, pinned toolchain; get every crate compiling for its real
target; add CI so this cannot silently regress; and bring the
documentation back in line with the code. No feature work.

## 2. Background

The repository at `04696b1` fails its own gate in several independent
ways (see RFC 024 §Motivation for the verified table). A June 2026
handoff claimed fixes that were never committed. The declared MSRV
(1.85) is false; the true floor is 1.88. The Component crate has never
built for `wasm32-wasip2`. There is no CI.

## 3. Applicable requirements and design

- RFC 024 decisions D1–D7. D6 (publishing `wasm-smtp-test`) is
  conditional; see slice S7.
- RFC 001 (crate boundaries), RFC 010 (security baseline, `unsafe`
  forbid, no secret leakage), RFC 018 (WIT contract `wasm-smtp:smtp@0.1.0`
  is frozen).
- Project rules: English for all docs and comments; Rust 2024 module
  style, tests in `src/<mod>/tests.rs` or `src/tests/`; keep README
  concise, details in `docs/src`.
- Decision DEC-009: never `--all-features`.

## 4. Change scope

Files and areas you may change:

- `Cargo.toml` (workspace), `rust-toolchain.toml` (new), `Cargo.lock`
  (regenerated only as a consequence of the toolchain pin or
  dependency edits below).
- `.github/workflows/ci.yml` (new).
- `crates/wasm-smtp/src/**` — imports, doc comments, doctests, the
  `UnsupportedMechanism` Display text, clippy fixes.
- `crates/wasm-smtp-wasi/src/**` — `tls.rs` native-roots branch,
  `lib.rs` test gating and doc, clippy fixes.
- `crates/wasm-smtp-component/src/**` and its `Cargo.toml` —
  bindings macro form, private executor, native stubs import, clippy.
- `crates/wasm-smtp-test/Cargo.toml` (S7 only).
- `crates/wasm-smtp-tokio/src/**`, `crates/wasm-smtp-cloudflare/src/**`
  — clippy and unused-import fixes only.
- `docs/src/**`, `README.md`, `NOTICE`, `.github/CONTRIBUTING.md`,
  `.github/ISSUE_TEMPLATE/bug_report.md`, `CHANGELOG.md`, `ROADMAP.md`.
- `rfcs/done/001-*.md` — amendment note only (S7).
- `rfcs/README.md` is maintained by the architect; do not edit.

## 5. Non-change scope

Do **not**:

- Change any public API signature, add or remove public items, or
  change any error variant. Doc comments and Display strings may
  change; types may not.
- Change `wit/smtp.wit`.
- Change SMTP protocol behavior, dot-stuffing, state transitions,
  policy or audit semantics.
- Change dependency majors or add new dependencies (the wit-bindgen
  pin may move within 0.x only if 0.57 cannot be made to work; report
  it if so).
- Rewrite the `let` chains to lower the MSRV.
- Add `#![allow(clippy::pedantic)]` or any crate-wide blanket allow.
- Move, rename, or renumber any RFC.
- Tag or publish anything. The architect reviews first; the owner
  approves the release.

## 6. Required implementation, by slice

Each slice is one commit (or a short commit series) and is reviewable
on its own. Keep them in this order.

### S1. Toolchain baseline (D1, D2)

1. Add `rust-toolchain.toml`:
   ```toml
   [toolchain]
   channel = "1.88"
   components = ["rustfmt", "clippy"]
   targets = ["wasm32-unknown-unknown", "wasm32-wasip2"]
   ```
2. Set `rust-version = "1.88"` in `[workspace.package]`.
3. Confirm `cargo check --workspace` passes on the pinned toolchain
   (it does at baseline; this is the control).

### S2. Compile and doctest fixes

Known defects with locations:

| Where | Defect | Expected fix |
|---|---|---|
| `crates/wasm-smtp/src/client/send.rs` around line 519 | `ProtocolError` not imported; breaks `--features smtputf8` (and therefore the cloudflare/tokio `smtputf8` pass-throughs) | import it |
| `crates/wasm-smtp/src/audit.rs` doctest on `VecAuditSink` (around line 136) | `events()` returns `Vec<String>`; the example matches on the enum | assert on the string label, e.g. `events[0].starts_with("Connected")` |
| `crates/wasm-smtp/src/policy.rs` module doctest (around line 15) | imports `PolicyError` from `wasm_smtp::policy`; it lives at the crate root | fix the path; consider re-exporting `PolicyError` from `policy` **only if** it adds no new public item — it does, so just fix the path |
| `crates/wasm-smtp-wasi/src/lib.rs` crate-level doctest | uses `connect_smtps`, which exists only on wasm32 | mark the block `ignore` with a reason comment, or make it `no_run` behind `#[cfg(target_arch = "wasm32")]` if that compiles; `ignore` is acceptable |
| `crates/wasm-smtp-wasi/src/tls.rs` lines 103–112 | `rustls-native-certs` 0.8 returns `CertificateResult`, not `Result` | adopt the same policy as `wasm-smtp-tokio/src/transport.rs::default_root_store`: add every cert that decoded, fail only if the store is empty |
| `crates/wasm-smtp-wasi/src/lib.rs` | `mod tests;` is unconditional | `#[cfg(test)] mod tests;` |
| `crates/wasm-smtp/src/tests/stream_tests.rs:275`, `oauthbearer_tests.rs:4` | unused imports | remove |
| `crates/wasm-smtp-tokio` | unused `ProtocolError` import in tests | remove |

### S3. Component crate builds for `wasm32-wasip2` (D5)

1. In `crates/wasm-smtp-component/src/lib.rs`:
   - Replace the `exports: { ... }` option inside `wit_bindgen::generate!`
     with the `export!(SmtpSendImpl)` macro form that wit-bindgen 0.57
     requires (see the crate's own rustdoc in the registry source).
   - Remove every reference to `wasm_smtp_component_rt`. Add a private
     `fn block_on<F: Future>(fut: F) -> F::Output` using
     `Waker::noop()` and `pin!`, panicking with a clear message on
     `Pending`. Document why this is sound (the WASI transport never
     yields).
   - Import `ConnectOptions` from `wasm_smtp_wasi` where used, or drop
     the unused `opts` binding.
   - Restore `TlsMode` in the native stubs import
     (`#[cfg(not(target_arch = "wasm32"))] use stubs::{..., TlsMode};`
     with `#[allow(unused_imports)]` if needed by tests only).
2. Run `cargo check --target wasm32-wasip2 -p wasm-smtp-wasi -p wasm-smtp-component`.
   This is the first time either crate is compiled for the target in
   this repository. If `wasi_impl/**` has its own compile errors,
   fix them **only** if they are local (imports, API renames within
   `wasi` 0.14). If a fix would change structure or behavior, stop
   and file a technical issue report; do not guess.

### S4. Format and lint

1. `cargo fmt --all` on the pinned toolchain. Commit the result as its
   own commit with no other changes, so the diff is reviewable as
   pure formatting.
2. `cargo clippy --workspace --all-targets -- -D warnings`. Fix each
   warning, or add a targeted `#[allow(clippy::<lint>)]` on the item
   with a one-line reason. Existing module-level allows in
   `src/tests/mod.rs` may stay. `#[ignore]` attributes must carry a
   reason string.

### S5. CI workflow (D3, D4)

Create `.github/workflows/ci.yml`:

- Triggers: `pull_request`, and `push` to `main`.
- Job `gate`: `ubuntu-latest`; toolchain from `rust-toolchain.toml`
  (e.g. `dtolnay/rust-toolchain` reading the file, or plain
  `rustup show`); runs every command in RFC 024 §D3 verbatim.
- Job `stable` (lower priority, may be a follow-up commit):
  `continue-on-error: true`, `rustup toolchain install stable`,
  runs `cargo +stable test --workspace` and the clippy step.
- Cache `~/.cargo` and `target/` with a standard action.
- No secrets. Never `--all-features`.

Also update `.github/CONTRIBUTING.md` "Required checks" to match D3.

### S6. Documentation sync

Correct every statement below; keep wording concise and in English.

| File | Fix |
|---|---|
| `docs/src/intro.md` | Cloudflare adapter is not "planned"; four adapters exist. Auth list: SCRAM-SHA-256 preferred, then PLAIN, LOGIN; XOAUTH2 and OAUTHBEARER opt-in. Remove "SASL SCRAM ... not supported". |
| `docs/src/concepts/architecture.md` | Crate map shows all adapters; module table reflects `client/` directory, `policy.rs`, `audit.rs`, `message_body.rs`, `outcome.rs`, `scram.rs`, `src/tests/`; `Transport` has four methods including `flush`. |
| `docs/src/concepts/errors.md` | Five variants including `Policy`; `SmtpOp` list matches `error.rs`; `AuthError` has `MalformedChallenge` and `Other`. |
| `docs/src/core/core.md` | Public surface matches `lib.rs`; `Transport` includes `flush`; `login` prefers SCRAM; "no external dependencies" is false with default features — say "no required dependencies; optional crypto behind `scram-sha-256`". |
| `docs/src/core/usage.md` | "Choosing an authentication mechanism" and "Testing your code" sections reflect SCRAM preference and the published mock (if S7 lands) or the in-tree pattern. |
| `docs/src/adapters/wasi.md` and `crates/wasm-smtp-wasi/src/lib.rs` doc | Remove the claim that helpers "return a compile-time error on non-WASM targets"; they are absent on non-wasm32. |
| `README.md` | Dependency snippets use `"0.15"`; the features table includes `scram-sha-256`, `mail-builder`, `tracing`; MSRV 1.88 stated once. |
| `NOTICE` | Crate list: `wasm-smtp`, `wasm-smtp-cloudflare`, `wasm-smtp-tokio`, `wasm-smtp-wasi`, `wasm-smtp-component`, `wasm-smtp-test`. |
| `.github/CONTRIBUTING.md` | Repository layout lists all six crates; tests live in `crates/<crate>/src/tests/`; required checks per D3; MSRV via `rust-toolchain.toml`. |
| `.github/ISSUE_TEMPLATE/bug_report.md` | Crate list covers all published crates. |
| `crates/wasm-smtp/src/lib.rs`, `client/auth.rs` (`login` doc), `client/mod.rs` (orphan doc block above `quit`), `protocol.rs` (`AuthMechanism` doc) | Mechanism preference is SCRAM > PLAIN > LOGIN; remove "Today the crate implements PLAIN and LOGIN". Delete the orphaned `login` doc comment in `client/mod.rs`. |
| `crates/wasm-smtp/src/client/send.rs` | `send_message` doc block currently begins with a copy of the SMTPUTF8 text; make it describe `send_message` only. |
| `crates/wasm-smtp/src/error.rs` `AuthError::UnsupportedMechanism` Display | List the mechanisms actually compiled in (PLAIN, LOGIN, plus SCRAM-SHA-256 / XOAUTH2 / OAUTHBEARER per feature). Add a unit test asserting the text under default features. |
| `crates/wasm-smtp/src/audit.rs` module doc | `AuthCompleted` carries `"SCRAM-SHA-256"`, not `"AUTH SCRAM-SHA-256"`. |
| `rfcs/done/019-*.md` | Add a one-line note under Status that Phase 3 shipped in v0.13.0; do not rewrite the body. |
| `CHANGELOG.md` | (a) Translate the `[0.15.1]` entry to English, preserving content. (b) Merge the two `[0.9.4]` sections (lines ~282 and ~400) into one heading with both blocks kept verbatim and a one-line note that both shipped under tag `0.9.4`. (c) Add comparison links for 0.10.0 through 0.15.2 in the existing style but with the project's tag format (`0.15.1`, no `v` prefix); fix the existing `v`-prefixed links likewise. (d) Add the `[0.15.2]` entry (see S8). |
| `ROADMAP.md` | Add "Phase 18 — Release gate integrity *(v0.15.2)*" summarizing RFC 024. |

### S7. Publish `wasm-smtp-test` — **conditional on owner confirmation**

Do not start this slice until the review request for S1–S6 is
answered or the architect tells you D6 is confirmed.

1. `crates/wasm-smtp-test/Cargo.toml`: remove `publish = false`; set
   `wasm-smtp = { path = "../wasm-smtp", version = "0.15.2" }` (the
   workspace version at release time).
2. Add an amendment note to `rfcs/done/001-workspace-crate-boundaries-release-structure.md`
   directly under the Status line: "Amended by RFC 024 (v0.15.2):
   `wasm-smtp-test` is published." Do not edit the body.
3. CHANGELOG 0.15.2 entry gains a line.

### S8. Release candidate

1. Bump `[workspace.package] version` to `0.15.2`; bump the three
   `version = "0.15.1"` pins in adapter manifests to `0.15.2`.
2. CHANGELOG `[0.15.2]` entry, English, sections: Fixed (each compile
   and doctest fix), Changed (MSRV 1.88, toolchain pin, CI, clippy
   deny policy), Documentation.
3. Run the full gate (RFC 024 §D3) and capture output to
   `.git-exclude/review-request/evidence/024/` as one log per command.
4. Do not tag.

## 7. Required tests

- All existing tests continue to pass; do not delete or `#[ignore]`
  any test to get green.
- New unit test for the `UnsupportedMechanism` Display text (S6).
- Doctests are part of the gate; every `///` example either runs or
  is `ignore`/`no_run` with a reason.

## 8. Acceptance criteria

1. Every command in RFC 024 §D3 passes on the pinned toolchain, with
   captured logs.
2. `cargo +1.88 test --workspace` passes; `cargo +1.85 check -p wasm-smtp`
   is **not** required to pass and must not be made to.
3. `cargo check --target wasm32-wasip2 -p wasm-smtp-component` passes.
4. `.github/workflows/ci.yml` exists and encodes D3; `stable` job is
   non-blocking.
5. The formatting commit contains formatting changes only.
6. No public API change: `cargo public-api` is not required, but the
   review will diff `pub` items; none may appear or disappear.
7. Documentation items in S6 are all addressed.
8. CHANGELOG has exactly one `[0.9.4]` section and a `[0.15.2]` entry.

## 9. Prohibited shortcuts

- Lowering the gate: no removed tests, no `ignore` without reason,
  no `--all-features`, no crate-wide `allow`.
- Bundling unrelated refactors into any slice.
- Changing the WIT file or any public type to make something compile.
- Guessing at WASI socket semantics if `wasi_impl` fails to compile;
  file an issue report instead.
- Committing secrets or machine-specific paths into the workflow.

## 10. Module boundaries and compatibility constraints

- Core never depends on an adapter; adapters never implement SMTP
  logic. Unchanged.
- `wasm-smtp-test` must not become a dependency of `wasm-smtp`
  (cycle; DEC-011).
- Public API stable across the family; one version number.
- MSRV 1.88 from this release forward.

## 11. Security constraints

- No credential, token, or message body may appear in any new log
  line, panic message, error text, or doctest output.
- CI uses no secrets.
- The `plaintext-only` WASI feature remains clearly marked test-only.

## 12. Known risks

| Risk | Mitigation |
|---|---|
| `wasi_impl/**` has never been compiled; `cargo check --target wasm32-wasip2` may reveal more than import errors | S3 step 2: fix local issues only, otherwise escalate with an issue report |
| wit-bindgen 0.57 macro migration may need a different pinned 0.x | Report before changing the pin; keep the WIT contract frozen |
| Clippy `-D warnings` may surface lints that require a small restructuring | Prefer a targeted allow with a reason over a behavior-affecting change; note each in the review request |
| tokio error-path tests bind OS ports and may flake in CI | If observed, run that crate's tests with `--test-threads=1` in the workflow and record it |

## 13. Required evidence in the review request

Write `.git-exclude/review-request/024-release-gate-integrity.md` with:

1. Implementation summary per slice.
2. Addressed requirements (RFC 024 decision IDs).
3. Changed files, grouped by slice.
4. Important implementation decisions (e.g. the exact wit-bindgen
   invocation, each clippy allow and its reason).
5. Differences from this handoff, if any, with justification.
6. Executed commands and results: paste the tail of each gate command
   and reference the full logs under `evidence/024/`.
7. Build and static-analysis results on the pinned toolchain.
8. Unresolved issues and anything escalated.
9. Known limitations.
10. Requested review focus.

Tell the owner only the path of that file.

---

# Revision 2 — 2026-09-12, after review 1

Review: `.git-exclude/reviewed/024-release-gate-integrity-review-1.md`.
Slices S1–S6 and S8 are accepted. The following replaces §6's
"conditional" status of S7 and adds S9 and S10. Change scope (§4) is
extended by exactly the files named here.

## Correction C1 (from F1)

`.github/CONTRIBUTING.md`: the "Repository layout" tree lists six
crates; "Code style" says tests live in `crates/<crate>/src/tests/`
(or `src/tests.rs`), not `crates/core/src/tests.rs`.

## S7 — authorized. Execute as written in §6, dependency pinned to `0.15.2`.

## S9 — WIT contract amendment (RFC 024 D8)

Scope additions: `wit/smtp.wit`, `wit/deps/**` (new),
`crates/wasm-smtp-component/Cargo.toml`, `rfcs/done/018-*.md`
(amendment note only), `.github/workflows/ci.yml` (remove the NOTE
comment once green).

1. `wit/smtp.wit`: rename the record field to `%from: string`; change
   every `@0.2.0` import annotation in `world smtp-client` to `@0.2.4`;
   fix the header's docs path to `docs/src/adapters/component-model.md`.
2. Vendor WASI 0.2.4 `wasi:io`, `wasi:sockets`, `wasi:clocks` under
   `wit/deps/io/`, `wit/deps/sockets/`, `wit/deps/clocks/`. Source: the
   WebAssembly WASI 0.2.4 release (an identical copy exists locally in
   the `wasip2` 1.0.1 crate's `wit/deps/`). Keep upstream headers. Add
   `wit/deps/README.md`: one paragraph naming the packages, version
   0.2.4, the source, and that the files are vendored unmodified.
3. `crates/wasm-smtp-component/Cargo.toml`: add
   `wasi = { version = "0.14", default-features = false }` under
   `[target.'cfg(target_arch = "wasm32")'.dependencies]`.
4. `generate!`: add a `with:` block mapping each imported interface
   (`wasi:io/error@0.2.4`, `wasi:io/poll@0.2.4`, `wasi:io/streams@0.2.4`,
   `wasi:sockets/network@0.2.4`, `wasi:sockets/instance-network@0.2.4`,
   `wasi:sockets/tcp@0.2.4`, `wasi:sockets/tcp-create-socket@0.2.4`,
   `wasi:sockets/ip-name-lookup@0.2.4`, and `wasi:clocks/monotonic-clock@0.2.4`
   if the resolver requires it) onto the corresponding `wasi::…` module.
   Drop `generate_all` if the mapping covers every import. If the
   mapping cannot be made to work with wit-bindgen 0.57, fall back to
   `generate_all` with the vendored deps and say so in the review
   request; do not move the pin unreported.
5. `rfcs/done/018-component-model-wit-interface.md`: add under the
   Status line: "Amended by RFC 024 D8 (v0.15.2): field `from` escaped
   as `%from`; WASI imports at 0.2.4 with packages vendored under
   `wit/deps/`."
6. Acceptance: `cargo check -p wasm-smtp -p wasm-smtp-wasi -p wasm-smtp-component --target wasm32-wasip2`
   passes; the WIT package version remains `wasm-smtp:smtp@0.1.0`.

## S10 — Explicit rustls crypto provider per adapter (RFC 024 D9)

Scope additions: `crates/wasm-smtp-tokio/src/transport.rs`,
`crates/wasm-smtp-wasi/src/tls.rs`, and the two test modules that
currently install a provider.

1. `wasm-smtp-tokio/src/transport.rs::build_client_config`: build with
   `ClientConfig::builder_with_provider(Arc::new(provider))` where
   `provider` is `tokio_rustls::rustls::crypto::aws_lc_rs::default_provider()`
   under `cfg(feature = "aws-lc-rs")` and `…::ring::default_provider()`
   under `cfg(feature = "ring")`; then
   `.with_safe_default_protocol_versions()` mapped to `IoError`; then
   the existing root-store and client-auth steps.
2. `wasm-smtp-wasi/src/tls.rs::make_tls_config`: same shape with
   `rustls::crypto::ring::default_provider()` and
   `.with_protocol_versions(rustls::DEFAULT_VERSIONS)`.
3. Remove `install_test_crypto_provider` and its two call sites from
   `wasm-smtp-tokio/src/tests/`; remove the two `install_default()`
   lines from `wasm-smtp-wasi/src/tests.rs`.
4. Acceptance: `cargo test --workspace` passes with no
   `install_default` anywhere in the workspace (`grep` is part of the
   evidence); public API unchanged.
5. CHANGELOG 0.15.2, Changed: one entry describing the provider
   selection change and why.

## Evidence and re-review

Refresh every log under `evidence/024/`; command 10 now includes the
component and must pass. Write the re-review request to
`.git-exclude/review-request/024-release-gate-integrity-2.md` with the
same structure as request 1, listing only what changed since `b2cc145`.
