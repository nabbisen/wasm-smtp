# RFC 020 — Large message handling and memory behavior tests

**Status.** Implemented (v0.11.0)
**Priority.** P3
**Tracks.** Testing / Performance
**Touches.** `crates/wasm-smtp/src/tests/bytes_tests.rs`

## Summary

Tests for large message payloads, heavy dot-stuffing, many recipients,
and memory-growth bounds. These tests exercise the `send_mail_bytes` API
(RFC 019 Phase 2) and provide regression coverage for the DATA path.

## Implemented tests

- `large_body_100kb_sends_correctly` — 100 KB body, completes in CI time limit.
- `large_body_1mb_sends_correctly` — 1 MB body.
- `large_body_10mb_sends_correctly` — 10 MB body, `#[ignore]` (developer-only).
- `dot_heavy_body_10k_lines_all_stuffed` — 10,000 consecutive `.`-leading lines,
  verifies all are dot-stuffed.
- `many_recipients_sends_one_rcpt_per_address` — 10 RCPT TO commands.
- `body_with_max_length_lines_sends_correctly` — 998-byte lines (RFC 5321 limit).

## Memory-growth check

Each large-body test asserts `wire_len <= body.len() * 3 + 4096`. This
catches obvious allocator bugs (e.g., accumulating all lines in a `Vec<String>`
instead of writing them to the output buffer directly).

For precise allocator measurements (developer workstation), a
`CountingAllocator` as the global allocator can be added to a separate
test binary. This is out of scope for CI.

## Status

Implemented as part of v0.11.0 alongside `send_mail_bytes`.
