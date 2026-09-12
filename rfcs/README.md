# wasm-smtp RFCs

This directory holds the design documents (RFCs) for `wasm-smtp`. The
lifecycle policy is defined in
[RFC 000](./done/000-rfc-lifecycle-policy.md).

**Folder = state:**

| Folder | State | Meaning |
|---|---|---|
| `draft/` | Draft | Being written |
| `proposed/` | Proposed | Open for review |
| `accepted/` | Accepted | Design approved |
| `done/` | Implemented | Shipped; permanent record |
| `archive/` | Withdrawn / Superseded | Will not be pursued |

## Accepted

| ID | Title | Priority | Handoff |
|----|-------|----------|---------|
| [028](./accepted/028-wasi-contract-version-alignment.md) | WASI contract version alignment and component execution | P1 | [yes](./handoffs/028-wasi-contract-alignment/implementation-handoff.md) |

## Proposed

| ID | Title | Priority |
|----|-------|----------|
| [030](./proposed/030-component-trust-anchor-configuration.md) | Trust-anchor configuration for the Component Model interface | P2 |
| [031](./proposed/031-self-tests-for-the-projects-checkers.md) | Self-tests for the project's own checkers | P3 |

## Draft

| ID | Title | Priority |
|----|-------|----------|
| [022](./draft/022-direct-sockets-iwa-experimental-adapter.md) | Direct Sockets / IWA experimental adapter | P3 |
| [023](./draft/023-browser-side-secret-consent-model.md) | Browser-side secret and consent model | P3 |

## Implemented

| ID | Title | Shipped in |
|----|-------|-----------|
| [000](./done/000-rfc-lifecycle-policy.md) | RFC lifecycle policy | 0.10.0 |
| [001](./done/001-workspace-crate-boundaries-release-structure.md) | Workspace, crate boundaries, and release structure | 0.10.0 |
| [002](./done/002-rfc-lifecycle-adoption.md) | RFC lifecycle adoption and repository documentation policy | 0.10.0 |
| [003](./done/003-smtp-response-parser-protocol-model.md) | SMTP response parser and protocol model | 0.1.0 |
| [004](./done/004-data-handling-crlf-dot-stuffing.md) | DATA handling, CRLF normalization, and dot-stuffing | 0.1.0 |
| [005](./done/005-core-transport-abstraction.md) | Core transport abstraction | 0.1.0 |
| [006](./done/006-smtp-session-state-machine.md) | SMTP session state machine | 0.1.0 |
| [007](./done/007-authentication-mechanisms.md) | Authentication mechanisms | 0.9.0 |
| [008](./done/008-test-transport-deterministic-tests.md) | Test transport and deterministic protocol tests | 0.1.0 |
| [009](./done/009-error-model-failure-classification.md) | Error model and failure classification | 0.1.0 |
| [010](./done/010-security-baseline-secret-leakage-prevention.md) | Security baseline and secret leakage prevention | 0.5.0 |
| [011](./done/011-policy-hook-anti-abuse-guard.md) | Policy hook and anti-abuse guard | 0.10.0 |
| [012](./done/012-audit-event-model.md) | Audit event model | 0.10.0 |
| [013](./done/013-cloudflare-adapter-design.md) | Cloudflare adapter design | 0.3.0 |
| [014](./done/014-cloudflare-tls-starttls-strategy.md) | Cloudflare TLS and STARTTLS strategy | 0.5.0 |
| [015](./done/015-cloudflare-integration-example-limitations.md) | Cloudflare integration example and runtime limitations | 0.3.0 |
| [016](./done/016-wasi-adapter-design.md) | WASI adapter design | 0.12.0 |
| [017](./done/017-wasi-tls-strategy.md) | WASI TLS strategy | 0.12.0 |
| [018](./done/018-component-model-wit-interface.md) | Component Model and WIT interface | 0.14.0 |
| [019](./done/019-streaming-data-bounded-memory.md) | Streaming DATA and bounded memory | 0.13.0 |
| [020](./done/020-large-message-handling-memory-tests.md) | Large message handling and memory behavior tests | 0.11.0 |
| [021](./done/021-no-std-alloc-feasibility.md) | no_std / alloc feasibility | 0.11.0 |
| [024](./done/024-release-gate-integrity-toolchain-baseline.md) | Release gate integrity, toolchain baseline, and MSRV correction ([handoff](./handoffs/024-release-gate-integrity/implementation-handoff.md)) | 0.15.2 |
| [025](./done/025-wasi-hardening-on-target-verification.md) | WASI hardening and on-target verification ([handoff](./handoffs/025-wasi-hardening/implementation-handoff.md)) | 0.16.0 |
| [026](./done/026-anti-abuse-patterns-documentation.md) | Anti-abuse patterns at the application boundary (documentation) ([handoff](./handoffs/026-anti-abuse-patterns/implementation-handoff.md)) | 0.16.1 |
| [027](./done/027-docs-publication-dependency-currency.md) | Documentation publication and dependency currency ([handoff](./handoffs/027-docs-publication-dependency-currency/implementation-handoff.md)) | 0.17.0 |
| [029](./done/029-documentation-version-audit-and-guard.md) | Documentation version audit, and a guard so it stops recurring ([handoff](./handoffs/029-documentation-version-audit/implementation-handoff.md)) | 0.17.1 |

## Archive

_(empty)_

---

RFC 000–021, 024–027 and 029 are implemented; 028 is accepted; 030–031 are proposed; 022–023 are drafts. The next RFC is **032**.
