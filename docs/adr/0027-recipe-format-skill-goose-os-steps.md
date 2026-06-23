# 27. Recipe format — SKILL.md envelope + Goose parameters + Lashon `os_steps`

## Status

Accepted — landed on the `m9-recipe-schema` branch as M9 Phase 1a.

## Context

M9 introduces **recipes**: pre-recorded parameterised desktop workflows
the dispatcher's intent cascade routes to before the LLM full-planner
runs (`docs/stories/m9-recipes.md`). The Discord send-message
benchmark collapses from 7–10 LLM turns to 0–1 (slot extraction only)
on the recipe path, and the 4 B local model becomes structurally
sufficient for the common case.

The format has to do three jobs:

1. **Describe identity** — name, description, tags — so the Hub can
   render a browser tab of installed recipes and the intent cascade
   can serialise them into an LLM classifier prompt
   (`docs/stories/m9-recipes.md` Phase 1c tier 3).
2. **Declare parameters** — typed slots the Hub creator UI renders as
   inputs and the cascade extracts from spoken commands.
3. **Sequence OS-UI primitives** — key chords, type-Unicode, click
   labels, focus windows, screenshots, clipboard reads/writes, shell
   commands. None of these are standardised in any other agent
   framework: Lashon's `tools/*.rs` set is the de facto vocabulary.

A bespoke single-purpose format would have been quick to ship but
would lock the recipes into Lashon's runtime forever, foreclose any
future marketplace standardisation, and force every new agent client
to learn a one-off format. The session-progress doc and the M9 story
explicitly call out the "design portable from day one" direction
even though v1 only runs in Lashon.

## Decision

Compose three existing formats so each part of the recipe is the
shape the wider 2026 agent ecosystem already speaks:

| Layer | Source | Why this source |
|---|---|---|
| **Identity envelope** | Anthropic Agent Skills `SKILL.md` (Apache-2.0, 32+ tools support it in 2026) | The most-adopted format for "this is a skill, here's what it does, here are its inputs" |
| **Parameter schema** | Goose Recipes (`block/goose`, donated to AAIF / Linux Foundation Dec 2025) | The only widely-used desktop-agent parameter spec; fields match Lashon's slot-fill needs verbatim |
| **OS-UI primitive vocabulary** | Lashon-specific `os_steps:` | Nobody else has standardised these — Skia / Flutter / Electron a11y, BiDi-safe text injection, click-by-label all need our domain logic |

Concretely:

- A recipe lives in its own directory under either
  `<user-data>/recipes/<id>/` or `recipes/starters/<id>/` (this PR
  ships 10 starters at the repo path) with a `recipe.yaml` carrying
  the full spec. An optional sibling `SKILL.md` may carry richer
  markdown documentation; the runtime ignores it but Agent-Skills-
  aware tools pick it up.
- Top-level fields: `version`, `id`, `name`, `description`,
  `long_description`, `author`, `recipe_version`, `tags`, `intents`,
  `parameters`, `permissions`, `os_steps`. The first eight are
  identity-envelope-flavoured; `parameters` is Goose-shaped;
  `os_steps` is Lashon-specific.
- Parameters are an ordered list of `{ key, input_type, requirement,
  description, default? }` — Goose's keys verbatim. `input_type` is
  `string | number | boolean | file | date`; `requirement` is
  `required | optional | user_prompt`. The Hub slot-fill modal
  renders by `input_type`; the cascade extracts by `key`.
- `os_steps` has three optional variants — `windows`, `macos`,
  `linux`. The Phase 1 runtime only honours `windows`; the other
  slots exist so authors can declare them today and the runtime
  gains coverage later without a schema bump.
- The 12 step types v1 ships are the Lashon-tools surface the M8.2
  catalogue normalised on: `key_chord`, `type_unicode`,
  `click_label`, `focus_window`, `wait_for_window`, `wait_ms`,
  `screenshot_to_clipboard`, `clipboard_set`, `clipboard_get_into`,
  `run_shell`, `open_url`, `open_app`. Every variant carries an
  optional `comment: string` field for inline rationale that
  survives serde roundtrip (YAML `#` comments do not).
- `{{ key }}` interpolation is the slot reference syntax — same as
  Goose / SKILL.md. It works in every text-bearing step field
  (`text`, `command`, `url`, `name`, `title_contains`, `process`,
  `label`, `window`). The validator complains about references that
  resolve to neither a declared parameter nor a step-local variable
  (`clipboard_get_into.var`, `run_shell.capture_into`).
- The JSON Schema is auto-derived from the Rust types via
  `schemars` v1 and committed at
  [`recipes/schema/lashon-recipe.schema.json`](../../recipes/schema/lashon-recipe.schema.json).
  A snapshot test (`packages/shared-rust/tests/recipe_schema_snapshot.rs`)
  fails on any drift between the file and the types; an
  `#[ignore]` "regenerate" test rewrites the file on demand.

## Versioning

Two version fields with distinct semantics:

