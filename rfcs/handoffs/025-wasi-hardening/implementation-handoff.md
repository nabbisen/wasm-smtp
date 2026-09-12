# Developer Handoff — RFC 025: WASI hardening and on-target verification

**Governing RFC.** [`../../accepted/025-wasi-hardening-on-target-verification.md`](../../accepted/025-wasi-hardening-on-target-verification.md)
**Target release.** 0.16.0. Release approval is the owner's; this handoff produces the release commit only.
**Prepared.** 2026-09-12 by the architect. Baseline: `356f05c` (0.15.2 plus documentation commits).
**Review request goes to.** `.git-exclude/review-request/025-wasi-hardening.md`

---

## 1. Purpose

Run the WASI adapter on a real `wasm32-wasip2` host for the first time,
fix what that exposes, and close the audit, pipelining, error-chain,
and advisory gaps listed in RFC 025 §Motivation.

## 2. Applicable design

RFC 025 D1–D8. Also RFC 005 (transport contract: `Ok(0)` means clean
EOF and nothing else), RFC 012 (audit events carry no addresses,
bodies, or credentials), RFC 024 D3 (the gate) and D9 (explicit crypto
provider). Project rules: English; tests separated from implementation;
no `unsafe`.

## 3. Change scope

- `tools/smoke/` (new crate `wasm-smtp-smoke`, `publish = false`),
  added to `[workspace.members]`.
- `crates/wasm-smtp-wasi/`: `Cargo.toml` (an `[[example]]` needs no
  entry; add `rcgen`/`rustls-pemfile`-style dev-dependencies only if
  the example needs them), `examples/smoke.rs` (new), `src/lib.rs`
  (one new public function), `src/wasi_impl/stream.rs`,
  `src/wasi_impl/transport.rs`, `src/tests.rs`.
- `crates/wasm-smtp/src/client/{mod.rs,io.rs,send.rs}`,
  `src/audit.rs` (docs only), `src/tests/{audit_tests,pipelining_tests,client_tests}.rs`.
- `crates/wasm-smtp-cloudflare/src/{adapter.rs,socket.rs}` — error
  construction only.
- `.github/workflows/ci.yml`, `.github/CONTRIBUTING.md`.
- `rfcs/done/010-*.md`, `rfcs/done/012-*.md`, `rfcs/done/013-*.md` —
  amendment notes under the Status line only.
- `CHANGELOG.md`, `ROADMAP.md`, `docs/src/adapters/wasi.md`,
  `docs/src/core/policy-audit.md`, `docs/src/concepts/protocol.md`.

## 4. Non-change scope

- No change to `wit/`, to any existing public signature, or to any
  error variant. Exactly one new public item:
  `wasm_smtp_wasi::connect_smtp_starttls_with`.
- No new dependencies in published crates beyond what D1 strictly
  needs for the example; `rcgen` and any TLS-server helper live in the
  unpublished smoke tool or in `[dev-dependencies]`.
- No `--all-features`. No blanket lint allows. No `unsafe`.
- Do not tag, push, or publish. Stop at the release commit.
- Do not touch `rfcs/README.md`.

## 5. Slices, in order

### S1. Smoke harness (D1) — expected to fail at first

1. `tools/smoke/Cargo.toml`: `name = "wasm-smtp-smoke"`, binary,
   `publish = false`, `[lints] workspace = true`. Dependencies:
   `rustls` (workspace line: 0.23, `ring`), `rcgen`, `rustls-pki-types`;
   nothing async is required — `std::net` threads are fine.
2. `tools/smoke/src/main.rs`:
   - Generate a `localhost` certificate and key with `rcgen`; write
     the CA PEM to a temp dir.
   - Start an implicit-TLS responder and a STARTTLS responder on
     `127.0.0.1:0`, each on its own thread, each replaying a scripted
     session (`220`, `250-…\r\n250-AUTH PLAIN\r\n250 …` with
     `STARTTLS` advertised on the plaintext one, `235`, `250`, `250`,
     `354`, `250 2.0.0 OK: queued as SMOKE0001`, `221`) and recording
     every received line and the DATA body verbatim.
   - For each mode, run `wasmtime run --wasi inherit-network --wasi allow-ip-name-lookup <guest> <mode> localhost <port> <ca.pem>`
     where `<guest>` is
     `target/wasm32-wasip2/debug/examples/smoke.wasm` (accept an
     override via `SMOKE_GUEST` for CI layouts).
   - Assert: guest exit 0; the recorded command sequence in order; the
     body contains `..leading-dot` for an input line `.leading-dot`;
     for STARTTLS, `STARTTLS` then a second `EHLO`; the STARTTLS
     responder saw the second `EHLO` over TLS, not plaintext.
   - Print a one-line PASS/FAIL per mode; exit non-zero on any FAIL.
3. `crates/wasm-smtp-wasi/examples/smoke.rs`: `#![cfg(target_arch = "wasm32")]`
   body (a stub `main` on other targets so `cargo check` stays clean),
   parses args, builds `ConnectOptions::default().with_root_store(store)`
   from the PEM, calls `connect_smtps_with` or
   `connect_smtp_starttls_with`, `login_with(AuthMechanism::Plain, …)`,
   `send_mail` with a body that includes a line starting with `.`,
   `quit`. Errors go to stderr via `Display`; exit 1.
