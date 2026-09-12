# RFC 025 — WASI hardening and on-target verification

**Status.** Accepted
**Priority.** P0
**Tracks.** Adapter / WASI / Testing / Audit / Core / CI
**Touches.** `crates/wasm-smtp-wasi/`, `crates/wasm-smtp/src/client/`, `crates/wasm-smtp/src/audit.rs`, `crates/wasm-smtp-cloudflare/src/`, `tools/smoke/` (new), `.github/workflows/ci.yml`, `rfcs/done/010-*` (amendment note), `rfcs/done/012-*` (amendment note), docs
**Handoff.** [`../handoffs/025-wasi-hardening/implementation-handoff.md`](../handoffs/025-wasi-hardening/implementation-handoff.md)
**Authorized.** Theme and ordering approved by the owner on 2026-09-12 after the roadmap-exhaustion report that closed RFC 024.

## Summary

Make the WASI adapter actually work, and prove it by running it. Until
now no code in `wasm-smtp-wasi` or `wasm-smtp-component` has ever
executed on a `wasm32-wasip2` host; RFC 024 got both crates compiling
for the target, which exposed two defects that only execution would
have caught. This RFC adds an on-target smoke test to the release gate,
fixes the defects it would have found, and closes four smaller gaps
found in the same review: two promised audit events that are never
emitted, pipelining that covers only one of four send methods, adapters
that discard error causes, and a dependency-advisory check that RFC 010
claims exists but does not.

Ships as **0.16.0** (new public function, new emitted events, wire-order
change on three send methods when the server advertises PIPELINING),
subject to the owner's release approval.

## Motivation

Verified in code on 2026-09-12:

| # | Defect | Effect |
|---|---|---|
| 1 | `WasiTlsTransport::upgrade_to_tls` evaluates a placeholder-stream function that panics unconditionally, as an argument to `mem::replace` | every STARTTLS session on WASI panics at the upgrade |
| 2 | `WasiStream::read` maps an empty non-blocking read to `Ok(0)`; WASI's `input-stream.read` returns an empty list whenever no bytes are ready yet, and the adapter's wait-and-retry sits in the error branch that case never reaches | the core sees "peer closed" on the first reply that is not already buffered; on a real network this is nearly every reply |
| 3 | `SmtpAuditEvent::RecipientRejected` is never emitted; `SessionAborted` is emitted only from `quit`, so failures during a transaction leave no abort record | RFC 012's event model is incomplete for exactly the cases audit exists for |
| 4 | `send_mail` pipelines when PIPELINING is advertised; `send_mail_bytes`, `send_mail_stream`, `send_mail_smtputf8` never do | inconsistent latency and four near-identical envelope implementations |
| 5 | Cloudflare and WASI adapters build `IoError` from formatted strings; the source chain RFC 013 specified and the tokio adapter preserves is lost | callers cannot classify the underlying failure |
| 6 | RFC 010 says `cargo deny` runs in CI; nothing does | the advisory floor in `Cargo.toml` is unenforced |

Defects 1 and 2 are inferred from the WASI 0.2.4 interface contract and
the adapter's code, not from a run. That is the strongest possible
argument for defect 0: there is no run.

## Goals

- Execute a real SMTP session from a `wasm32-wasip2` component under
  wasmtime against a local TLS-terminated SMTP responder, on every CI
  run, for both implicit TLS and STARTTLS.
- Fix defects 1 through 6.
- Reduce the four send-method bodies to one envelope implementation.

## Non-goals

- A caller-supplied size hint for streaming policy checks (separate,
  API-design theme).
- `Send`-bound ergonomics for the tokio adapter.
- Publishing the mdBook; Turnstile and anti-abuse documentation
  (RFC 026).
- Testing the Component Model export end to end under a host that
  calls `smtp-send` (needs `cargo component`; deferred until the
  smoke test is stable).
- Any change to `wit/`.

## Design

### D1. On-target smoke test

A new workspace member `tools/smoke` (`wasm-smtp-smoke`,
`publish = false`, native binary) is the test driver:

1. Generates a self-signed certificate for `localhost` at run time
   (`rcgen`), so no key material is committed.
2. Starts two scripted SMTP responders on loopback using the existing
   `rustls` dependency: one implicit-TLS listener and one plaintext
   listener that offers `STARTTLS` and upgrades in place. Each replays
   a fixed server script and records every command line it receives.
3. Runs `wasmtime run --wasi inherit-network --wasi allow-ip-name-lookup`
   on the guest, passing host, port, mode, and the CA PEM path as
   arguments.
4. Asserts the guest exit code and the recorded command sequence
   (`EHLO`, `AUTH PLAIN`, `MAIL FROM`, `RCPT TO`, `DATA`, body with the
   dot-stuffed line, `QUIT`; for STARTTLS additionally `STARTTLS` and
   the second `EHLO`).

The guest is `crates/wasm-smtp-wasi/examples/smoke.rs`, built with
`cargo build --target wasm32-wasip2 -p wasm-smtp-wasi --example smoke`.
It loads the CA PEM into a `RootCertStore`, connects with
`connect_smtps_with` or the new `connect_smtp_starttls_with`, logs in
with `AUTH PLAIN`, sends one message containing a leading-dot line,
and quits. Exit code 0 on success; the error `Display` on stderr and a
non-zero exit otherwise.

Public API addition: `connect_smtp_starttls_with(host, port, ehlo_domain, options)`
in `wasm-smtp-wasi`, mirroring the implicit-TLS pair. Without it a
STARTTLS session cannot trust a test CA.

Gate addition (D3 of RFC 024 gains command 13):

