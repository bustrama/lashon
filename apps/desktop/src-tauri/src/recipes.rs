//! Tauri-side glue for the M9 Phase 1d Hub Recipes tab
//! (`docs/stories/m9-hub-recipes-tab.md`).
//!
//! Every operation a Hub recipe row supports — list, preview, run,
//! open file, duplicate, delete — surfaces as a `#[tauri::command]`
//! in this module. The lib-side logic lives in
//! [`lashon_core::recipes`] and [`lashon_core::recipes::storage`];
//! this module is the thin Tauri wrapper that resolves env paths,
//! plugs the `EventBasedConfirm` from M8 into the runtime, and emits
//! the matching tongue events.
//!
//! Tracing on these commands logs shapes only — counts, ids,
//! permissions list — never argument values. The runtime itself
//! handles interpolated text; nothing here re-logs it, in line with
//! `.claude/rules/security.md`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener};

use lashon_core::recipes::storage::{
    collect_hub_listings, delete_user_recipe as core_delete_user_recipe, duplicate_to_user,
    find_recipe_by_id, load_recipe, update_recipe_comment as core_update_recipe_comment,
    HubRecipeListing,
};
use lashon_core::recipes::{execute_recipe, ConfirmDecision, ConfirmHandler, Recipe, RuntimeError};

/// What [`run_recipe`] returns when a recipe runs to completion. The
/// Hub uses `steps_executed` for the "ran N steps" footer and
/// `summary` as the bilingual flash text.
#[derive(Debug, Clone, Serialize)]
pub struct RunOutcome {
    pub steps_executed: usize,
    pub summary: String,
}

/// Payload of the `recipe:confirm` event the Hub modal listens for.
/// Same shape as the M8 `command:confirm` event so the frontend can
/// route both through the same modal component — the only difference
/// is the event name, so we can subscribe / unsubscribe per surface
/// without mixing recipe + Command-mode confirmations.
#[derive(Debug, Clone, Serialize)]
struct RecipeConfirmRequest {
    id: String,
    /// Logical "tool" name for the modal copy. For recipes the only
    /// destructive step type is `run_shell`, so this is always
    /// `"run_shell"` in v1. Kept as a field so the modal can
    /// dispatch the same way as M8.
    tool: String,
    /// The interpolated command line — the user MUST see it
    /// verbatim before approving. Matches the M8 `run_command` field
    /// of the same name.
    command_preview: String,
}

/// Payload of the `recipe:confirm:reply` event the Hub emits.
#[derive(Debug, Deserialize)]
struct RecipeConfirmReply {
    id: String,
    decision: String,
}

/// List every recipe the Hub Recipes tab should render. Errors are
/// surfaced as rows with `parse_error: Some(_)` so a broken
/// `recipe.yaml` still appears in the list (with the row's "open
/// file" affordance) instead of vanishing silently.
#[tauri::command]
pub async fn list_recipes_for_hub() -> Result<Vec<HubRecipeListing>, String> {
    let rows = tauri::async_runtime::spawn_blocking(collect_hub_listings)
        .await
        .map_err(|err| err.to_string())?;
    tracing::info!(
        recipe_count = rows.len(),
        broken_count = rows.iter().filter(|r| r.parse_error.is_some()).count(),
        "hub: list_recipes_for_hub"
    );
    Ok(rows)
}

/// Load a full recipe (parameters + os_steps) so the Hub can render
/// the slot-fill modal.
#[tauri::command]
pub async fn get_recipe(id: String) -> Result<Recipe, String> {
    let id_for_log = id.clone();
    let recipe = tauri::async_runtime::spawn_blocking(move || load_recipe(&id))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    tracing::info!(
        recipe_id = %id_for_log,
        param_count = recipe.parameters.len(),
        permission_count = recipe.permissions.len(),
        "hub: get_recipe"
    );
    Ok(recipe)
}

/// Execute a recipe with the slot values from the Hub's slot-fill
/// modal. Wires the M8 `EventBasedConfirm` pattern so a `run_shell`
/// step in the recipe surfaces the same modal the M8 `run_command`
/// tool does — one confirmation flow, two callers.
#[tauri::command]
pub async fn run_recipe(
    app: AppHandle,
    id: String,
    args: HashMap<String, String>,
) -> Result<RunOutcome, String> {
    let recipe = load_recipe(&id).map_err(|err| err.to_string())?;
    // Snapshot count + permissions BEFORE handing `args` and
    // `recipe` to the runtime — both get consumed (`args` by value,
    // `recipe.permissions.len()` would still work via the borrow but
    // the snapshot is symmetric and reads cleanly).
    let arg_count = args.len();
    let permission_count = recipe.permissions.len();
    let id_for_log = recipe.id.clone();

    let confirm = EventBasedConfirm::new(app.clone());
    let run = execute_recipe(&recipe, args, &confirm)
        .await
        .map_err(|err| format_runtime_error(&err))?;

    tracing::info!(
        recipe_id = %id_for_log,
        arg_count,
        permission_count,
        steps_executed = run.steps_executed,
        "hub: run_recipe complete"
    );

    let summary = format!(
        "המתכון {id} הסתיים בהצלחה ({n} צעדים).",
        id = id_for_log,
        n = run.steps_executed
    );
    Ok(RunOutcome {
        steps_executed: run.steps_executed,
        summary,
    })
}

/// Open the recipe.yaml in the OS default text editor. v1's "Edit"
/// affordance for user recipes — no inline editor. Resolves the path
/// via `find_recipe_by_id` so the dir-name-vs-id mismatch on the
/// bundled starters is handled correctly.
#[tauri::command]
pub async fn open_recipe_file(app: AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let id_for_log = id.clone();
    let (path, source) = tauri::async_runtime::spawn_blocking(move || find_recipe_by_id(&id))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    tracing::info!(
        recipe_id = %id_for_log,
        source,
        "hub: open_recipe_file"
    );
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|err| format!("פתיחת הקובץ נכשלה: {err}"))?;
    Ok(())
}

