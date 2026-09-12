# Developer Handoff — RFC 026: Anti-abuse patterns at the application boundary

**Governing RFC.** [`../../accepted/026-anti-abuse-patterns-documentation.md`](../../accepted/026-anti-abuse-patterns-documentation.md)
**Prepared.** 2026-09-12 by the architect. Baseline: `9fcda99` (0.16.0 released).
**Release.** None by default. This is documentation plus one compiled example; it ships with whatever release comes next. If the owner wants it on crates.io sooner, that is a separate patch-release decision.
**Review request goes to.** `.git-exclude/review-request/026-anti-abuse-patterns.md`

---

## 1. Purpose

Show readers where abuse controls belong when building on `wasm-smtp`:
at the HTTP request boundary, before any SMTP session starts, and
separate from the SMTP-layer policy hook. One worked Cloudflare Worker
example with Turnstile, one conceptual section, and a compiled example
so the code cannot rot silently.

## 2. Applicable design

RFC 026 in full. The boundary it draws is settled: challenge
verification, rate limiting, and honeypots are application-layer,
vendor-specific; `SendPolicy` is SMTP-layer, vendor-neutral; the
library provides the latter and deliberately not the former. RFC 011
(policy hook), RFC 010 (no secrets in logs), `TERMS_OF_USE.md`.

## 3. Change scope

- `docs/src/reference/examples.md` — new section.
- `docs/src/concepts/security.md` — new section.
- `docs/src/core/policy-audit.md` — one cross-reference sentence.
- `crates/wasm-smtp-cloudflare/examples/contact_form_turnstile.rs` (new).
- `crates/wasm-smtp-cloudflare/Cargo.toml` — `[dev-dependencies]` only.
- `.github/workflows/ci.yml`, `.github/CONTRIBUTING.md` — one gate line.
- `CHANGELOG.md` — under `[Unreleased]`.

## 4. Non-change scope

- No change to any library source under `src/`, any public API, or any
  published dependency. A `serde` dev-dependency with `derive` for the
  example is acceptable; nothing else.
- No HTTP client, verification helper, or vendor type in any published
  crate.
- No new cargo feature.
- Do not tag, push, or publish.

## 5. The single slice

### 5.1 Compiled example: `crates/wasm-smtp-cloudflare/examples/contact_form_turnstile.rs`

A Worker `fetch` handler for a contact form. Order of operations, each
step returning without touching SMTP on failure:

1. **Method and content.** Accept only `POST` with a form body; anything
   else is `405` or `400`.
2. **Honeypot.** A hidden form field (`website`) that humans leave
   empty; if it is non-empty, return `400` immediately. No network
   call, no log line beyond a counter-style message.
3. **Turnstile verification.** `POST https://challenges.cloudflare.com/turnstile/v0/siteverify`
   as `application/x-www-form-urlencoded` with `secret`
   (from `env.secret("TURNSTILE_SECRET")`), `response` (the
   `cf-turnstile-response` form field), and `remoteip`
   (`CF-Connecting-IP` header). Parse the JSON reply with a small
   `#[derive(Deserialize)]` struct holding `success: bool` and
   `error_codes: Vec<String>` (`#[serde(rename = "error-codes", default)]`).
   - `success == false` → `403`, log only the error codes.
   - request failure or unparseable reply → `503`, log "verification
     unavailable". **Fail closed**: never proceed to SMTP because the
     verifier could not be reached.
   - Never log the token, the secret, or the form contents.
4. **Send.** Only now: `connect_smtps` with host and port from
   secrets/vars, `login` with `SMTP_USER`/`SMTP_PASS` from
   `env.secret`, a body in the same shape as the existing contact-form
   example (`Reply-To` set to the submitter, `From` fixed to the
   authenticated mailbox), `send_mail`, `quit`. Map `SmtpError` to a
   `502` with a generic message; log the error `Display` only.
5. Return `200`.

Include a short `//!` header stating what the example demonstrates and
that Turnstile is one vendor's challenge; hCaptcha or reCAPTCHA slot
into step 3 identically.

Compile constraints:

- It must compile on the host under `cargo test --workspace` (cargo
  builds examples) and for the real target under
  `cargo check -p wasm-smtp-cloudflare --examples --target wasm32-unknown-unknown`.
- If the `#[event(fetch)]` macro will not compile on the host, write the
  handler as a plain `pub async fn handle(req: Request, env: Env) -> worker::Result<Response>`
  and show the one macro line in the docs prose instead. If `worker`
  types themselves fail to compile as an example on the host, stop and
  report; do not add `cfg` scaffolding.

### 5.2 `docs/src/reference/examples.md`

