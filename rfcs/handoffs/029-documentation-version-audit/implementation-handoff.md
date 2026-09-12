# Developer Handoff — RFC 029: Documentation version audit and guard

**Governing RFC.** [`../../done/029-documentation-version-audit-and-guard.md`](../../done/029-documentation-version-audit-and-guard.md)
**Prepared.** 2026-09-12 by the architect. Baseline: `79de96f` (0.17.0 released).
**Starts.** Now. The owner accepted RFC 029 on 2026-09-13 and opened **0.17.1** as its release.
**Target release.** 0.17.1 (patch). Release approval is the owner's; this handoff produces the release commit only.
**Review request goes to.** `.git-exclude/review-request/029-documentation-version-audit.md`

## 1. Purpose

Correct every stale version string in the documentation, remove the
class of defect by stating dependencies as commands rather than
constants, and add a gate check so a stale own-crate version cannot
reach `main` again. The exact stale list is in RFC 029 §Motivation and
is the authoritative work list.

## 2. Change scope

`README.md`, `docs/src/adapters/{tokio,wasi}.md`,
`docs/src/concepts/protocol.md`,
`docs/src/core/{composing-messages,usage}.md`, `TERMS_OF_USE.md`,
`tools/check-doc-versions.sh` (new, or a Rust bin under `tools/` if you
prefer — say which and why), `.github/workflows/ci.yml`,
`.github/CONTRIBUTING.md`, `CHANGELOG.md`, and for the release commit
the workspace `version` plus the five inter-crate pins.

## 3. Non-change scope

No library source. No dependency change — the only manifest edits are
the version bump in S4. No `--all-features`. Do not tag, push, or
publish: stop at the release commit. Do not edit `rfcs/README.md`.

## 4. Slices

### S1. The guard, first and failing (D2)

1. `tools/check-doc-versions.sh`, POSIX shell, no new dependency:
   - Read `major.minor` from `[workspace.package] version` in the root
     `Cargo.toml` (today `0.17`).
   - Read the `mail-builder` version from `[workspace.dependencies]`
     (today `0.5`).
   - Scan `README.md`, `docs/src/**/*.md`, and `crates/*/README.md` for
     dependency version strings of the form `<crate> = "X.Y"` and
     `<crate> = { version = "X.Y"` where `<crate>` is any `wasm-smtp*`
     name or `mail-builder`.
   - For each hit, compare against the expected value; on mismatch
     print `file:line: found "X.Y", expected "A.B"` and exit non-zero.
     Print nothing and exit 0 when clean.
   - Ignore fenced blocks that are explicitly historical if you need an
     escape hatch, but prefer having none: if a line needs an exception,
     that is a sign it should be reworded instead.
2. Wire it into the `gate` job after the existing documentation-adjacent
   checks, and add it to CONTRIBUTING's required-checks list.
3. **Demonstrate it fails.** Run it on the tree as it stands: it must
   report all 20 strings from RFC 029 §Motivation. Paste that output in
   the review request. A check that cannot fail is not a check — the
   same standard RFC 025's negative smoke mode was held to.

### S2. The sweep (D1, D3)

1. Convert every "add this to Cargo.toml" instruction to the `cargo add`
   form. The cases and their intended commands:
   - `docs/src/adapters/wasi.md` lines 11, 18 → `cargo add wasm-smtp-wasi`
     and `cargo add wasm-smtp-wasi --no-default-features --features native-roots`.
   - `docs/src/core/usage.md` 279, 281 → `cargo add wasm-smtp --features smtputf8`
     and the cloudflare equivalent; 376 → `cargo add --dev wasm-smtp-test`.
   - `docs/src/concepts/protocol.md` 375 → `cargo add wasm-smtp --features smtputf8`.
   - `docs/src/core/composing-messages.md` 43–45, 56–57, 316–318 →
     `cargo add` lines; the `mail-builder` version becomes 0.5 wherever
     a version is still shown, and `mail-auth` stays as prose.
   - `README.md` 119, 126 → `cargo add wasm-smtp --no-default-features`
     and `cargo add wasm-smtp --features smtputf8`.
   - `docs/src/adapters/tokio.md` 65–75 → **keep as TOML.** The
     four-configuration comparison is clearer as a block; update the
     versions to the current minor and let the guard hold them.
2. `TERMS_OF_USE.md` opening: describe the library as it is — SMTP from
   WebAssembly and other constrained runtimes, with adapters for
   Cloudflare Workers, WASI, tokio, and the Component Model. Keep the
   rest of the document untouched; it is a policy document.
3. Re-run the guard: it must now be silent.

### S3. Verify

`mdbook build docs` clean; full gate green including the new check.

### S4. Release commit (0.17.1)

1. Workspace `version` and the five inter-crate pins to `0.17.1`.
   Note the ordering trap: the guard reads the manifest, so bumping the
   version makes every `0.17` string in the documentation stale again.
   Run the guard after the bump and fix whatever it names — if S2 left
   any TOML block showing a version, that block moves too. A green
   guard after the bump is the proof the two are actually coupled.
2. `CHANGELOG.md`: a `[0.17.1]` section dated the release day, with a
   Documentation entry naming the audit and the guard, and the
   comparison links. Nothing belongs under `[Unreleased]` afterwards.
3. Full gate on the bumped tree; evidence under
   `.git-exclude/review-request/evidence/029/`.
4. Commit as "Release 0.17.1". Stop.

## 5. Acceptance criteria

RFC 029 §Acceptance criteria, plus: the review request contains the
guard's failing output from S1.3 and its silent output after S2.

## 6. Prohibited shortcuts

- Correcting the strings without the guard. That is what failed twice.
- An exception list in the guard to make a line pass rather than
  rewording it.
- Touching library source or any dependency.
- Bumping the version before the guard is green on the unbumped tree;
  the two must be shown to disagree and then agree.

## 7. Review request contents

Summary per slice; changed files; the guard's before and after output;
which form each converted snippet took and why any stayed TOML; gate
results; anything the audit list got wrong.
