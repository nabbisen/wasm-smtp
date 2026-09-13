# Developer Handoff — RFC 030: Component trust-anchor configuration

**Governing RFC.** [`../../done/030-component-trust-anchor-configuration.md`](../../done/030-component-trust-anchor-configuration.md) — D3's eight invariants are fixed; nothing in this handoff relaxes them.
**Prepared.** 2026-09-13 by the architect.
**Starts.** After RFC 032's review request is **approved**, not merely submitted. Both touch `tools/component-smoke/`, and RFC 032 S2 moves the transcript assertions this work reuses.
**Target release.** 0.18.0; WIT package `wasm-smtp:smtp@0.2.0`. The owner has approved the version. Release approval remains the owner's: this handoff ends at a release commit.
**Review request goes to.** `.git-exclude/review-request/030-component-trust-anchors.md`

## 1. Purpose

Let a component caller name exactly which certificate authorities to
trust, through a validated, immutable configuration resource, so a
private-CA deployment can use the component and its success path can be
tested end to end.

## 2. What is already known

Verified by the architect. Check anything you rely on.

- `wit/smtp.wit` today: package `wasm-smtp:smtp@0.1.0`; `record
  smtp-config { host, port, ehlo-domain, tls-mode }`; `send(config,
  credentials, message)`; `send-error` already has `invalid-input(string)`.
- `src/lib.rs` `send_impl` selects `wasm_smtp_wasi::connect_smtps` or
  `connect_smtp_starttls` by `tls_mode`. The adapter also has
  `connect_smtps_with` and `connect_smtp_starttls_with`, and
  `ConnectOptions::with_root_store(RootCertStore)` in
  `crates/wasm-smtp-wasi/src/tls.rs`. Confirm their signatures.
- `rustls-pki-types` (workspace, 1.14) is already a dependency of the
  WASI adapter and provides PEM iteration; `rustls` 0.23 provides
  `RootCertStore`.
- The documentation names three binding generators: `jco types`,
  `wit-bindgen go`, `componentize-py … bindings`.

## 3. Change scope

`crates/wasm-smtp-component/{wit/smtp.wit,src/**,Cargo.toml}`;
`tools/component-smoke/**`; `docs/src/adapters/component-model.md`;
`rfcs/done/018-*.md` (amendment note under Status only); `CHANGELOG.md`;
the version and inter-crate pins in the release commit.

## 4. Non-change scope

- No change to any other crate's source or public API. If the adapter's
  API turns out to be insufficient, stop and report.
- `smtp-credentials`, `smtp-message`, `send-result`, and `send-error`
  unchanged, except that `send-error::invalid-input`'s doc comment gains
  the configuration case.
- No option beyond trust anchors, however tempting.
- No `unsafe` beyond the two generated-glue modules that already allow
  it.
- No `--all-features`. Do not tag, push, or publish. Do not edit
  `rfcs/README.md`.

## 5. Slices, in order

### S1. Prove the shape before building it

Nothing in `crates/` changes in this slice. Work in a scratch directory,
and commit only a record of what you ran.

1. A minimal WIT package, `0.2.0`: `resource smtp-config` with `create:
   static func(...) -> result<smtp-config, send-error>`, the
   `trust-anchors` variant, and `send: func(config: borrow<smtp-config>,
   ...)`. Stub bodies are fine.
2. Generate bindings with **each** documented generator: `jco types`,
   `wit-bindgen go`, `componentize-py bindings`. For each, paste the
   version and the generated signatures for `create` and `send`. If a
   generator cannot be installed here, say which and why. Do not
   substitute a different generator silently.
3. Build the stub for `wasm32-wasip2` and call `create` then `send` from
   a `wasmtime::component::bindgen!` host, dropping the resource
   afterwards. Show it drops cleanly.
4. **Compatibility.** Add a method to the resource in a `0.2.1` copy of
   the WIT. Build the component from `0.2.1` and run it under the host
   generated from `0.2.0`. It must instantiate and `send` must work.
   Paste the result, whatever it is.
5. **Stop and report** if any documented generator cannot express the
   shape or step 4 fails. The owner chooses between dropping that
   generator from the documentation and a different shape. Otherwise
   continue.

### S2. Trust-anchor parsing, as a pure function

A private module in the component crate that turns `trust-anchors` into
a `RootCertStore` or an error, with native unit tests. No WIT yet.

