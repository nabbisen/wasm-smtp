# RFC 027 — Documentation publication and dependency currency

**Status.** Accepted
**Priority.** P1
**Tracks.** Docs / Release / Dependencies / CI
**Touches.** `.github/workflows/docs.yml` (new), `README.md`, `docs/book.toml`, `Cargo.toml`, `crates/wasm-smtp-wasi/Cargo.toml`, `crates/wasm-smtp/Cargo.toml`, `crates/wasm-smtp-component/Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`
**Handoff.** [`../handoffs/027-docs-publication-dependency-currency/implementation-handoff.md`](../handoffs/027-docs-publication-dependency-currency/implementation-handoff.md)
**Origin.** Two improvement themes raised by the owner on 2026-09-12 while restructuring the README: publish the mdBook, and address what deps.rs reports.

## Summary

Publish the mdBook under `docs/` to GitHub Pages from CI on every push
to `main`, and bring the dependency declarations up to date: raise the
rustls floor past the advisory that deps.rs flags, move the two
dependencies that have a newer major, and refresh the lockfile. Ships
as **0.17.0**: the owner authorized taking the `mail-builder` major now.

## Motivation

The book has existed since 0.15.0 and has never been published; the
README points readers at a source directory. The owner has configured
GitHub Pages for the repository; the workflow to fill it is missing.

deps.rs, read on 2026-09-12 for the 0.16.1 crates, reports:

| Crate | Dependency | Declared | Latest | deps.rs status |
|---|---|---|---|---|
| `wasm-smtp` | `mail-builder` | `^0.4` | 0.5.0 | out of date |
| `wasm-smtp-component` | `wit-bindgen` | `^0.57` | 0.62.0 | out of date |
| `wasm-smtp-wasi` | `rustls` | `^0.23` | 0.23.44 | maybe insecure: RUSTSEC-2024-0399, patched in 0.23.18 |

The rustls flag is about the declared range, not what we ship: the
lockfile resolves 0.23.40, `cargo audit` is silent, and the advisory
concerns a server-side accept path this client never calls. But the
range as declared would let a downstream lockfile land on an affected
version, and the badge on the README says "maybe insecure" to every
reader. Raising the floor is the honest fix.

Separately, `cargo update --dry-run` lists 87 compatible updates the
lockfile has not taken, including `aws-lc-rs` 1.16 → 1.18. The lockfile
is what CI and the release gate actually test.

## Goals

- The book is published at `https://nabbisen.github.io/wasm-smtp/` and
  rebuilt on every push to `main`; the README links to it.
- No deps.rs status other than "up to date" on any published crate.
- The lockfile is current and the full gate passes on it.

## Non-goals

- Publishing rustdoc anywhere other than docs.rs.
- Versioned book snapshots per release (one live book from `main`).
- Automating dependency updates (a bot is a separate decision).
- A README badge for the book: the owner adds it once the site is live.

## Design

### D1. GitHub Pages workflow

`.github/workflows/docs.yml`: on `push` to `main` (and manual
dispatch), install a pinned mdBook (0.5.x, the version verified
locally), run `mdbook build docs`, upload `docs/book` with
`actions/upload-pages-artifact`, deploy with `actions/deploy-pages`.
Permissions: `pages: write`, `id-token: write`, `contents: read`. The
repository's Pages source must be "GitHub Actions"; if the owner
configured a branch source instead, the handoff says what to change.
`docs/book/` stays gitignored. The build is not part of the release
gate; a broken book must not block a crate release, and the workflow's
own status is visible on the Actions tab.

`docs/book.toml` gains `site-url = "/wasm-smtp/"` so links resolve
under the project path on Pages.

### D2. rustls floor

`crates/wasm-smtp-wasi/Cargo.toml`: `rustls = "0.23.18"` at minimum;
the handoff uses the version the refreshed lockfile resolves so the
floor and the tested version agree. Same treatment for the workspace
`tokio-rustls` line only if deps.rs flags it after the refresh (it does
not today).

### D3. `mail-builder` 0.5

Optional dependency behind the `mail-builder` feature;
`SmtpClient::send_message` takes `mail_builder::MessageBuilder<'_>`, so
the major is part of this crate's public API for users of that feature.
Move the floor to `0.5` if `MessageBuilder::write_to_string` and the
builder methods the docs use are unchanged or trivially adapted;
otherwise report the API delta before changing anything. Either way
the release is 0.17.0, because the feature's public type changes major.

### D4. `wit-bindgen` 0.62

Build-time only, wasm32 only. Move the pin if `generate!` with the
`with:` map and `export!` still work; the wasip2 check and the smoke
test decide. If 0.62 changes the macro contract in a way that needs
more than local edits, keep 0.57 and report; a build tool version is
not worth a design change.

### D5. Lockfile refresh

`cargo update` on the pinned toolchain, then the full gate including
the on-target smoke test. `aws-lc-rs` and `ring` both move; the
explicit-provider design (RFC 024 D9) means no behavior depends on
which is process default.

### D6. Release

0.17.0 if D3 lands as a bump; 0.16.2 otherwise. The changelog names
the rustls floor as a compatibility note. Owner approval at release
time; tag, push, publish are the owner's and the architect's.

## Security considerations

D2 closes a declared range that admitted an affected rustls, even
though no shipped lockfile ever resolved one. The Pages workflow needs
`id-token: write` for OIDC deployment; it touches no secrets and
publishes only the built book.

## Simplicity and maintainability considerations

One workflow file, one `site-url` line, three floor edits, one
lockfile. Nothing touches library code unless D3's API delta forces a
one-line adaptation in `send_message`.

## Alternatives considered

**Publish the book from a `gh-pages` branch pushed by CI.** Works, but
the Actions-source deployment leaves no generated files in git and is
what the owner configured.

**Leave `mail-builder` at 0.4.** deps.rs would keep flagging it, and
the longer the gap the larger the eventual jump. The feature is opt-in
and its users can pin; taking the major now with a minor bump is the
cheaper path.

## Implementation plan

Handoff, three slices: docs workflow; dependency floors and lockfile;
release commit.

## Acceptance criteria

- The Pages workflow runs green on `main` and the book is reachable at
  the site URL with working intra-book links.
- README's Documentation section links to the site.
- deps.rs shows every published crate as up to date after the release.
- Full gate green on the refreshed lockfile, smoke test included.
- Public API delta: none, unless D3 requires a signature adaptation,
  which must be reported.

## Open questions

None. Both closed by the owner on 2026-09-12: the Pages source is
"GitHub Actions", and the release is 0.17.0 with `mail-builder` moved
to 0.5 now.

## Amendment log

- 2026-09-12: accepted by the owner with both open questions answered.