/// Duplicate a bundled recipe into the per-user dir. Returns the new
/// id so the Hub can re-select the row immediately. Refuses to
/// duplicate a recipe whose id already lives in the user dir — the
/// design surfaces the Duplicate icon only on bundled rows, but the
/// command defends against an unexpected concurrent state anyway.
#[tauri::command]
pub async fn duplicate_recipe_to_user(id: String) -> Result<String, String> {
    let id_for_log = id.clone();
    let new_id = tauri::async_runtime::spawn_blocking(move || duplicate_to_user(&id))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    tracing::info!(
        recipe_id = %id_for_log,
        new_id = %new_id,
        "hub: duplicate_recipe_to_user"
    );
    Ok(new_id)
}

/// Delete a user recipe. Refuses to delete bundled recipes — the
/// design hides the Trash icon on bundled rows; this is the
/// defence-in-depth.
#[tauri::command]
pub async fn delete_user_recipe(id: String) -> Result<(), String> {
    let id_for_log = id.clone();
    tauri::async_runtime::spawn_blocking(move || core_delete_user_recipe(&id))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    tracing::info!(recipe_id = %id_for_log, "hub: delete_user_recipe");
    Ok(())
}

/// Update the `comment:` field on a single step in a user recipe.
/// Backs the Steps panel's v1.5 inline comment-editing affordance —
/// the Hub calls this on blur / Enter after the user finishes typing.
///
/// `comment` of `null` (None) or `""` removes the comment; the
/// storage layer normalises whitespace-only strings to `None` so the
/// YAML stays clean. Bundled recipes refuse the call defensively even
/// though the design hides the edit affordance on bundled rows.
#[tauri::command]
pub async fn update_recipe_comment(
    id: String,
    step_index: usize,
    comment: Option<String>,
) -> Result<(), String> {
    let id_for_log = id.clone();
    let comment_present = comment.is_some();
    tauri::async_runtime::spawn_blocking(move || core_update_recipe_comment(&id, step_index, comment))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    tracing::info!(
        recipe_id = %id_for_log,
        step = step_index,
        comment_present,
        "hub: update_recipe_comment"
    );
    Ok(())
}

/// Render a [`RuntimeError`] into a Hebrew-friendly string the Hub
/// can show inline beside the slot-fill modal. The `Display` impl on
/// each variant is already user-facing; this wrapper centralises the
/// "what string surfaces to the frontend" decision so we can extend
/// it later without touching every command.
fn format_runtime_error(err: &RuntimeError) -> String {
    err.to_string()
}

/// Event-emitting confirmation gate for the recipe runtime. Same
/// shape as the M8 `EventBasedConfirm` in `command_mode.rs`, but
/// emits the `recipe:confirm` event (not `command:confirm`) so the
/// two surfaces can be wired to independent modal components.
///
/// The lashon-core recipe [`ConfirmHandler`] trait is synchronous —
/// the runtime calls it from an async context but parks the
/// executor thread on the answer (an explicit decision: don't
/// advance to the next step while the user is reading the prompt).
/// A `std::sync::mpsc::sync_channel` is the matching primitive: the
/// Tauri-event listener (which runs on a Tauri internal thread) sends
/// the decision; the runtime's `confirm()` blocks on `recv_timeout`.
/// No nested executor, no `block_on`.
///
/// A 30-second timeout protects against a forgotten modal wedging
/// the take forever — same backstop as the M8 confirm.
/// `pub(crate)` so the M9 dispatcher wire-up in `command_mode.rs`
/// can reuse the same modal channel — voice-triggered recipes
/// and Hub-click-triggered recipes both hit `recipe:confirm`, so
/// the Svelte modal doesn't need to know which surface fired the
/// recipe. Matches ADR-0028's "one modal per concern, not per
/// trigger" pattern.
pub(crate) struct EventBasedConfirm {
    app: AppHandle,
}

impl EventBasedConfirm {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ConfirmHandler for EventBasedConfirm {
    fn confirm(&self, prompt: &str) -> ConfirmDecision {
        let id = format!(
            "recipe-confirm-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let (tx, rx) = std::sync::mpsc::sync_channel::<ConfirmDecision>(1);
        let tx = Arc::new(Mutex::new(Some(tx)));
        let id_clone = id.clone();
        let tx_clone = tx.clone();
        let handler = self.app.listen("recipe:confirm:reply", move |event| {
            let Ok(reply) = serde_json::from_str::<RecipeConfirmReply>(event.payload()) else {
                return;
            };
            if reply.id != id_clone {
                return;
            }
            let decision = if reply.decision == "allow" {
                ConfirmDecision::Allow
            } else {
                ConfirmDecision::Deny
            };
            if let Ok(mut slot) = tx_clone.lock() {
                if let Some(tx) = slot.take() {
                    let _ = tx.send(decision);
                }
            }
        });

        if let Err(err) = self.app.emit(
            "recipe:confirm",
            RecipeConfirmRequest {
                id,
                tool: "run_shell".to_string(),
                command_preview: prompt.to_string(),
            },
        ) {
            tracing::warn!("recipes: failed to emit confirm request: {err}");
            self.app.unlisten(handler);
            return ConfirmDecision::Deny;
        }

        let decision = match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(d) => d,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!("recipes: confirmation timed out");
                ConfirmDecision::Deny
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => ConfirmDecision::Deny,
        };
        self.app.unlisten(handler);
        decision
    }
}
