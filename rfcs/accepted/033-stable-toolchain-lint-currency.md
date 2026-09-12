# RFC 033 — Stable-toolchain lint currency

**Status.** Accepted (owner, 2026-09-13)
**Priority.** P3
**Tracks.** Code quality / CI
**Touches.** `crates/wasm-smtp/src/{error.rs,policy.rs,protocol.rs,message_body.rs,tests/**}`, `crates/wasm-smtp/tests/public_api.rs`, `crates/wasm-smtp-cloudflare/src/{adapter.rs,tests/**}`, `crates/wasm-smtp-test/src/transport.rs`, `tools/smoke/tests/tokio_adapter.rs`, `.github/workflows/ci.yml` (advisory job only)
**Handoff.** [`../handoffs/033-stable-toolchain-lint-currency/implementation-handoff.md`](../handoffs/033-stable-toolchain-lint-currency/implementation-handoff.md)
**Origin.** The advisory `stable` CI job has failed since at least 2026-09-12. The owner asked for a handoff on 2026-09-13.

## Summary

Clippy 0.1.98 reports 22 lints the pinned 1.88 Clippy does not. Six are
ordinary code improvements that also compile on 1.88: fix them. Fourteen
are one pedantic lint, `clippy::unused_async_trait_impl`, which fires on
`async fn` trait implementations that have nothing to await: allow it,
at the impl block, with its reason. Make the advisory job list every
lint, not just the first crate's.

No public API, behaviour, or published artifact changes.

## Motivation

The gate is pinned to 1.88 on purpose, and it is green. The advisory
job exists to warn before a toolchain bump turns warnings into gate
failures. It only does that job while it is green: a job that has been
red for weeks gets ignored, and then it misses the real failure it
exists for.

**CI understated the problem.** It reported 7 errors, because Clippy
stopped at the first crate that failed. Run with lints as warnings
across the whole workspace, stable reports **22**:

| Lint | Sites | Where |
|---|---|---|
| `unused_async_trait_impl` | 14 | 8 impl blocks: core's `SliceBody` and test harness, `public_api.rs`, `wasm-smtp-test`'s `MockTransport`, the Cloudflare adapter and its test transport |
| `collapsible_if` | 3 | `policy.rs` ×2, `tools/smoke/tests/tokio_adapter.rs` |
| chunks_exact with constant size (`as_chunks`) | 2 | `protocol.rs` base64 encode and decode |
| manual `is_multiple_of` | 1 | `protocol.rs` base64 decode |
| unnecessary trailing comma | 1 | `error.rs` `Display` |
| byte char slice | 1 | `tests/protocol_tests.rs` |

## Amendment — 2026-09-13, after review 1

**A1. The count.** As accepted, this RFC said 16 `unused_async_trait_impl`
sites, and its table summed to 24 against a stated total of 22. The
implementer counted 14, and the architect's own Clippy output confirms
it. The total of 22 and the eight impl blocks were right. Corrected
above; nothing in the design depended on the number.

## Design

### D1. Fix the six that are improvements

Each fix is what Clippy suggests. The architect verified on 1.88.0
that `usize::is_multiple_of` and `<[T]>::as_chunks` exist, and the
`let` chains `collapsible_if` asks for were already the reason for the
1.88 MSRV. The base64 encoder's remainder handling maps directly: the
remainder `chunks_exact(..).remainder()` provided is the second value
`as_chunks` returns.

### D2. Allow `unused_async_trait_impl`, deliberately

No rewrite works, and the architect verified each alternative:

- **`fn … -> impl Future { async move { … } }`** triggers
  `clippy::manual_async_fn` on 1.88 and on stable. The two lints ask for
  opposite things.
- **`fn … -> impl Future { …; std::future::ready(…) }`** changes
  behaviour. It runs the body when the function is called rather than
  when the future is first polled. Every flagged body has side effects:
  `MockTransport` records writes, counts upgrades, and moves queued
  chunks. For a published test double, that is a semantic change hidden
  inside a lint fix.
- **`#[allow(clippy::unused_async_trait_impl)]` alone** is an unknown
  lint to 1.88 Clippy and fails the pinned gate under `-D warnings`.
- **A `[lints.clippy]` table entry** keeps the 1.88 gate green, but
  prints an unknown-lint warning on every run. A gate whose clean
  output contains a warning trains readers to skip warnings.

What works on both toolchains, verified: on the **impl block**,

```rust
// The trait is async because real transports await; this
// implementation completes without awaiting.
#[allow(unknown_lints, clippy::unused_async_trait_impl)]
impl Transport for MockTransport { … }
```

`unknown_lints` is scoped to that one attribute's item. It follows the
precedent already in `crates/wasm-smtp/src/transport.rs`, where the
trait's default `flush` carries `#[allow(clippy::unused_async)]` with the
same reason. When the toolchain pin moves past the version that
introduced the lint, `unknown_lints` should come out of these attributes.
That is recorded here, not tracked separately.

### D3. The advisory job reports everything

`cargo +stable clippy` in the `stable` job gains `--keep-going`, so one
crate's errors no longer hide the rest, if Cargo accepts that flag
through Clippy (the handoff verifies it). The gate job does not change.

## Non-goals

- Moving the toolchain pin or the MSRV.
- Enabling or disabling any lint group. `wasm-smtp-component`'s
  `pedantic` setting stays as it is.
- Any rewrite beyond what the lint asks for.

## Security considerations

None. `protocol.rs`'s base64 code is used for `AUTH` credentials, so its
rewrite is held to the existing tests plus one stated check (handoff
S1.3). It is not assumed safe because it is mechanical.

## Release

Nothing a consumer receives changes. The work lands on `main` and rides
with the next release.
