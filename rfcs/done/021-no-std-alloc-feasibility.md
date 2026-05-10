# RFC 021 — no_std / alloc feasibility

**Status.** Implemented (v0.11.0)
**Priority.** P3
**Tracks.** Future / Embedded
**Touches.** `crates/wasm-smtp/` (investigation only — no code changes)

## Summary

Assess whether `wasm-smtp-core` can be made `no_std + alloc` compatible,
and at what cost, to enable use on constrained IoT / WASM native OS
targets that lack a full `std` implementation.

## Motivation

IoT devices running WASM runtimes (e.g., WasmEdge, WAMR on RTOS) may
not provide a full `std` environment. Some provide `alloc` (heap, String,
Vec) but not the OS-dependent parts of `std` (threads, file I/O, time).
`no_std + alloc` is the boundary that lets `wasm-smtp` compile on these
platforms.

## Goals

- Enumerate every `std`-specific API used in `wasm-smtp-core`.
- Classify each use as: `alloc`-replaceable, `core`-replaceable, or
  `std`-required (not replaceable without new design).
- Assess the impact on the public API (error types, async).
- Make a recommendation: proceed, defer, or do not pursue.

## Non-goals

- Implementing `no_std` support (this is a feasibility study).
- `no_std` for adapter crates (Cloudflare and tokio are inherently `std`).
- Embedded TLS implementation.
- `no_std` async runtime (async on `no_std` requires a custom executor).

## Design

### `std` usage inventory (as of v0.11.0)

| Item | Location | `no_std` replacement | Blocker? |
|---|---|---|---|
| `std::string::String` | everywhere | `alloc::string::String` ✓ | No |
| `std::vec::Vec` | everywhere | `alloc::vec::Vec` ✓ | No |
| `std::collections::VecDeque` | harness only | `alloc::collections::VecDeque` ✓ | No |
| `std::fmt` | error Display | `core::fmt` ✓ | No |
| `std::error::Error` | `IoError` source chain | No stable `core::error::Error` in Rust 1.91 | **Yes** |
| `std::io::Error` / `std::io::ErrorKind` | `IoError` helpers | Requires `std` | **Yes** |
| `std::boxed::Box` | `dyn Transport`, error source | `alloc::boxed::Box` ✓ | No |
| `std::sync::Mutex` | `VecAuditSink` (test helper) | `spin::Mutex` or gating | No (non-core) |
| `std::sync::Arc` | `VecAuditSink` (test helper) | `alloc::sync::Arc` ✓ | No (non-core) |
| `getrandom` (OsRng for SCRAM nonce) | `scram.rs` | Requires OS entropy | **Yes** |

### Key blockers

**Blocker 1 — `std::error::Error` source chain.**
`IoError::with_source` stores `Box<dyn std::error::Error + Send + Sync>`.
`core::error::Error` trait exists since Rust 1.81 (stable) — just barely
within reach. However, `IoError`'s additional helpers (`io_kind`,
`is_timeout`, etc.) depend on `std::io::ErrorKind`, which is `std`-only.
Removing those helpers or making them feature-gated would reduce API
surface but is a breaking change.

**Blocker 2 — `std::io::Error` / `std::io::ErrorKind`.**
The `is_timeout()`, `is_connection_refused()`, etc. methods on `IoError`
walk the error chain looking for `std::io::Error`. On `no_std` targets
there is no `std::io::Error` to walk. These methods could be gated behind
`#[cfg(feature = "std")]` but that splits the API.

**Blocker 3 — `getrandom` for SCRAM nonce.**
SCRAM-SHA-256 requires a CSPRNG for the client nonce. On `no_std` targets
without OS entropy, callers would need to supply the nonce. This is a
breaking API change for the `scram-sha-256` feature.

**Non-blockers now resolved:**
- `core::error::Error` stabilised in Rust 1.81 ✓
- `alloc::sync::Arc` available ✓
- All `String`/`Vec`/`Box` usage is replaceable ✓

### Feasibility assessment

`no_std + alloc` is **feasible with a feature flag**, but the work
required is non-trivial:

1. Gate `std::io::Error` helpers (`io_kind`, `is_timeout`, etc.) behind
   `#[cfg(feature = "std")]`.
2. Replace `std::error::Error` with `core::error::Error`
   (requires Rust 1.81+ MSRV bump; currently MSRV is 1.85 so this is
   already satisfied).
3. For `scram-sha-256` on `no_std`: either disable the feature or add a
   caller-supplied nonce injection API.
4. The async executor requirement (callers must provide an `async`
   runtime) is unchanged.

Estimated effort: medium (2–4 days of careful audit and feature-gate
threading), low risk of breaking `std` users if done carefully.

## Recommendation: **Defer**

**Do not implement `no_std` support in v0.11.0.** The rationale:

1. **No concrete IoT use case yet.** There is no known deployment of
   `wasm-smtp` on an IoT device. Implementing `no_std` for a hypothetical
   future user is premature.
2. **WASI is the better path for constrained environments.** The WASI
   adapter (RFC 016) targets the same server-side-WASM use case with
   full `std` and is a higher priority.
3. **Streaming DATA (RFC 019 Phase 3) is a prerequisite.** The main
   value of `no_std` for IoT is bounded memory. Streaming DATA must land
   first for the feature to be useful.

**Conditions for re-evaluation:**
- A concrete IoT deployment uses `wasm-smtp` (opens an issue with
  hardware/runtime details).
- RFC 019 Phase 3 (streaming DATA) is implemented.
- MSRV is already ≥ 1.81 ✓ (Blocker 2 resolved once `io_kind` is gated).

### Feature flag design (for future implementation)

If pursued, the structure would be:

```toml
[features]
default = ["std", "scram-sha-256", "xoauth2", "tracing"]
std = ["getrandom/std"]
# no-std implies alloc; disables io_kind helpers and scram nonce via OsRng
```

## Security considerations

On `no_std` platforms, SCRAM nonce generation is security-critical. A
weak or predictable nonce breaks SCRAM's replay defence. Any `no_std`
implementation must use a platform-provided CSPRNG or disable SCRAM.

## Implementation plan

*This is a feasibility study RFC. No code changes are made.*

## Acceptance criteria (study complete)

- Every `std`-specific API in `wasm-smtp-core` is listed. ✅
- A recommendation (proceed / defer) is recorded. ✅ **Defer.**
- Conditions for re-evaluation are stated. ✅

## Open questions

None remaining at study completion.
