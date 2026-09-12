# Developer Handoff — RFC 032: Verification coverage

**Governing RFC.** [`../../accepted/032-verification-coverage.md`](../../accepted/032-verification-coverage.md) — read §Resolution of the open questions; all three were settled at acceptance.
**Prepared.** 2026-09-13 by the architect. Baseline: the commit accepting RFC 032, on top of `bd1c121` (Release 0.17.2).
**Starts.** After 0.17.2 is tagged and published. Do not branch from, amend, or rebase anything at or before `bd1c121`.
**Target release.** None required. No published crate's API, dependencies, or artifact changes; the work lands on `main` and rides with whatever release comes next. Say in the review request if you find a reason that is not true.
**Review request goes to.** `.git-exclude/review-request/032-verification-coverage.md`

## 1. Purpose

Make the gate reach what it does not yet: the tokio adapter's success
path over a real socket, the book's code, advisories between commits,
and the two shell guards' own behaviour — and put CI's pass before the
release tag.

## 2. What is already known, so you do not re-derive it

Verified by the architect at `62e5ab9`. Check anything you rely on.

- The shared responder is a library (`wasm_smtp_smoke`: `serve_implicit`,
  `serve_starttls`, `generate_cert`, `server_config`, `Recording`, `Leg`).
  It is synchronous, on `std::net::TcpStream`, and built on `ring`.
- **The transcript assertions are not in that library.** They are in
  `tools/smoke/src/main.rs` (`run_mode` and what it calls), and
  `tools/component-smoke/src/main.rs` has its own `check`. D1 cannot
  reuse them without moving them first. That is S2's first step.
- `wasm-smtp-tokio` refuses two crypto providers at compile time and
  defaults to `aws-lc-rs`. `ConnectOptions::with_root_store` exists.
- Both guards take an optional root directory as `$1`, so fixture trees
  need no guard changes. On an empty tree both exit 2, but on different
  files: `check-doc-versions.sh` requires `Cargo.toml`,
  `check-wasi-version.sh` requires `Cargo.lock`. Each fixture needs the
  file its guard reads.
- The book has 54 Rust fences: 31 `rust`, 18 `rust,ignore`, 5
  `rust,no_run`. The `rust` fences that name a crate are in
  `core/{usage,policy-audit,streaming}.md`, `adapters/{cloudflare,wasi}.md`,
  and `reference/examples.md`. Cloudflare's use `worker` and cannot
  build on the host.
- `docs.yml` triggers on push to `main` and `workflow_dispatch` only.
- The WASI smoke test installs the wasmtime CLI with
  `bytecodealliance/actions/wasmtime/setup@v1`, `version: "27.0.0"`.

## 3. Change scope

`tools/smoke/{src/lib.rs,src/main.rs,Cargo.toml,tests/**}`;
`tools/component-smoke/src/main.rs` (only to use the moved assertions);
one new unpublished crate under `tools/` for D2; `tools/check-*.sh`
headers, and fixtures and a runner for D5; `docs/src/**` fence
annotations, hidden `# ` lines, and the one-sentence notes D2 requires;
`.github/workflows/{ci.yml,docs.yml}`, and a new scheduled workflow or
job; `.github/CONTRIBUTING.md`; `CHANGELOG.md` under `[Unreleased]`;
`Cargo.lock` only for the new tool crates.

## 4. Non-change scope

- No library source in `crates/**`. No change to any published crate's
  manifest — tests needing an adapter depend on it from `tools/`.
- No change to what any book chapter *says*. Annotations, hidden setup
  lines, and D2's notes only. If a fence is wrong, not merely
  uncompiled, stop and report it: that is a documentation defect, and
  its fix is reviewed as one.
- No guard behaviour change for D5. If a fixture shows a guard is
  wrong, report it before changing the guard.
- No nightly toolchain, no fuzzing, no Node, no Cloudflare runtime.
- No Dependabot. No branch protection or repository settings.
- No `--all-features`. Do not tag, push, or publish. Do not edit
  `rfcs/README.md`.

## 5. Slices, in order

Each slice ends with the full gate green and its own commit.

### S1. Guard self-tests (D5)

1. Fixtures for every case in RFC 032 D5 — six for
   `check-doc-versions.sh`, four for `check-wasi-version.sh` — each a
   minimal tree plus expected exit code and expected stdout.
2. A runner. Shell or Rust test under `tools/`: choose on which prints
   the more readable diff when expected and actual stdout differ, and
   paste one such failure in the review request.
3. Every branch of each guard reached by at least one fixture. List
   the branches and the fixture that reaches each.
4. `check-doc-versions.sh` header: its deliberate exclusions
   (`CHANGELOG.md`, `rfcs/`), and that the tokio chapter's TOML block
   is intentional live coverage.
