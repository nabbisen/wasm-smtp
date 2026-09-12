# RFC 026 — Anti-abuse patterns at the application boundary (documentation)

**Status.** Proposed
**Priority.** P2
**Tracks.** Docs / Security
**Touches.** `docs/src/reference/examples.md`, `docs/src/concepts/security.md`, `docs/src/SUMMARY.md`
**Authorized.** Theme approved by the owner on 2026-09-12 as lower priority than RFC 025.

## Summary

Document how applications built on `wasm-smtp` keep abusers from
turning a contact form or notification endpoint into a mail relay,
using controls that sit before the SMTP session starts: bot
challenges such as Cloudflare Turnstile, request rate limits, and
honeypot fields. Documentation only. No crate gains a dependency, an
HTTP client, or a vendor integration.

## Motivation

The project's terms of use forbid abusive sending and RFC 011 gives
applications a policy hook, but the hook runs after a request has
already been accepted. The most effective anti-abuse controls run
earlier, at the HTTP boundary, and the examples chapter's leading
scenario, a contact form handled by a Worker, is exactly where a
reader needs to see them. Today the documentation says nothing about
that layer.

## Goals

- A worked Worker example: verify a Turnstile token against the
  `siteverify` endpoint, then and only then open the SMTP session.
- A short section explaining the boundary: CAPTCHA-style challenges,
  rate limits, and honeypots are HTTP-layer, vendor-specific concerns;
  `SendPolicy` is the SMTP-layer, vendor-neutral hook; the library
  provides the latter and deliberately not the former.
- Equivalent pointers for non-Cloudflare deployments (any challenge
  provider, any rate limiter) so the pattern reads as general.

## Non-goals

- A `wasm-smtp-turnstile` crate or any HTTP call inside the family.
- Making `SendPolicy` asynchronous or I/O-capable.
- Endorsing one vendor; Turnstile is the worked example because the
  Cloudflare adapter is the first-class Workers target.

## Design

New subsection "Contact form with a bot challenge" in the examples
chapter, and a new "Anti-abuse at the request boundary" section in the
security chapter. The example keeps the secret key in a Workers
Secret, treats a failed verification as a client error with no SMTP
session opened, and logs only the verdict, never the token.

## Security considerations

The example must not log the Turnstile token or the secret, must
fail closed when `siteverify` is unreachable, and must not present the
challenge as a substitute for the policy hook or for server-side rate
limiting.

## Simplicity and maintainability considerations

Two documentation sections; no code. Vendor API drift affects one
example, which is acceptable for documentation.

## Alternatives considered

A separate crate: rejected on 2026-09-12 as thirty lines of fetch and
parse with no SMTP content and a vendor-bound maintenance tax.

## Implementation plan

After RFC 025 ships. A one-slice handoff.

## Acceptance criteria

- Both sections exist, render in the mdBook, and the example compiles
  as a `no_run` snippet against the current Cloudflare adapter API.
- Neither section suggests the library performs the verification.

## Open questions

None.
