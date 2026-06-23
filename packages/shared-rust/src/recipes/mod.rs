//! M9 Phase 1a — recipe schema and validator (docs/stories/m9-recipes.md).
//!
//! A **recipe** is a pre-recorded parameterised desktop workflow stored as
//! `recipe.yaml` in its own directory under either the per-user recipes
//! directory (`%APPDATA%\lashon\recipes\`, `~/.config/lashon/recipes/`) or
//! the bundled `recipes/starters/` tree. The dispatcher's intent cascade
//! (Phase 1c) routes natural-language commands to a matching recipe
//! whenever one is available, short-circuiting the LLM full-planner path
//! to a deterministic 0–1-turn replay.
//!
//! Phase 1a was the spec + parser + validator ([`schema`] +
//! [`validate`]). Phase 1b is the runtime executor at [`runtime`].
//! Phase 1c is the intent cascade at [`intent`] (matches a
//! transcript to a recipe) plus [`cascade`] (orchestrates match +
//! execute as a single helper).
//!
//! ## Format ancestry
//!
//! The schema deliberately composes three existing formats so recipes can
//! be authored by any Agent-Skills-aware client and could be lifted into a
//! standalone marketplace later:
//!
//! | Layer | Source | What we adopt |
//! |---|---|---|
//! | Identity envelope | Anthropic Agent Skills `SKILL.md` | `id`, `name`, `description`, `tags`, `permissions` |
//! | Parameter schema | Goose Recipes (`block/goose`, now AAIF) | `parameters[]` with `key`, `input_type`, `requirement`, `description`, `default` |
//! | OS-UI primitives | Lashon-specific | `os_steps:` per-OS step list — `key_chord`, `type_unicode`, `click_label`, `focus_window`, `wait_for_window`, `wait_ms`, `screenshot_to_clipboard`, `clipboard_set`, `clipboard_get_into`, `run_shell`, `open_url`, `open_app` |
//!
//! No vendor crate is depended on; the schema is reconstructed from public
//! documentation so we can extend the OS-step set freely.

pub mod cascade;
pub mod intent;
pub mod runtime;
pub mod schema;
#[cfg(feature = "mcp-server")]
pub mod storage;
pub mod validate;

pub use cascade::{try_recipe_cascade, CommandRoute};
pub use intent::{CascadeMatcher, IntentMatcher, MatchTier, MatchedIntent, RegexMatcher};
pub use runtime::{
    execute_recipe, execute_recipe_for_os, AlwaysAllow, AlwaysDeny, ConfirmDecision,
    ConfirmHandler, RecipeRun, RuntimeError,
};
pub use schema::{
    OsSteps, Parameter, ParameterRequirement, ParameterType, Recipe, Region, Step, SCHEMA_VERSION,
};
#[cfg(feature = "mcp-server")]
pub use storage::{
    collect_hub_listings, delete_user_recipe, duplicate_to_user, find_recipe_by_id, load_recipe,
    HubRecipeListing, StorageError,
};
pub use validate::{validate_recipe, ValidationError, ValidationIssue};
