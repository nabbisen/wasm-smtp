# RFC 032 — Verification coverage: what the gate does not yet reach

**Status.** Accepted (owner, 2026-09-13)
**Handoff.** [`../handoffs/032-verification-coverage/implementation-handoff.md`](../handoffs/032-verification-coverage/implementation-handoff.md)
**Priority.** P2
**Tracks.** Testing / CI / Documentation
**Touches.** `tools/smoke/` (or a new `tools/` crate), a new unpublished doctest crate under `tools/`, `docs/src/**` (code-fence annotations only), `tools/check-doc-versions.sh`, `tools/check-wasi-version.sh` and their fixtures, `.github/workflows/{ci.yml,docs.yml}`, `.github/CONTRIBUTING.md`
**Supersedes.** [RFC 031](../archive/031-self-tests-for-the-projects-checkers.md), folded in as D5 at the owner's direction on 2026-09-13.
**Origin.** The architect's review of tests, verification, and CI, requested by the owner on 2026-09-13 after RFC 028.

## Summary

The gate is honest about what it runs, and CI runs all of it. What it
does not run is the problem. The tokio adapter has never completed a
send over a socket in a test; 31 Rust code blocks in the book compile
nowhere; the advisory scan runs only when someone pushes; and the smoke
test's host is an unsupported wasmtime release that no scanner sees. Close those, fold in the guard self-tests RFC 031 proposed, and
fix the release order so that CI has passed on a commit before it is
tagged.

No published crate's API, dependencies, or built artifact changes.

## Motivation

The worst defects this project found in 2026 were found by **running
the thing on a path nobody had run**: the WASI STARTTLS panic, the WASI
read reporting live connections closed, and the WASI stream trapping
the guest on drop. Each had passed every test that existed. RFC 025 and RFC 028 each closed that gap for one runtime.
The review on 2026-09-13 measured what is left.

| Surface | Positive path, end to end, on a real connection | State |
|---|---|---|
| WASI adapter | yes, under wasmtime, both TLS modes | RFC 025 |
| Component | refusal only | positive path is RFC 030 |
| **tokio adapter** | **never** | 10 tests: options and error paths |
| Cloudflare adapter | through a tokio mock, not on workerd | out of scope here, see Non-goals |

The tokio adapter is the one a native Rust user picks. Its success path
— TLS handshake against a real certificate, STARTTLS upgrade in place,
authentication, dot-stuffed DATA, QUIT — is exercised only through the
core's mock transport, which by construction cannot see what the
adapter does with a socket.

Four further gaps, each small and each the kind that rots quietly:

- **The book's code is not compiled.** Of 54 Rust fences in
  `docs/src/`, 31 carry a plain `rust` annotation and so claim to be
  real code; nothing builds them, and 18 of those call the crate's API.
  RFC 029 guards version numbers in the book; nothing guards a renamed
  function. The docs workflow also runs only on pushes to `main`, so a
  book that fails to build is found after merge.
- **Advisories arrive between commits.** `cargo audit` runs on push and
  pull request. During a quiet period — RFC 022 and 023 are blocked on
  an external specification — an advisory against the lockfile is
  invisible until the next unrelated commit. The 10 MB body test is
  `#[ignore]`d for speed and runs nowhere at all.
- **The smoke test's host is unscanned and old.** The WASI smoke test
  installs the wasmtime **27.0.0** CLI — not an LTS release, and long out
  of support — as a downloaded binary outside `cargo audit`'s view. RFC 028 moved the component harness to the
  36.0.x LTS line because a test host should be trustworthy rather than
  convenient; the same argument applies to the other host.
- **Supply-chain hygiene in CI.** Cargo commands run without
  `--locked`; third-party actions are pinned by mutable tag;
  `cargo-audit` installs at whatever version is current.

And RFC 031's gap, unchanged: the two shell guards were each shown
failing once, by hand, in a review request, and those demonstrations
run nowhere. `check-doc-versions.sh`'s `mail-builder` branch has had no
live input since RFC 029's sweep.

Finally, a process gap the review exposed. `main` carries no branch
protection, so CI reports without enforcing, and the release sequence
has been: release commit, local gate, tag, push, publish. CI therefore
first saw each release commit **after** its tag existed. The local gate
is the same command list — but "the same list on another machine" is
exactly the claim a CI run exists to check, and it was checked only
after the fact.

