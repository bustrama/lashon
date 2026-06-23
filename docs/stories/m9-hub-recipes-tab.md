# M9 Phase 1d v1 — Hub Recipes tab

> **Status: shipped on `main` in PR #79.** The Steps panel
> extension that opens from this tab's Eye affordance shipped
> later in PR #81 — see `m9-steps-panel.md`.

Branch: `claude/admiring-lamport-IxJWN` (squash-merged + deleted)
Story doc: `docs/stories/m9-hub-recipes-tab.md`
Design source: `recipes-tab.jsx`, `recipes-modals.jsx`, `recipe-system.jsx`.

## What this lands

The first user-facing surface of M9 — a Hub tab that lists every
recipe on disk, lets the user run them by clicking Run, and slot-fills
parameters via a modal that mirrors the design source. Voice → recipe
(cascade dispatcher wire-up) and the MCP `run_recipe` tool are
separate follow-ups; this PR is the click-driven entry point.

### Backend (Rust)

- **`lashon_core::recipes::storage`** — new module gated behind the
  existing `mcp-server` Cargo feature (since it depends on
  `mcp::recipe_tools` for dir resolution). Surfaces:
  - `HubRecipeListing` — richer than `RecipeListing` (the MCP variant):
    `name`, `permissions`, `tags`, `parameter_count`, `step_count`,
    `path`, `parse_error`. Parse errors surface as rows in the list
    so the Hub can show "open file" instead of vanishing a broken
    recipe.
  - `collect_hub_listings()` — walks the bundled + user dirs,
    dedupes on id (user-dir entry wins), returns a sorted vec.
  - `find_recipe_by_id(id)` — walk-and-match-by-`id`, NOT by dir
    name, since `recipes/starters/lock_workstation/` carries
    `id: lock-workstation`. Returns `(PathBuf, "user" | "bundled")`.
  - `load_recipe(id) -> Recipe` — full parse for the slot-fill modal.
  - `duplicate_to_user(id) -> String` — clones a bundled recipe into
    the user dir, picks the first free `<id>-custom`, `-custom-2`, …,
    rewrites the `id:` field in the new YAML. Returns the new id.
  - `delete_user_recipe(id)` — refuses bundled recipes; deletes the
    user dir. Belt-and-braces canonicalisation check so a symlink
    can't escape `user_recipes_dir`.
  - 7 unit tests covering the user-vs-bundled precedence, parse-error
    rows, dir-vs-id mismatch handling, duplicate naming sequence, and
    the bundled-refused / user-deletes happy paths.

- **`apps/desktop/src-tauri/src/recipes.rs`** — six `#[tauri::command]`
  wrappers:
  - `list_recipes_for_hub`, `get_recipe`, `run_recipe`,
    `open_recipe_file`, `duplicate_recipe_to_user`, `delete_user_recipe`.
  - `run_recipe` builds an `EventBasedConfirm` that emits
    `recipe:confirm` / awaits `recipe:confirm:reply` — same shape as
    the M8 `command:confirm` flow so a future PR can fold both modal
    UIs into a single component. The lashon-core `recipes::ConfirmHandler`
    trait is synchronous (the runtime parks the executor thread on
    the user's answer) so the wire-up uses a `std::sync::mpsc::sync_channel`,
    not `block_on` — no nested executor.
  - Tracing on every command logs shapes only (counts, ids,
    permission list size) — never arg values. `.claude/rules/security.md`.

### Frontend (SvelteKit / Svelte 5)

Hub gains a new section, "מתכונים / Recipes", added to the existing
section switcher rather than a new route — the Hub uses one route
with an in-page nav, and that's the pattern to extend.

- **Design tokens** — `--hearth`, `--hearth-glow`, `--hearth-soft`
  added to `app.css` (the M9 accent for recipe matches; solid, not
  pulsing, to keep "pulse = thinking" reserved for LLM states).

- **Shared design components** — eight reusable Svelte components
  under `apps/desktop/src/lib/design/`:
  - `RecipeGlyph` — the ↯ lightning-fold cascade chevron.
  - `PermissionBadge` — 8-variant badge (`keyboard.type`, `app.focus`,
    `app.open`, `shell.run`, `destructive`, `clipboard`, `screenshot`,
    `network`) + a neutral fallback for unknown kinds. Each kind has
    `aria-label="<he> · <en>"` so screen readers don't just see the glyph.
  - `SourceBadge` — `bundled` / `user` / `mcp` pill with a coloured dot.
  - `TagChip` — `#messaging`-style tag pill.
  - `CodeBlock` — mono code container; ready for the YAML preview pane
    and the run-shell command preview that M9 v1.1 wires.
  - `Banner` — info / warn / success / recipe variants for inline status.
  - `FieldShell` — input shell used by slot-fill controls.
  - `StepDots` — recipe-run progress pill row (for future runs > 3 steps).

- **Recipe-specific components** in `apps/desktop/src/lib/recipes/`:
  - `RecipeRow` — three-column grid (`1.2fr / 2.3fr / auto`), hover-
    or focus-tracked action affordances (`pointerenter` / `focusin`
    so keyboard navigation also reveals them), variant-specific icon
    set (user: Edit + Trash, bundled: Eye + Duplicate). Destructive
    recipes get a rose `insetInlineEnd` marker. Parse errors render
    a rose "פתח קובץ" button instead of Run.
  - `FilterChip` — small pill button for the toolbar (`active`,
    `danger` variants).
  - `EmptyState` — design-spec'd empty pane (10 starters always ship,
    so this only fires when the bundle is corrupt or the user nukes
    the user dir).
  - `SlotFillModal` — modal with typed inputs per `parameter.input_type`
    (string / number / boolean toggle / date / file path), the
    `run_shell` warning banner when the recipe contains one,
    inline runtime-error display via `Banner kind="warn"`. Esc cancels;
    Enter submits. The first input gets autofocus via a `use:autofocus`
    Svelte action; required-slot validation gates the submit button.
  - `RecipesSection` — the tab itself. Holds the listing, search,
    filter, tag scrubber, toast, delete confirmation modal. Routes
    Tauri command results through `flashToast` for the user-visible
    feedback. Zero-parameter recipes skip the slot-fill modal and
    fire directly per the open-question default; the runtime's
    confirm-handler still gates `run_shell` so a zero-param destructive
    recipe (`toggle-dark-mode`, future) won't surprise the user.
  - `types.ts` — TypeScript mirrors of the Rust structs, kept in sync
    by hand. The Tauri command surface is the compile-time enforcement
    point; the types in this file annotate the calls so a future
    refactor surfaces the right errors.