```bash
cargo build --target wasm32-wasip2 -p wasm-smtp-wasi --example smoke
cargo run -p wasm-smtp-smoke
```

CI installs wasmtime with the Bytecode Alliance setup action. The
smoke test touches loopback only; no external network.

### D2. WASI stream I/O uses the blocking primitives

`WasiStream::read` calls `input-stream.blocking-read`, which returns
only when at least one byte is available or the stream closes:
`Ok(empty)` cannot occur, `StreamError::Closed` maps to `Ok(0)`,
`LastOperationFailed` maps to an error. `write_all` and `flush` use
`blocking-write-and-flush`, which removes the hand-rolled
`check-write` / `subscribe` / `poll` loop. The adapter is synchronous by
design (RFC 024 D5 depends on it), so blocking calls are the honest
primitive.

### D3. WASI transport state is explicit

`Inner` gains a `Closed` variant. `upgrade_to_tls` takes the plaintext
stream out by replacing `inner` with `Closed`, performs the handshake,
and stores `Tls`; on handshake failure `inner` stays `Closed` and the
error is returned. `close` sets `Closed` after shutting down. `read`,
`write_all`, and `flush` on `Closed` return an `IoError`. The panicking
placeholder function is deleted.

### D4. Audit events are complete

- `RecipientRejected { code }` is emitted for a non-2xx `RCPT TO` reply
  before the error is returned, on both the sequential and pipelined
  paths.
- `SessionAborted` is emitted exactly once per session, from the single
  place that moves the state machine to `Closed` on failure. The I/O
  failure path in `fill_buf`, which currently sets `Closed` directly,
  goes through that same place. `quit` keeps its existing behavior.
- RFC 012 receives an amendment note.

### D5. One envelope implementation

The `MAIL FROM` → `RCPT TO` → `DATA` → `354` phase is factored into one
private method taking the already-formatted `MAIL FROM` line, the
recipient list, and the operation tag, containing the pipelined and
sequential branches. All four send methods call it; the SMTPUTF8
variant passes its own `MAIL FROM` formatter output. Wire output on
non-pipelining servers is unchanged. On pipelining servers the three
methods that previously ran sequentially now batch, which is the
behavior `send_mail` already had. Existing pipelining tests are
extended to the three other methods.

### D6. Adapters preserve error causes

- WASI: every `IoError::new(e.to_string())` becomes the existing
  `From<WasiSmtpError> for IoError`, which already uses `with_source`.
- Cloudflare: `worker::Error` wraps JavaScript values and is not
  `Send + Sync`, which `IoError::with_source` requires. If the
  implementer confirms that, the adapter keeps string messages and
  the reason is documented at the conversion site and in RFC 013's
  amendment note; if `worker::Error` does satisfy the bounds, use
  `with_source`. Either way the outcome is recorded, not assumed.

### D7. Dependency advisories in CI

A third CI job `audit` runs `cargo audit` (RustSec) on the lockfile.
Blocking: a new advisory against a pinned dependency should turn the
build red until addressed. RFC 010's text names `cargo deny`; the
amendment note records that the enforced tool is `cargo audit`, since
license checking is not in scope and one tool is enough.

### D8. Release

0.16.0. Minor rather than patch because of the new public function and
the wire-order change on pipelining servers, consistent with the
project's 0.x practice. Owner approval at release time; tag, push, and
publish are the owner's and the architect's.

## Security considerations

- The smoke test generates ephemeral key material per run and commits
  none. It listens on loopback only.
- D2 removes a path where a transient "no data yet" was reported as a
  closed connection; the failure mode was a spurious abort, not a
  downgrade, so no security property changes.
- D4 makes abuse-relevant failures (recipient rejections, aborted
  sessions) observable through the audit sink, which strengthens the
  RFC 011/012 posture.
- D7 enforces the advisory floor that RFC 010 has claimed since 0.5.0.

## Simplicity and maintainability considerations

D5 removes roughly three hundred lines of near-duplicate envelope code.
D2 removes a hand-written poll loop in favor of the primitive WASI
already provides. The smoke tool is a dev-only binary and adds no
published surface.

## Alternatives considered

**Test the Component Model export instead of a WASI example binary.**
More faithful, but needs `cargo component` in CI and a host harness
that calls the WIT export. Deferred; the example binary exercises the
identical adapter code, which is where the defects are.

**Fix defect 2 by polling before every read instead of using
`blocking-read`.** Works, but re-implements what the interface already
offers.

**Keep `SessionAborted` scattered per call site.** Rejected; a single
choke point is the only way to guarantee exactly one event.

## Implementation plan

See the handoff. Order: smoke harness first (expected red), then the
two WASI fixes (harness turns green), then audit, envelope
consolidation, error chains, advisory job, release commit.

## Acceptance criteria

- `cargo run -p wasm-smtp-smoke` passes on the pinned toolchain in CI
  for both implicit-TLS and STARTTLS modes.
- No `dummy_stream` or equivalent placeholder remains; STARTTLS on WASI
  completes in the smoke test.
- A unit test proves `RecipientRejected` is emitted on a 550 to
  `RCPT TO`, and that `SessionAborted` is emitted exactly once for an
  I/O failure and exactly once for a protocol failure.
- Pipelining tests cover all four send methods; wire output on a
  non-pipelining server is byte-identical to 0.15.2 for all four.
- `cargo audit` job exists and passes.
- Public API delta is exactly one new function in `wasm-smtp-wasi`.

## Open questions

None at acceptance. D6's Cloudflare outcome is an implementation
finding to be reported, not a design choice left open.
