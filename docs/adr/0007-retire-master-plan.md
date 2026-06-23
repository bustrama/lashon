# 7. Retire the master plan: decompose into focused docs, a roadmap, and stories

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** Lashon contributors
- **Relation:** retires `LASHON_MASTER_PLAN.md`

## Context

The repository was built against a single document, `LASHON_MASTER_PLAN.md` — a
~63 KB file designated "the single source of truth" by `CLAUDE.md`, with a
"when anything conflicts with the plan, the plan wins" rule echoed in
`CONTRIBUTING.md`, `docs/soul.md`, and `docs/architecture.md`.

One file carried five unrelated kinds of content: stable architecture spec, the
UI design system, the forward roadmap, Lashon's identity, and an obsolete
initial-commit checklist. That created friction:

- **It was a bottleneck.** Every other doc deferred to it; nothing else could
  be authoritative, so the spec and the plan could not evolve independently.
- **It went stale by construction.** A 63 KB monolith is updated rarely; §4's
  pinned versions, for example, had already drifted from the manifests.
- **It mixed time horizons.** Stable architecture and a fast-moving roadmap do
  not belong in the same document with the same update cadence.

Roughly 30 files referenced the plan by section number (`master plan §1.3`,
`§12 DoD`) — including `CLAUDE.md`, `CONTRIBUTING.md`, every ADR, `NOTICE`, and
many source-code comments.

## Decision

Retire `LASHON_MASTER_PLAN.md` and decompose it into focused, living documents,
each authoritative for its own area and kept current with the code:

- **Spec** → `docs/architecture.md` (expanded with the §10 budgets' companion
  and the §11 risks table), `docs/providers.md`, `docs/tech-stack.md`,
  `docs/design-system.md`, `docs/testing.md`, `docs/soul.md`.
- **Roadmap** → `docs/roadmap.md` — scope, phases, the fourteen milestones, and
  the per-phase workstreams — with a short summary section in `README.md`.
- **Work units** → `docs/stories/` — one self-contained story per picked-up
  milestone, seeded with `m3-tongue-ui.md`.
- The obsolete §14 initial-commit checklist is dropped; §7's verbose monorepo
  tree is dropped in favour of the "Repository layout" section already in
  `CLAUDE.md`.

`CLAUDE.md` and `CONTRIBUTING.md` no longer name a single source of truth; each
doc is authoritative for its area.

Stories live in `docs/stories/`, not `.claude/`: a story is a documentation
artifact and belongs with the other docs, next to `docs/roadmap.md`.

## Alternatives considered

- **Keep the monolith.** Rejected — it is the problem being solved.
- **Rename it to `TODO.md` / `PLAN.md`.** Rejected: the file is ~80 % spec, not
  a task list; a rename would leave a mislabelled monolith and fix nothing.
- **One consolidated `docs/spec.md`.** Rejected: that is the monolith again,
  minus the roadmap. Focused per-area docs evolve independently.

## Consequences

- Documentation-level references (`README.md`, `CONTRIBUTING.md`, `CLAUDE.md`,
  ADRs 0001–0006, `NOTICE`, `models/README.md`, `tests/hebrew-corpus/README.md`)
  are rewired to the new docs in the same change.
- **Source-code comments** that cite `master plan §N` (~25, in Rust, Python,
  Svelte, CSS, and TOML/JSON config) are intentionally **not** rewired here, to
  keep this a documentation-only change. They are harmless as stale citations
  and can be updated opportunistically when those files are next touched.
- New milestones are written up as stories in `docs/stories/` when picked up;
  the roadmap's milestone table tracks status.
- ADRs remain the record of architectural decisions; this one records the
  retirement so the move is not lost.
