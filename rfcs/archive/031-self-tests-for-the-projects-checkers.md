# RFC 031 — Self-tests for the project's own checkers

**Status.** Superseded by RFC 032
**Priority.** P3
**Tracks.** Testing / CI
**Touches.** `tools/check-doc-versions.sh`, `tools/check-wasi-version.sh`, a test harness for both, `.github/workflows/ci.yml`, `.github/CONTRIBUTING.md`
**Origin.** Recorded as a follow-up after RFC 025, again after RFC 029, and again after RFC 028. Promoted to an RFC in `.git-exclude/reviewed/028-wasi-contract-alignment-review-1.md` §5 because prose follow-ups were not being picked up.

> **Superseded.** Folded into [RFC 032](../accepted/032-verification-coverage.md) as D5, unchanged in substance, at the owner's direction on 2026-09-13. RFC 032 covers the wider verification review this RFC turned out to be one part of.

## Summary

The project has three checkers that exist to catch a class of defect
the gate cannot otherwise see: the two shell version guards and the
component host harness's assertion function. Each was demonstrated
failing once, by hand, in a review request. Those demonstrations live
in review files and run nowhere. Give the two shell guards synthetic-
input self-tests that run in the gate, so the demonstrations live in
the repository.

## Motivation

A checker that cannot fail is worse than no checker, because it reports
success. This project has made that argument three times — for the
release gate, for the on-target smoke test, and for the documentation
version guard — and each time the demonstration that the new check
could fail was performed by hand and recorded in prose.

That is fine as evidence at review time and worthless six months later.
Two concrete gaps today:

- `check-doc-versions.sh`'s `mail-builder` comparison branch had **no
  live coverage** after RFC 029's sweep removed every versioned
  `mail-builder` line from the documentation. The architect had to
  inject one by hand to confirm the branch worked at all. It is now
  code that runs on every gate invocation and compares nothing.
- `check-wasi-version.sh` has four behaviours worth pinning — a clean
  tree, a bumped lock, a partial hand-update, and a missing `wasip2`
  that must exit 2 rather than fall back — all four shown once, in a
  review request.

The component harness's checker already has four synthetic-input tests
from RFC 028 S3, which is the shape this RFC generalises.

## Goals

- Each shell guard has tests over synthetic fixtures: a clean case that
  passes, and one case per failure mode that fails with the expected
  message.
- The tests run in `cargo test --workspace` or as a gate step.
- Every branch of each guard is exercised by at least one fixture,
  including branches with no live content to compare.
- `check-doc-versions.sh`'s header records its deliberate exclusions
  (`CHANGELOG.md`, `rfcs/`) and that the tokio chapter's TOML block is
  intentional live coverage, so neither reads as an oversight.

## Non-goals

- Rewriting either guard in Rust. They are shell for a stated reason:
  the gate runs them without building anything.
- Testing the gate's other commands. `cargo` tests itself.
- A test framework. Fixture directories and expected output are enough.

## Design sketch

Both guards already accept a root directory argument, which is what
makes this cheap: point them at a fixture tree instead of the
repository.

A fixture is a directory holding a minimal `Cargo.toml` and the few
documentation or WIT files a case needs. The test runs the guard
against it and compares exit code and stdout to an expected value.
The runner can be a shell script the gate calls, or a Rust integration
test under `tools/` that shells out — the handoff should pick whichever
makes the expected-output comparison easier to read in a diff, since
that is what someone debugging a failure will look at.

Cases, at minimum: for `check-doc-versions.sh`, a clean tree, a stale
own-crate version, a stale `mail-builder` version, a correct
`mail-builder` version, a commented-out stale line (which must fail,
because a reader uncomments it), and a manifest whose version cannot be
read (exit 2). For `check-wasi-version.sh`, the four behaviours listed
in §Motivation.

## Security considerations

None. No shipped artifact changes.

## Open questions

1. Shell runner or Rust integration test? Decide on diff readability.
2. Should the fixtures live under `tools/` beside the guards, or in a
   shared `tools/fixtures/`? One guard's fixtures are WIT trees and the
   other's are Markdown, so they may not want a common home.
