# Developer Handoff — RFC 027: Documentation publication and dependency currency

**Governing RFC.** [`../../accepted/027-docs-publication-dependency-currency.md`](../../accepted/027-docs-publication-dependency-currency.md)
**Prepared.** 2026-09-12 by the architect. Baseline: the README commits `81fb3a4`, `4f0db09`.
**Starts.** Now. The owner accepted RFC 027 on 2026-09-12; the Pages source is "GitHub Actions"; the release is **0.17.0** with `mail-builder` moved to 0.5.
**Review request goes to.** `.git-exclude/review-request/027-docs-publication-dependency-currency.md`

## 1. Change scope

`.github/workflows/docs.yml` (new), `docs/book.toml`, `README.md`
(Documentation section only), `crates/wasm-smtp-wasi/Cargo.toml`,
`crates/wasm-smtp/Cargo.toml`, `crates/wasm-smtp-component/Cargo.toml`,
`Cargo.toml` (workspace line only if needed), `Cargo.lock`,
`CHANGELOG.md`, and, only if RFC 027 D3 forces it,
`crates/wasm-smtp/src/client/send.rs::send_message` plus
`docs/src/core/composing-messages.md`.

## 2. Non-change scope

No other library source. No new features. No `--all-features`. Do not
tag, push, or publish. Do not add the README badge; the owner does.

## 3. Slices

### S1. Book publication (D1)

1. `.github/workflows/docs.yml`:
   - `on: push: branches: [main]` and `workflow_dispatch`.
   - `permissions: contents: read, pages: write, id-token: write`.
   - `concurrency: group: pages, cancel-in-progress: true`.
   - Steps: checkout; install mdBook at a pinned version (the locally
     verified line is 0.5.x; `peaceiris/actions-mdbook` or a direct
     download, either is fine, pin the version); `mdbook build docs`;
     `actions/configure-pages`; `actions/upload-pages-artifact` with
     `path: docs/book`; `actions/deploy-pages` in a `deploy` job with
     `environment: github-pages`.
2. `docs/book.toml`: add `site-url = "/wasm-smtp/"` under `[output.html]`.
   Run `mdbook build docs` locally and open `docs/book/index.html` to
   confirm intra-book links still resolve.
3. `README.md`, Documentation section: first sentence links the
   published book at `https://nabbisen.github.io/wasm-smtp/`; keep the
   `docs/src` pointer as the source location. Leave the badge row alone.
4. If the repository's Pages source is set to a branch rather than
   "GitHub Actions", the deploy step will fail with a clear message; do
   not switch to a `gh-pages` branch push, report it and the architect
   asks the owner to flip the setting.

### S2. Dependency currency (D2–D5)

1. `cargo update` on the pinned toolchain. Record the resolved `rustls`
   version.
2. `crates/wasm-smtp-wasi/Cargo.toml`: `rustls` floor to the resolved
   version (at least `0.23.18`). Keep `default-features = false` and
   the feature list.
3. `mail-builder`: move the workspace floor to `0.5` (owner's
   decision). Run `cargo test -p wasm-smtp --features mail-builder` and
   re-read the composing-chapter snippets against the 0.5 API. If
   `write_to_string` or a builder method the docs use changed signature,
   adapt `send_message` and the snippets minimally and list every change
   in the review request; if the change is more than a rename or a
   return-type wrapper, stop and report first.
4. `wit-bindgen`: try `0.62` in the component crate. Acceptance is the
   wasip2 check and the smoke test (`cargo check --target wasm32-wasip2 -p wasm-smtp-component`
   and `cargo run -p wasm-smtp-smoke`). If `generate!`'s `with:` map or
   `export!` need more than a local edit, keep `0.57` and report why.
5. Full gate, all commands, on the refreshed lockfile.

### S3. Release commit

Version 0.17.0
across the workspace and pins; CHANGELOG entry with a compatibility
note for the rustls floor and, if applicable, the `mail-builder` major;
full gate; evidence under `.git-exclude/review-request/evidence/027/`;
commit "Release X.Y.Z"; stop.

## 4. Acceptance criteria

RFC 027 §Acceptance criteria. The review request states the resolved
versions of `rustls`, `mail-builder`, and `wit-bindgen`, and whether the
book deployed (link to the Actions run).

## 5. Prohibited shortcuts

Lowering any floor; disabling the smoke test to get through a
`wit-bindgen` change; committing `docs/book/`.

---

# Revision 2 — 2026-09-12, after review 1

Review: `.git-exclude/reviewed/027-docs-publication-dependency-currency-review-1.md`.
S1–S3 are accepted. The §4 escalation is decided: **ship 0.17.0 with the
WASI version drift.** It is not a regression (every consumer since
0.14.0 with a current lockfile already has it), and no lockfile of ours
can fix it for a consumer, because `wasi` 0.14.7 declares `wasip2` with
a caret range. The real fix is RFC 028 (proposed): align the
annotations, guard the drift in the gate, and execute the component
under a host. Do not touch `wit/` in this release beyond C1.

Three documentation corrections, all inside the release.

## C1 — `wit/deps/README.md` justification is now false

It says the packages are 0.2.4 because that is "the version the `wasi`
0.14 crate implements". Replace with two sentences saying what is true:
the vendored packages are WASI 0.2.4; the `wasi` crate's transitive
`wasip2` may implement a later 0.2.x because that range is outside this
project's control; the mismatch and its fix are tracked in RFC 028.
This file ships inside the published crate, so the wording matters.

## C2 — mdBook version for contributors

`.github/CONTRIBUTING.md`: one line near the required checks naming the
mdBook version the published site is built with (0.5.4, matching
`docs.yml`), so a contributor cannot render a book that differs from the
live one without knowing. Not a gate command.

## C3 — Fold the `[Unreleased]` entry into 0.17.0

Move the RFC 026 documentation entry from `[Unreleased]` into 0.17.0's
Documentation section, so the 0.17.0 notes mention the anti-abuse
chapter that ships in those crates. Leave 0.16.1's entry alone.
`[Unreleased]` ends up empty; that is correct.

## S3 (again) — Release commit

Fold C1–C3, rerun the full gate, refresh `evidence/027/`, commit as
"Release 0.17.0" superseding `75f4251`, and stop. Second request:
`.git-exclude/review-request/027-docs-publication-dependency-currency-2.md`,
listing only what changed. Do not tag, push, or publish.
