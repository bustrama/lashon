# 29. Cascade lives as a pre-pass inside the Command-mode dispatcher

## Status

Accepted — landed in PR #81 as the wire-up for M9 Phase 1c (PR6 on
the task list). Resolves open question #1 in
`docs/stories/m9-recipes.md`.

## Context

Phase 1c shipped `lashon_core::recipes::cascade::try_recipe_cascade`
as a pure helper. The remaining question was *where in the Tauri
shell* it gets called. The two candidates were:

1. **Pre-pass inside `command_mode::dispatch`.** Voice → STT → cascade
   → on match, return; on miss, fall through to LLM planner. Couples
   recipes to Command mode; the cascade is invisible to Chat mode
   and to any future surface (e.g. an in-tray quick-action launcher).
2. **Parallel dispatcher `recipe_mode::dispatch`.** A second top-level
   route the Tauri shell can call independently of `command_mode::dispatch`.
   Cleanly separates the LLM-planning concern from the deterministic
   recipe-execution concern. More code to wire; needs the dictation
   worker to know which dispatcher to call.

## Decision

**Pre-pass inside `command_mode::dispatch`** in `apps/desktop/src-tauri/src/command_mode.rs::run`:

1. After the post-STT word-aliases substitution (ADR-0030), build a
   `CascadeMatcher::default_phase_1c_v1()` and load installed recipes
   via `lashon_core::recipes::storage::collect_recipes()`.
2. Call `try_recipe_cascade(&matcher, &recipes, recipe_confirm, &transcript)`.
3. On `Ok(CommandRoute::Recipe { recipe_id, tier, run })`: emit a
   `command:recipe-matched` event (recipe_id + tier + steps), build
   a `CommandResultEvent` from the `RecipeRun`, emit `command:result`
   + `command:state idle`, **return early**.
4. On `Ok(CommandRoute::Planner)`: fall through to the existing
   provider resolution + `dispatch(...)` call unchanged.
5. On `Err(RuntimeError)`: surface the error to the tongue rather
   than silently retrying via the planner — if the user *meant* the
   recipe, the planner won't help and could do something unexpected.

Reuses the existing `recipes::EventBasedConfirm` (promoted from
private to `pub(crate)`) so voice-triggered and Hub-click-triggered
`run_shell` confirmations both fire `recipe:confirm` and share the
same modal.

## Why pre-pass and not parallel

- **Skip the expensive Local-LLM spawn on a cascade hit.** The
  pre-pass runs *before* `crate::llm::ensure_local_llm_base_url`,
  which can take 5–30 s on first chat (Vulkan device init + model
  load). Voice → recipe now takes ~50 ms cascade + ~1.4 s runtime;
  voice → planner takes ~5–10 s. The pre-pass placement is
  load-bearing for the perceived speed.
- **Lashon has one voice entry point today (Command mode).** A
  parallel dispatcher buys flexibility for surfaces that don't
  exist. When Chat mode wants to short-circuit on a recipe pattern
  too (M10+), it can call `try_recipe_cascade` from its own
  pre-pass — the helper is generic.
- **One audit surface.** Every voice command flows through one
  dispatch path; tracing + the security rule about
  transcript-content-never-logged stay in one place.

## Consequences

- **Recipes are coupled to Command mode** (per design). Chat mode
  doesn't see them today. M10 picks this up when Chat mode lands.
- **Failure on the recipe path stops the take** rather than
  fallback-to-planner. This is deliberate: a recipe that matched
  intent but failed at runtime (window not found, user denied
  shell, etc.) means the user's intent was understood — there's
  nothing for the planner to add. Surfacing the error directly is
  more honest.
- **Provider resolution + LLM tool catalogue construction happen
  only on cascade miss.** Saves ~50–200 ms even on the LLM path
  (no wasted work that gets thrown away on a hit).
- **`command:recipe-matched` is a new event** the tongue will use
  in a future Phase 1d polish to render a distinct Hearth-tinted
  match indicator. v1 just emits it for diagnostics; the tongue
  consumer is a follow-up.

## Notes

The "couples recipes to Command mode" concern from open question
#1 is real but small: the cascade helper is pure (no Command-mode
specifics), so a future Chat-mode caller is two lines of wiring,
not a refactor. The parallel-dispatcher option stays available
without code change.