5. **Demonstrate failure:** alter one comparison in each guard locally
   (for example, compare the full version instead of major.minor) and
   show the runner going red on a named fixture. Revert; do not commit
   the alteration.

### S2. tokio adapter end to end (D1)

1. Move the transcript assertions from `tools/smoke/src/main.rs` into
   the library, unchanged in what they assert. Point the WASI smoke
   binary at them, and the component harness too where it asserts the
   same things. **Run adapter smoke in all four modes and the component
   harness before going further:** a refactor of a checker is exactly
   where a checker quietly stops checking.
2. `tools/smoke/tests/tokio_adapter.rs`, with `wasm-smtp-tokio` as a
   dev-dependency at `default-features = false, features =
   ["webpki-roots", "ring"]`. Confirm with `cargo tree -e features -i
   rustls` that the test binary has one provider, and paste the line.
3. Three tests — implicit success, STARTTLS success, implicit refused —
   using the moved assertions. Trust via `with_root_store` holding only
   the run's CA. The refused case asserts that no SMTP command reached
   the responder, not merely that an error came back.
4. **Demonstrate failure:** give the refused case the run's CA. It must
   go red. Revert.

### S3. CI hygiene and scheduled verification (D4, D3)

1. wasmtime CLI to the 36.0.x LTS line. **Run adapter smoke locally in
   all four modes under that exact CLI** before changing CI, and paste
   `wasmtime --version`. If the action cannot install it, say so and
   stop on this item.
2. `--locked` on the gate's `cargo build`, `test`, `run`, and `check`
   commands in CI and in `CONTRIBUTING.md`, both. `cargo fmt` and
   `cargo package --list` do not need it; say if you find otherwise.
3. `cargo install cargo-audit --locked --version <exact>`, the current
   release, recorded in a comment.
4. Third-party actions by full SHA with `# vX.Y.Z` trailing:
   `Swatinem/rust-cache`, `bytecodealliance/actions/wasmtime/setup`,
   `peaceiris/actions-mdbook`. Resolve each SHA from the tag with
   `git ls-remote`, and list tag → SHA in the review request so the
   review can check them independently. First-party `actions/*` stay on
   major tags.
5. Scheduled run: weekly `schedule` plus `workflow_dispatch`,
   `permissions: contents: read`, running `cargo audit` and `cargo test
   --workspace --locked -- --include-ignored`. Separate workflow file or
   a job in `ci.yml` guarded by event — your call, argued.
6. Confirm GitHub's **current** documentation on (a) who is notified of
   a scheduled-run failure and (b) automatic disabling of scheduled
   workflows after inactivity. Quote the lines and link them in the
   review request. Record both in the workflow comment. Add a sentence to
   `CONTRIBUTING.md` only if (b) still applies.
7. `workflow_dispatch` cannot be demonstrated locally. Say so; the
   architect will dispatch it once after the push.

### S4. The book's code is compiled (D2)

1. **Inventory before converting anything.** For each of the 31 `rust`
   fences: chapter and line, whether it compiles as a doctest as it
   stands, and if not, how many hidden `# ` lines it needs or whether it
   is host-impossible. Put the table in the review request.
2. **Threshold.** If more than **eight** fences need more than **five**
   hidden lines each, stop after the inventory, commit nothing for S4,
   and report. The owner chooses between compiled chapters and fewer,
   fuller examples (RFC 032, resolution 2). Otherwise continue.
3. The unpublished crate — name it `wasm-smtp-book` or similar, `publish
   = false` — with one `#[cfg(doctest)] #[doc = include_str!(…)]` item
   per chapter holding a Rust fence, and the features those chapters
   need. The tokio chapter's adapter dependency follows S2's provider
   rule.
4. Host-impossible fences become `rust,ignore`, each with a sentence
   next to it in the chapter naming the example that compiles that code
   on its real target. Leave existing `rust,ignore` fences alone unless
   they now compile, in which case list them and ask; do not convert
   them unilaterally.
5. A test in the same crate: walks `docs/src/**/*.md`, finds files with
   any `rust` fence, and fails naming each not included by the crate.
6. `docs.yml`: `pull_request` trigger on the build job only; deploy
   stays push-to-`main`. The `pages` concurrency group must not be
   shared by pull-request builds, so a PR cannot cancel a deployment.
   `mdbook build docs` still clean.
7. **Demonstrate failure, three ways:** rename an API a compiled fence
   uses — red; remove one chapter from the crate — the coverage test
   names it; add a scratch chapter with a `rust` fence — the coverage
   test names it. Revert all three.

### S5. Release order and the gate list (D6)

1. `CONTRIBUTING.md`: one short paragraph beside the required checks
   stating RFC 032 D6's order. Contributors do not release, so keep it
   to what they need to know: a release commit is tagged only after CI
   has passed on that exact commit.