4. `crates/wasm-smtp-wasi/src/lib.rs`: add
   `pub async fn connect_smtp_starttls_with(host, port, ehlo_domain, options: ConnectOptions)`
   and make the existing `connect_smtp_starttls` delegate to it. This
   requires `WasiTlsTransport::connect_plain` to accept options; keep
   the change local.
5. CI: a step installing wasmtime (`bytecodealliance/actions/wasmtime/setup@v1`),
   the example build, and `cargo run -p wasm-smtp-smoke`, added to the
   `gate` job after the packaging check. Mirror in CONTRIBUTING.
6. Run it. Record the failure output in the review request; the
   expected failures are the two defects S2 and S3 fix. If it fails in
   some other way, stop and report before touching the adapter.

### S2. Blocking stream primitives (D2)

`stream.rs`: `read` → `blocking_read`; `write_all` → loop over
`blocking_write_and_flush` in chunks no larger than the interface
allows (4096 bytes per call in WASI 0.2); `flush` becomes a no-op
comment or a final `blocking_flush`. Map `Closed` → `Ok(0)` on read and
to an error on write; map `LastOperationFailed(e)` to
`WasiSmtpError` with the error's debug text.

### S3. Explicit transport state (D3)

`transport.rs`: `Inner::Closed`; `upgrade_to_tls` replaces with
`Closed`, handshakes, stores `Tls` on success; `close` sets `Closed`;
I/O on `Closed` returns `IoError::new("transport is closed")`. Delete
`dummy_stream`. Add a native unit test that `upgrade_to_tls` on an
already-TLS transport returns an error rather than panicking, using the
existing mock pattern in `tests.rs` if the type can be constructed there;
if it cannot on native, say so and rely on the smoke test.

After S2 and S3 the smoke test must pass in both modes.

### S4. Audit completeness (D4)

- `client/mod.rs`: `mark_closed_on_logical_failure` emits
  `SessionAborted` if and only if the state was not already `Closed`.
  `io.rs::fill_buf` calls it instead of assigning `Closed` directly.
- `send.rs` (after S5, or before; your choice, but S5 must keep it):
  on a non-2xx `RCPT TO` reply emit `RecipientRejected { code }` before
  returning.
- Tests in `audit_tests.rs`: 550 on RCPT → `RecipientRejected` then
  `SessionAborted`, exactly once each; I/O failure mid-session →
  exactly one `SessionAborted`; successful session → zero.
- `docs/src/core/policy-audit.md`: event list matches reality.
- RFC 012 amendment note: "Amended by RFC 025 D4 (0.16.0):
  `RecipientRejected` and `SessionAborted` emission defined."

### S5. One envelope implementation (D5)

Private `async fn run_envelope(&mut self, mail_from_line: &[u8], to: &[&str]) -> Result<(), SmtpError>`
in `send.rs` holding the pipelined/sequential branches, the audit
events, and the state transitions through `Data`. All four send
methods call it and then do their own body write and final reply.
Extend `pipelining_tests.rs` with the three other methods (wire order
and single-write assertion), and add a non-pipelining wire-equality
test for each against 0.15.2's recorded output.

### S6. Error causes (D6)

WASI: use `IoError::from(wasi_err)` everywhere in `transport.rs`.
Cloudflare: check whether `worker::Error: Send + Sync + 'static`
(try `IoError::with_source`); if it compiles, use it; if not, keep
the string form and add a comment at the conversion site stating the
bound that fails. Record the outcome in the review request. RFC 013
amendment note either way.

### S7. Advisory job (D7)

`audit` job in `ci.yml`: `rustsec/audit-check` or
`cargo install cargo-audit --locked && cargo audit`. Blocking. RFC 010
amendment note: "Amended by RFC 025 D7 (0.16.0): the enforced
advisory check is `cargo audit` in CI, not `cargo deny`." Run it once
locally and include the output; if it reports an advisory today, do
not silence it — report it.

### S8. Release commit

Bump workspace and pins to `0.16.0`; CHANGELOG `[0.16.0]` with
Compatibility notes (wire-order change on pipelining servers; new
events emitted; new function); ROADMAP Phase 19; docs updates; full
gate (now thirteen commands plus the audit job) on the pinned
toolchain; evidence under `.git-exclude/review-request/evidence/025/`;
commit "Release 0.16.0"; stop.

## 6. Required tests

Listed per slice above. Plus: the whole existing suite unchanged and
green; no test removed or newly ignored.

## 7. Acceptance criteria

RFC 025 §Acceptance criteria, verbatim.

## 8. Prohibited shortcuts

- Making the smoke test pass by weakening its assertions or by
  scripting the responder to tolerate a wrong client sequence.
- Fixing D2 with sleeps or retry counts instead of the blocking
  primitives.
- Emitting `SessionAborted` from more than one place.
- Silencing `cargo audit`.

## 9. Known risks

| Risk | Mitigation |
|---|---|
| wasmtime version drift changes CLI flags | pin the setup action to a wasmtime version; record it in CI |
| `localhost` resolves to `::1` first and the responder listens on IPv4 only | listen on both, or have the guest connect to `127.0.0.1` with a certificate carrying an IP SAN; state which you chose |
| rustls handshake inside `tls_handshake` may need a `blocking` read to complete | the smoke test will show it; fix within `tls_handshake`, not in the core |
| S5 refactor changes wire output unintentionally | the byte-equality tests against 0.15.2 output are mandatory |

## 10. Review request contents

Same structure as the RFC 024 requests: summary per slice, changed
files, decisions, deviations, gate outputs with the smoke tool's full
log, the S6 Cloudflare finding, unresolved items, review focus.
