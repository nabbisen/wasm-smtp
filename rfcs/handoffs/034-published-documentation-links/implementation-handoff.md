# Developer Handoff — RFC 034: Links that work where the documentation is published

**Governing RFC.** [`../../done/034-published-documentation-links.md`](../../done/034-published-documentation-links.md). Read *Decisions at acceptance* and the *Amendment* first.
**Prepared.** 2026-09-13 by the architect.
**Starts.** Now.
**Target release.** **0.18.0**, before it is tagged. The release commit is redone as the last commit (S13). Release approval remains the owner's.
**Review request goes to.** `.git-exclude/review-request/034-published-documentation-links.md`

## 0. History: already arranged for you

The architect has already moved the branch. Its tip is the commit
accepting RFC 034. Below it are the architect's record-keeping commits,
then **S8 `78495e7`**. The previous release commit **`7c446aa` is no
longer on the branch**. It is not lost: S13 re-applies it by hash, as last
time. Do **not** reset anything. Work on the tip as it is.

## 1. Purpose

Make every link resolve where it is read (GitHub, crates.io, docs.rs, and
the book), publish the documentation docs.rs is missing, and make the gate
catch a broken link before a reader does.

## 2. What is already known

Every item was run by the architect, not reasoned. Check anything you
rely on.

**crates.io.**
- `wasm-smtp`, `wasm-smtp-tokio`, and `wasm-smtp-cloudflare` publish the
  root `README.md` (`readme = "../../README.md"`). The other three
  published crates have their own.
- crates.io rewrites relative links to
  `https://github.com/nabbisen/wasm-smtp/blob/HEAD/crates/<crate>/<link>`.
  That was seen in the README crates.io serves for `wasm-smtp` and
  `wasm-smtp-tokio` 0.17.2.
- Broken as a result: the License badge (`README.md:3`, target `LICENSE`),
  `[docs/src]` (definition line 157, used once), and `[TERMS_OF_USE.md]`
  (line 160, used at line 144).
- `[LICENSE]` (158) and `[NOTICE]` (159) are defined but never used.
- The default branch is `main`.

**docs.rs item paths.** `wasm_smtp`'s modules are public, so rustdoc
documents each item at its module path. A local `cargo doc` produces the
same paths docs.rs serves. `latest/wasm_smtp/trait.Transport.html` is 404;
`…/transport/trait.Transport.html` is 200. The ten broken links and their
correct targets:

| Location | Wrong target | Correct target |
|---|---|---|
| `README.md:156` | `wasm_smtp/trait.Transport.html` | `wasm_smtp/transport/trait.Transport.html` |
| `crates/wasm-smtp-cloudflare/src/lib.rs:74` | same | same |
| `docs/src/adapters/wasi.md:137` | same | same |
| `docs/src/concepts/security.md:120` | same | same |
| `docs/src/concepts/protocol.md:322–324` | `wasm_smtp/struct.SmtpClient.html#method.login…` | `wasm_smtp/client/struct.SmtpClient.html#method.login`, `#method.login_with`, `#method.login_xoauth2` |
| `docs/src/concepts/protocol.md:329` | `wasm_smtp/struct.EnhancedStatus.html` | `wasm_smtp/protocol/struct.EnhancedStatus.html` |
| `docs/src/concepts/protocol.md:381` | `protocol/fn.validate_address_utf8.html` | unchanged. It 404s only because docs.rs lacks `smtputf8`, which D3 fixes |
| `docs/src/core/usage.md:250` | `wasm_smtp/struct.SendOutcome.html` | `wasm_smtp/outcome/struct.SendOutcome.html` |
| `CHANGELOG.md:943–944` | `struct.SendOutcome.html`, `struct.SmtpClient.html#method.send_mail` | `outcome/…`, `client/…#method.send_mail` |

All the correct targets, and the anchors `method.login`, `method.login_with`,
`method.login_xoauth2`, `method.send_mail`, and `method.send_mail_smtputf8`,
exist in a local `cargo doc` build (rustdoc writes anchors as
`id="method.<name>"`).

**Rustdoc.**
- The combined doc build in RFC 034 D5 succeeds under `--locked` for the
  five host crates, with the core's features written as
  `wasm-smtp/<feature>`.
- With `RUSTDOCFLAGS="-D warnings"` it fails with exactly four errors:
  `client/send.rs:348` (`MessageBody` not in scope),
  `client/starttls.rs:55` (`InvalidInputError` not in scope), and
  `message_body.rs:1`, `:45` (`SmtpClient` not in scope).
- `cargo doc -p wasm-smtp-wasi --target wasm32-wasip2` with `-D warnings`
  passes, and documents `connect_smtps`, `connect_smtps_with`,
  `connect_smtp_starttls`, and `connect_smtp_starttls_with`. Built on the
  host, it warns three times, because those functions are
  `cfg(target_arch = "wasm32")`.

**docs.rs.**
- No crate has `[package.metadata.docs.rs]`.
- `wasm-smtp-wasi` 0.17.2's `all.html` lists no functions.
- docs.rs's metadata page states that `targets` accepts any
  rustup-supported target, and that its first entry is the default when
  `default-target` is unset.