2. Update the command list in `CONTRIBUTING.md` so it is again exactly
   what the `gate` job runs, in order. State the resulting list in the
   review request, numbered, marking every added or changed command.
   The list is the architect's artifact: propose, do not present as
   settled.
3. `CHANGELOG.md` under `[Unreleased]`: one entry per decision, in terms
   a contributor would care about. No version bump.

## 6. Acceptance criteria

- `cargo test --workspace` runs the three tokio adapter cases and the
  book doctests and the coverage test; S1's runner runs in the gate.
- The adapter smoke test (four modes) and the component harness assert
  what they asserted before S2 moved their checks.
- Every `rust` fence compiles, or is `ignore` with its note; the
  coverage test would catch a new chapter.
- Both test hosts on the wasmtime 36.0.x line, or the reason not.
- Third-party actions SHA-pinned; `cargo-audit` pinned; `--locked`
  applied.
- A weekly scheduled run exists with read-only permissions.
- Every demonstration in §5 performed and pasted.
- No `crates/**` source or published manifest changed.

## 7. Prohibited shortcuts

- Marking a fence `ignore` because converting it is tedious. `ignore` is
  for host-impossible code, with its note.
- Asserting "an error was returned" in the refused case. The property is
  that nothing was sent.
- Rewriting a guard so a fixture passes.
- Resolving an action SHA from anywhere but the upstream repository's
  tag.
- Weakening an assertion while moving it in S2.
- Combining slices into one commit.

## 8. Review request contents

In order: S1's branch-to-fixture table and one pasted failing diff;
S2's provider line, and before/after smoke output in all four modes plus
the component harness; S3's `wasmtime --version`, tag → SHA table, and
the quoted GitHub documentation; S4's inventory table first, then either
the stop report or the conversion, and all three failure
demonstrations; the proposed gate list, numbered, with changes marked;
changed files; full gate results per slice.

---

# Revision 2 — 2026-09-13, after review 1

Review: `.git-exclude/reviewed/032-verification-coverage-review-1.md`.
S1, S2, S3, and S5 are approved. The 21-command gate list is accepted.
Read RFC 032's **Amendment** first: A1–A4 correct premises this handoff
got wrong, including §2's claim that the Cloudflare fences cannot build
on the host.

What remains is S4, now in two parts. Nothing else in §3–§4 changes,
except that the fence fixes below are in scope.

## S4a — Fix the three wrong fences

One commit, before any D2 work.

1. `concepts/errors.md:160` (fence 8): each arm gets a block body holding
   its comment, `=> { /* retry later */ }`, so the pattern reads the
   same and compiles.
2. `core/policy-audit.md:18` (fence 12): import `PolicyError` from the
   crate root, `wasm_smtp::PolicyError`.
3. `core/usage.md:336` (fence 29): add the missing
   `Err(wasm_smtp::SmtpError::Policy(p))` arm, with a comment in the
   same register as the others: the caller's own policy refused the
   message, and it is not retryable as-is.
4. Every other visible line stays as it is. Show each fix compiling in
   the scratch crate from your inventory.

## S4b — Compile the book (D2), under the amended rules

1. **Measure before adding anything.** Record, on the current tree:
   `cargo tree --workspace -e features -i wasm-smtp`, and the same for
   `wasm-smtp-cloudflare`, `wasm-smtp-tokio`, and `wasm-smtp-wasi`. Add the
   book crate and record them again. **If any output differs, stop,**
   commit nothing for S4b, and report which fences need which features.
   Do not narrow the crate's features to make the outputs match if that
   leaves fences uncompiled. That trade is the architect's (RFC 032 A4).
2. Fences: the 6 that compile as written stay as they are; the 17 get
   hidden lines as inventoried; Cloudflare fences 2 and 3 compile; the 2
   WASI fences become `rust,ignore`, with a note that the same connection
   path is compiled for `wasm32-wasip2` by the gate in
   `crates/wasm-smtp-wasi/examples/smoke.rs`; fence 16 becomes `no_run`.
3. Listings (RFC 032 A3):
   - `core/core.md:9` restated as `use wasm_smtp::{…};` naming the same
     items, so it compiles. Nothing else in that section changes.
   - `core/core.md:72` compiled with hidden setup, with visible changes
     limited to what compiling needs (`let mut client =`).
   - `adapters/cloudflare.md:39` becomes `rust,ignore`, preceded by one
     sentence saying it lists signatures rather than a program and that
     the crate's API documentation is authoritative.
4. The crate, its coverage test, the `serde`/`derive` dependency for
   `reference/examples.md:122`, and the `docs.yml` pull-request trigger
   with its separate concurrency group: as in original S4.3, S4.5, and
   S4.6.
5. Remove `CHANGELOG.md`'s `### Not in this release` and describe what
   landed.
