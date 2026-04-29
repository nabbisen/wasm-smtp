# Contributing

Thanks for considering a contribution. This document records the
expectations for changes to `wasm-smtp` so that the work is predictable
for both contributors and reviewers.

## Before you start

- For anything beyond a typo or a one-line fix, please open an issue
  describing the change you intend to make. We try to keep the public
  surface small and the dependency graph short, so it is easier to
  agree on a design before code is written than after.
- Read the project's [`ROADMAP.md`]. Work that lies outside the current
  phase, or in the explicitly out-of-scope section, will need a stronger
  justification.
- Read [`TERMS_OF_USE.md`]. We do not accept changes whose primary
  purpose is to enable bulk mail or impersonation.

## Repository layout

```text
wasm-smtp/
├─ crates/
│  ├─ wasm-smtp/              wasm-smtp: pure protocol engine, no I/O
│  ├─ wasm-smtp-cloudflare/   Cloudflare Workers socket adapter
│  └─ wasm-smtp-tokio/        Tokio + rustls socket adapter
├─ docs/src/                  long-form, mdBook-ready documentation
└─ .github/                   policy and issue-template files
```

## Code style

- Rust 2024 edition. Stable Rust, MSRV declared in the workspace
  `Cargo.toml`.
- Modern module style: do not introduce `mod.rs`. Each module is one
  file at the same level as its parent.
- Tests for the core crate live in `crates/core/src/tests.rs` and use
  the in-tree synchronous mock transport. Do not introduce a runtime
  dependency on `tokio`, `futures`, or any executor.
- Keep `unsafe` out of the core. The workspace `Cargo.toml` enforces
  `unsafe_code = "forbid"`.
- All public items must have a doc comment. Comments and documentation
  are written in English.

## Required checks

Before sending a pull request, please run, from the workspace root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

A pull request that does not pass all three is unlikely to be merged.

## Commit messages

Use the imperative mood ("Add X", "Fix Y", "Refuse Z"). Reference the
issue you are fixing in the body, not in the subject. One logical
change per commit; squash fixups before opening the pull request.

## Licensing of contributions

By contributing, you agree that your contribution will be licensed under
the project's [`LICENSE`] (Apache-2.0).

[`LICENSE`]: ../LICENSE
[`ROADMAP.md`]: ../ROADMAP.md
[`TERMS_OF_USE.md`]: ../TERMS_OF_USE.md