## 3. Change scope

The files in RFC 034's **Touches** line, a new check under `tools/` with
fixtures under `tools/guard-tests/`, `.github/workflows/ci.yml`,
`.github/CONTRIBUTING.md`, and `CHANGELOG.md`.

## 4. Non-change scope

- No change to any public item, visibility, or re-export to make a URL
  work. The links follow the code, not the reverse.
- No link text or prose rewritten. Only link targets change, and the two
  unused definitions are removed.
- No `#[allow(rustdoc::broken_intra_doc_links)]` or equivalent, anywhere.
- No network access in the gate.
- `rfcs/**` untouched. `wasm-smtp-component`'s docs.rs target unchanged.
- Do not tag, push, or publish. Do not edit `rfcs/README.md`.

## 5. Slices

Each slice gets its own commit, and the full gate must pass at every one.

### S9. Links and docs.rs metadata (D1, D2, D3, D6)

1. `README.md`: the badge target, `[docs/src]`, and `[TERMS_OF_USE.md]`
   become `https://github.com/nabbisen/wasm-smtp/blob/main/LICENSE`,
   `…/tree/main/docs/src`, and `…/blob/main/TERMS_OF_USE.md`. Remove the
   unused `[LICENSE]` and `[NOTICE]` definitions.
2. The table in §2, every row, with the correct target exactly as shown.
3. `crates/wasm-smtp/Cargo.toml`: `[package.metadata.docs.rs]`, with
   `features = ["smtputf8", "mail-builder", "tracing"]` and a one-line
   comment saying it is gate command 5's set.
4. `crates/wasm-smtp-wasi/Cargo.toml`: `[package.metadata.docs.rs]`, with
   `targets = ["wasm32-wasip2"]` and a comment saying why (the connect API
   exists only on wasm32).
5. `cargo package --list` for both crates still passes, and neither metadata
   table changes what gets packaged.

### S10. Intra-doc links, and rustdoc in the gate (D5)

1. Fix the four links with module-resolvable paths: `crate::MessageBody`,
   `crate::InvalidInputError` (or the `error` path), and
   `crate::SmtpClient::send_mail_stream`. Prose stays as it is.
2. Two gate commands after command 22, in `ci.yml` and in
   `CONTRIBUTING.md`, exactly as RFC 034 D5 writes them. In CI, set
   `RUSTDOCFLAGS: -D warnings` in the **step's** `env`, not the job's. It
   must not reach `cargo test`, whose doctests also read `RUSTDOCFLAGS`.
3. **Demonstrate failure:** revert one of the four fixes. The host doc
   command must go red, naming the line. Revert the revert.

### S11. The link check (D4)

1. `tools/check-doc-links.sh`, a POSIX shell script like the other two
   guards (or a Rust tool, argued in the request). It reads only local
   files and runs after the doc commands. It exits 0 silently when clean;
   otherwise it prints `file:line: <problem>` for each violation and
   exits 1. Missing prerequisites, such as an absent `target/doc`, exit 2
   with a reason.
2. **Published READMEs.** For each crate under `crates/` whose manifest is
   not `publish = false`, take the `readme` file. Resolve every relative
   link, inline `](…)` and reference definitions `[…]: …`, from **that
   crate's directory**. Fail if the target does not exist. Skip
   `http(s):`, `mailto:`, fragment-only links, and fenced code blocks.
3. **docs.rs links.** In `README.md`, `crates/*/README.md`,
   `docs/src/**/*.md`, `crates/*/src/**/*.rs`, and `CHANGELOG.md`, each
   `https://docs.rs/<crate>/<version>/<ident>/<path>.html[#<anchor>]`
   maps to `target/doc/<ident>/<path>.html`. For `wasm_smtp_wasi` it maps
   to `target/wasm32-wasip2/doc/…`. The file must exist and, with an
   anchor, contain `id="<anchor>"`. A docs.rs link to a crate outside this
   workspace is skipped, and says so in the script's header.
4. **Fixtures** in `tools/guard-tests/doc-links/<case>/`: a minimal tree
   with a fake `target/doc`, run by the existing runner. Cases, at least:
   - clean;
   - a README relative link broken from the crate directory but valid
     from the root, which must fail, since this is the defect;
   - a wrong docs.rs path;
   - a missing anchor;
   - a wasi link resolved from the wasip2 tree;
   - a link inside a code fence, which is ignored;
   - a `publish = false` crate, which is ignored;
   - no `target/doc`, which exits 2.
5. **Gate command 25**, `./tools/check-doc-links.sh`, after 23 and 24.
6. **Demonstrate failure** on the real tree, reverting each after: restore
   one README relative link (red, naming line 3 or the definition), restore
   one wrong docs.rs path from §2 (red), and change one anchor to
   `#method.logn` (red).

### S12. Changelog and the gate list

1. `CHANGELOG.md`: fold into the existing `[0.18.0]` Documentation entries
   one bullet covering crates.io links, docs.rs links, the docs.rs feature
   set, the WASI adapter's docs.rs target, and the new checks. No new
   section.
