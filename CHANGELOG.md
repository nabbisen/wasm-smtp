## [Unreleased]

RFC 032: verification coverage. Nothing a consumer receives changes — no
published crate's source, manifest, dependencies, or built artifact. What
changes is what the gate reaches, and the order releases happen in.

### Testing

- **The tokio adapter completes a send in a test.** A new integration test
  drives `wasm-smtp-tokio` over a real loopback socket against the scripted
  TLS responder: implicit TLS, STARTTLS upgraded on the same socket, and an
  untrusted certificate refused. The refused case asserts that no SMTP
  command reached the server, not merely that an error came back. Runs
  under `cargo test --workspace`.
- **The transcript assertions are shared.** The WASI smoke test, the
  component harness, and the tokio test now hold their recordings to the
  same `check_session` / `check_refused` in `tools/smoke`'s library, each
  with self-tests showing it rejects the sessions it exists to reject.
- **The shell guards are tested.** `tools/guard-tests/run.sh` points
  `check-doc-versions.sh` and `check-wasi-version.sh` at thirteen fixture
  trees, one per branch, and compares exit status and output byte for
  byte. A new gate command.

### CI

- **wasmtime CLI 27.0.0 → 36.0.15** for the WASI smoke test: the 36.0.x LTS
  line, the same one the component harness links, chosen for its security
  backports.
- **`--locked`** on every gate command that resolves the lockfile, in CI
  and in `CONTRIBUTING.md`, so CI tests the committed `Cargo.lock`.
- **Third-party actions pinned by commit SHA**, with the release in a
  trailing comment; `cargo-audit` installed at an exact version.
- **A weekly scheduled workflow** runs `cargo audit` and the `#[ignore]`d
  tests, so advisories and the slow tests are seen between commits.
  GitHub disables scheduled workflows after 60 days without repository
  activity; `CONTRIBUTING.md` says how to re-enable it.

### Process

- **Releases are tagged only after CI passes on the release commit.** The
  release commit is pushed alone, and the tag and publish wait for its
  green run. Recorded in `CONTRIBUTING.md`.

### Documentation

- **Three code examples in the book were wrong, not merely uncompiled**,
  and are fixed (RFC 032). The error-classification match in
  *Errors* had comments where Rust requires expressions; the policy
  chapter imported `PolicyError` through a private path; and the retry
  example in *Usage* did not compile because it had no arm for
  `SmtpError::Policy` — the example a reader copies into production.

### Code quality

- **Clean under the current stable Clippy** (RFC 033). Six lints fixed as
  the code improvements they are — among them the base64 encoder and
  decoder, now on `as_chunks`, with a new round-trip test over every
  input length from 0 to 7. One lint, `unused_async_trait_impl`, is
  allowed on the eight trait impls that are async by contract but have
  nothing to await, each with its reason. No API or behaviour change; the
  pinned 1.88 gate is unaffected, and the advisory `stable` job reports
  every crate's lints rather than stopping at the first.

### Not in this release

- **Compiling the book's Rust code blocks (RFC 032 D2) did not land.**
  One example needs the `smtputf8` feature, and a book crate enabling it
  would change the features the core's own workspace tests build with.
  How to resolve that is pending.

## [0.17.2] — 2026-09-13

RFC 028. The `wasm-smtp-component` crate had shipped for three releases
without ever being executed. It now runs in the gate, and what running
it showed corrects both the contract it declares and the instructions
for building it. No change to the `smtp-send` interface: no type, field,
function, or the `wasm-smtp:smtp@0.1.0` package version.

**The built component is byte-for-byte the same as 0.17.1's in what it
imports.** Nothing about the artifact changed. What changed is the
declaration of what a host must provide, which had never matched the
artifact in the first place: the world said WASI 0.2.4 while the
component has always imported 0.2.12 and 0.2.3. A host built to satisfy
only the declaration could never have instantiated this component, so
there is no working setup that this release breaks — only a false
statement it stops making.

### Changed

- **The world's WASI imports move from `@0.2.4` to `@0.2.12`**, and the
  packages vendored under `wit/deps/` move with them — three flat files
  now, where the 0.2.4 release shipped three directories. The old number
  came from reading `wasi 0.14.7+wasi-0.2.4`'s own version; that crate is
  a facade over `wasip2 1.0.4+wasi-0.2.12`, and it is `wasip2` the
  bindings actually come from. The declared contract had been eight
  minors from the artifact since 0.15.2.
  - This changes nothing about the built artifact. Its import names were
    captured before and after and are byte-identical: the `with:` map
    means this WIT generates no import bindings, only the export. It does
    change the `wit/` directory the crate publishes, which is why it is
    recorded here rather than passed over.
- **Building it never required `cargo-component`.** `cargo build
  --target wasm32-wasip2` emits a Component Model component directly.
  The README, the crate docs, the WIT header, the book chapter, and the
  roadmap all said otherwise and now do not.

### Added

- **`tools/component-smoke`**, run by the gate: it embeds wasmtime 36 as
  a library, instantiates the built artifact, and calls `smtp-send.send`
  against the same scripted TLS SMTP responder the adapter smoke test
  uses — which is now a library shared by both, rather than a second
  copy of the SMTP script. It asserts the component validates the
  server's certificate, refuses one it cannot chain, reports that as a
  `send-error` rather than trapping or hanging, and speaks no SMTP over
  the unvalidated channel.
- **`tools/check-wasi-version.sh`**, also in the gate: the WASI minor is
  written in nineteen places across the WIT and the crate source, and
  this compares every one against `wasip2`'s `+wasi-X.Y.Z` metadata in
  `Cargo.lock`. A `cargo update` that bumps `wasip2` now fails the gate
  naming both versions, instead of silently widening the gap this
  release closed.

### Documentation

- The book's component chapter gains a **host requirement** section: a
  WASI 0.2 host, with the declared minor explained as the one the
  bindings come from rather than an exact description of the artifact —
  which also carries `wasi:*@0.2.3` interfaces from the Rust standard
  library, outside this project's control. Hosts satisfy these imports
  by semver compatibility, which the gate demonstrates by running the
  artifact under wasmtime 36, whose WASI packages are at 0.2.6 —
  neither of the two minors in the artifact, and both resolve.
- `wit/deps/README.md` rewritten around what the vendored files are for
  and what the version annotation does and does not mean.

## [0.17.1] — 2026-09-13

A documentation release. No change to any published crate's source,
dependencies, or public API.

### Documentation

- **Every dependency version string in the documentation was stale or is
  now guarded** (RFC 029). Twenty of them, spanning 0.4 through 0.16
  against a current 0.17, across the README, four book chapters, and the
  `mail-builder` advice in the composing chapter — which still named 0.4
  after 0.17.0 moved that dependency to 0.5. A reader copying any of
  them got a version we no longer publish.
- **The class is removed, not just the instances.** Dependency
  instructions are now `cargo add` invocations, which name no version and
  cannot go stale. The tokio chapter's four-way comparison of feature
  configurations stays as TOML, because it reads better as one block, and
  is held by the check below.
- **`tools/check-doc-versions.sh` runs in the gate**, comparing every
  `wasm-smtp*` version string in the README, the book, and the crate
  READMEs against the workspace manifest, and every `mail-builder` string
  against the workspace dependency. Major.minor only, so a patch release
  does not invalidate the documentation. This is the third attempt at
  this problem; the first two were hand-corrections that rotted by the
  next release.
- `TERMS_OF_USE.md` described the library as being for "constrained
  runtimes (initially Cloudflare Workers)". Four adapters ship and two of
  them are not WASM-constrained; it now says so.

## [0.17.0] — 2026-09-12

RFC 027: publish the book, and bring the dependency declarations up to
date. No library behaviour changes.

**Compatibility notes.** Two items affect downstream builds. (1) The
optional `mail-builder` dependency moves to **0.5**. `SmtpClient::send_message`
takes a `mail_builder::MessageBuilder<'_>`, so for users of the
`mail-builder` feature that type is part of this crate's public API and
the major belongs in a version bump — hence 0.17.0 rather than a patch.
The method's own signature is unchanged and no code in this crate needed
adapting. (2) The `rustls` floor in `wasm-smtp-wasi` rises from `0.23` to
**0.23.44**; anything already on a current 0.23.x is unaffected.

### Added

- **The book is published** at <https://nabbisen.github.io/wasm-smtp/>,
  rebuilt from `main` on every push by `.github/workflows/docs.yml`. It
  has existed since 0.15.0 and has never been served; the README pointed
  readers at a source directory. The workflow is deliberately outside the
  release gate: a broken book should be visible on the Actions tab, not a
  reason a crate release cannot go out.

### Changed

- **`rustls` floor raised to 0.23.44** in `wasm-smtp-wasi`, matching the
  version the lockfile resolves. The old `0.23` range admitted releases
  before 0.23.18 that RUSTSEC-2024-0399 affects — a server-side accept
  path this client never calls, and no lockfile of ours ever resolved
  one, but a downstream lockfile could have, and deps.rs reported the
  crate as "maybe insecure" to every reader.
- **`mail-builder` moved to 0.5.** No source change was required.
- **`wit-bindgen` moved to 0.62** in `wasm-smtp-component`. Build-time
  only, `wasm32` only; the `generate!` `with:` map and the `export!` form
  are unchanged.
- **Lockfile refreshed**, taking the compatible updates it had not,
  including `aws-lc-rs` 1.16 → 1.18 and `ring`, `tokio-rustls`, `worker`,
  and `web-sys` moves. The full gate, including the on-target smoke test,
  passes on the refreshed lockfile.

### Documentation

- The README's Documentation section leads with the published book and
  keeps the source pointer for anyone who would rather build it locally.
- `docs/src/reference/examples.md` linked to `TERMS_OF_USE.md` with a
  relative path that escapes the book and would have 404'd once served;
  it now uses the repository URL, as the security chapter already did.

## [0.16.1] — 2026-09-12

A documentation release. No change to any published crate's source or
dependencies; the Cloudflare adapter's package gains the compiled
contact-form example described below.

### Documentation