## Goals

- The tokio adapter completes a send over loopback TLS in both modes,
  and refuses an untrusted certificate, under `cargo test --workspace`.
- Every plain `rust` fence in the book is compiled by the gate. A fence
  that cannot compile on the host says why, visibly.
- The advisory scan and the ignored tests run on a schedule.
- Both test hosts are on a maintained wasmtime line.
- Both shell guards have fixture tests covering every branch.
- A release commit is tagged only after CI has passed on that commit.

## Non-goals

- **Cloudflare on-target execution.** Running under workerd or Miniflare
  brings Node into CI. That is a cost the owner should weigh on its own,
  and it gets its own RFC if pursued.
- **Fuzz or property tests of the parsers.** Worth doing — replies, EHLO
  capabilities, and SCRAM server messages are server-controlled bytes —
  but a separate decision; `cargo fuzz` needs nightly, which the pinned
  gate does not have.
- **The component's positive path.** RFC 030.
- **Branch protection.** A repository setting, and the owner's. This RFC
  makes the release order correct without it (D6); protection would
  make it enforced.
- Any change to a published crate's manifest. Test crates that need the
  adapters depend on them from `tools/`, not the other way round.

## Design

### D1. tokio adapter end to end, over loopback

A test that binds `127.0.0.1:0`, serves the scripted responder from
`tools/smoke`'s library on a `std::thread`, and drives
`wasm-smtp-tokio` against it. Three cases, matching RFC 025's smoke
modes:

- implicit TLS: success;
- STARTTLS: success, with the upgrade happening on the same socket;
- implicit TLS with the run's CA **not** trusted: refused before any
  SMTP command is written.

Trust is supplied through `ConnectOptions::with_root_store`, which is
the public API a private-CA user would call, so the test also covers
that path. Assertions are the adapter smoke test's, not fewer: command
sequence in order, `AUTH PLAIN` present, the dot-stuffed body on the
wire, and the reply code the responder sent.

**Placement.** An integration test in `tools/smoke/tests/`, with
`wasm-smtp-tokio` as a dev-dependency there, so it runs under
`cargo test --workspace` without a new gate command and without
touching a published manifest. **Constraint:** the responder is built
on `ring`, and the adapter must be depended on with
`default-features = false, features = ["webpki-roots", "ring"]` so the
test binary compiles one provider — the unambiguous-provider rule of
RFC 024 D9 applies to test binaries too.
*(Superseded by amendment A1: this configuration cannot build in the
workspace, and the rule it cites is not what D9 requires.)*

### D2. The book's code is compiled

An unpublished crate under `tools/` whose library contains, under
`#[cfg(doctest)]`, one item per chapter:

```rust,ignore
#[cfg(doctest)]
#[doc = include_str!("../../../docs/src/core/usage.md")]
struct CoreUsage;
```

`cargo test --workspace` then compiles every fence in those chapters as
a doctest. mdBook and rustdoc share the `# ` hidden-line convention, so
setup lines can be hidden from readers without leaving the book.

**Why a tools crate, not `wasm-smtp` itself.** The chapters live outside
every published crate's package directory. `include_str!` of a path
outside the package is harmless under `cfg(doctest)` today and a trap
the day someone removes the `cfg`. The tools crate can also enable the
features the chapters use without affecting anyone.

**Fences that cannot compile on the host.** Cloudflare chapters use the
`worker` crate and WASI chapters use `wasm32-wasip2`-only APIs; neither
builds as a host doctest. Those fences become `rust,ignore`, and each
chapter holding one says so in a sentence beside it, naming the compiled
example that covers it on the real target (`contact_form_turnstile.rs`,
`smoke.rs`). An `ignore` with no such sentence is a review failure.

**Coverage of new chapters.** A chapter added later and not listed in
the crate would silently escape. A test in the same crate enumerates
`docs/src/**/*.md`, finds those with any `rust` fence, and fails naming
each one not included. That makes "every plain fence compiles" a
property of the tree, not of this sweep.

**The docs workflow on pull requests.** `docs.yml` gains a
`pull_request` trigger for the **build** job only; deploy stays on push
to `main`. The concurrency group must not let a pull-request build
cancel a deployment.

### D3. Scheduled verification

A weekly `schedule` trigger, plus `workflow_dispatch`, running:

- `cargo audit`;
- `cargo test --workspace -- --include-ignored`, so the 10 MB body test
  runs somewhere.

