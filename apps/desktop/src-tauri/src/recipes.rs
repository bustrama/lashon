//! Tauri-side glue for the M9 Phase 1d Hub Recipes tab
//! (`docs/stories/m9-hub-recipes-tab.md`).
//!
//! Every operation a Hub recipe row supports — list, preview, run,
//! open file, duplicate, delete — surfaces as a `#[tauri::command]`
//! in this module. The lib-side logic lives in
//! [`ottid_core::recipes`] and [`ottid_core::recipes::storage`];
//! this module is the thin Tauri wrapper that resolves env paths and
//! plugs the approval card (`crate::approval`, docs/adr/0048) into the
//! runtime's confirmation gate.
//!
//! Tracing on these commands logs shapes only — counts, ids,
//! permissions list — never argument values. The runtime itself
//! handles interpolated text; nothing here re-logs it, in line with
//! `.claude/rules/security.md`.

use std::collections::HashMap;

use serde::Serialize;
use tauri::AppHandle;

use ottid_core::approval::{Cancel, Decision, Request};
use ottid_core::recipes::storage::{
    collect_hub_listings, delete_user_recipe as core_delete_user_recipe, duplicate_to_user,
    find_recipe_by_id, load_recipe, update_recipe_comment as core_update_recipe_comment,
    HubRecipeListing,
};
use ottid_core::recipes::{execute_recipe, ConfirmDecision, ConfirmHandler, Recipe, RuntimeError};

/// What [`run_recipe`] returns when a recipe runs to completion. The
/// Hub uses `steps_executed` for the "ran N steps" footer and
/// `summary` as the bilingual flash text.
#[derive(Debug, Clone, Serialize)]
pub struct RunOutcome {
    pub steps_executed: usize,
    pub summary: String,
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
/// modal. A `run_shell` step asks through the same approval card as the
/// M8 `run_command` tool — one confirmation flow, every caller.
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

    // The Hub has no way to cancel a run, so nothing calls this off.
    let confirm = CardConfirm::new(app.clone(), Cancel::new());
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

/// The recipe runtime's confirmation gate: asks through the approval
/// card (`crate::approval`) with the interpolated command line, exactly
/// what would run.
///
/// The ottid-core recipe [`ConfirmHandler`] trait is synchronous — the
/// runtime calls it from an async context but parks the executor thread
/// on the answer (an explicit decision: don't advance to the next step
/// while the user is reading the prompt). The card denies the step after
/// 30 s unanswered.
///
/// `pub(crate)` so the M9 dispatcher in `command_mode.rs` reuses it:
/// voice-triggered and Hub-triggered recipes both ask through the card
/// (ADR-0028's "one modal per concern, not per trigger"). Aborting a
/// voice-triggered take doesn't reach a thread parked here, so the take
/// calls its `Cancel` too: the card comes down and the step is denied.
pub(crate) struct CardConfirm {
    app: AppHandle,
    cancelled: Cancel,
}

impl CardConfirm {
    pub(crate) fn new(app: AppHandle, cancelled: Cancel) -> Self {
        Self { app, cancelled }
    }
}

impl ConfirmHandler for CardConfirm {
    fn confirm(&self, prompt: &str) -> ConfirmDecision {
        let request = Request::for_recipe_step(prompt);
        // The runtime asks from inside its async run, on one of Tauri's
        // tokio workers, and the user may take up to 30 s to answer. Tell
        // tokio this worker blocks, so it hands the worker's other tasks
        // (the approval timer and hotkey answers among them) to another.
        let decision = tokio::task::block_in_place(|| {
            crate::approval::ask_blocking(&self.app, request, &self.cancelled)
        });
        match decision {
            Decision::Allow => ConfirmDecision::Allow,
            Decision::Deny => ConfirmDecision::Deny,
        }
    }
}