- **Anti-abuse patterns at the application boundary** (RFC 026). A new
  examples section walks a Cloudflare Worker contact form that refuses in
  order — method, honeypot, then a Turnstile challenge — and opens an SMTP
  session only once the request has earned it, failing closed if the
  verifier is unreachable. A new security section draws the boundary the
  library works to: challenges, rate limits, and honeypots are HTTP-layer
  and vendor-specific; `SendPolicy` is the SMTP-layer, vendor-neutral hook;
  the library provides the latter and deliberately not the former. The
  Worker is a compiled example
  (`crates/wasm-smtp-cloudflare/examples/contact_form_turnstile.rs`) and
  the gate checks it for `wasm32-unknown-unknown`, so it cannot rot
  silently. Both contact-form examples now reject a submitted name or
  address containing CR or LF before it reaches the header block: the
  library validates the envelope addresses it is given, but a header block
  the application builds by hand is its own responsibility, and a line
  break there appends headers of the submitter'''s choosing. No published
  crate gains a dependency.

## [0.16.0] — 2026-09-12

RFC 025: run the WASI adapter on a real host for the first time, and fix
what that exposed. Also completes the audit event model, gives the four
send methods one envelope implementation, restores error causes in two
adapters, and puts dependency advisories in CI.

**Compatibility notes.** Three items affect behaviour even though nothing
was removed: (1) one new public function,
`wasm_smtp_wasi::connect_smtp_starttls_with`; (2) `send_mail_bytes`,
`send_mail_stream`, and `send_mail_smtputf8` now batch the envelope when
the server advertises `PIPELINING`, as `send_mail` already did — on a
server that does not advertise it the bytes are unchanged, and the test
suite asserts byte-equality with 0.15.2 for all four methods; (3)
`RecipientRejected` and `SessionAborted` are now actually emitted, so an
`AuditSink` that matches exhaustively will see events it never saw
before.

### Fixed

- **STARTTLS on WASI panicked on every upgrade.** `upgrade_to_tls`
  evaluated a placeholder function that panics unconditionally, as an
  argument to `mem::replace`, so the argument was evaluated before the
  branch that would have avoided it. Deleted along with the placeholder.
- **The WASI adapter reported "peer closed" for any reply that was not
  already buffered.** `read` used the non-blocking `input-stream.read`,
  whose empty result means "nothing ready yet", and mapped it to `Ok(0)`
  — which the core reads as a clean EOF (RFC 005). On a real network that
  is nearly every reply. It now uses `blocking-read`, and an empty result
  from that blocks again rather than reporting EOF.
- **The WASI adapter trapped the guest when a connection was dropped.**
  `WasiStream` held its `TcpSocket` before the input and output streams
  taken from it. Those are child resources of the socket in WASI 0.2, and
  dropping a parent while a child is alive traps. Rust drops fields in
  declaration order, so the streams are now declared first.
- **`SmtpAuditEvent::RecipientRejected` was never emitted**, and
  `SessionAborted` only from `quit`, so a failure during a transaction
  left no abort record — exactly the case audit exists for. Both are now
  emitted, `SessionAborted` exactly once per session from the single
  place that closes the state machine on failure.
- **The Cloudflare adapter discarded error causes**, formatting
  `worker::Error` into a message string instead of preserving it as the
  source. `worker::Error` does satisfy `Error + Send + Sync + 'static`,
  so callers can now walk `.source()` to the underlying failure, as they
  already could on the tokio adapter.

### Added

- **On-target smoke test** (`tools/smoke`, dev-only and never published).
  Starts a scripted TLS-terminated SMTP responder on loopback, runs a
  `wasm32-wasip2` guest under wasmtime against it, and asserts the
  session that actually crossed the wire — command order, dot-stuffing,
  and for STARTTLS that everything after the upgrade arrived inside TLS.
  Four modes run on every CI change: implicit TLS, STARTTLS, and two
  negative modes in which the guest is given a CA that did not sign the
  server's certificate and must refuse the connection without speaking
  SMTP — including proof that a refused upgrade does not fall back to the
  plaintext channel. This discharges RFC 017's untrusted-certificate
  acceptance criterion, which had never been verified on a host.
- **`wasm_smtp_wasi::connect_smtp_starttls_with`**, mirroring the
  implicit-TLS pair. Without it a STARTTLS session cannot be pointed at a
  private or test CA.
- **`cargo audit` CI job.** RFC 010 has claimed since 0.5.0 that CI
  enforced dependency advisories; nothing did.

### Changed

- **One envelope implementation.** `MAIL FROM` → `RCPT TO` → `DATA`
  through the `354` is now a single private method that all four send
  methods call, replacing four near-identical copies. This is what
  extends pipelining to the three methods that lacked it.
- **The WASI transport's state is explicit.** A `Closed` variant replaces
  the placeholder stream: a failed STARTTLS handshake leaves the
  transport closed rather than falling back to plaintext, `close` is
  idempotent, and I/O after close reports it.
- RFC 010's advisory rule is amended to name `cargo audit` rather than
  `cargo deny`; licence checking is not in scope and one tool is enough.
- `anyhow` moved 1.0.102 → 1.0.104 in the lockfile, clearing
  RUSTSEC-2026-0190 (an unsoundness advisory). It reaches the lockfile
  only through wit-bindgen's build-time tooling, so no published crate's
  runtime graph was affected either way; `cargo audit` is now silent.

### Documentation

- `docs/src/adapters/wasi.md` documents the on-target test and states
  plainly that no code in the crate had executed on its target before
  this release.
- `docs/src/core/policy-audit.md` lists the full event set, when each
  fires, and the exactly-once rule for `SessionAborted`.
- `docs/src/concepts/protocol.md` gains a PIPELINING section covering
  what is batched, what is not, and the unchanged sequential path.

## [0.15.2] — 2026-09-12

A maintenance release. RFC 024: make the release gate trustworthy. No
public API changes; no protocol behavior changes.

**Compatibility notes.** Two items in this release affect downstream
builds even though nothing in the public API moved: (1) the minimum
supported Rust version is now **1.88**, corrected from a previously
false 1.85; and (2) `wasm-smtp-component` now compiles its lints with
`unsafe_code = "deny"` rather than the workspace's `"forbid"`, with the
allowance scoped to wit-bindgen's generated Component Model glue —
every other crate keeps `"forbid"` unchanged.

### Fixed

- **`wasm-smtp` did not compile with the `smtputf8` feature.**
  `client/send.rs` used `ProtocolError` without importing it, so
  `cargo check -p wasm-smtp --features smtputf8` — and the `smtputf8`
  pass-throughs on the Cloudflare and tokio adapters — failed.
- **`wasm-smtp-wasi` did not compile with `native-roots`.**
  `rustls-native-certs` 0.8 returns a `CertificateResult` struct rather
  than a `Result`. The adapter now follows the same policy as
  `wasm-smtp-tokio`: keep every certificate that decoded, and fail only
  if the resulting trust store is empty.
- **`wasm-smtp-wasi` could not build for `wasm32-wasip2`.** Its `rustls`
  dependency did not disable default features, which pulled in the
  aws-lc-rs provider; the `aws-lc-sys` C sources cannot cross-compile to
  that target. The dependency now selects `ring` explicitly, as RFC 017
  Strategy A specifies.
- **Three doctests were broken.** `VecAuditSink::events` returns the
  `Debug` label of each event rather than the enum, `PolicyError` lives
  at the crate root rather than in `policy`, and the `wasm-smtp-wasi`
  crate-level example uses helpers that exist only on `wasm32`.
- **`wasm-smtp-component` had never built for `wasm32-wasip2`**, its
  only real target. It referenced a non-existent `wasm_smtp_component_rt`
  crate, used an `exports:` key that wit-bindgen 0.57 does not accept,
  and dropped `TlsMode` from the native stub imports; `wit/smtp.wit`
  itself did not parse, and the WASI packages its world imports were not
  present to resolve against. The crate now drives its futures with a
  private no-op-waker `block_on` — sound because the WASI transport
  polls inline and resolves on first poll — uses the `export!` macro
  form, and compiles for the target. See Changed for the WIT amendment.
- **`wasm-smtp-component` published without its WIT contract.** The
  contract lived at the workspace root and the crate reached it with
  `path: "../../wit"`, which cargo cannot include in a package, so every
  previously published version shipped without it — invisible until the
  crate could build for its target at all. The contract now lives at
  `crates/wasm-smtp-component/wit/`, and the release gate checks the
  packaged file list.
- **`wasm-smtp-wasi` compiled its test module unconditionally**, rather
  than under `cfg(test)`.
- **`cargo test --workspace` panicked in the tokio adapter's tests.**
  Feature unification across the workspace leaves rustls with both the
  `ring` and `aws-lc-rs` providers compiled in, so rustls could not pick
  one automatically. Fixed properly in the adapters themselves — see
  Changed.
- **A stale doc comment** describing `login` sat orphaned in
  `client/mod.rs`, attached to no item, and the `send_message` doc block
  began with a copy of the SMTPUTF8 text.

### Changed

- **MSRV is now 1.88** (`rust-version` in `[workspace.package]`). The
  previously declared 1.85 was not honored: the core uses `let` chains,
  stable since 1.88 in edition 2024. This is a compatibility-relevant
  change, recorded here as such.
- **`rust-toolchain.toml` pins the toolchain** to 1.88 with `rustfmt`,
  `clippy`, and the `wasm32-unknown-unknown` / `wasm32-wasip2` targets,
  so formatting, linting, building, and testing are reproducible for
  every contributor and in CI.
- **The release gate is an explicit command list** (RFC 024 §D3) and is
  now enforced by CI (`.github/workflows/ci.yml`): a blocking `gate` job
  on the pinned toolchain and an advisory `stable` job. Clippy runs with
  `-D warnings`; every warning is fixed or carries a targeted `#[allow]`
  with a one-line reason. `--all-features` is never used, since the
  tokio adapter's `compile_error!` on conflicting crypto providers is
  deliberate.
- **`AuthError::UnsupportedMechanism` now names the mechanisms actually
  compiled into the build** (PLAIN and LOGIN always, plus SCRAM-SHA-256,
  XOAUTH2, and OAUTHBEARER per feature), instead of mentioning only
  XOAUTH2. A unit test asserts the text follows the features.

- **The WIT contract is amended** (RFC 024 D8), without an interface
  change. `smtp-message`'s envelope sender field is written `%from`,
  since `from` is a reserved WIT keyword and the file had never parsed;
  binding generators still see a field named `from`, and the package
  stays at `wasm-smtp:smtp@0.1.0`. The `wasi:io`, `wasi:sockets`, and
  transitive `wasi:clocks` packages are vendored under `wit/deps/` at
  WASI 0.2.4 — the version the `wasi` 0.14 crate implements — and the
  world's import annotations move from `@0.2.0` to `@0.2.4` to match.
  The component's `generate!` maps those imports onto the `wasi` crate
  so the component carries one copy of the WASI bindings rather than
  two.
- **Both TLS adapters now name their rustls crypto provider explicitly**
  (RFC 024 D9) instead of relying on the process-wide default:
  `wasm-smtp-tokio` uses whichever of aws-lc-rs or ring its features
  selected, `wasm-smtp-wasi` uses ring. A library should not depend on,
  or install, that default — with both adapters in one process rustls
  has two providers compiled in and the automatic choice panics. Public
  API is unchanged; an application that installed a different process
  default no longer influences these adapters.
- **`wasm-smtp-test` is now published** to crates.io (RFC 024 D6,
  confirmed by the owner). It is the reference implementation of the
  `Transport` contract for third-party adapter authors. Its
  "development and testing only, not for production" framing is
  unchanged, and its `wasm-smtp` dependency is pinned to the workspace
  version.
- **`wasm-smtp-component` declares its own lint table** with
  `unsafe_code = "deny"` rather than inheriting the workspace's
  `forbid`. The Component Model export glue that wit-bindgen generates
  is `unsafe` by construction, and `forbid` cannot be lifted anywhere in
  a crate. The allowance is scoped to the two generated-code modules;
  hand-written `unsafe` in that crate is still refused, and every other
  crate keeps `forbid`.

### Documentation

- The Cloudflare adapter is no longer described as planned: four
  adapters ship, and `intro.md`, `architecture.md`, and the crate map
  say so.
- Authentication docs now state the real preference order —
  SCRAM-SHA-256 over PLAIN over LOGIN — with the bearer-token
  mechanisms opt-in per call. PIPELINING is documented alongside the
  other extensions.
- `errors.md` covers five `SmtpError` variants including `Policy`, the
  full `SmtpOp` list, and all four `AuthError` variants.
- `core.md` matches the real public surface, documents `Transport`'s
  four methods including `flush`, and no longer claims the crate has no
  external dependencies (the optional SCRAM crypto crates are the
  exception).
- `usage.md` points at the `wasm-smtp-test` mock, now a published
  dev-dependency, and at the self-contained example in
  `tests/public_api.rs`.
- `adapters/component-model.md` explains the `%from` escape and the
  vendored WASI packages; `wit/deps/README.md` names their source,
  version, and license.
- `CONTRIBUTING.md`'s repository layout lists all six crates, and its
  code-style section points at `crates/<crate>/src/tests/` rather than a
  path that has not existed for several releases.
- The WASI adapter no longer claims its helpers "return a compile-time
  error" on non-WASM targets; they are simply absent there.
- `README.md` documents every cargo feature, uses `"0.15"` in dependency
  snippets, and states the MSRV once. `NOTICE`, `CONTRIBUTING.md`, and
  the bug-report template list the current crate set.
- `CHANGELOG.md`: the 0.15.1 entry is translated to English, the two
  duplicated `[0.9.4]` headings are merged into one section with a note
  about the version offset, and the comparison links use the project's
  tag format (no `v` prefix) and cover 0.10.0 onward.

## [0.15.1] — 2026-05-11

### Added

- **`crates/wasm-smtp/src/client/` — module split.**
  `client.rs` (1743 lines) split into 5 files by responsibility:

  | File | Contents |
  |---|---|
  | `client/mod.rs` | The `SmtpClient` struct, `SmtpClientOptions`, `connect` / `quit`, session-state helpers |
  | `client/auth.rs` | `login`, `login_with`, `login_oauthbearer`, `login_xoauth2`, `run_auth_*` |
  | `client/send.rs` | `send_mail`, `send_mail_bytes`, `send_mail_stream`, `send_mail_smtputf8`, `send_message` |
  | `client/io.rs` | `read_greeting`, `send_ehlo`, `write_all`, `flush`, `read_reply`, the I/O buffer (all `pub(super)`) |
  | `client/starttls.rs` | `connect_starttls`, `starttls` |

- **Integration tests** (`crates/wasm-smtp/tests/public_api.rs`).
  9 tests that use the public API only. They define a self-contained
  `TestTransport` directly and carry no dev-dependency on
  `wasm-smtp-test`, which avoids a circular reference.

- **`docs/src/` subfolder layout.**
  The flat 16-file layout reorganised into 4 subfolders:

  | Folder | Contents |
  |---|---|
  | `concepts/` | architecture, protocol, errors, security |
  | `core/` | core, usage, composing-messages, connection-reuse, policy-audit, streaming |
  | `adapters/` | cloudflare, tokio, wasi, component-model (the `-adapter` suffix dropped) |
  | `reference/` | examples |

- **`.gitignore`** — excludes `target/`, `*.rs.bk`, `*.pdb`, `docs/book/`.
- **`.vscode/settings.json`** — `editor.formatOnSave: true`.
- **`.vscode/extensions.json`** — recommends `rust-lang.rust-analyzer`.

### Fixed

- **`cargo publish` failure resolved.**
  `wasm-smtp-test` uses `wasm-smtp` as a regular dependency while
  `wasm-smtp` referenced `wasm-smtp-test` as a dev-dependency, which
  created a circular reference. `wasm-smtp-test` was removed from
  `wasm-smtp`'s `[dev-dependencies]` and the integration tests were
  rewritten to be self-contained.

- **All compiler warnings resolved.** Fixed the unused-import,
  unused-variable, and dead_code warnings left in each file after the
  split:
  - `client/mod.rs`, `auth.rs`, `io.rs`, `send.rs`, `starttls.rs`:
    tidied up imports that became unnecessary after the move.
  - `wasm-smtp-wasi/src/tls.rs`, `error.rs`: added `#[cfg_attr]` for
    dead_code on non-wasm32 builds.
  - `wasm-smtp-wasi/Cargo.toml`: removed the redundant `version` key on
    `rustls-pki-types`.
  - `wasm-smtp-component/src/lib.rs`: suppressed an unused variable on
    the non-wasm32 path.

## [0.15.0] — 2026-05-11

### Added

- **`AUTH OAUTHBEARER` (RFC 7628)** — IETF-standard OAuth 2.0 SASL
  mechanism (`oauthbearer` feature, default-on).
  - `SmtpClient::login_oauthbearer(user, token)` convenience method.
  - `login_with(AuthMechanism::OAuthBearer, …)` explicit variant.
  - Wire format: `n,a={user},\x01auth=Bearer {token}\x01\x01` (GS2
    header + key-value pairs, per RFC 7628 §3).
  - Error-challenge path: server `334` → client sends `\x01` → final
    `535` is captured in `AuthError::Rejected`.
  - Shares `validate_oauth2_token` with `XOAUTH2`; `user` (authzid)
    may be empty.

- **SMTP PIPELINING (RFC 2920)** — `pipelining` feature, default-on.
  - `send_mail` detects `PIPELINING` in the server's EHLO capabilities
    and batches `MAIL FROM` + all `RCPT TO` + `DATA` into a single
    write followed by one flush, then reads all responses. Reduces RTTs
    from `3 + N` to `2` per transaction.
  - Falls back to the original sequential path for servers that do not
    advertise `PIPELINING`.
  - `Transport::flush()` default-implementation (no-op) added to the
    trait; adapters that buffer writes can override.
  - `protocol::ehlo_advertises_pipelining(caps)` helper exposed.

- **docs/ mdBook** — `docs/book.toml` in place; `SUMMARY.md` updated
  with new chapter structure. New pages:
  - `security.md` — threat model, credential handling, STARTTLS
    injection defence, dot-stuffing.
  - `policy-audit.md` — `SendPolicy`, `AuditSink`, `VecAuditSink`.
  - `streaming.md` — `send_mail_stream`, `MessageBody`, `DotStufferState`.
  - `wasi-adapter.md` — `wasm-smtp-wasi` usage, build, TLS roots.
  - `component-model.md` — `wasm-smtp-component`, WIT bindings for
    TypeScript / Go / Python.

- **README.md** updated: new Crates table (wasm-smtp-wasi,
  wasm-smtp-component rows), updated adapter description, updated
  Cargo features table (`oauthbearer`, `pipelining`), fixed badge
  label (`wasi-adapter`).

### Changed

- `Transport` trait gains `flush() -> Result<(), IoError>` with a
  default no-op implementation. This is a **non-breaking** change for
  existing `Transport` implementors.

### Tests

- 269 passing (core), 9 (wasi), 5 (component). +16 new tests:
  - `oauthbearer_tests.rs` (7 tests): protocol helpers, mechanism name,
    login success and failure paths.
  - `pipelining_tests.rs` (9 tests): capability detection, single/multi-
    recipient pipelining, sequential fallback, wire-order assertion.

## [0.14.0] — 2026-05-10

### Added

- **`wasm-smtp-component` crate** (RFC 018). WASM Component Model interface
  for `wasm-smtp`.

  - **`wit/smtp.wit`** — language-neutral WIT interface definition for the
    `smtp-send` interface. Defines `smtp-config`, `smtp-credentials`,
    `smtp-message`, `send-result`, and `send-error` types, plus the
    single `send` function. Compatible with any WIT-supporting language
    (TypeScript via jco, Go via wit-bindgen-go, Python via
    componentize-py, C/C++, etc.).

  - **`SmtpSendImpl`** — Rust implementation of the WIT export. Wraps
    `wasm-smtp-wasi` for WASI socket I/O and delegates all SMTP logic to
    `wasm-smtp`. On `wasm32-wasip2`: bindings generated by `wit-bindgen
    0.57`. On native hosts: hand-written type stubs for unit testing.

  - **5 native-host tests** (no WASM runtime required for `cargo test`).

  - **Build instructions** (requires `cargo-component` + `wasm32-wasip2`
    target):
    ```sh
    cargo component build --target wasm32-wasip2 -p wasm-smtp-component
    ```

  - **Language binding generation**:
    ```sh
    jco types wit/smtp.wit -o ./ts-types    # TypeScript
    wit-bindgen go wit/smtp.wit             # Go
    ```

- `SmtpCredentials` (stub) implements `Debug` with password redacted
  (`[REDACTED]`).

## [0.13.0] — 2026-05-10

### Added

- **`SmtpClient::send_mail_stream`** (RFC 019 Phase 3). Streaming DATA
  transmission: the message body is read from a [`MessageBody`] source in
  8 KB chunks, dot-stuffed incrementally, and written to the transport.
  Peak memory is O(chunk size) rather than O(body size).

- **`MessageBody` trait** (`wasm_smtp::message_body`). Project-defined async
  read abstraction (runtime-independent, no tokio dependency). Built-in
  implementations:
  - `SliceBody<'a>`: wraps `&[u8]`.
  - `StrBody<'a>`: wraps `&str`.

- **`DotStufferState`** (`wasm_smtp::DotStufferState`). Streaming dot-stuffer
  state machine. Correctly handles `.` at line starts across chunk boundaries.
  `process_chunk(&[u8]) -> Vec<u8>` + `finish() -> Vec<u8>` API.

### Notes

`send_mail_stream` passes `usize::MAX` to `SendPolicy::check_message_size`
because total body size is unknown. Callers that need precise size enforcement
should use `send_mail_bytes` instead.

## [0.12.0] — 2026-05-10

### Added

- **`wasm-smtp-wasi` crate** (RFC 016 + RFC 017). New adapter crate for
  `wasm32-wasip2` (WASI 0.2 Component Model) runtimes.

  - **`connect_smtps(host, port, ehlo_domain)`** — Implicit TLS (port 465):
    DNS lookup via `wasi:sockets/ip-name-lookup`, TCP connect via
    `wasi:sockets/tcp`, TLS handshake via rustls + ring + webpki-roots.
    Returns a ready-to-use `SmtpClient<WasiTlsTransport>`.
  - **`connect_smtp_starttls(host, port, ehlo_domain)`** — STARTTLS (port 587):
    plaintext TCP connect followed by in-place rustls upgrade after the SMTP
    `STARTTLS` handshake. Implements `StartTlsCapable`.
  - **`ConnectOptions`** — optional SNI override, custom root store, ALPN.
  - **TLS strategy** (RFC 017 Strategy A): rustls 0.23 + ring + webpki-roots.
    Certificate validation is enforced; no API to disable it.
  - **`plaintext-only` feature** for TLS-offload / test environments (clearly
    marked as not for production).
  - **9 native-host tests** via `MockTransport` + rustls unit checks; no WASI
    runtime required to run `cargo test -p wasm-smtp-wasi`.

- Added `crates/wasm-smtp-wasi` to the workspace.

### Notes

Building for the actual WASM target requires:

```sh
rustup target add wasm32-wasip2   # or equivalent apt package
cargo build --target wasm32-wasip2 -p wasm-smtp-wasi
```

Tests run on any platform without a WASI runtime:

```sh
cargo test -p wasm-smtp-wasi
```

## [0.11.0] — 2026-05-10

### Added

- **`SmtpClient::send_mail_bytes`**. Sends a message body supplied as
  `&[u8]` rather than `&str`. Identical semantics to `send_mail` —
  same dot-stuffing, same policy checks, same audit events — but skips
  the UTF-8 validity check on the input slice. Useful for payloads
  serialised by `mail-builder` and for binary-encoded content.

### Changed

- All large-message / dot-stuffing / many-recipient / max-line-length
  tests ported to use `send_mail_bytes` (previously tested only through
  `send_mail`). 239 tests total; 1 `#[ignore]` (10 MB body).

### Internals

- RFC 020 (large-message tests) and RFC 021 (no_std feasibility study)
  moved to `rfcs/done/`. RFC 021 conclusion: **defer** no_std support —
  no concrete IoT use case; WASI adapter (RFC 016) is the better path.

## [0.10.0] — 2026-05-10

This is the first release of the `wasm-smtp` extension development plan.
It introduces RFC-based design governance, extracts the test transport into
a dedicated crate, and adds two new core features: a pre-send policy hook
and an audit event model.

### Breaking

- **`SmtpError::Policy` variant added.** Code that matches exhaustively on
  `SmtpError` must add a `Policy(PolicyError)` arm. The enum is not marked
  `#[non_exhaustive]`; this is an intentional breaking change.
- **`SmtpClientOptions` required for policy/audit.** The new
  `SmtpClient::connect_with` entry point takes a `SmtpClientOptions`
  argument. `SmtpClient::connect` is unchanged and uses the defaults
  (allow-all policy, no-op audit sink).

### Added

- **`wasm-smtp-test` crate.** Extracts `MockTransport`, `block_on`, and
  `flatten` from the core's test-only harness into a standalone
  dev-dependency crate. Downstream crates can now use `MockTransport`
  for their own tests without copying code.

- **`SendPolicy` trait** (`crate::policy`). Application-defined pre-send
  validation: `check_sender`, `check_recipients`, `check_message_size`.
  Runs before any SMTP command is sent; rejection returns
  `SmtpError::Policy`. Ships with `DefaultPolicy` (allow all) and
  `BoundedPolicy` (configurable recipient count and message size caps).

- **`AuditSink` trait and `SmtpAuditEvent` enum** (`crate::audit`). Observe
  SMTP session milestones without credentials or message body exposure.
  Events: `Connected`, `GreetingReceived`, `EhloCompleted`, `TlsUpgraded`,
  `AuthCompleted { mechanism }`, `MailFromAccepted`, `RecipientAccepted`,
  `RecipientRejected`, `MessageAccepted`, `QuitCompleted`, `SessionAborted`.
  Ships with `NoopAuditSink` (default, zero overhead) and `VecAuditSink`
  (collects events for tests).

- **`SmtpClientOptions`** (`crate::SmtpClientOptions`). Builder for
  policy and audit sink configuration. Use with `SmtpClient::connect_with`.

- **`PolicyError`** (`crate::PolicyError`). New error type for policy
  rejections; wraps a caller-supplied message string.

- **`rfcs/` directory.** 24 RFC documents under a 5-folder lifecycle
  structure. RFC 000 (lifecycle policy) and RFC 003–015 are `Implemented`;
  RFC 001–002 are `Accepted` (v0.10.0 plan); RFC 011–012 are `Implemented`
  by this release; RFC 016–019 are `Proposed`; RFC 020–023 are `Draft`.

### Fixed

- `quit` now emits `SmtpAuditEvent::QuitCompleted` on clean close and
  `SessionAborted` on failure.

## [0.9.4] — 2026-05-02

Both blocks below shipped under tag `0.9.4`: the workspace content
planned as v0.10.0 was released externally as v0.9.4, so internal
milestone numbers and published tags are offset from here on.

This release introduces a single breaking change: the
`send_mail` family of methods now return [`SendOutcome`] instead
of `()`. Most callers will not need code changes; see "Migration
guide" below for the details.

### Breaking

- **`SendOutcome` return type for the `send_mail` family.** All
  three submission methods now return `Result<SendOutcome,
  SmtpError>` instead of `Result<(), SmtpError>`:

  - [`SmtpClient::send_mail`]
  - `SmtpClient::send_mail_smtputf8` (with the `smtputf8` feature)
  - `SmtpClient::send_message` (with the `mail-builder` feature)

  The `SendOutcome` struct exposes the SMTP reply code, the full
  server reply text, and a best-effort extraction of the server's
  queue identifier:

  ```rust,ignore
  pub struct SendOutcome {
      pub code: u16,
      pub server_message: String,
      pub queue_id: Option<String>,
  }
  ```

  Queue id extraction recognises the patterns used by Postfix
  (`Ok: queued as 4ABCDE12345`), Exim (`OK id=...`), and Stalwart
  (`Message queued with id ...`). Servers that do not emit a
  recognisable pattern (Microsoft Exchange / O365 most notably)
  yield `queue_id: None`; the verbatim reply is preserved in
  `server_message` for application-side parsing if needed. The
  extractor is intentionally conservative — better `None` than
  a fabricated string.

#### Migration guide

The most common pattern continues to work unchanged. The `?`
operator drops the `SendOutcome` on the floor for callers who
do not need it:

```rust,ignore
// v0.9.0~v0.9.3 and v0.9.4 — unchanged:
client.send_mail(from, to, body).await?;
client.send_message(from, to, msg).await?;
```

Callers using `match` need a one-character change in the `Ok`
arm:

```rust,ignore
// v0.9.0~v0.9.3:
match client.send_mail(from, to, body).await {
    Ok(()) => { /* ... */ }
    Err(e) => { /* ... */ }
}

// v0.9.4:
match client.send_mail(from, to, body).await {
    Ok(_) => { /* ... */ }              // or Ok(outcome) to use it
    Err(e) => { /* ... */ }
}
```

Callers with explicit return-type annotations need to update them:

```rust,ignore
// ~v0.9.3:
let r: Result<(), SmtpError> = client.send_mail(from, to, body).await;

// v0.9.4:
let r: Result<SendOutcome, SmtpError> = client.send_mail(from, to, body).await;
```

#### Why this is in 0.9.4 (not v0.9.0~0.9.3)

Adding fields to a return type widens the public API; we keep
that change behind a minor-major boundary so downstream
lockfile-less builds notice it. The change was prompted by
production-deployment feedback asking for the queue id to be
available for audit-log correlation with later DSN bounces —
the queue id was already coming over the wire in the
post-`DATA` reply text, but the previous API discarded it.

### Added

- **`SendOutcome` struct** at the crate root. Carries SMTP reply
  code, full reply text, and extracted queue id. Implements
  `Display`, `Debug`, `Clone`, `PartialEq`, `Eq`. The constructor
  `SendOutcome::new(code, server_message)` runs the queue-id
  extractor — public for callers writing custom client code on
  top of the lower-level protocol primitives.

- **Two new wire-level tests** verifying that the queue id is
  correctly extracted end-to-end: one with a Postfix-style 250
  reply, one with a Microsoft-style reply that omits the queue
  id (`queue_id` is `None`).

### Documentation

- New "Capturing the queue id and server response" section in
  `usage.md` showing the typical audit-logging pattern and how to
  drop the outcome with `?` when it is not needed.

- The four existing `match` examples in `connection-reuse.md`,
  `errors.md`, and `usage.md` updated from `Ok(())` to `Ok(_)`.

### Acknowledgements

This release closes the last item from the production-deployment
feedback documented in v0.9.4's acknowledgements section.

[`SendOutcome`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/struct.SendOutcome.html
[`SmtpClient::send_mail`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/struct.SmtpClient.html#method.send_mail


This release adds three observability and ergonomics improvements
based on production-deployment feedback. All changes are
non-breaking.

### Added

- **`tracing` cargo feature** on `wasm-smtp` (default-off). When
  enabled, the crate emits structured `tracing` events at the
  major SMTP transitions:

  | Event | Level |
  |---|---|
  | Connect / `EHLO` complete | `debug` |
  | `AUTH` start (auto-selected mechanism) | `debug` |
  | `AUTH` success | `debug` |
  | `AUTH`: no supported mechanism | `warn` |
  | `STARTTLS` upgrade requested / completed | `debug` |
  | `STARTTLS` extension not advertised | `error` |
  | `STARTTLS` buffer-residue defense triggered | `error` |
  | `MAIL FROM` accepted (with envelope sender) | `debug` |
  | `RCPT TO` accepted (per recipient) | `debug` |
  | `DATA` accepted | `debug` |
  | `QUIT` | `debug` |
  | Session moved to `Closed` on logical failure | `warn` |

  Events use `target = "wasm_smtp"` so `tracing-subscriber` can
  filter on this single name. Passwords, OAuth tokens, AUTH
  challenge bytes (server-first, server-final, SCRAM nonces),
  and message bodies are **never** logged at any level. Envelope
  addresses (`MAIL FROM` / `RCPT TO`) are logged at `debug` — they
  are already visible in the server's own SMTP log, so this is
  not new exposure.

  When the feature is disabled, all log calls compile to no-ops
  and `tracing` is not pulled into the dependency graph. Both
  adapter crates (`wasm-smtp-tokio`, `wasm-smtp-cloudflare`)
  expose a matching `tracing` pass-through feature.

- **`IoError` classification helpers** (pure additions; no
  breaking change). Useful for retry-decision logic in caller
  code:

  ```rust,ignore
  use wasm_smtp::SmtpError;
  match client.send_mail("from", &["to"], &body).await {
      Err(SmtpError::Io(e)) if e.is_timeout() => {
          // retry with backoff
      }
      Err(SmtpError::Io(e)) if e.is_connection_refused() => {
          // server down; switch to fallback host
      }
      // ...
  }
  ```

  New methods on [`IoError`]:
  - `io_kind() -> Option<std::io::ErrorKind>`: walks the
    [`std::error::Error::source`] chain and returns the kind of
    the first `std::io::Error` found, or `None` if the chain
    contains no `io::Error`. Useful for kinds not covered by a
    named helper (e.g. `NotFound` for missing certificates).
  - `is_timeout()`, `is_connection_refused()`,
    `is_connection_reset()`, `is_connection_aborted()`:
    convenience methods that wrap `io_kind()` for the most
    common retry-relevant kinds.

  All four `is_*` helpers correctly walk through nested error
  wrappers (e.g. when an adapter wraps an `io::Error` inside an
  intermediate error type before passing to `IoError`).

### Documentation

- New **"DKIM signing"** chapter section in
  `composing-messages.md`. DKIM (RFC 6376) is a message-layer
  concern, not an SMTP-layer concern, so `wasm-smtp` does not
  sign messages itself. The new section documents the
  recommended pairing with [Stalwart Labs's `mail-auth`] crate,
  including:
  - A minimum signing example (Ed25519 / RFC 8463).
  - Clarification of what DKIM does and does not protect
    (tamper-evidence and domain accountability — not
    confidentiality, not envelope-sender authentication).
  - Operational notes on key management, DNS publishing, and
    interaction with SPF/DMARC.

  No code changes; pure documentation.

[Stalwart Labs's `mail-auth`]: https://crates.io/crates/mail-auth

### Acknowledgements

Improvements in this release were prompted by deployment
feedback from a production user. The feedback distinguished
between requirements that fit a general SMTP client and those
that did not, which made the integration scope easy to define.

## [0.9.3] — 2026-04-29

This is a maintenance release. `getrandom` major bump for the
SCRAM-SHA-256 implementation. Native build is unchanged for
callers; **Cloudflare Workers deployments require one new
build-time configuration step** (see Documentation below).

### Changed

- **`getrandom` dependency floor: 0.2 → 0.4** (skipping the 0.3
  intermediate generation). The 0.2 → 0.4 jump is a deliberate
  jump of two majors:
  - 0.3 changed how `wasm32-unknown-unknown` targets configure
    their JS backend (cargo feature `js` → `wasm_js` + a rustc
    `--cfg` flag).
  - 0.4 removed several legacy backends and adjusted the public
    function naming (`getrandom::getrandom` → `getrandom::fill`).
  Adopting 0.4 directly captures both sets of changes in a single
  release.

  Code change: a single line in `crates/wasm-smtp/src/scram.rs`
  renamed `getrandom::getrandom(&mut bytes)` to
  `getrandom::fill(&mut bytes)`. Same semantics; same RFC 7677
  output (verified by the round-trip test, which still passes).

- **`wasm-smtp-cloudflare` features section reorganized.** The
  adapter's pass-through feature list now mirrors the main
  crate's feature surface in full:

  | Feature | 0.9.2 | 0.9.3 |
  |---|---|---|
  | `xoauth2` (default) | ✅ | ✅ |
  | `smtputf8` | ✅ | ✅ |
  | `mail-builder` | ❌ (gap) | ✅ |
  | `scram-sha-256` (default) | ❌ (gap) | ✅ |

  The gaps in 0.9.2 were oversights — callers depending only on
  `wasm-smtp-cloudflare` had to refer to the main crate's name to
  toggle these. Now the adapter exposes the full surface
  symmetrically with `wasm-smtp-tokio`.

  Additionally, the `scram-sha-256` feature pulls in `getrandom`
  with the `wasm_js` cargo feature enabled, so consumers do not
  need to add it manually to their `Cargo.toml` for Workers
  deployments.

### Documentation

- **New "Building for `wasm32-unknown-unknown`" section** in the
  Cloudflare adapter chapter. `getrandom` 0.4 requires both the
  `wasm_js` cargo feature (handled automatically by this adapter
  when SCRAM is enabled) AND a rustc `--cfg` flag in the
  consumer's `.cargo/config.toml`:

  ```toml
  [target.wasm32-unknown-unknown]
  rustflags = ['--cfg=getrandom_backend="wasm_js"']
  ```

  The flag is a `getrandom` 0.4 design decision that cannot be
  expressed as a cargo feature, so consumer-side configuration
  is unavoidable for SCRAM-enabled Workers deployments. The
  adapter chapter spells out the recipe.

  Consumers who disable `scram-sha-256` (e.g. only XOAUTH2
  for Gmail submission) avoid this configuration entirely;
  `getrandom` is not pulled into the dep graph in that case.

## [0.9.2] — 2026-04-29

This is a maintenance release. RustCrypto major bump for the
SCRAM-SHA-256 implementation. No API changes for callers.

### Changed

- **RustCrypto crates major bump** for the optional `scram-sha-256`
  feature on `wasm-smtp`:

  | Crate    | Floor 0.9.1 | Floor 0.9.2 |
  |----------|-------------|-------------|
  | `hmac`   | 0.12        | **0.13**    |
  | `sha2`   | 0.10        | **0.11**    |
  | `pbkdf2` | 0.12        | **0.13**    |

  These three crates upgrade in lockstep because they share the
  `digest` crate's trait surface, which underwent a major revision
  in this generation. The user-visible API change is small —
  `new_from_slice` moved from the `Mac` trait to the `KeyInit`
  trait — and is contained inside the SCRAM helper module. Crate
  consumers see no change.

  RFC 7677 §3 official SCRAM-SHA-256 test vector still round-trips
  correctly on the new crates, confirming that the cryptographic
  output is byte-identical between the old and new RustCrypto
  generations. This is the most important regression test for a
  crypto-dep major bump and it passes.

## [0.9.1] — 2026-04-29

This is a maintenance release. No new features, no behaviour
changes for callers using the default-features build.

### Changed

- **Renamed `crates/cloudflare/` to `crates/wasm-smtp-cloudflare/`**
  to align the directory name with the published crate name and
  with the `crates/wasm-smtp-tokio/` layout that landed in v0.7.0.
  No code or API change; the published crate name has been
  `wasm-smtp-cloudflare` since v0.4.0. Anyone tracking the
  workspace by path (e.g. via a git submodule or a path-based
  `[patch]`) needs to update their path; everyone using crates.io
  is unaffected.

- **`rustls-native-certs` dependency floor: 0.7 → 0.8** in
  `wasm-smtp-tokio`. The 0.8 release changed `load_native_certs()`
  to return a `CertificateResult` struct instead of
  `Result<Vec<CertificateDer>, io::Error>`; the new struct exposes
  loaded certs and per-source errors as separate fields, letting
  the caller pick the partial-failure policy. We adopt the same
  policy as before (accept any cert that decoded cleanly, fail
  hard only if the resulting trust store is empty), now with a
  slightly more informative error message when both conditions
  hit. No API change for callers; this is a transparent dependency
  bump.

- **`webpki-roots` dependency floor: 0.26 → 1.0** in
  `wasm-smtp-tokio`. The `webpki-roots` 0.26 line uses the
  "semver trick" to re-export 1.0, so the floor bump is
  source-compatible — no code changes were needed in the
  adapter. We bump the floor anyway to make the support level
  explicit and to signal that downstream lockfiles should
  resolve to the 1.x line.

### Documentation

- **Crate-level "Scope" section in `wasm-smtp` rewritten** to
  reflect current capabilities. The previous wording read
  "STARTTLS is intentionally out of scope for the initial
  release", which was accurate for v0.1.0 but became misleading
  once STARTTLS landed in v0.2.0. The replacement text states
  that both implicit TLS (port 465) and STARTTLS (port 587) are
  supported, and links to `SmtpClient::connect_starttls` and
  the `StartTlsCapable` trait. Triggered by an external
  bug-report-turned-clarification noting that the stale wording
  on the v0.6.0 docs.rs page was a real adoption hazard.

## [0.9.0] — 2026-04-29

This release implements **SCRAM-SHA-256 (RFC 5802 / RFC 7677)** as
a default-on authentication mechanism, completing Phase 12.

### Added

- **`AUTH SCRAM-SHA-256` support** behind the new `scram-sha-256`
  cargo feature (default-on). SCRAM is the modern challenge-response
  SASL mechanism: the client never transmits the password in
  plaintext, and the server proves possession of the salted hash
  through a signature step that the client verifies. The
  implementation passes the official RFC 7677 §3 test vector.

  ```rust,ignore
  // Auto-selection now prefers SCRAM-SHA-256:
  client.login("user@example.com", "secret").await?;

  // Or pin explicitly:
  client.login_with(AuthMechanism::ScramSha256, user, password).await?;
  ```

  Defenses included:
  - PBKDF2 iteration count clamped to `[4096, 600_000]` to defend
    against weak server policies and DoS attacks.
  - Server nonce MUST start with the client nonce we sent (replay
    defense per RFC 5802 §5.1).
  - Unknown SCRAM `m=` extensions reject the exchange.
  - Server signature verified with constant-time comparison
    (`subtle` crate) before the client returns success.
  - Both the standard 334-then-235 server response and the inline
    235-with-signature variant (Stalwart, some Postfix builds)
    are accepted.

- **`AuthMechanism::ScramSha256`** variant. The enum is
  `non_exhaustive`, so this is source-compatible: existing
  pattern-match code with a wildcard arm continues to compile.

- **`SmtpOp::AuthScramSha256`** variant for error attribution
  during `AUTH SCRAM-SHA-256` exchanges.

- **`AuthError::Other(&'static str)`** variant for surfacing
  SCRAM-specific protocol failures (server-nonce mismatch,
  iteration count out of bounds, server signature verification
  failure, malformed server messages). Callers should NOT
  pattern-match on the static-str content; it is a debug aid only.

- **`protocol::base64_decode`** is now public, mirroring the
  existing `base64_encode`. Standard RFC 4648 padded base64. Used
  internally by SCRAM but generally useful and worth exposing.

### Changed

- **`select_auth_mechanism` (and `SmtpClient::login` auto-selection)**
  now prefers `SCRAM-SHA-256` over `PLAIN` over `LOGIN` when the
  server advertises multiple mechanisms. Previously the order was
  `PLAIN > LOGIN`. This is a behaviour change for callers who use
  `login()` against servers that advertise both `SCRAM-SHA-256`
  and `PLAIN`: they will now use SCRAM. The change is a security
  improvement (passwords stop being transmitted in plaintext) and
  is the reason this is a minor bump rather than a patch bump.

  Callers who specifically need the old behaviour — for example,
  testing PLAIN-only paths — should use
  `login_with(AuthMechanism::Plain, ...)` explicitly. When
  `scram-sha-256` is disabled at the feature level, the selection
  function falls through to the previous PLAIN/LOGIN ordering.

- **`AuthError` is `non_exhaustive`** (already was) — the new
  `Other(&'static str)` variant fits without breaking pattern
  matches that use a wildcard arm.

- **`SmtpOp` is `non_exhaustive`** (already was) — same story for
  `AuthScramSha256`.

### Documentation

- New chapter section on SCRAM-SHA-256 in the protocol reference,
  documenting the four-message exchange, our defenses, and what's
  intentionally not implemented (channel binding / `-PLUS`,
  SCRAM-SHA-1, SASLprep normalization).

## [0.8.0] — 2026-04-29

This release bundles three Phase-12 follow-ups that landed together
because none of them was large enough to warrant its own version
bump.

### Added

- **`mail-builder` integration helper** (Phase 12 step 1). New
  `SmtpClient::send_message` method behind the `mail-builder` cargo
  feature accepts a `mail_builder::MessageBuilder` directly:

  ```rust,ignore
  client.send_message(
      "notify@example.com",
      &["alice@example.org"],
      MessageBuilder::new()
          .from("notify@example.com")
          .to("alice@example.org")
          .subject("hi")
          .text_body("hello"),
  ).await?;
  ```

  Equivalent to `let body = msg.write_to_string()?;
  client.send_mail(from, to, &body).await?` but skips the manual
  serialization step and preserves any `mail_builder` error as the
  `IoError` source chain.

  - Off by default — enable with `features = ["mail-builder"]`.
  - When disabled, `mail-builder` is not pulled into the
    dependency graph at all.
  - SMTP envelope vs. message-header semantics are unchanged from
    the manual `send_mail` path; the helper is purely a
    serialization shortcut.

- **`wasm-smtp-tokio`: `aws-lc-rs` and `ring` cargo features**
  (Phase 12 step 3). Callers can now switch the rustls crypto
  provider:

  - `aws-lc-rs` (default): AWS BoringSSL-derived provider.
    Best runtime performance, FIPS-compliant build paths.
    Compiles C code (~3-5 min on a clean build).
  - `ring`: the traditional rustls provider. Faster to compile,
    no C dependencies. Use if build time matters or if your
    target doesn't support `aws-lc-sys`.

  The two are mutually exclusive at build time: picking both, or
  neither, fails the build with a descriptive `compile_error!`.
  Same hard-error treatment is now applied to the existing
  `native-roots` / `webpki-roots` pair so configuration mistakes
  are caught at `cargo build` rather than as a runtime panic.

### Changed

- **Workspace bumped 0.7.1 → 0.8.0.** Adding new optional cargo
  features and exposing a new public method on `SmtpClient`
  warrants a minor bump under our 0.x versioning posture so
  downstream lockfile-less builds notice the change.
- **`tokio-rustls` is now configured with `default-features = false`**
  in the workspace dependency declaration so individual crypto
  providers can be selected through the `wasm-smtp-tokio`
  feature surface. Callers using `wasm-smtp-tokio` through its
  default features see no behaviour change (`aws-lc-rs` is still
  the default provider).

### Documentation

- New chapter **"Connection reuse"** (`docs/src/connection-reuse.md`,
  Phase 12 step 2) documents the existing connection-reuse pattern:
  one `SmtpClient` instance can submit multiple messages over a
  single authenticated session. Covers what state persists, idle
  timeouts, graceful failure handling, the `quit()` vs drop
  behaviour, why connection pooling is not built in, and the
  authentication-reuse subtlety. No code changes — the support
  has been there since Phase 1; the chapter just makes it
  discoverable.
- The "Composing messages" chapter now leads with the
  `send_message` shortcut for callers who enable the
  `mail-builder` feature, retaining the manual
  `write_to_string()? + send_mail` pattern as the explicit
  fallback.
- The Tokio adapter chapter rewrites its features table to cover
  both pairs of mutually-exclusive features (trust source,
  crypto provider) and lists four representative cargo
  configurations side-by-side.

## [0.7.1] — 2026-04-29

### Added

- **`IoError` source chain support (Phase 12).** Adapter crates can
  now preserve the underlying `io::Error`, rustls handshake error,
  etc. as the [`std::error::Error::source`] chain on
  [`wasm_smtp::IoError`]. Caller-side error formatters (anyhow's
  `{:#}`, eyre, manual `.source()` walks) see the full diagnostic
  while the high-level `Display` of `IoError` stays terse.
  - New constructor: `IoError::with_source(message, source)` accepts
    any `StdError + Send + Sync + 'static`.
  - New `From<std::io::Error> for IoError` conversion: `io_err.into()`
    produces an `IoError` carrying the original as its source.
  - `IoError` is now `Send + Sync` (its source field is
    `Box<dyn Error + Send + Sync>`), important for tokio-based
    adapters where errors may surface on a different worker
    thread than the one that observed them.
  - 6 new unit tests in `error_tests.rs` covering the new
    constructor, the `From<io::Error>` conversion, source-chain
    walking through `SmtpError → IoError → io::Error`, and the
    `Send + Sync` bounds at compile time.

### Changed

- **`wasm-smtp-tokio` adapter now preserves `io::Error` source.**
  The internal `map_io_err` helper switched from
  `IoError::new(static_context)` to
  `IoError::with_source(static_context, io_err)`, propagating the
  underlying TCP / TLS / handshake error into the source chain.
  No public API or behaviour change for callers using the adapter
  through its public API; the change is visible via `.source()`
  walks.
- **No changes to `IoError::new` or `IoError::message`.** Adapters
  not yet migrated to `with_source` continue to compile and behave
  as before — `new()` simply produces an `IoError` with no source.

### Documentation

- The `IoError` rustdoc carries a worked example showing how an
  adapter preserves an `io::Error` through the source chain.
- ROADMAP Phase 12 reflects this item as complete.

[`std::error::Error::source`]: https://doc.rust-lang.org/std/error/trait.Error.html#method.source
[`wasm_smtp::IoError`]: https://docs.rs/wasm-smtp/latest/wasm_smtp/struct.IoError.html

## [0.7.0] — 2026-04-29

### Added (Phase 11 — `wasm-smtp-tokio` adapter crate)

- **New sibling crate: `wasm-smtp-tokio`.** A production-quality
  `Transport` implementation for tokio + rustls, parallel to
  `wasm-smtp-cloudflare`. Lets axum / actix / warp / hyper / plain
  tokio servers connect to SMTP submission endpoints without writing
  the rustls + `tokio_rustls::TlsConnector` plumbing themselves.
  - `TokioTlsTransport::connect_implicit_tls(host, port, sni)` for
    implicit-TLS submission (port 465).
  - `TokioPlainTransport::connect(host, port, sni)` followed by
    `SmtpClient::connect_starttls(...)` for STARTTLS submission
    (port 587).
  - `ConnectOptions` builder for advanced cases: alternate SNI,
    custom root store (private CA, dev-only self-signed certs), ALPN.
  - Two cargo features for trust-anchor source: `native-roots`
    (default; `rustls-native-certs`) and `webpki-roots` (bundled
    Mozilla root set; for minimal/distroless containers). Mutually
    exclusive — pick one. Pass-through `xoauth2` and `smtputf8`
    features mirror the main crate.
  - Certificate validation is on by default and there is **no**
    public API to disable verification. Callers needing test
    convenience install a self-signed CA via
    `ConnectOptions::with_root_store`.
  - 9 unit tests covering builder ergonomics, error paths
    (unbound port, plaintext-server-on-TLS-port, invalid SNI), and
    pre/post-upgrade lifecycle.
  - 3 docstring examples in `lib.rs` (implicit TLS, STARTTLS,
    custom options).

### Changed

- **Workspace bumped 0.6.0 → 0.7.0.** Pure SemVer would not require
  a bump — adding a new crate at the same version line is
  technically additive. This release uses the bump anyway because
  it adds new top-level entries to `[workspace.dependencies]`
  (`tokio-rustls`, `rustls-pki-types`, `rustls-native-certs`,
  `webpki-roots`) which downstream lockfile-less builds will now
  resolve, and bumping to 0.7.0 makes that visible.
- `crates/cloudflare/Cargo.toml`'s pin on `wasm-smtp` is now
  `version = "0.7.0"`.
- Pure non-functional update for the `wasm-smtp` and
  `wasm-smtp-cloudflare` crates themselves — the bump tracks the
  workspace, but no public API or behaviour has changed in either.

### Documentation

- New `docs/src/composing-messages.md` chapter explaining the
  recommended path for callers who need to construct RFC 5322 / MIME
  message bodies before passing them to `SmtpClient::send_mail`.
  Recommends [`mail-builder`] as the composition partner — actively
  maintained by Stalwart Labs, no dependencies, RFC 5322 + RFC
  2045-2049 + automatic encoding selection. Covers the typical
  notification-email pattern, HTML + multipart, non-ASCII subjects
  (RFC 2047), the SMTPUTF8 capability interaction, and dot-stuffing
  responsibilities.
- Decision: **`wasm-smtp-message` will not be built.** The
  ecosystem already has `mail-builder` in this niche; rebuilding
  it would produce either a thin wrapper (no value) or a duplicate
  (continuing-maintenance cost). The new chapter documents the
  integration path instead.

[`mail-builder`]: https://docs.rs/mail-builder

## [0.6.0] — 2026-04-28

### Changed (breaking)

- **Crate renamed: `wasm-smtp-core` → `wasm-smtp`.** This crate is the
  main, externally-facing library — the public API
  (`SmtpClient`, `Transport`, `Reply`, the error types, etc.) lives
  here, and direct dependents (non-Cloudflare adapters, custom
  `Transport` implementations, host-tooling tests) all consume it
  directly. Calling it `-core` was an artifact of the early workspace
  layout and gave the misleading impression that `wasm-smtp-core` and
  `wasm-smtp-cloudflare` were peer-tier crates. They are not:
  `wasm-smtp` is the main library, `wasm-smtp-cloudflare` is one
  adapter for it. The rename makes that hierarchy obvious in the
  Rust-ecosystem-conventional way (`serde` / `serde_json`,
  `tokio` / `tokio-util`, `tracing` / `tracing-subscriber`, …).

  **Migration for callers depending on the main crate:**

  ```toml
  # before
  [dependencies]
  wasm-smtp-core = "0.5"

  # after
  [dependencies]
  wasm-smtp = "0.6"
  ```

  ```rust
  // before
  use wasm_smtp_core::{SmtpClient, Transport, SmtpError};

  // after
  use wasm_smtp::{SmtpClient, Transport, SmtpError};
  ```

  Callers depending only on `wasm-smtp-cloudflare` need no source
  changes — the adapter re-exports the public API of `wasm-smtp`
  exactly as before.

- **Workspace member directory renamed: `crates/core/` → `crates/wasm-smtp/`.**
  This is internal — it does not affect any package on crates.io —
  but it keeps the directory name consistent with the package name.

- **Workspace version bumped 0.5.1 → 0.6.0.** Pure Rust SemVer would
  not require a major (0.x) bump for a crate-name change because it
  is technically a different package. This release uses the bump
  anyway to make the discontinuity unmissable in dependency
  resolution: a caller upgrading mechanically will fail to find
  `wasm-smtp-core 0.6` and will see the failure immediately rather
  than silently picking up an unrelated 0.5.x.

### Notes

- All historical changelog entries below this section reference
  `wasm-smtp-core` as the crate's name at the time those releases
  shipped; that text is preserved as historical record.
- ROADMAP, README, and the `docs/` book have all been updated to use
  the new name.

## [0.5.1] — 2026-04-28

### Changed (Phase 10 — test-suite layout & dependency hygiene)

This is a non-functional refactor: no public API changes, no behaviour
changes.

- **In-tree test files split.** The 2,950-line `crates/core/src/tests.rs`
  has been split into a `crates/core/src/tests/` directory with one
  file per sub-module (`harness.rs`, `protocol_tests.rs`,
  `session_tests.rs`, `error_tests.rs`, `client_tests.rs`,
  `smtputf8_tests.rs`). The previous in-file sub-module structure is
  preserved exactly; only the physical file boundaries have changed.
  `crates/cloudflare/src/tests.rs` (305 lines) gets the same treatment
  for consistency: `tests/io_tests.rs` and
  `tests/e2e_via_tokio_mock.rs`. Tests remain in-tree (rather than
  being moved to a top-level `tests/` integration-test directory) so
  they can continue to reach `pub(crate)` items and module-private
  helpers without inflating the public API surface.
- **Centralised dependency floors via `[workspace.dependencies]`.**
  `tokio`, `tokio-test`, and `worker` are now declared at the
  workspace root with explicit minimum versions (`tokio >= 1.38`,
  `tokio-test >= 0.4.3`, `worker = 0.8`). Member crates inherit them
  with `{ workspace = true, features = [..] }`. The previous bare
  `tokio = "1"` would, in adversarial resolution scenarios, allow
  selecting tokio < 1.23.1, which is the patched floor for
  RUSTSEC-2023-0001 (`reject_remote_clients` configuration corruption
  on Windows named pipes). This crate does not use the affected
  `tokio::net::windows::named_pipe` API at all (we run on WASM /
  Cloudflare Workers, with `tokio` enabled only for `io-util`), so
  the floor is precautionary defence-in-depth — but it documents the
  intent and protects callers depending on lockfile-less builds.

## [0.5.0] — 2026-04-28

### Added (security hardening — Phase 9)

This release is a security-focused minor bump. Following an internal
audit, eight findings were addressed across the SMTP, WASM, and
general internet-security threat surfaces. None of the findings were
rated critical or high; the changes below collectively raise the
defensive posture of the crate.

- **STARTTLS injection defense (RFC 3207 §5).** The new
  `ProtocolError::StartTlsBufferResidue { byte_count }` variant is
  raised when bytes remain in the receive buffer at the moment of
  TLS upgrade. This is the signature of a CVE-2011-1575-class
  attack: an attacker pipelines additional SMTP commands onto the
  plaintext channel after the `220` reply, hoping the client will
  read them after the upgrade and treat them as authenticated
  post-TLS traffic. `SmtpClient::starttls` and
  `SmtpClient::connect_starttls` now refuse to upgrade and close
  the session in this case rather than silently absorbing the
  injected bytes.
- **RFC 5321 length limits enforced in address validation.**
  `validate_address` and `validate_address_utf8` now reject:
  - addresses longer than 254 octets total (§4.5.3.1.3),
  - local-parts longer than 64 octets (§4.5.3.1.1), and
  - domains longer than 255 octets (§4.5.3.1.2).

  Three new public constants — `MAX_ADDRESS_LEN`,
  `MAX_LOCAL_PART_LEN`, `MAX_DOMAIN_LEN` — expose these values for
  callers that want to validate before invocation.
- **`validate_login_username` / `validate_login_password` are now
  thin aliases for the corresponding `validate_plain_*` functions.**
  Previously they performed only an empty-string check, which would
  accept NUL bytes and other characters that corrupt SASL framing
  on the post-base64 server side. The aliases preserve source
  compatibility for v0.4.x callers; new code should call the
  `validate_plain_*` functions directly.

### Documentation

- `Reply::joined_text` documents that the returned text may contain
  `\n`, with explicit guidance for log-handler implementors to
  escape newlines and avoid log injection.
- `SmtpError`'s top-level doc carries a "Logging caveat" section
  explaining that `Display` output embeds server reply text, which
  may include envelope addresses or other PII. Suggests structured-
  field logging instead.
- `SmtpClient::login` documents that the crate does not retain
  credentials after the call, with a "Credential lifetime and
  zeroization" section pointing callers to the `zeroize` crate for
  caller-side memory hygiene.
- `Transport` trait gains a "Security responsibilities of
  implementors" section explicitly requiring certificate-chain
  validation, hostname matching, and no-fallback handshake failure
  semantics. Aimed at out-of-tree adapter authors.
- `SmtpClient::send_mail` carries a "Body size" note: the crate
  does not impose a body-length limit; callers should enforce
  application-appropriate caps and respect any `SIZE` advertised
  by the server (RFC 1870).

### Changed

- `ProtocolError` is `non_exhaustive`, so adding the
  `StartTlsBufferResidue` variant is not a SemVer-breaking change
  for callers using a wildcard arm in their pattern matches.
- The MockTransport test harness's `with_starttls` constructor now
  takes separate `pre_chunks` / `post_chunks` parameters, modelling
  real-server behavior where post-TLS reply bytes are not delivered
  on the plaintext channel. This affects internal tests only;
  callers do not depend on `MockTransport`.

## [0.4.0] — 2026-04-27

### Added

- **Phase 7 — `SMTPUTF8` (RFC 6531) — feature-gated.**
  - New `smtputf8` cargo feature, **off by default**. The crate's
    first feature flag, intended primarily to keep WASM bundle size
    down for the common case of ASCII-only submission.
  - `SmtpClient::send_mail_smtputf8(from, to, body)` for sending
    with the `SMTPUTF8` ESMTP parameter on `MAIL FROM`. Available
    only when the feature is enabled.
  - `protocol::validate_address_utf8` — Unicode-permissive address
    validator. Rejects only structural hazards: CR/LF/NUL, ASCII
    `<`/`>`, ASCII whitespace, ASCII control characters
    (C0 + DEL), and C1 control characters (U+0080-U+009F).
    Everything else, including non-Latin scripts and IDEOGRAPHIC
    SPACE U+3000, is accepted.
  - `protocol::ehlo_advertises_smtputf8` capability inspection.
  - `protocol::format_mail_from_smtputf8` for the `MAIL FROM:<addr>
    SMTPUTF8\r\n` wire form.
  - `wasm-smtp-cloudflare` exposes a matching `smtputf8` feature
    that pass-through-enables it in `wasm-smtp-core`, so adapter-
    only callers do not need to depend on the core crate by name
    to opt in.
  - No silent fallback: if the server does not advertise
    `SMTPUTF8`, `send_mail_smtputf8` returns
    `ProtocolError::ExtensionUnavailable { name: "SMTPUTF8" }` and
    closes the session.
- **Phase 8 — `xoauth2` cargo feature (default-on).**
  - `SmtpClient::login_xoauth2`, the `XOAuth2` arm of `login_with`,
    and the `protocol::build_xoauth2_initial_response` /
    `validate_xoauth2_user` / `validate_oauth2_token` helpers are
    now gated behind the new `xoauth2` cargo feature (default-on).
    Callers that do not authenticate via Gmail / Microsoft 365 OAuth
    can disable this feature with `default-features = false` to
    drop roughly 250 LOC of protocol code from their WASM bundle.
  - The `AuthMechanism::XOAuth2` and `SmtpOp::AuthXOAuth2` enum
    variants remain present in either configuration. Both enums are
    `non_exhaustive`, so the default-on→opt-in transition is not a
    SemVer-breaking change.
  - When the feature is disabled, calling `login_with(XOAuth2, ..)`
    or `login_xoauth2` fails fast with a clear `InvalidInputError`
    rather than a confusing "not advertised" mechanism error.
  - `AuthError::UnsupportedMechanism`'s Display message now
    reflects the active feature configuration (mentions XOAUTH2
    when the feature is enabled, omits it otherwise).

### Changed

- `validate_address`'s doc comment now explicitly notes that UTF-8
  addresses require the `smtputf8` feature. The function's behavior
  is unchanged from v0.3.0.

## [0.3.0] — 2026-04-27

### Added

- **Phase 6 — `ENHANCEDSTATUSCODES` (RFC 2034 / 3463).**
  - New public type `EnhancedStatus { class, subject, detail }` with
    `Display`, `to_dotted()`, and structured field access.
  - `ProtocolError::UnexpectedCode` gains an `enhanced:
    Option<EnhancedStatus>` field. The Display impl renders the code
    in square brackets between the basic code and the message:
    `during MAIL FROM, expected 2xx response but received 550
    [5.7.1]: relay access denied`.
  - `AuthError::Rejected` gains an `enhanced: Option<EnhancedStatus>`
    field, so callers can distinguish (e.g.) `5.7.8` from `5.7.9`
    without parsing reply text.
  - `Reply::enhanced()` and `Reply::message_text()` (the latter
    returns the reply text with the enhanced prefix stripped, for
    human-friendly display).
  - `protocol::ehlo_advertises_enhanced_status_codes` capability
    inspection helper.
  - Parsing is gated on EHLO advertisement: a stray
    `class.subject.detail`-shaped substring in a reply from a server
    that did not advertise the extension is not parsed.
- **Phase 6 — `AUTH XOAUTH2` (Google / Microsoft OAuth 2.0).**
  - `SmtpClient::login_xoauth2(user, access_token)` for opt-in
    OAuth 2.0 bearer-token authentication.
  - `AuthMechanism::XOAuth2` variant, `SmtpOp::AuthXOAuth2` for
    error tagging.
  - Full handling of the RFC 7628 §3.2.3 two-step error flow: on a
    `334` reply during XOAUTH2, the client sends an empty
    continuation line, reads the final 5xx, and surfaces it as
    `AuthError::Rejected` with the provider's diagnostic preserved.
  - `protocol::build_xoauth2_initial_response`,
    `protocol::validate_xoauth2_user`,
    `protocol::validate_oauth2_token` public helpers.
  - `select_auth_mechanism` deliberately does NOT pick XOAUTH2 even
    when advertised: bearer tokens have different semantics from
    static passwords and must be passed in explicitly.

### Changed

- `AuthError` is now `non_exhaustive`. This is a SemVer-incompatible
  change for callers that pattern-match on the enum without a
  wildcard arm, hence the minor bump from 0.2.0 to 0.3.0 under the
  pre-1.0 versioning convention.
- `AuthError::UnsupportedMechanism`'s Display message now lists all
  three supported mechanisms (PLAIN, LOGIN, XOAUTH2) rather than
  just the two it covered before.
- `Reply` now has a private `enhanced` field; constructed via
  `Reply::new(code, lines)` rather than struct literal. External
  callers that built `Reply` directly (an unusual pattern, but
  technically possible) will need to switch to the constructor.

## [0.2.0] — 2026-04-27

### Added

- **Phase 5 — STARTTLS support (RFC 3207).**
  - New `StartTlsCapable: Transport` trait for transports that can be
    upgraded to TLS in-place. Transports that connect with Implicit
    TLS (port 465) need not implement it.
  - `SmtpClient::starttls(&mut self)` — explicit STARTTLS upgrade on
    a connected client.
  - `SmtpClient::connect_starttls(transport, ehlo_domain)` —
    convenience entry point that performs greeting, `EHLO`,
    `STARTTLS`, transport upgrade, and re-`EHLO` per RFC 3207 §4.2
    in a single call.
  - `SessionState::StartTls` variant to model the
    `Authentication → StartTls → Ehlo` transition.
  - `ProtocolError::ExtensionUnavailable { name: &'static str }` for
    the case where `STARTTLS` was requested but not advertised.
  - `SmtpOp::StartTls` so protocol errors during the upgrade
    handshake are tagged like every other SMTP step.
  - `protocol::ehlo_advertises_starttls` capability inspection helper.
- **`wasm-smtp-cloudflare`:**
  - `connect_starttls(host, port)` — open a plaintext socket
    pre-configured for in-place TLS upgrade
    (`SecureTransport::StartTls`).
  - `connect_smtp_starttls(host, port, ehlo_domain)` — one-call
    STARTTLS connect, greeting, `EHLO`, upgrade, re-`EHLO`.
  - `StartTlsCapable` impl on `CloudflareTransport` that drives the
    `worker::Socket::start_tls()` consume-and-replace upgrade.

### Changed

- `SessionState` and `ProtocolError` are now `non_exhaustive`. This is
  a SemVer-incompatible change for callers that pattern-match on these
  enums without a wildcard arm — hence the minor bump from 0.1.0 to
  0.2.0 under the pre-1.0 versioning convention. Callers using
  `match … { … _ => … }` are unaffected.
- `CloudflareTransport`'s inner socket is now held in an `Option`
  internally so that `Socket::start_tls()` (which consumes `self`)
  can be called from a `&mut self` method. `into_inner()` now
  returns `Option<Socket>`. The change is invisible to read/write
  code paths.

## [0.1.0] — 2026-04-27

### Added

- `wasm-smtp-core` v0.1.0: SMTP state machine, response parser, command
  formatter, dot-stuffing, base64 helper, and `AUTH LOGIN` flow.
- `Transport` async trait as the only I/O contract.
- Error taxonomy: `IoError`, `ProtocolError`, `AuthError`,
  `InvalidInputError`.
- `SessionState` with an explicit transition table.
- In-tree synchronous mock transport for unit and integration tests.
- `wasm-smtp-cloudflare` v0.1.0: Cloudflare Workers socket adapter.
  - `CloudflareTransport` wrapping `worker::Socket`.
  - `connect_implicit_tls(host, port)` — Implicit TLS on the caller's
    port (typically 465) via `SecureTransport::On`.
  - `connect_smtps(host, port, ehlo_domain)` — one-call connect,
    greeting, and `EHLO` returning a ready-to-use client.
  - Adapter-level unit tests against `tokio_test::io::Builder`,
    including a full authenticated SMTP transaction over the mock.
- **Phase 4 hardening:**
  - `AUTH PLAIN` (RFC 4616) using the initial-response form (one
    round-trip).
  - `AuthMechanism` enum, re-exported at the crate root.
  - `SmtpClient::login_with(mechanism, user, pass)` for explicit
    mechanism selection.
  - `protocol::select_auth_mechanism` and
    `protocol::build_auth_plain_initial_response` public helpers.
  - `protocol::validate_plain_username` /
    `protocol::validate_plain_password` reject NUL bytes that would
    corrupt SASL framing.
  - `AuthError::UnsupportedMechanism` Display now lists the supported
    mechanisms (`PLAIN` and `LOGIN`) so operators can diagnose
    incompatibilities directly from the error message.
  - `SmtpOp` enum and a new `during: SmtpOp` field on
    `ProtocolError::UnexpectedCode`. Errors now identify the exact
    SMTP step that failed: "during MAIL FROM, expected 2xx response
    but received 550: …" rather than just "550".
  - Worked `examples.md` covering contact-form delivery, transactional
    alerts, multi-recipient messages, and connection reuse.
- Project-level `ROADMAP`, `TERMS_OF_USE`, `NOTICE`, GitHub policy
  documents (`SECURITY`, `CODE_OF_CONDUCT`, `CONTRIBUTING`,
  `ISSUE_TEMPLATE`).
- Long-form documentation under `docs/src` (mdBook-ready structure).

### Changed

- `SmtpClient::login` now auto-selects the best mechanism advertised
  by the server, preferring `PLAIN` over `LOGIN`. Servers that
  advertise only `LOGIN` continue to work unchanged.

[Unreleased]: https://github.com/nabbisen/wasm-smtp/compare/0.17.2...HEAD
[0.17.2]: https://github.com/nabbisen/wasm-smtp/compare/0.17.1...0.17.2
[0.17.1]: https://github.com/nabbisen/wasm-smtp/compare/0.17.0...0.17.1
[0.17.0]: https://github.com/nabbisen/wasm-smtp/compare/0.16.1...0.17.0
[0.16.1]: https://github.com/nabbisen/wasm-smtp/compare/0.16.0...0.16.1
[0.16.0]: https://github.com/nabbisen/wasm-smtp/compare/0.15.2...0.16.0
[0.15.2]: https://github.com/nabbisen/wasm-smtp/compare/0.15.1...0.15.2
[0.15.1]: https://github.com/nabbisen/wasm-smtp/compare/0.15.0...0.15.1
[0.15.0]: https://github.com/nabbisen/wasm-smtp/compare/0.14.0...0.15.0
[0.14.0]: https://github.com/nabbisen/wasm-smtp/compare/0.13.0...0.14.0
[0.13.0]: https://github.com/nabbisen/wasm-smtp/compare/0.12.0...0.13.0
[0.12.0]: https://github.com/nabbisen/wasm-smtp/compare/0.11.0...0.12.0
[0.11.0]: https://github.com/nabbisen/wasm-smtp/compare/0.10.0...0.11.0
[0.10.0]: https://github.com/nabbisen/wasm-smtp/compare/0.9.4...0.10.0
[0.9.4]: https://github.com/nabbisen/wasm-smtp/compare/0.9.3...0.9.4
[0.9.3]: https://github.com/nabbisen/wasm-smtp/compare/0.9.2...0.9.3
[0.9.2]: https://github.com/nabbisen/wasm-smtp/compare/0.9.1...0.9.2
[0.9.1]: https://github.com/nabbisen/wasm-smtp/compare/0.9.0...0.9.1
[0.9.0]: https://github.com/nabbisen/wasm-smtp/compare/0.8.0...0.9.0
[0.8.0]: https://github.com/nabbisen/wasm-smtp/compare/0.7.1...0.8.0
[0.7.1]: https://github.com/nabbisen/wasm-smtp/compare/0.7.0...0.7.1
[0.7.0]: https://github.com/nabbisen/wasm-smtp/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/nabbisen/wasm-smtp/compare/0.5.1...0.6.0
[0.5.1]: https://github.com/nabbisen/wasm-smtp/compare/0.5.0...0.5.1
[0.5.0]: https://github.com/nabbisen/wasm-smtp/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/nabbisen/wasm-smtp/compare/0.3.0...0.4.0
[0.3.0]: https://github.com/nabbisen/wasm-smtp/compare/0.2.0...0.3.0
[0.2.0]: https://github.com/nabbisen/wasm-smtp/compare/0.1.0...0.2.0
[0.1.0]: https://github.com/nabbisen/wasm-smtp/releases/tag/0.1.0
