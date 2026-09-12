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
│  ├─ wasm-smtp/              pure protocol engine, no I/O
│  ├─ wasm-smtp-cloudflare/   Cloudflare Workers socket adapter
│  ├─ wasm-smtp-tokio/        Tokio + rustls socket adapter
│  ├─ wasm-smtp-wasi/         WASI 0.2 sockets adapter (wasm32-wasip2)
│  ├─ wasm-smtp-component/    WASM Component Model export
│  │  └─ wit/                 the contract (smtp.wit) and its WASI deps
│  └─ wasm-smtp-test/         mock Transport for tests
├─ tools/smoke/               on-target smoke-test driver (never published)
├─ docs/src/                  long-form, mdBook-ready documentation
├─ rfcs/                      design records; see rfcs/README.md
└─ .github/                   policy, issue templates, CI workflow
```

## Code style

- Rust 2024 edition. Stable Rust, MSRV declared in the workspace
  `Cargo.toml`.
- Modern module style: a module with submodules is a `foo.rs` next to a
  `foo/` directory. Do not introduce `mod.rs`-only modules.
- Tests live under `crates/<crate>/src/tests/` (or `src/tests.rs` for a
  small crate), separate from the implementation, and use a synchronous
  mock transport — `wasm-smtp-test` in this workspace. Do not introduce
  a runtime dependency on `tokio`, `futures`, or any executor in the
  core.
- Keep `unsafe` out of the code you write. The workspace `Cargo.toml`
  enforces `unsafe_code = "forbid"`; the one exception is
  `wasm-smtp-component`, at `deny`, where wit-bindgen's generated
  Component Model glue carries a scoped allowance (RFC 024 D10).
- All public items must have a doc comment. Comments and documentation
  are written in English.

## Required checks

Before sending a pull request, please run, from the workspace root, the
same command list CI runs (RFC 024 §D3):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace                       # lib + integration + doctests

# Feature combinations. Never use --all-features: the tokio adapter has a
# deliberate compile_error! on aws-lc-rs + ring.
cargo check -p wasm-smtp --no-default-features
cargo check -p wasm-smtp --features smtputf8,mail-builder,tracing
cargo check -p wasm-smtp-wasi --no-default-features --features native-roots
cargo check -p wasm-smtp-wasi --features plaintext-only
cargo check -p wasm-smtp-tokio --no-default-features --features webpki-roots,ring

# Real targets.
cargo check -p wasm-smtp -p wasm-smtp-cloudflare --target wasm32-unknown-unknown
cargo check -p wasm-smtp -p wasm-smtp-wasi -p wasm-smtp-component --target wasm32-wasip2

# Packaged contents: the published component must carry its contract.
# Add --allow-dirty when checking uncommitted work.
cargo package --list -p wasm-smtp-component | grep -q '^wit/smtp.wit$'
cargo package --list -p wasm-smtp-component | grep -q '^wit/deps/sockets/tcp.wit$'

# The documented Worker examples must compile for the Workers target.
cargo check -p wasm-smtp-cloudflare --examples --target wasm32-unknown-unknown

# Dependency versions in the documentation must match the manifest.
./tools/check-doc-versions.sh
cargo test -p wasm-smtp-cloudflare --examples   # example tests are not run by --workspace

# On-target: a real wasm32-wasip2 guest under wasmtime against a scripted
# TLS SMTP responder on loopback, in four modes — two positive and two
# that prove an untrusted certificate is refused. Needs wasmtime on PATH
# (or WASMTIME set).
cargo build --target wasm32-wasip2 -p wasm-smtp-wasi --example smoke
cargo run -p wasm-smtp-smoke

# Dependency advisories.
cargo audit
```

A pull request that does not pass these is unlikely to be merged.

The documentation book is built with **mdBook 0.5.4**, the version
`.github/workflows/docs.yml` pins for the published site at
<https://nabbisen.github.io/wasm-smtp/>. Build it locally with
`mdbook build docs`; using a different version may render differently
from the live site. Not a gate command.

The toolchain comes from `rust-toolchain.toml` at the workspace root: it
pins the channel (which is also the MSRV, currently 1.88), `rustfmt` and
`clippy`, and the two wasm targets. Run `rustup show active-toolchain`
once to install them. If you work on a newer toolchain, invoke it
explicitly with `cargo +stable ...`; the pinned version is what CI
enforces, including rustfmt output and the clippy lint set.

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
