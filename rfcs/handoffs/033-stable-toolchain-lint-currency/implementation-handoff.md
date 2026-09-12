# Developer Handoff — RFC 033: Stable-toolchain lint currency

**Governing RFC.** [`../../accepted/033-stable-toolchain-lint-currency.md`](../../accepted/033-stable-toolchain-lint-currency.md). Its D2 lists the rewrites that do **not** work, and why. Do not retry them.
**Prepared.** 2026-09-13 by the architect. Baseline: `a7bd640`.
**Starts.** Now. Independent of RFC 032 S4, which touches only `docs/src/**` and a new `tools/` crate. Commit separately. If both are in progress, rebase whichever lands second.
**Target release.** None required; lands on `main`.
**Review request goes to.** `.git-exclude/review-request/033-stable-toolchain-lint-currency.md`

## 1. Purpose

Make `cargo +stable clippy --workspace --all-targets -- -D warnings`
pass without changing behaviour, public API, or anything the pinned 1.88
gate reports.

## 2. What is already known

Each item below was run by the architect, not reasoned. Check anything
you rely on.

- Stable is `rustc 1.98.1` / `clippy 0.1.98` on this machine. The full
  list, gathered with lints as warnings across the workspace
  (`cargo +stable clippy --locked --workspace --all-targets
  --message-format=short`), is 22 sites. RFC 033 §Motivation has them by
  lint.
- On 1.88.0, `usize::is_multiple_of`, `<[T]>::as_chunks`, and a trait
  impl written as `fn … -> impl Future` all compile.
- `fn … -> impl Future { async move { … } }` triggers
  `clippy::manual_async_fn` on **both** 1.88 and 1.98.
- `#[allow(clippy::unused_async_trait_impl)]` alone is an unknown lint to
  1.88 Clippy. `#[allow(unknown_lints, clippy::unused_async_trait_impl)]`
  placed on an **impl block** passes `-D warnings` on both 1.88.0 and
  1.98.1.
- The flagged bodies have side effects. `wasm-smtp-test`'s
  `MockTransport` is published, and it records writes, counts upgrades,
  and moves queued chunks inside those bodies.

## 3. Change scope

The files in RFC 033's **Touches** line, and `CHANGELOG.md` under
`[Unreleased]`.

## 4. Non-change scope

- No public signature changes, and no change to what any function does
  or when it does it. `std::future::ready` rewrites are prohibited
  (RFC 033 D2).
- No lint group enabled or disabled; no `[lints]` table edits.
- No toolchain pin or MSRV change.
- No gate-job change. `.github/workflows/ci.yml` changes only in the
  advisory `stable` job.
- Do not tag, push, or publish. Do not edit `rfcs/README.md`.

## 5. Slices

### S1. The six improvements (D1)

1. `crates/wasm-smtp/src/error.rs` `InconsistentMultiline`: remove the
   trailing comma inside `write!`.
2. `crates/wasm-smtp/src/policy.rs` `check_recipients` and
   `check_message_size`: collapse each nested `if` into one `if let … &&`
   chain.
3. `crates/wasm-smtp/src/protocol.rs`: `base64_encode` takes
   `let (chunks, rem) = input.as_chunks::<3>();`, and each chunk becomes
   an array, so its indexing is unchanged. `base64_decode` uses
   `!bytes.len().is_multiple_of(4)` and `bytes.as_chunks::<4>().0`. The
   length check above guarantees an empty remainder. Say so in a
   comment, or assert it in a test, not both.
   **Base64 carries credentials:** run the existing `protocol_tests`, and
   add a round-trip test covering input lengths 0 through 7 if none
   exists. Paste which one applied.
4. `crates/wasm-smtp/src/tests/protocol_tests.rs:488`: the byte string
   Clippy suggests.
5. `tools/smoke/tests/tokio_adapter.rs:197`: collapse the nested `if let`
   into one chain. The refused-case semantics must not change. That test
   must still carry on as a caller would if the connection ever
   succeeds. Show it still going red when given the run's CA (the RFC
   032 S2 demonstration), then revert.

### S2. The allowed lint (D2)

1. Add the attribute to **each of the 8 impl blocks**, not to each
   function:
   - `crates/wasm-smtp/src/message_body.rs` — `impl MessageBody for SliceBody<'_>`
   - `crates/wasm-smtp/src/tests/harness.rs` — `impl Transport for MockTransport`, `impl StartTlsCapable for MockTransport`
   - `crates/wasm-smtp/tests/public_api.rs` — `impl Transport for TestTransport`
   - `crates/wasm-smtp-test/src/transport.rs` — `impl Transport for MockTransport`, `impl StartTlsCapable for MockTransport`
   - `crates/wasm-smtp-cloudflare/src/adapter.rs` — `impl StartTlsCapable for CloudflareTransport`
   - `crates/wasm-smtp-cloudflare/src/tests/e2e_via_tokio_mock.rs` — `impl StartTlsCapable for StreamTransport<S>`
2. One comment line above each attribute, in the register of the
   `transport.rs` precedent: why the implementation is `async` with
   nothing to await. Word it for its own impl. A mock records instead
   of doing I/O; the Cloudflare upgrade is synchronous in `worker`.
3. **Check the Cloudflare adapter against the target it ships for.**
   `cargo +stable clippy -p wasm-smtp-cloudflare --target
   wasm32-unknown-unknown -- -D warnings` on stable, and gate command 9
   on 1.88. Paste both.

### S3. The advisory job (D3)

1. Confirm that `cargo +stable clippy --keep-going --workspace
   --all-targets -- -D warnings` is accepted, by running it with one S1
   fix temporarily reverted, and that it reports lints from **more than
   one crate** in a single run. Paste the output. If Cargo rejects the
   flag through Clippy, say so and leave the job unchanged.
2. If accepted: add `--keep-going` to the `stable` job's Clippy step
   only, with a one-line comment saying why.

### S4. Changelog and verification

1. `CHANGELOG.md` under `[Unreleased]`: one short entry. No API change;
   the stable Clippy job is green again.
2. Run the full 21-command gate on 1.88. It must be unchanged.
3. Run `cargo +stable clippy --locked --workspace --all-targets -- -D
   warnings` and `cargo +stable test --workspace`. Both must pass.

## 6. Acceptance criteria

- Stable Clippy passes with `-D warnings` across the workspace and
  `--all-targets`.
- The 1.88 gate passes with no new warnings in its output.
- No `std::future::ready`, no `manual_async_fn` allow, no lint-table
  edits.
- Base64 covered as S1.3 requires.
- The tokio refused-case demonstration re-run after S1.5.

## 7. Prohibited shortcuts

- Allowing any of the six D1 lints instead of fixing it.
- Allowing `unused_async_trait_impl` per function, crate-wide, or in
  `[lints]`.
- A crate-level or workspace-level `unknown_lints` allow.
- Rewriting an async body eagerly.

## 8. Review request contents

The before and after stable Clippy site counts; S1.3's base64 evidence;
S1.5's demonstration; S2.3's two outputs; S3's `--keep-going` evidence;
the 1.88 gate; stable Clippy and stable test results; changed files.
