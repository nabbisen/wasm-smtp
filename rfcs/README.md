# wasm-smtp RFCs

This directory holds the design documents (RFCs) for `wasm-smtp`. The
lifecycle policy is defined in
[RFC 000](./done/000-rfc-lifecycle-policy.md).

**Folder = state:**

| Folder | State | Meaning |
|---|---|---|
| `draft/` | Draft | Being written |
| `proposed/` | Proposed | Open for review |
| `accepted/` | Accepted | Design approved; implementation may begin |
| `done/` | Implemented | Shipped; permanent record |
| `archive/` | Withdrawn / Superseded | Will not be pursued |

## Accepted

_(empty)_

## Proposed

| ID | Title | Priority | Target |
|----|-------|----------|--------|
| [018](./proposed/018-component-model-wit-interface.md) | Component Model and WIT interface | P2 | v0.14.0 |

## Draft

| ID | Title | Priority |
|----|-------|----------|
| [022](./draft/022-direct-sockets-iwa-experimental-adapter.md) | Direct Sockets / IWA experimental adapter | P3 |
| [023](./draft/023-browser-side-secret-consent-model.md) | Browser-side secret and consent model | P3 |

## Implemented

| ID | Title | Shipped in |
|----|-------|-----------|
| [000](./done/000-rfc-lifecycle-policy.md) | RFC lifecycle policy | v0.10.0 |
| [001](./done/001-workspace-crate-boundaries-release-structure.md) | Workspace, crate boundaries, and release structure | v0.10.0 |
| [002](./done/002-rfc-lifecycle-adoption.md) | RFC lifecycle adoption and repository documentation policy | v0.10.0 |
| [003](./done/003-smtp-response-parser-protocol-model.md) | SMTP response parser and protocol model | v0.1.0 |
| [004](./done/004-data-handling-crlf-dot-stuffing.md) | DATA handling, CRLF normalization, and dot-stuffing | v0.1.0 |
| [005](./done/005-core-transport-abstraction.md) | Core transport abstraction | v0.1.0 |
| [006](./done/006-smtp-session-state-machine.md) | SMTP session state machine | v0.1.0 |
| [007](./done/007-authentication-mechanisms.md) | Authentication mechanisms | v0.9.0 |
| [008](./done/008-test-transport-deterministic-tests.md) | Test transport and deterministic protocol tests | v0.1.0 |
| [009](./done/009-error-model-failure-classification.md) | Error model and failure classification | v0.1.0 |
| [010](./done/010-security-baseline-secret-leakage-prevention.md) | Security baseline and secret leakage prevention | v0.5.0 |
| [011](./done/011-policy-hook-anti-abuse-guard.md) | Policy hook and anti-abuse guard | v0.10.0 |
| [012](./done/012-audit-event-model.md) | Audit event model | v0.10.0 |
| [013](./done/013-cloudflare-adapter-design.md) | Cloudflare adapter design | v0.3.0 |
| [014](./done/014-cloudflare-tls-starttls-strategy.md) | Cloudflare TLS and STARTTLS strategy | v0.5.0 |
| [015](./done/015-cloudflare-integration-example-limitations.md) | Cloudflare integration example and runtime limitations | v0.3.0 |
| [016](./done/016-wasi-adapter-design.md) | WASI adapter design | v0.12.0 |
| [017](./done/017-wasi-tls-strategy.md) | WASI TLS strategy | v0.12.0 |
| [019](./done/019-streaming-data-bounded-memory.md) | Streaming DATA and bounded memory | v0.13.0 |
| [020](./done/020-large-message-handling-memory-tests.md) | Large message handling and memory behavior tests | v0.11.0 |
| [021](./done/021-no-std-alloc-feasibility.md) | no_std / alloc feasibility | v0.11.0 |

## Archive

_(empty)_

---

Next RFC: **024**
