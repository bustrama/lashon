# M9 Phase 1d extension — Steps panel + v1.5 comment editing

> **Status: shipped on `main` in PR #81** (bundled with PR6
> dispatcher wire-up, STT word-aliases, wait_ms tuning, CLI
> lookup-by-id fix, CREATE_NO_WINDOW fix). Follow-up to Phase 1d
> ([`docs/stories/m9-hub-recipes-tab.md`](m9-hub-recipes-tab.md)).
> Design source: `recipes-steps.jsx` from the second design drop.

## What this lands

The visual recipe viewer — the middle ground between *YAML in Notepad*
(power-user) and *one Run button* (no visibility). Click the Eye
affordance on any recipe row → a side drawer opens showing each step
as a typed card. The user can scan exactly what a recipe will do
before pressing Run; on user recipes, they can also annotate
individual steps with comments without touching YAML.

### Steps panel chrome
- **Side drawer** at 520 px, slides in from the inline-end edge so
  the recipe list stays visible behind. Esc + Close button + outside-
  click all dismiss.
- **Hearth top-edge glow** — the recipe accent. Solid, not pulsing.
- **Header:** recipe glyph + Hebrew/English name + source badge +
  permission row + three action buttons (Run / Edit YAML / Duplicate
  [bundled only]) + a slot-pill legend when the recipe takes params.
- **Footer:** step count + per-variant duration estimate
  (`≈ 3.2 s` summing `wait_ms`, `wait_for_window`, etc.) + an `Esc`
  hint.

### Per-step rendering — 12 typed variants
| Variant | Visual |
|---|---|
| `key_chord` | Real keycap visuals — `[Ctrl] + [K]` with gradient + shadow, always Latin |
| `type_unicode` | Quoted-text bubble; `rtl_safe: true` shows a small clipboard tag |
| `click_label` | Quoted label + optional `scope:` tag for the window narrow |
| `focus_window`, `wait_for_window` | Quoted `title_contains`; wait shows an "up to N s" tag |
| `wait_ms` | Mono `N ms` |
| `screenshot_to_clipboard` | Italic note (no variables) |
| `clipboard_set` | Quoted text |
| `clipboard_get_into` | "Save into $varname" with Hearth-tinted var pill |
| `run_shell` | Rose-tinted code block; dry-run shows a rose italic note |
| `open_url` | Mono URL |
| `open_app` | Quoted app name |

`{{ slot }}` placeholders render as Hearth-tinted **`SlotPill`**
inline inside step content — the user sees at a glance which fields
the slot-fill modal substitutes at run time. A small legend in the
header explains it.

### v1.5 inline comment editing
Each step card has an editable comment line below the primary
content. Three states:
- **Populated:** italic `# <text>` below the step
- **Empty + editable:** subtle "+ Add comment" affordance
- **Editing:** input box with `↵ save · Esc revert` hint

The `Step::<variant>.comment` field already exists in the schema
([ADR-0027](../adr/0027-recipe-format-skill-goose-os-steps.md)); this
PR just surfaces it. Read-only on bundled recipes — the affordance is
hidden entirely; existing bundled comments still render.

## Backend changes

### `Step::RunShell.dry_run: bool` (additive)
New field, `#[serde(default)]` so existing recipes parse unchanged.
The runtime branches on it:
- `true` → skips spawn entirely, **bypasses the confirmation gate**
  (nothing to confirm — no side effect), logs `INFO` with the
  command length only, binds `capture_into` (if set) to the sentinel
  string `(dry-run)` so later steps that reference it via
  interpolation don't error
- `false` (default) → existing path

Useful for recipe authors testing a new shell step without side
effects, and for the design's "dry-run only — no changes" annotation
on the rendered shell card.

Added unit test `dry_run_shell_step_skips_execution_and_binds_capture`
exercises the bypass even under `AlwaysDeny` — confirms the gate is
truly skipped.

### `lashon_core::recipes::storage::update_recipe_comment`
Sets / clears the comment on a single step. The Steps panel's v1.5
editor calls this on blur / Enter. Three new `StorageError` variants:
- `NoStepsForHostOs` — the recipe doesn't declare steps for the
  current OS
- `StepNotFound { index, len }` — `step_index` is past the end
- `Validation(String)` / `Serialise(String)` — defensive guards

Refuses bundled recipes (defence-in-depth — the design hides the
affordance, but the storage layer rejects the call anyway).
Canonicalises the resolved path so a symlink can't escape
`user_recipes_dir`. Normalises whitespace-only / empty comments to
`None` so the YAML stays clean. 3 new unit tests.

