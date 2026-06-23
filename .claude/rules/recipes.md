# Recipes & subprocess spawning

M9 introduced recipes (`recipe.yaml` files run by
`lashon_core::recipes::runtime`), the cascade dispatcher (recipes
short-circuit voice commands before the LLM planner), the MCP
server (`lashon-mcp` stdio binary exposes recipe management to
agent hosts), and a new family of subprocess-spawn sites. A few
patterns are now load-bearing across all of them.

## Spawning subprocesses

**Every Windows subprocess spawn that the user shouldn't see
flash a console window MUST set
`.creation_flags(0x0800_0000)` (CREATE_NO_WINDOW).** Tokio's
`Command` exposes this directly on Windows; `std::process::Command`
needs `use std::os::windows::process::CommandExt;` in scope.

This is non-negotiable for **any spawn that runs as part of a
voice take or a Hub click** because the recipe runtime's whole
purpose is to act on the user's foreground app — a console flash
that steals focus mid-recipe defeats the point. The same applies
to the M8 `run_command` tool, the MCP binary, llama-server, the
STT sidecar, and `open_app`.

Sites that already follow this:
- `lashon_core::llama_server::spawn`
- `lashon_core::sidecar::spawn`
- `lashon_core::tools::open_app::launch`
- `lashon_core::tools::run_command::build_command`
- `lashon_core::recipes::runtime::run_powershell`
- `lashon_core::recipes::runtime::run_open_app`

The pattern was retrofitted to `run_command` (pre-existing M8 bug)
+ both new recipe-runtime sites in PR #81 after the bug surfaced
when running runtime tests stole the user's window focus. New
spawn sites must not regress this. CI doesn't catch it because
the symptom is visual (focus theft); it caught us during
end-to-end testing instead.

## Recipe storage layer (`lashon_core::recipes::storage`)

Three invariants the storage helpers preserve — preserve them in
any new helper you add:

1. **Canonicalise the resolved path against `user_recipes_dir`
   before any write or delete.** A symlink that escapes the user
   dir is refused. `delete_user_recipe` + `update_recipe_comment`
   already do this; new mutating helpers must too.
2. **Refuse to mutate bundled recipes.** They ship from the
   installer and shouldn't be silently overwritten. `find_recipe_by_id`
   returns `(PathBuf, "user" | "bundled")` so the caller knows which
   it has; mutators check the second tuple element and bail with
   `StorageError::BundledNotWritable` on bundled.
3. **Normalise whitespace-only / empty inputs to `None`** before
   writing back as YAML. The `comment` field is the canonical
   example; empty-string normalisation keeps the YAML clean and
   matches `serde`'s `skip_serializing_if = "Option::is_none"`
   behaviour.

The storage layer lives behind the `mcp-server` Cargo feature only
because it shares dir-resolution helpers with `mcp::recipe_tools`;
the gating is incidental, not architectural. If a non-MCP caller
needs storage, fold the dir resolution into a shared module
rather than splitting the feature.

## Recipe authoring

When adding a new recipe under `recipes/starters/`:

- The directory name uses **snake_case**; the recipe's `id:` field
  uses **kebab-case**. `tests/recipe_starters.rs::every_starter_has_unique_id_matching_directory_name`
  asserts the snake_case form matches the kebab-case id with `-`
  replaced by `_`.
- Every recipe declares **at least one intent phrase** in both
  Hebrew and English. The cascade is regex-only in v1 (per
  `docs/stories/m9-intent-cascade.md`); phrasings that aren't in
  the list won't match. Mirror Hebrew variants across the 2×2×2
  matrix of `{שלח/תשלח} × {ל/ב} × {with/without הודעה}` for chat
  recipes — see the four `send_*_message` starters.
- `run_shell` steps require the `shell.run` permission in
  `permissions:`. The validator catches a missing declaration.
- For pure-preview shell steps (rendering the interpolated
  command without executing), set `dry_run: true` — see ADR-0031.

## Cascade extension

When adding a new tier to the cascade (tier 2 embedding, tier 3
LLM classifier — both deferred per `docs/stories/m9-recipes.md`):

- Implement `recipes::intent::IntentMatcher` and add to
  `CascadeMatcher::new(...)` — the existing trait is the extension
  point.
- The tier order is priority order; the cheaper / more deterministic
  tier runs first. Tier 1 (regex) returns instantly on a clean
  match; only on a miss does the next tier get called.
- Each tier MUST return `Some(MatchedIntent)` only when the slot
  values it extracted are non-empty + the recipe id exists in the
  passed-in `recipes` slice. The runtime errors otherwise.