With `permissions: contents: read`. Two operational facts to record in
the workflow comment, because both surprise people: GitHub notifies the
user who last changed the cron line, not the repository; and on a public
repository it **disables scheduled workflows after 60 days without
repository activity** — which is precisely the quiet period this job is
for. The handoff should confirm current GitHub behaviour and say what,
if anything, the project does about it; it may be only a note in
`CONTRIBUTING.md`.

### D4. CI hygiene

- **wasmtime CLI** for the WASI smoke test: 27.0.0 → the 36.0.x LTS line,
  the same line as the component harness, unless the action cannot
  install it — in which case the handoff records why and picks the
  newest maintained line it can. The pin comment explains, as the
  harness's manifest does, that it is chosen for backports.
- **`--locked`** on the gate's `cargo` build, test, and run commands, so
  CI tests the committed lockfile rather than one it resolved.
- **`cargo-audit`** installed at an exact version.
- **Third-party actions** (`Swatinem/rust-cache`,
  `bytecodealliance/actions`, `peaceiris/actions-mdbook`) pinned by
  commit SHA with the version in a trailing comment. First-party
  `actions/*` may stay on major tags. Keeping SHAs current is open
  question 1.

### D5. Self-tests for the shell guards *(from RFC 031)*

Both guards take a root directory, so they can be pointed at fixture
trees. Each fixture is a minimal tree plus expected exit code and
stdout; a runner compares both.

Cases, at minimum:

- `check-doc-versions.sh`: clean tree; stale own-crate version; stale
  `mail-builder` version; correct `mail-builder` version; a
  commented-out stale line (must fail — a reader uncomments it); an
  unreadable manifest version (exit 2).
- `check-wasi-version.sh`: clean tree; `wasip2` bumped in the lock (every
  site reported); one site left behind on a partial update; `wasip2`
  absent from the lock (exit 2, never a pass).

The runner is a shell script the gate calls or a Rust test under
`tools/` that shells out; the handoff picks whichever makes a failing
expected-output comparison easier to read in a diff, since that is what
someone debugging it will look at.

`check-doc-versions.sh`'s header records its deliberate exclusions
(`CHANGELOG.md`, `rfcs/`) and that the tokio chapter's TOML block is
intentional live coverage, so neither reads as an oversight.

### D6. Release order

The release sequence becomes: release commit → full local gate →
**push the release commit alone** → CI passes on that exact commit →
tag → publish from the tag.

If later commits exist locally, push the release commit by hash
(`git push origin <hash>:main`) so CI's push run is on the commit being
released, not on a descendant. A red CI run stops the release before a
tag exists, and nothing needs un-tagging.

This is recorded in `.github/CONTRIBUTING.md` beside the required checks
in one short paragraph. It takes effect for 0.17.2, which predates this
RFC's acceptance, as a practice the architect adopts now.

## Amendment — 2026-09-13, after review 1

Three premises in this RFC and its handoff were wrong, and each was
found by the implementer **running** something the architect had only
reasoned about. They are recorded here, not corrected silently.

**A1. D1's provider constraint.** `default-features = false, features =
["webpki-roots", "ring"]` on an in-workspace dependency of
`wasm-smtp-tokio` cannot build: Cargo unifies a package's features across
a workspace run, the adapter is also selected at its defaults, and both
mutual-exclusion `compile_error!`s fire. The dev-dependency takes the
adapter's defaults. RFC 024 D9's actual rule — no configuration relies
on rustls's process-wide default provider — still holds, because both
sides of the test name their provider explicitly, and the test checks it
on every run.

**A2. D2's host-impossibility of the Cloudflare fences.** The Cloudflare
adapter and `worker` build on the host; only a Workers *runtime* does
not exist there. Its code fences compile as doctests like any others.

**A3. D2 assumed every `rust` fence was either correct code or
host-impossible.** The inventory found two more kinds. The rules for
them:

- **A wrong fence** is a documentation defect. It is fixed as its own
  reviewed change before D2 compiles it. Three were found.
- **A listing** (re-exports, a call sequence, signatures) is compiled
  wherever minimal visible changes allow it. A signature listing with no
  bodies cannot compile without ceasing to be a listing, so it becomes
  `rust,ignore` with a sentence saying so. That is the only `ignore`
  permitted apart from host-impossible code.

**A4. D2 feature unification.** The book crate is a workspace member.
Whatever features it enables on `wasm-smtp` or an adapter apply to every
`cargo test --workspace` build, so enabling them silently changes what
the core's own tests compile against. The crate must leave the
workspace's feature resolution unchanged, measured before and after. If
it cannot, the choice between excluding it from the workspace and
another shape is the architect's.

## Amendment — 2026-09-13, after review 2

**A5. How the book crate avoids A4.** A measurement showed that exactly
one fence needs a non-default feature (`smtputf8`, for
`send_mail_smtputf8`), so a book crate compiling everything would change
the workspace's resolution. The resolution is a workspace member with a
**non-default** feature, `book = ["wasm-smtp/smtputf8"]`. Every chapter
include is `#[cfg(all(doctest, feature = "book"))]`, and a dedicated gate
command compiles them:

```
cargo test --locked -p wasm-smtp-book --features book
```

`cargo test --workspace` never enables a member's non-default feature,
and `-p` scopes that command's unification to the book crate's graph.
The architect verified each property on 1.88 in a scratch workspace
before ruling: chapters are off in the workspace run and on in the
dedicated one, drift in a chapter fails only there, and neither the
optional feature nor the dedicated run changes later workspace
resolution. This replaces D2's "`cargo test --workspace` then compiles
every fence": the dedicated command does.

**A6. The core's feature-gated tests.** The same measurement showed the
gate never *runs* the core's tests behind `smtputf8`, `mail-builder`, or
`tracing`: gate command 5 was `cargo check`. On 2026-09-13 that was 30
unit tests (309 with the features against 279 without), all passing,
none ever run by the gate. Command 5 becomes

```
cargo test --locked -p wasm-smtp --features smtputf8,mail-builder,tracing
```

which compiles everything the `check` did and then runs it, with `-p`
keeping the features out of the workspace run.

## Amendment — 2026-09-13, after review 3

**A7. Stale `ignore`, and permissions that followed a new trigger.** Two
follow-ups from review 3, both small:

- Four fences marked `rust,ignore` already compile. They carry hidden
  wrappers and were written to compile. They become `rust,no_run`. The
  rule stays as A3 wrote it: `ignore` is for host-impossible code and
  signature listings only.
- D2 made `docs.yml` run on pull requests, so the workflow-level
  `pages: write` and `id-token: write` now reach pull-request builds that
  use neither. They move to the job that deploys, as
  `actions/deploy-pages` documents; the workflow keeps `contents: read`.

## Gate changes

The gate command list is the architect's artifact (RFC 024 D3). D1, D2,
and a Rust-runner D5 add tests to `cargo test --workspace` and need no
new command. A shell-runner D5 adds one. D4 changes flags on existing
commands. D3 is a separate scheduled job, not part of the per-push gate.
The handoff states the resulting list.

## Security considerations

- D1 generates its certificate per run, as the smoke test does; no key
  material is committed. The refusal case matters as much as the
  success cases: a test that only proves the adapter *can* connect would
  pass against an adapter that trusts everything.
- D4 narrows who can change what CI executes: a moved tag on a
  third-party action can no longer change the gate silently.
- D3 runs with read-only repository permissions.
- No change to any shipped TLS, credential, or audit code.

## Release

Nothing a consumer receives changes. The book changes only in fence
annotations and hidden lines, and deploys on push without a crate
release. This can land between releases or ride along with one.

## Resolution of the open questions

Settled at acceptance as architect defaults, which the owner may
override at any point before the handoff's slice lands:

1. **No Dependabot.** Third-party action SHAs are bumped by hand, as a
   visible commit. Revisit if the project moves to pull requests.
2. **Convert, but inventory first.** The handoff's D2 slice counts the
   fences needing hidden setup before converting any; above the
   threshold it names, it stops and reports.
3. **Weekly.**

## Open questions (as proposed)

1. **Keeping action SHAs current.** Dependabot for `github-actions`
   opens pull requests, which is a workflow change for a project that
   pushes directly to `main`. The alternative is a periodic manual bump.
   The owner's call.
2. **D2 cost.** If more than a handful of fences need substantial hidden
   setup to compile, the handoff should report the count before
   converting them, so the owner can choose between compiled chapters
   and fewer, fuller examples.
3. **D3 cadence.** Weekly is proposed; RustSec publishes irregularly and
   a daily run costs little.