1. Implement D3 rules 1–8 exactly. For rule 3, add each certificate with
   the root store's single-certificate, error-returning API. Name the
   API you used, and name the skipping API you did not use.
2. The size limit (rule 7) is checked on the input's byte length before
   any parsing. The count limit fails as soon as block 65 is seen.
3. Error strings: fixed formats built only from the block ordinal and a
   fixed reason. Put the formats in one place.
4. Unit tests, at minimum: one valid certificate; a bundle with comment
   text between blocks; a `PRIVATE KEY` block alone; a valid certificate
   followed by a `PRIVATE KEY` block (the whole input fails); a
   `CERTIFICATE` block whose base64 is valid but whose DER is not a
   certificate; malformed base64; no blocks; 65 certificates; 256 KiB +
   1 byte; `bundled`.
5. **The no-echo test** uses a real private key generated in the test.
   It asserts that the error contains no line of the PEM, no base64
   substring of 16 or more characters from it, and not the label.
6. **Demonstrate failure, three ways:** switch to the skipping add API —
   the malformed-DER test must go red; fall back to the bundled store on
   error — the private-key test must go red; include the label in an
   error — the no-echo test must go red. Revert each.

### S3. The interface and its implementation

1. `wit/smtp.wit` to D1 and D2 at `wasm-smtp:smtp@0.2.0`, with doc
   comments carrying D4's semantics, and the pseudocode example updated.
   The WASI import lines do not change; `check-wasi-version.sh` must stay
   silent.
2. The resource implementation holds the validated fields and the built
   root store. `create` runs the existing host, port, and EHLO checks
   plus S2's parser. `send` does no validation of the configuration.
3. `send` connects through the `_with` helpers with
   `ConnectOptions::with_root_store` for `custom`, and without it for
   `bundled`.
4. Confirm the component still packages its WIT: gate commands for
   `cargo package --list` must pass.

### S4. The harness proves the success path

Using the assertions RFC 032 S2 moved into the shared library.

1. Implicit TLS and STARTTLS with `custom` holding the run's CA: full
   transcript assertions, as the adapter smoke test makes.
2. `bundled` against the run's certificate: refused, and no SMTP command
   reached the responder.
3. `create` with the run's **private key** as `custom`: fails with
   `invalid-input`, and the error, captured on the host side, contains
   nothing from the key.
4. **Demonstrate failure:** make `send` ignore `custom` and use the
   bundled store. Both positive cases must go red. Revert.

### S5. Documentation, changelog, amendment

1. `docs/src/adapters/component-model.md`: the new shape; D4's three
   points; a migration section from `0.1.0` with before and after.
   Regenerate the binding command outputs only if they changed.
2. `rfcs/done/018-*.md`: an amendment note under Status naming RFC 030
   and `0.2.0`.
3. `CHANGELOG.md` under `[Unreleased]`, with a **Breaking** heading for
   component consumers, stating plainly that the Rust crates' APIs are
   unchanged.

### S6. Release commit (0.18.0)

1. Workspace version and the inter-crate pins to `0.18.0`. Run both
   version guards: `check-doc-versions.sh` compares major.minor, so it
   **will** name every `0.17` it finds this time. Fix what it names.
2. `CHANGELOG.md`: `[0.18.0]` section dated the release day, and the
   comparison links.
3. The full gate as it stands after RFC 032.
4. Commit as "Release 0.18.0". Stop.

## 6. Acceptance criteria

- S1's evidence for all three generators and the compatibility run,
  pasted.
- Every D3 invariant has a test that fails when the invariant is broken,
  and the break was demonstrated.
- The component sends successfully through the harness in both TLS modes
  and refuses under `bundled`.
- No error path can contain caller-supplied PEM content.
- `send` performs no configuration validation.
- WIT at `0.2.0`; WASI imports unchanged; packaging checks pass.
- No other crate's API changed.

## 7. Prohibited shortcuts

- Skipping S1, or continuing past a failed S1 step without the owner's
  decision.
- Any root-store API that skips certificates it cannot parse.
- Falling back to `bundled` for any reason.
- Formatting a parser error with `{:?}` of anything derived from the
  input.
- Validating the configuration in `send` "as well". It moves; it is not
  duplicated.
- Asserting that an error occurred where the property is that nothing
  was sent, or that nothing leaked.

## 8. Review request contents

In order: S1's generator table and the compatibility result; the
root-store API chosen and the one avoided; the three S2 and one S4
failure demonstrations; the error-format table; before/after WIT diff;
changed files; gate results; the release commit hash.