### Tauri command `update_recipe_comment(id, step_index, comment)`
Thin wrapper around the storage function, wired into the builder.
Tracing logs shapes only — `recipe_id`, `step_index`,
`comment_present: bool` — never the comment text itself
(`.claude/rules/security.md`).

## Frontend changes

### New shared design components (`apps/desktop/src/lib/design/`)
- **`SlotPill.svelte`** — `{{ slot }}` rendering with dimmed braces
- **`Keycap.svelte`** — gradient + shadow keycap; modifier detection
  for wider widths
- **`KeyChord.svelte`** — chord rendering as `[Ctrl] + [K]`
- **`StepIcon.svelte`** — 12 SVG glyphs (one per step variant) lifted
  verbatim from the design source for byte-exact fidelity

### New recipes components (`apps/desktop/src/lib/recipes/`)
- **`EditableComment.svelte`** — three-state comment cell; bundled
  recipes get the read-only branch
- **`StepBody.svelte`** — variant-typed primary content; tokenises
  text fields to render `SlotPill`s inline; handles `run_shell` with
  the rose-tinted CodeBlock + dry-run note
- **`StepCard.svelte`** — the typed card with number gutter + icon
  tile + content + comment. Rose tint for shell. `notImplemented`
  badge for `click_label` (the v1 runtime returns `StepNotImplemented`
  for it).
- **`StepsPanel.svelte`** — the drawer chrome

### Wired into `RecipesSection.svelte`
- New state: `stepsListing`, `stepsRecipe`, `stepsParseError`
- `onEye` now loads the recipe + opens the panel (instead of
  shelling out to the YAML editor as in Phase 1d v1)
- `closeStepsPanel`, `onCommentSave` handlers
- `.steps-overlay` scrim with `backdrop-filter: blur(2px)` (respects
  `prefers-reduced-motion`)

### Type tightening (`types.ts`)
`RecipeStep` is now a tagged-union over the 12 variants instead of
the previous `{ type: string; [key: string]: unknown }` shape. The
TS compiler is now the place where a schema mismatch surfaces;
`StepBody` switches on `step.type` exhaustively.

### i18n
29 new keys per locale under `hub.recipes.steps.*` — chrome strings,
variant labels (`hub.recipes.steps.variants.<variant>`), body
extras (`hub.recipes.steps.bodyExtras.upTo`, `.rtlSafe`, etc.).

## Decisions called out by the designer that ship as-is
- **Side drawer** (not modal) — recipe list stays visible behind
- **Latin step numbers** on the inline-start gutter
- **Keycaps always Latin** regardless of locale
- **Long step content wraps**, no truncation
- **Hearth top-edge glow + solid (not pulsing) accent** — keeps
  "pulse = thinking" reserved for LLM states

## Design assumptions NOT shipped
- **`bundle_id` on `open_app`** — designer's sample data had it; not
  in our Rust schema, runtime doesn't need it on Windows. Field
  absent; renders nothing.

## Test plan

- [x] `cargo test -p lashon-core --lib` — 345 passed, 6 ignored
  (was 305 + 8 storage + 4 runtime + 1 dry_run + ...)
- [x] `cargo test -p lashon-core --test recipe_runtime` — 6 passed (1
  ignored for clipboard)
- [x] `cargo test -p lashon-core --test recipe_starters` — 3 passed
- [x] `cargo test -p lashon-core --test recipe_schema_snapshot` — 1
  passed (regenerated the committed JSON Schema for the new
  `dry_run` field)
- [x] `cargo check --workspace --all-targets` clean
- [x] `npm run check` — 0 errors, 0 warnings
- [ ] CI green on Windows
- [ ] Manual: open Hub → Recipes → Eye on `send-discord-message` →
  panel shows 8 step cards with the `{{ recipient }}` / `{{ message }}`
  slots highlighted in Hearth. Eye on `batch-rename-files` → shell
  step shows rose-tinted code block. Click Add comment on a user
  recipe → type → Enter → comment persists. Re-open → comment is
  still there.

## Definition of done

- Steps panel opens from the Eye affordance, renders all 12 step
  variants correctly
- Slot placeholders render as Hearth-tinted pills inline
- Run / Edit YAML / Duplicate header buttons wire to existing handlers
- v1.5 comment editing persists via `update_recipe_comment` Tauri command
- Bundled recipes are read-only (no Add comment affordance)
- Parse-error state renders the banner + "Open file" button
- Empty-steps state renders the info banner
- `Step::RunShell.dry_run` field added; runtime honours it; schema
  snapshot regenerated
- Story doc + CLAUDE.md branch summary updated
