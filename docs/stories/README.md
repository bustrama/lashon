# Stories

A **story** is one self-contained unit of work — usually a milestone from the
[roadmap](../roadmap.md), sometimes a vertical slice of one. A story carries
everything a developer needs to pick it up without re-reading the whole roadmap.

One story = one feature branch (`mN-slug`) = one PR. The PR merges only when the
story's Acceptance Criteria are all met and CI is green on all three runners
(see [`CONTRIBUTING.md`](../../CONTRIBUTING.md)).

## Format

Each story is a Markdown file named `mN-slug.md` (e.g. `m3-tongue-ui.md`):

### Title

Short and action-oriented — `Tongue UI minimum`, not `Implement M3`.

### Why

One or two sentences. What does this unlock? What is missing without it?

### How

Walk the developer through the approach — what to build, where it lives, which
patterns to follow. Reference real file paths, types, and the relevant `docs/`.

### Acceptance Criteria

The milestone's Definition of Done as concrete, testable checkboxes. Hebrew is
exercised explicitly at every layer the story touches.

### Files

The files and directories this story is expected to touch.

### Dependencies

What must land first; what this story unblocks.

## Rules

- **Vertical slices.** A story delivers working functionality, not a layer.
- **Self-contained.** Include enough context that the developer needn't read the
  whole roadmap. Copy the relevant interface or DoD text into the story.
- **Speak like a developer.** Real file paths, function names, types — no jargon.
- **Why before how.** Always start with why it matters.
- **An ADR for every architectural decision** — `docs/adr/NNNN-slug.md`.

Completed milestones are not kept here — git history and the roadmap's milestone
table are the record. This directory holds active and upcoming work only.