6. **Demonstrate failure** as in original S4.7, plus one more: undo
   S4a's fix to fence 29, and the doctest must go red with `E0004`.
   Revert.

## Review request

`.git-exclude/review-request/032-verification-coverage-2.md`: the
before/after `cargo tree` outputs (or the stop report), S4a's three
diffs, the fence-by-fence result against the inventory table, the four
failure demonstrations, changed files, and the full gate.

---

# Revision 3 — 2026-09-13, after review 2

Review: `.git-exclude/reviewed/032-verification-coverage-review-2.md`.
S4a is approved. Read RFC 032 amendments **A5** and **A6** first. S4b
resumes under them; Revision 2's S4b steps still apply except where this
section replaces them.

## S4c — Run the core's feature-gated tests (A6)

Its own commit, before S4b. It is independent of the book.

1. Gate command 5, in `ci.yml` and in the `CONTRIBUTING.md` block:
   `cargo check --locked -p wasm-smtp --features smtputf8,mail-builder,tracing`
   becomes
   `cargo test --locked -p wasm-smtp --features smtputf8,mail-builder,tracing`.
2. Paste its unit-test count next to `cargo test --locked -p wasm-smtp`'s at
   defaults. The architect measured 309 against 279. If yours differ, say
   why.
3. **Demonstrate failure:** break one assertion in `smtputf8_tests.rs`.
   Command 5 must go red and command 3 must stay green, which is the gap
   this closes. Revert.
4. `CHANGELOG.md` under `[Unreleased]`: the feature-gated core tests
   now run in the gate, with the count.

## S4b — resumed, with the shape changed by A5

1. **Measure first**, as Revision 2 S4b.1, with the crate shaped as
   below: `wasm-smtp-book` in `members`, its dependencies at defaults,
   and `book` declared but **not** enabled. Normalized feature sets, as in
   your request 2, raw files kept. **Stop if any set changes.**
2. The crate: `tools/book/` (or the name you argue for),
   `publish = false`, feature `book = ["wasm-smtp/smtputf8"]`, and nothing
   else unless a fence proves it needs more. Every chapter item is
   `#[cfg(all(doctest, feature = "book"))]`. The coverage test is **not**
   feature-gated, so it also runs under command 3.
3. Fences, listings, notes, `docs.yml` pull-request trigger with its own
   concurrency group, and the changelog: as in Revision 2 S4b.2–S4b.5.
4. Gate command 22, new, after command 21, in both places:
   `cargo test --locked -p wasm-smtp-book --features book`.
5. **Demonstrate failure**, all in command 22:
   - a renamed API in a compiled fence goes red;
   - fence 29's S4a fix undone goes red with `E0004`;
   - the coverage test names a chapter removed from the crate, and names
     an added scratch chapter;
   - and **command 3 stays green** for the renamed-API case, which shows the
     chapters really are outside the workspace run.

   Revert each.

## Review request

`.git-exclude/review-request/032-verification-coverage-3.md`: S4c's counts
and demonstration; S4b's normalized before/after sets (or the stop
report); the fence-by-fence result; the demonstrations; the gate list as
22 commands with changes marked; the full gate.

---

# Revision 4 — 2026-09-13, after review 3

Review: `.git-exclude/reviewed/032-verification-coverage-review-3.md`.
S4c and S4b are approved; the 22-command gate list is accepted. One small
slice remains, governed by RFC 032 amendment **A7**. RFC 030 may start in
parallel: its files do not overlap these, apart from `CHANGELOG.md`.

## S4d — Stale `ignore` and job-level permissions

One commit.

1. **Fences.** `adapters/tokio.md:80` and `:105`, and
   `core/connection-reuse.md:39` and `:103`: `rust,ignore` →
   `rust,no_run`. No other change to them. Command 22's count must move
   from 33 compiled / 21 ignored to **37 / 17**. Paste it.
2. **`docs.yml` permissions.**
   - Workflow level: `contents: read` only.
   - `build` job: `contents: read` and `pages: read`.
   - `deploy` job: `pages: write` and `id-token: write`.

   One comment line on each block saying what it is for. Nothing else in
   the file changes.
3. **The `pages: read` on `build` is the architect's expectation, not a
   verified fact.** `actions/configure-pages`' README states no
   permission. You cannot run it locally; say so, and the architect
   confirms it on the first push to `main` after approval. If that run's
   `configure-pages` step fails on permissions, the fix is the next
   commit, not a revert of the job-level split.
4. `CHANGELOG.md` under `[Unreleased]`: fold both into the existing
   Documentation / CI entries. No new heading.
5. Full gate: 22 commands.

## Review request

`.git-exclude/review-request/032-verification-coverage-4.md`, short:
command 22's new count, the `docs.yml` diff, and the gate result.
