# RFC 002 — RFC lifecycle adoption and repository documentation policy

**Status.** Implemented (v0.10.0)
**Priority.** P0
**Tracks.** Documentation / Governance / RFC process
**Touches.** `rfcs/`, `rfcs/README.md`, `docs/src/`, `README.md`

## Summary

This RFC formally adopts the RFC lifecycle policy (RFC 000) for the
`wasm-smtp` project and establishes the concrete folder structure,
index conventions, and documentation layout that follow from it.

## Motivation

`wasm-smtp` accumulated design decisions through ROADMAP phases and
ad-hoc commit messages. As the project grows toward a multi-adapter,
multi-target library, design decisions need durable, findable records
so that:

- New contributors can understand *why* the architecture is the way it
  is, not just *what* it is.
- Alternative approaches that were considered and rejected are not
  re-proposed from scratch.
- The distance between design intent (RFC) and implementation is
  visible: an RFC in `proposed/` is a signal that work is ready to
  start but hasn't shipped; an RFC in `done/` means it has shipped.

The RFC process itself is defined in RFC 000. This RFC applies that
process to this repository.

## Goals

- Establish the `rfcs/` folder with the 5-folder variant.
- Define the Status field values and their meaning in this project.
- Define the `rfcs/README.md` index structure.
- Record the documentation layout split between `README.md` and
  `docs/src/`.
- Record the RFC template every new RFC must follow.

## Non-goals

- Defining the content of any specific RFC (that is each RFC's own job).
- Establishing review SLAs.
- Automating CI checks on RFC files (may be added later).

## Design

### Folder structure

```text
rfcs/
├─ README.md         ← index of all RFCs; state-grouped
├─ draft/            ← RFCs being written; not yet open for review
├─ proposed/         ← Open for review; implementation must not start
├─ accepted/         ← Design settled; implementer may begin work
├─ done/             ← Shipped; historical record, never deleted
└─ archive/          ← Withdrawn or Superseded; historical record
```

The 5-folder variant is used (RFC 000 §"Folder layout: 5-folder
variant") because the `wasm-smtp` project separates design and
implementation roles: the author of a design document is not always
the implementer. The `accepted/` folder makes the "design approved,
implementation may start" event explicit.

### State definitions

| State | Folder | Meaning |
|---|---|---|
| Draft | `draft/` | Author is still writing. Not ready for external review. |
| Proposed | `proposed/` | Open for review. Implementation must not start yet. |
| Accepted | `accepted/` | Design approved. Implementation may begin. |
| Implemented | `done/` | Shipped in a release. Permanent historical record. |
| Withdrawn | `archive/` | Will not be pursued. One-line reason in Status field. |
| Superseded | `archive/` | Replaced by a later RFC. Replacement ID in Status field. |

### Status field

Each RFC carries a `**Status.**` line at the top. The value mirrors
the folder. When an RFC moves between folders, the Status field is
updated in the same commit. Examples:

```markdown
**Status.** Draft
**Status.** Proposed
**Status.** Implemented (v0.10.0)
**Status.** Implemented (v0.11.0)
**Status.** Withdrawn — scope absorbed by RFC 005.
**Status.** Superseded by RFC 042.
```

### Naming convention

Files follow `NNN-slug.md` where `NNN` is a zero-padded three-digit
sequence number assigned at creation time. Numbers are permanent and
never reused. Slugs are lowercase, hyphen-separated summaries.

RFC 000 is the lifecycle policy itself and lives in `done/` because it
is implemented (the policy is now in effect).

### RFC template

Every new RFC must include at minimum:

```markdown
# RFC NNN — Title

**Status.** Draft | Proposed | Accepted | Implemented (vX.Y.Z) | Withdrawn | Superseded
**Priority.** P0 | P1 | P2 | P3
**Tracks.** (e.g. Core protocol / Adapter / Security / Docs / Release)
**Touches.** (e.g. crates/..., docs/..., rfcs/...)

## Summary
## Motivation
## Goals
## Non-goals
## Design
## Security considerations
## Simplicity and maintainability considerations
## Alternatives considered
## Implementation plan
## Acceptance criteria
## Open questions
```

Sections may be omitted only if they are genuinely not applicable
(e.g. a Docs-only RFC has no Security considerations section). The
omission should be noted explicitly (`*Not applicable.*`).

### `rfcs/README.md` index

The index groups RFCs by state and shows ID, title, priority, and
(for done/) the shipped version. It is updated in the same commit
that moves an RFC between folders.

### Documentation layout

`README.md` is kept short (hero badges, overview, quick start,
pointer to docs). Full documentation lives under `docs/src/` and is
structured for three reader personas:

1. **New users** — tutorials, quick-start, FAQ.
2. **Experienced users** — API reference, protocol notes, feature
   explanations.
3. **Contributors / maintainers** — architecture, design philosophy,
   local development guide.

`docs/` is laid out for `mdBook` generation (`book.toml` at the root
of `docs/`). The RFC documents themselves are not part of the mdBook;
they live in `rfcs/` and are linked from `CONTRIBUTING.md`.

## Security considerations

The RFC process is a governance mechanism, not a security boundary.
However, RFCs that touch authentication, secret handling, or TLS
strategy carry an explicit **Security considerations** section.
Marking such RFCs clearly makes it easier to audit the design surface
for security-relevant decisions.

## Simplicity and maintainability considerations

The 5-folder variant adds one folder over the minimal 4-folder variant.
The trade-off is deliberate: the extra `accepted/` state prevents an
RFC from sitting in `proposed/` long after the design is settled, which
is a common source of confusion about whether implementation has started.

## Alternatives considered

**4-folder variant (no `accepted/`):** Suitable for projects where the
same person both designs and implements. Rejected here because `wasm-smtp`
routinely has separate design and implementation contributors, especially
for adapter crates.

**Flat `rfcs/` with no subfolders:** Rejected because the folder is the
source of truth for state. Without folders, contributors must open each
file to determine its state.

## Implementation plan

1. Create `rfcs/` with the five subfolders.
2. Move RFC 000 (lifecycle policy) to `rfcs/done/`.
3. Create initial RFC drafts for Batch 1 RFCs (RFC 001–002 accepted,
   RFC 003–010 done for already-implemented features, RFC 011+ as
   proposed/draft).
4. Write `rfcs/README.md` index.
5. Update `CONTRIBUTING.md` to describe the RFC process.

## Acceptance criteria

- `rfcs/` exists with `draft/`, `proposed/`, `accepted/`, `done/`,
  `archive/` subfolders.
- `rfcs/README.md` lists every RFC by state.
- Every RFC file has a Status field consistent with its folder.
- The RFC template is documented in `rfcs/README.md` or `CONTRIBUTING.md`.

## Open questions

None at acceptance time.