- `version: 1` — **schema version.** Bumps when the parser changes
  in a breaking way. v1 is the format documented in this ADR. The
  validator rejects anything else, so a future v2 parser can
  multiplex (`if recipe.version == 1 { parse_v1(&body) } else { … }`)
  rather than guessing.
- `recipe_version: "1.0.0"` — **content version**, semver. The Hub
  uses it to detect "newer than installed" upgrades when a bundled
  starter outpaces a user's local copy. Open question 5 from the M9
  story (last-write-wins vs user-prompted on bundled/local
  divergence) lands in Phase 1d's Hub work; the field carries the
  signal regardless.

## Why YAML, not JSON / TOML / HCL

- YAML supports the `|` multi-line string for `long_description`
  paragraphs and the `>` folded form for shell command lines — both
  fit hand-authored recipes naturally.
- YAML is the lingua franca of CI / config / Goose itself / Anthropic
  Agent Skills frontmatter, so recipe authors have already met it.
- TOML's table semantics fight the per-step ordered-list-of-tagged-
  unions shape; HCL is overkill for a non-expression-evaluating
  config; JSON is too punctuation-heavy for inline editing.

The chosen parser, `serde_yaml_ng = "=0.10.0"`, is dtolnay's
official continuation of the archived `serde_yaml`. The alternative
`serde_yml` fork is flagged
[`RUSTSEC-2025-0068`](https://rustsec.org/advisories/RUSTSEC-2025-0068.html)
as unsound and unmaintained and is avoided.

## Permission declarations (open question 4 from `m9-recipes.md`)

Recipes declare a `permissions: [...]` list of free-text identifiers
— conventional values: `keyboard.type`, `app.focus`, `app.open`,
`clipboard`, `screenshot`, `file.write`, `shell.run`, `network`,
`destructive`. v1 enforcement is partial: the validator complains
when a `run_shell` step is present without `shell.run`. The Hub
renders the list as a badge row on the recipe card so the user can
see at a glance what a recipe is allowed to do. Full sandboxing /
per-permission gating is M11+ stretch and lives in a separate ADR
when picked up.

## Why `comment:` per-step, not YAML `#`

YAML `#` comments don't survive `serde_yaml_ng`'s parse → serialise
roundtrip — no YAML library preserves them in the AST. The Hub
creator UI saves a recipe by parsing the LLM's draft, validating it,
then writing back via `serde_yaml_ng::to_string`. Inline `#`
comments would silently disappear on save, so any per-step
rationale the LLM produced would be lost on the user's first edit.
A `comment: string` field on every step variant survives the
roundtrip and renders in the Hub step list.

## Consequences

- **Authoring is portable in spirit.** A Goose user reading
  `recipe.yaml` recognises `parameters:`. An Agent Skills user
  recognises the `name` / `description` / `tags`. Lashon-specific
  bits are scoped to `os_steps:` so an LLM authoring a recipe can
  generate the standard parts from training-data familiarity and
  only the Lashon-specific suffix from the spec.
- **Runtime is portable in mechanism.** The validator is pure data —
  any language can re-implement it from the JSON Schema. The
  `os_steps` runtime adapter (Phase 1b) is Lashon-specific by
  design; another vendor could ship a different adapter against
  the same step vocabulary.
- **First-party feedback loop is tight.** The schemars-derive →
  JSON Schema snapshot test means we can never ship a parser that
  doesn't match the published contract. Hub Creator UI and MCP
  clients consume the schema directly; no hand-maintained schema
  drifts from the parser.
- **Spec extension is non-breaking for additions.** Adding a new
  step type or parameter field bumps the snapshot, regenerates the
  committed schema, but doesn't break existing recipes — `version:
  1` recipes still parse against the v1 types. A breaking change
  (renaming a field, removing a step) requires `version: 2` and a
  parser fork.
- **Composability with the M9 cascade is straightforward.** Tier 3
  (LLM classifier — "which of these 10 recipes fits, or none?") gets
  the `description` field verbatim; tier 1 (regex) gets the
  `intents` phrases verbatim. The schema gives the cascade
  everything it needs without per-recipe glue.

## Migration / open follow-ups

- **Phase 1b runtime** consumes `Step` directly via the public
  `lashon_core::recipes::Step` enum; no schema change needed.
- **Phase 1c intent cascade** uses `Recipe::intents` for tier 1 and
  `Recipe::description` for tier 3; no schema change.
- **Phase 1g MCP server** (ADR-0028) exposes `lashon.list_recipes`
  / `lashon.get_recipe` / `lashon.validate_recipe` /
  `lashon.save_recipe` on top of this format directly.
- **Future: encode permissions in step calls.** A `run_shell` step
  could carry a structured permission token rather than relying on
  a top-level `permissions:` list. Worth revisiting when sandboxing
  lands.

## Notes

The triple-source design is *deliberately under-clever*: we did not
build an abstraction layer that lets a recipe declare which envelope
it follows. The three layers are sequenced fields in a single
`recipe.yaml` because that's the format users will hand-edit; a
configurable envelope-of-envelopes would be more flexible and less
usable. The JSON Schema names the union shape unambiguously so
external tooling has no confusion about which fields are which.