- **i18n** — 28 new keys under `hub.recipes.*` in both `he.json` and
  `en.json`. Nav entry added to `hub.nav.recipes`.

### Open questions — resolutions

All four open questions from the brief were resolved with the
recommended defaults via `AskUserQuestion`:

1. **Edit** — opens the YAML in the OS default editor; no inline editor in v1.
2. **Zero-param Run** — skips the slot-fill modal; flashes a toast
   on completion. Destructive zero-param recipes still go through
   the M8 confirmation modal at `run_shell` time.
3. **Duplicate id collision** — `-custom`, then `-custom-2`, `-custom-3`, …
4. **Delete user recipe command** — shipped, with a confirmation modal.
   No recycle-bin; the recipe directory is `fs::remove_dir_all`d.

## Out of scope (future PRs)

- **Creator UI** (Phase 1e) — natural-language → cloud LLM → YAML.
  The "צור חדש" button in the toolbar currently opens the project's
  marketing site (where the Phase-1g MCP instructions live), so a
  user has a path to authoring recipes via Claude Desktop today.
- **MCP Server tab** (Phase 1g UI half).
- **Cascade dispatcher wire-up** — `try_recipe_cascade` is built
  but the Tauri shell's `command_mode::dispatch_transcript` doesn't
  call it yet. That's a separate, small PR.
- **Recipe Tongue states** (Hearth glow, ↯ glyph at the mark's foot,
  success/error rings) — `recipes-tongue.jsx` is in the design source
  but is a separate concern from the Hub tab.
- **`run_recipe` MCP tool** — exposes the same `run_recipe` over the
  MCP transport so an external agent can fire a recipe. The shape is
  trivial once the Tauri command works; punted to keep scope tight.

## Test plan

- `cargo test -p lashon-core --lib` — runs the 7 new storage tests.
  The tests serialise on a `Mutex<()>` because the recipe-dir env
  vars are process-wide globals; that lock means they cost ~7×
  setup_dirs serially, not 7× in parallel — acceptable for unit
  tests.
- `cargo test -p lashon-core --test recipe_starters` / `recipe_runtime`
  / `recipe_schema_snapshot` / `lashon_mcp_stdio` — should stay green;
  this work doesn't touch them.
- `cargo check --workspace --all-targets` — clean (verified by CI;
  the sandbox can't build `ort-sys` because the prebuilt-binary
  download URL returns 403).
- `npm run check` — clean (0 errors, 0 warnings).
- `npm run build` — clean.

## Manual smoke test (Windows)

1. Open the Hub, click "מתכונים".
2. Verify the 10 starter recipes appear with bundled badge,
   permission badges, and tag chips.
3. Click Run on `lock-workstation` — screen locks immediately
   (zero-param recipe → no modal).
4. Click Run on `send-discord-message` — modal opens with recipient
   + body text inputs; submitting kicks off the Discord chain.
5. Click Run on `batch-rename-files` (destructive — rose marker
   visible) — slot-fill modal shows the shell warning banner;
   submitting hits the M8 confirmation modal with the literal
   interpolated `rsync` command.
6. Hover a row: Run + Eye + Duplicate (bundled) or Run + Edit +
   Trash (user) appear.
7. Click Duplicate on `lock-workstation` — a new `lock-workstation-custom`
   row appears in the list.
8. Click Trash on the duplicate → confirmation modal → confirm → row
   removed.
9. Click Edit on the duplicate before deletion → YAML opens in the
   OS default editor.

## Files

```
packages/shared-rust/
  Cargo.toml                                   # added tempfile dev-dep
  src/recipes/mod.rs                            # storage module export
  src/recipes/storage.rs                        # new — 7 unit tests
apps/desktop/src-tauri/src/
  lib.rs                                        # registered 6 commands
  recipes.rs                                    # new — Tauri shell wrappers
apps/desktop/src/
  app.css                                       # hearth tokens
  lib/design/Banner.svelte                      # new
  lib/design/CodeBlock.svelte                   # new
  lib/design/FieldShell.svelte                  # new
  lib/design/PermissionBadge.svelte             # new
  lib/design/RecipeGlyph.svelte                 # new
  lib/design/SourceBadge.svelte                 # new
  lib/design/StepDots.svelte                    # new
  lib/design/TagChip.svelte                     # new
  lib/i18n/locales/en.json                      # +29 keys
  lib/i18n/locales/he.json                      # +29 keys
  lib/recipes/EmptyState.svelte                 # new
  lib/recipes/FilterChip.svelte                 # new
  lib/recipes/RecipeRow.svelte                  # new
  lib/recipes/RecipesSection.svelte             # new
  lib/recipes/SlotFillModal.svelte              # new
  lib/recipes/types.ts                          # new
  routes/hub/+page.svelte                       # added Recipes section
docs/stories/m9-hub-recipes-tab.md              # this doc
```