2. `CONTRIBUTING.md`: the gate list, now 25 commands, identical to the
   `gate` job and in order.

### S13. The release commit, redone

`git cherry-pick --no-commit 7c446aa` onto S12, and resolve `CHANGELOG.md`
if needed. Then run the same checks as last time: no `0.17.2` in any
manifest, tokio's five strings, the `[0.18.0]` header and links, both
version guards silent. Run the full gate, 25 commands plus `cargo audit`.
Commit as "Release 0.18.0" **last**, with the original message.

## 6. Acceptance criteria

- Every link listed in §2 resolves on the release commit, proven by
  command 25 and by the fixture that reproduces the crates.io rule.
- Commands 23 and 24 pass with warnings denied; command 23 fails when one
  intra-doc fix is reverted.
- Both docs.rs metadata tables are present; everything is still packaged.
- Every S10 and S11 demonstration has been performed and pasted.
- The release commit is last and identical in content to `7c446aa`, apart
  from `CHANGELOG.md`.

## 7. Prohibited shortcuts

- Deleting a link to make the check pass.
- Allowing a rustdoc lint.
- Making `transport` or any module private, or adding `#[doc(inline)]`,
  so that the old URLs work.
- Any network call in the check or its fixtures.
- Setting `RUSTDOCFLAGS` at job level.

## 8. After the release (the architect's)

Once 0.18.0 is published, the architect checks four things:
- crates.io: the three shared-README pages resolve all their links;
- docs.rs: `wasm-smtp` documents `send_mail_smtputf8`;
- docs.rs: `wasm-smtp-wasi`'s landing page is the `wasm32-wasip2` build;
- docs.rs: `wasm_smtp_wasi/fn.connect_smtps.html` returns 200.

---

# Revision 2 — 2026-09-13, before work starts

The owner confirmed both decisions, and added three README changes. Read
RFC 034 amendments **D7** and **D8**. This revision changes S9, and adds
S9b and S11b. Everything else stands.

## Changes to S9

- **S9.1:** the License badge target and `[TERMS_OF_USE.md]` still become
  absolute URLs, and the unused `[LICENSE]` and `[NOTICE]` definitions are
  still removed. **`[docs/src]` is not fixed. It is deleted** with the
  Documentation section in S9b.

## S9b. The README's first screen (D7)

One commit, after S9.

1. **Top.** Directly after `# wasm-smtp`, the badge row becomes the License
   badge and a documentation badge:
   `[![Documentation](https://img.shields.io/badge/docs-book-blue)](https://nabbisen.github.io/wasm-smtp/)`.
   The architect confirmed that this shields.io URL returns an SVG. Put
   the License badge first, as it is now.
2. **One line** after the opening paragraph:
   `Documentation: <https://nabbisen.github.io/wasm-smtp/>`. No further
   description.
3. **`## Acceptable use`** moves to directly after that line, above
   `## Minimum usage`, with its text unchanged.
4. **`## Documentation` is removed**, including its `docs/src` sentence
   and the `[docs/src]` definition.
5. **`### Minimum supported Rust version` is removed** from the README.
   In `docs/src/intro.md`, add `## Minimum supported Rust version` after
   the paragraph that ends "they never need to fork the protocol
   implementation." and before `## What this project is for`, with the same
   substance: 1.88, Rust 2024 edition, declared as `rust-version` in the
   workspace manifest; the repository pins that toolchain in
   `rust-toolchain.toml`, which CI enforces.
6. No other section moves, and no wording changes, except the one new
   Documentation line and the moved MSRV text.
7. `mdbook build docs` stays clean. Command 22 is unaffected, because the
   new section has no Rust code block. Paste the README's heading list
   before and after.

## S11b. The MSRV in the doc-version guard (D8)

One commit, alongside S11.

1. `tools/check-doc-versions.sh` also reads `[workspace.package]
   rust-version` as `N.N`, the same way it reads `version`. If it cannot,
   it exits 2 with a reason, like the other two readers.
2. In every file it scans, it fails any line that contains `MSRV` or
   `Minimum supported Rust version` **and** a `N.N` version different from
   `rust-version`. The message has the same shape:
   `file:line: found "X.Y", expected "A.B"`.
3. The scan also covers `.github/CONTRIBUTING.md`.
4. **Fixtures.** Every existing `doc-versions` fixture manifest needs a
   `rust-version` line, or the new reader makes it exit 2. Add that line
   and nothing else, and say so. New cases:
   - `stale-msrv`: a book line stating another version, which fails;
   - `correct-msrv`: which passes;
   - `unreadable-rust-version`: which exits 2;
   - `msrv-in-contributing`: a stale line in `.github/CONTRIBUTING.md`,
     which fails.
5. **Demonstrate failure** on the real tree: change the intro's `1.88` to
   `1.87`, and command 14 goes red naming the line. Revert.
6. Update the script's header to name the new reader and the new file.

## Changes to S12

The `[0.18.0]` Documentation bullet also covers the README's new first
screen and the MSRV moving to the book, which the guard now checks.

## Review request

Unchanged path, plus:
- S9b's before/after README heading list;
- the intro diff;
- S11b's fixture table and its demonstration.