New section **"Contact form with a bot challenge"** directly after
"Contact-form delivery". Content: the five steps above in prose, the
example file referenced by path with its handler body quoted as a
`rust,no_run` block (keep it to the verification and the hand-off to
SMTP; elide boilerplate with `# ` lines as the chapter already does),
the required secrets (`TURNSTILE_SECRET`, `SMTP_*`) and where the site
key goes (the HTML, not the Worker), and two sentences on why the
verification happens in the Worker and not in the library. Link to the
new security section.

### 5.3 `docs/src/concepts/security.md`

New section **"Anti-abuse at the request boundary"** before
"Acceptable use". Three short parts:

- *Two layers.* Request boundary (challenge, rate limit, honeypot):
  HTTP-layer, vendor-specific, decides whether a request may cause a
  send at all. SMTP layer (`SendPolicy`): vendor-neutral, decides
  whether a given envelope may go out. Both are needed; neither
  substitutes for the other.
- *Why the library stops at the SMTP layer.* The core does no I/O and
  speaks only SMTP; adapters are transport-only; `SendPolicy` is
  synchronous and I/O-free by design. Verification needs HTTP, JSON,
  and a vendor secret. Putting it in the family would bind every
  runtime to one vendor and add a maintenance surface with no SMTP
  content. This is the decision recorded in RFC 026.
- *Rate limiting.* One paragraph: per-IP or per-sender counters in the
  platform's store (Workers KV or Durable Objects, or the platform's
  built-in rate-limiting rules), checked before the challenge so bots
  do not consume challenge verifications; the library's `BoundedPolicy`
  bounds recipients and size per message, not requests per minute.

Link to the examples section.

### 5.4 Cross-reference and changelog

`docs/src/core/policy-audit.md`: one sentence at the top of the
`SendPolicy` section pointing to the security chapter for controls that
belong before the session. `CHANGELOG.md` `[Unreleased]`, Documentation:
one bullet.

### 5.5 Gate line

Add to the `gate` job after the packaging checks, and to CONTRIBUTING:

```bash
cargo check -p wasm-smtp-cloudflare --examples --target wasm32-unknown-unknown
```

## 6. Acceptance criteria

- Both sections exist and render in the mdBook; `mdbook build docs`
  succeeds if `mdbook` is available locally (not a gate command; say
  whether you ran it).
- The example compiles on the host and for `wasm32-unknown-unknown`.
- The example never proceeds to SMTP unless verification returned
  `success: true`, and fails closed when the verifier is unreachable.
- No token, secret, or form content appears in any log line of the
  example.
- Neither section suggests the library performs the verification, and
  the security section states the two-layer boundary in the terms
  above.
- Full gate (RFC 024 §D3, RFC 024 D11, RFC 025 D1, plus the new line)
  passes on the pinned toolchain.

## 7. Prohibited shortcuts

- Adding any HTTP or verification code to a `src/` directory.
- Presenting the challenge as a replacement for `SendPolicy` or for
  rate limiting.
- Logging the token "for debugging".

## 8. Review request contents

Summary, changed files, whether the macro compiled on the host, whether
`mdbook build` was run, gate outputs, and the rendered section text
pasted for review.

---

# Revision 2 — 2026-09-12, after review 1

Review: `.git-exclude/reviewed/026-anti-abuse-patterns-review-1.md`.
Everything is accepted except one required correction.

## C1 — Header injection guard (security)

Form fields reach message headers unvalidated; a CR/LF in `name` or
`email` injects headers such as `Bcc:`. Fix in the example and in both
prose sections:

1. `examples/contact_form_turnstile.rs`, before the send: reject `name`
   or `email` containing `\r` or `\n` with `400`; run `email` through
   `wasm_smtp::protocol::validate_address` and reject with `400` on
   error; normalize `message` line endings to CRLF before building the
   body. Keep the log lines content-free.
2. `docs/src/reference/examples.md`: a fifth step "Header safety" in
   the new section, linking the composing chapter's header-injection
   discussion and its `mail-builder` route; apply the same guard to the
   pre-existing "Contact-form delivery" block or point it at the guarded
   version.
3. `docs/src/concepts/security.md`, under "Two layers, both needed": one
   line that validating input before it is placed in a header is part of
   the request boundary, and that the library's envelope validation does
   not cover headers.
4. Optional single clause: non-ASCII `Subject:` values need RFC 2047,
   which `mail-builder` handles.

Acceptance: `400` for a `name` or `email` containing `\r\n`; full gate
passes; sections updated. Second request at
`.git-exclude/review-request/026-anti-abuse-patterns-2.md`. Do not push.
