---
description: Branch, milestone, ADR, and commit conventions
globs: ["**/*"]
---

# Workflow

## Milestones

- One milestone = one feature branch (`mN-slug`) = one PR. It merges only when
  the milestone's Definition of Done ([`docs/roadmap.md`](../../docs/roadmap.md))
  is met and CI is green on all three runners.
- A picked-up milestone is written up as a story in
  [`docs/stories/`](../../docs/stories/).

## ADRs

- Every architectural decision, trade-off, or reversal is recorded as an ADR in
  `docs/adr/NNNN-slug.md`, numbered sequentially. Write the ADR in the same PR
  that makes the decision.

## Commits

- Conventional, imperative mood: `<type>: <subject>`. Types: `feat`, `fix`,
  `refactor`, `docs`, `test`, `chore`, `perf`, `ci`. Subject ≤ 72 characters.
  One concern per commit.

## Versions

- Pin every dependency and toolchain exactly — no floating `^`, `~`, or `*` in
  any manifest. Lockfiles are committed. A version bump is its own deliberate
  commit.
