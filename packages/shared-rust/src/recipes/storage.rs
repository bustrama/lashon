//! M9 Phase 1d helpers — Hub-facing recipe discovery, duplication,
//! and deletion.
//!
//! [`crate::mcp::recipe_tools`] already knows how to walk the bundled
//! + per-user directories for the MCP server's `list_recipes` tool.
//! The Hub needs more than the MCP listing exposes — permissions,
//! tags, parameter + step counts, and a non-fatal `parse_error` row
//! when a `recipe.yaml` is malformed (so the user can see what's
//! broken and click "open file"). The richer listing plus the two
//! write operations (`duplicate_to_user`, `delete_user_recipe`) live
//! here so the lib stays GUI-independent and the Tauri shell is a
//! thin wrapper.
//!
//! Directory-name convention: a recipe's on-disk directory does not
//! have to match its `id:` (the starters use `lock_workstation/` for
//! `id: lock-workstation`). [`find_recipe_by_id`] therefore walks
//! each directory and matches on the parsed recipe's `id` field —
//! never on the directory name. Newly-duplicated recipes use the id
//! as the directory name (kebab-case), since the user wrote it that
//! way and we control the layout from that point on.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::mcp::recipe_tools::{bundled_recipes_dir, user_recipes_dir};
use crate::recipes::Recipe;

/// One row in the Hub's Recipes browser. Carries enough metadata to
/// render a row + decide the hover affordances without a follow-up
/// read of `recipe.yaml`. `parse_error` is `Some(_)` when the file
/// exists but didn't deserialise; the Hub renders an error row with
/// an "open file" button instead of a Run button.
#[derive(Debug, Clone, Serialize)]
pub struct HubRecipeListing {
    /// Recipe id from the YAML, or a synthesised id derived from the
    /// directory name when the file failed to parse.
    pub id: String,
    /// Display name in Hebrew. The YAML's `name:` field is bilingual
    /// by convention (Hebrew + English in a single string); the Hub
    /// renders the raw value with `dir="auto"` so bidi handles it.
    pub name: String,
    /// One-line description from the recipe (or the parse-error
    /// message when broken).
    pub description: String,
    /// `"bundled"` (read-only) or `"user"` (writable). The Hub
    /// decides hover affordances from this — bundled gets Eye +
    /// Duplicate; user gets Edit + Trash. MCP-spawned recipes don't
    /// land on disk in Phase 1d, so that source variant isn't
    /// produced yet (the design system still defines it for future
    /// use).
    pub source: String,
    /// Permission strings as declared in the YAML. The Hub maps each
    /// to a [`PermissionBadge`]; an unknown permission renders as a
    /// neutral tag.
    pub permissions: Vec<String>,
    /// Free-form tags (`messaging`, `media`, …) — drives the tag
    /// scrubber and the per-row chips.
    pub tags: Vec<String>,
    /// Number of declared parameters. The Hub uses this to decide
    /// whether to skip the slot-fill modal (zero parameters → single
    /// confirmation).
    pub parameter_count: usize,
    /// Number of host-OS steps. Zero is legal (a recipe with only
    /// non-host-OS variants returns 0 here). The Hub uses it for the
    /// "N steps" subtitle.
    pub step_count: usize,
    /// On-disk path to the recipe.yaml. Used by `open_recipe_file`
    /// to hand to the OS opener.
    pub path: String,
    /// `Some(message)` when the file failed to parse. The Hub uses
    /// it to render the error-row state. The `id` field is still
    /// populated (synthesised from the directory name) so the row
    /// has a stable key.
    pub parse_error: Option<String>,
}

/// Errors surfaced by the Hub's recipe-management operations. The
/// Tauri shell maps `Display` straight through to the frontend; the
/// strings are user-facing (Hebrew-friendly).
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("לא נמצא מתכון בשם {id:?}")]
    NotFound { id: String },

    #[error("מתכון מובנה אינו ניתן למחיקה: {id:?}")]
    BundledNotWritable { id: String },

    #[error("קריאה נכשלה: {0}")]
    Io(#[from] io::Error),

    #[error("פענוח YAML נכשל: {0}")]
    Parse(#[from] serde_yaml_ng::Error),

    #[error("הקובץ {path:?} אינו תחת תיקיית המתכונים של המשתמש")]
    PathOutsideUserDir { path: String },

    /// `os_steps.<host_os>` is `None` — no steps to mutate. Only
    /// surfaces from [`update_recipe_comment`]; the runtime maps the
    /// same condition to `RuntimeError::NoStepsForOs`.
    #[error("המתכון לא מוגדר עבור מערכת ההפעלה הנוכחית")]
    NoStepsForHostOs,

    /// `step_index` is past the end of the host-OS step list.
    #[error("צעד {index} אינו קיים (יש {len} צעדים)")]
    StepNotFound { index: usize, len: usize },

    /// Validator rejected the recipe after the comment edit. Should
    /// never fire from a comment-only edit (comments don't influence
    /// any validator rule), but cheap to check; surfaces the issue
    /// instead of writing a recipe that wouldn't load.
    #[error("אימות נכשל לאחר העריכה: {0}")]
    Validation(String),

    /// Writing the YAML back failed (rare — usually a serde bug or a
    /// type that can't be serialised). Carried as a string so the
    /// error doesn't pull a separate dep into callers.
    #[error("שמירה נכשלה: {0}")]
    Serialise(String),
}

/// Walk the bundled and per-user directories and produce one
/// [`HubRecipeListing`] per `recipe.yaml` found. Parse failures
/// become error rows (still surfaced to the Hub) rather than skipped
/// silently — a broken file is something the user wants to see.
///
/// The result is sorted by id for deterministic display order. When
/// the same id exists in both dirs, the user-dir entry wins (and
/// the bundled entry is suppressed). That precedence matches
/// [`crate::mcp::recipe_tools::find_recipe_path`].
pub fn collect_hub_listings() -> Vec<HubRecipeListing> {
    let mut out: Vec<HubRecipeListing> = Vec::new();
    // Collect user first; bundled rows are skipped when an
    // identically-id'd user row already exists.
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (dir, source) in [
        (user_recipes_dir(), "user"),
        (bundled_recipes_dir(), "bundled"),
    ] {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(err) => {
                tracing::debug!(dir = %dir.display(), %source, "skip listing: {err}");
                continue;
            }
        };
        for entry in entries.flatten() {
            let dir_path = entry.path();
            let recipe_yaml = dir_path.join("recipe.yaml");
            if !recipe_yaml.is_file() {
                continue;
            }
            let body = match fs::read_to_string(&recipe_yaml) {
                Ok(body) => body,
                Err(err) => {
                    out.push(error_row(&recipe_yaml, &dir_path, source, &err.to_string()));
                    continue;
                }
            };
            let row = match serde_yaml_ng::from_str::<Recipe>(&body) {
                Ok(recipe) => {
                    if seen_ids.contains(&recipe.id) {
                        // Already surfaced by the user dir; skip the
                        // bundled mirror.
                        continue;
                    }
                    seen_ids.insert(recipe.id.clone());
                    row_for(&recipe, &recipe_yaml, source)
                }
                Err(err) => error_row(&recipe_yaml, &dir_path, source, &err.to_string()),
            };
            out.push(row);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn row_for(recipe: &Recipe, path: &Path, source: &str) -> HubRecipeListing {
    let step_count = host_steps_count(recipe);
    HubRecipeListing {
        id: recipe.id.clone(),
        name: recipe.name.clone(),
        description: recipe.description.clone(),
        source: source.to_string(),
        permissions: recipe.permissions.clone(),
        tags: recipe.tags.clone(),
        parameter_count: recipe.parameters.len(),
        step_count,
        path: path.to_string_lossy().into_owned(),
        parse_error: None,
    }
}

fn error_row(path: &Path, dir: &Path, source: &str, err: &str) -> HubRecipeListing {
    let id = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    HubRecipeListing {
        id: id.clone(),
        name: id,
        description: err.lines().next().unwrap_or(err).to_string(),
        source: source.to_string(),
        permissions: vec![],
        tags: vec![],
        parameter_count: 0,
        step_count: 0,
        path: path.to_string_lossy().into_owned(),
        parse_error: Some(err.to_string()),
    }
}

/// Walk the bundled + per-user dirs and return every parseable
/// [`Recipe`]. The intent cascade ([`crate::recipes::cascade::try_recipe_cascade`])
/// consumes this slice. Parse failures are skipped silently here —
/// unlike [`collect_hub_listings`], the cascade has no UI to surface
/// "broken recipe X". Per-user wins over bundled on id collision.
///
/// Cheap: 10 starter YAMLs + however many user recipes; deserialised
/// once per dispatch. If/when the dispatcher dispatches > 100x per
/// minute the Tauri shell can cache the result behind a
/// `notify`-watched invalidation; v1 just re-reads.
pub fn collect_recipes() -> Vec<Recipe> {
    use crate::mcp::recipe_tools::{bundled_recipes_dir, user_recipes_dir};
    let mut out: Vec<Recipe> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for dir in [user_recipes_dir(), bundled_recipes_dir()] {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let yaml = entry.path().join("recipe.yaml");
            if !yaml.is_file() {
                continue;
            }
            let Ok(body) = fs::read_to_string(&yaml) else {
                continue;
            };
            let Ok(recipe) = serde_yaml_ng::from_str::<Recipe>(&body) else {
                continue;
            };
            if seen.insert(recipe.id.clone()) {
                out.push(recipe);
            }
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn host_steps_count(recipe: &Recipe) -> usize {
    recipe
        .os_steps
        .windows
        .as_ref()
        .map(|v| v.len())
        .unwrap_or(0)
}
#[cfg(target_os = "macos")]
fn host_steps_count(recipe: &Recipe) -> usize {
    recipe.os_steps.macos.as_ref().map(|v| v.len()).unwrap_or(0)
}
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn host_steps_count(recipe: &Recipe) -> usize {
    recipe.os_steps.linux.as_ref().map(|v| v.len()).unwrap_or(0)
}

/// Find the `recipe.yaml` for a given id by walking the same
/// dirs as [`collect_hub_listings`]. Matches on the parsed
/// `Recipe::id`, NOT the directory name — `recipes/starters/`
/// uses `lock_workstation/` for `id: lock-workstation`. Per-user
/// dir wins over bundled when both contain the same id.
///
/// Returns `Ok((path, source))` where `source` is `"user"` or
/// `"bundled"`. The source is what callers (delete_user_recipe,
/// duplicate_recipe_to_user) consult to refuse destructive ops on
/// bundled recipes.
pub fn find_recipe_by_id(id: &str) -> Result<(PathBuf, &'static str), StorageError> {
    for (dir, source) in [
        (user_recipes_dir(), "user"),
        (bundled_recipes_dir(), "bundled"),
    ] {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let recipe_yaml = entry.path().join("recipe.yaml");
            if !recipe_yaml.is_file() {
                continue;
            }
            let body = match fs::read_to_string(&recipe_yaml) {
                Ok(body) => body,
                Err(_) => continue,
            };
            let recipe: Recipe = match serde_yaml_ng::from_str(&body) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if recipe.id == id {
                return Ok((recipe_yaml, source));
            }
        }
    }
    Err(StorageError::NotFound { id: id.to_string() })
}

/// Load a full [`Recipe`] by id. Bubbles parse errors so the slot-fill
/// modal can surface a useful message; callers that need a graceful
/// fallback for a broken recipe should use [`collect_hub_listings`]
/// and check `parse_error` first.
pub fn load_recipe(id: &str) -> Result<Recipe, StorageError> {
    let (path, _) = find_recipe_by_id(id)?;
    let body = fs::read_to_string(&path)?;
    let recipe: Recipe = serde_yaml_ng::from_str(&body)?;
    Ok(recipe)
}

/// Duplicate a bundled recipe into the per-user dir, choosing a free
/// `<id>-custom` (or `-custom-2`, `-custom-3`, …) id and matching
/// directory name. Refuses to duplicate a recipe whose id is already
/// in the user dir — the Hub's hover affordances make that case
/// inaccessible, but we defend against an unexpected concurrent
/// state anyway.
///
/// Returns the new id so the Hub can re-select the row.
pub fn duplicate_to_user(id: &str) -> Result<String, StorageError> {
    let recipe = load_recipe(id)?;

    let user_root = user_recipes_dir();
    fs::create_dir_all(&user_root)?;

    let new_id = next_available_id(&user_root, &recipe.id);
    let new_dir = user_root.join(&new_id);
    fs::create_dir_all(&new_dir)?;

    let mut new_recipe = recipe;
    new_recipe.id = new_id.clone();
    // Author preserved; recipe_version reset to 1.0.0 so the user's
    // copy starts fresh and any future upstream bump doesn't shadow
    // their edits.
    new_recipe.recipe_version = "1.0.0".to_string();

    let yaml = serde_yaml_ng::to_string(&new_recipe)?;
    fs::write(new_dir.join("recipe.yaml"), yaml)?;

    Ok(new_id)
}

/// Pick the first un-occupied `<base>-custom[-N]` id under `user_root`.
/// Probes the directory name only — that's how the duplicate writes,
/// and it's the cheap, race-tolerant check.
fn next_available_id(user_root: &Path, base: &str) -> String {
    let first = format!("{base}-custom");
    if !user_root.join(&first).exists() {
        return first;
    }
    for n in 2..1000 {
        let candidate = format!("{base}-custom-{n}");
        if !user_root.join(&candidate).exists() {
            return candidate;
        }
    }
    // Fallback — 999 collisions on a single id is absurd, but
    // returning *something* is better than panicking.
    format!(
        "{base}-custom-{n}",
        n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    )
}

/// Delete a user recipe. Refuses to touch a bundled recipe.
/// Removes the recipe's directory and everything inside it — a
/// recipe directory only ever contains the `recipe.yaml`, so this is
/// equivalent to deleting the single file, but staying at directory
/// granularity matches the on-disk layout and future-proofs us against
/// per-recipe attachments (icons, sample inputs).
///
/// As a belt-and-braces check the resolved path must canonicalise to
/// be a descendant of [`user_recipes_dir`]; a symlink that escapes
/// the user dir is refused.
pub fn delete_user_recipe(id: &str) -> Result<(), StorageError> {
    let (path, source) = find_recipe_by_id(id)?;
    if source != "user" {
        return Err(StorageError::BundledNotWritable { id: id.to_string() });
    }
    let dir = path
        .parent()
        .ok_or_else(|| StorageError::NotFound { id: id.to_string() })?;
    let user_root = user_recipes_dir();
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_root = user_root
        .canonicalize()
        .unwrap_or_else(|_| user_root.clone());
    if !canonical_dir.starts_with(&canonical_root) {
        return Err(StorageError::PathOutsideUserDir {
            path: dir.to_string_lossy().into_owned(),
        });
    }
    fs::remove_dir_all(dir)?;
    Ok(())
}

/// Update the `comment:` field of a single step in a user recipe.
/// The Steps panel's v1.5 inline-comment-editing affordance calls this
/// per blur / Enter. Validates the resulting recipe before writing —
/// a malformed comment can't sneak past `validate_recipe`.
///
/// `step_index` is 0-based into the host-OS variant of `os_steps`.
/// `comment` of `None` removes the comment; `Some("")` is normalised
/// to `None` so the YAML stays clean.
///
/// Refuses bundled recipes (they ship from the installer and shouldn't
/// be silently mutated). Same canonicalisation guard as
/// [`delete_user_recipe`] for symlink safety.
pub fn update_recipe_comment(
    id: &str,
    step_index: usize,
    comment: Option<String>,
) -> Result<(), StorageError> {
    let (path, source) = find_recipe_by_id(id)?;
    if source != "user" {
        return Err(StorageError::BundledNotWritable { id: id.to_string() });
    }
    let dir = path
        .parent()
        .ok_or_else(|| StorageError::NotFound { id: id.to_string() })?;
    let user_root = user_recipes_dir();
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let canonical_root = user_root
        .canonicalize()
        .unwrap_or_else(|_| user_root.clone());
    if !canonical_dir.starts_with(&canonical_root) {
        return Err(StorageError::PathOutsideUserDir {
            path: dir.to_string_lossy().into_owned(),
        });
    }

    let body = fs::read_to_string(&path)?;
    let mut recipe: Recipe = serde_yaml_ng::from_str(&body)?;

    // Normalise empty / whitespace-only comment to `None` — the YAML
    // serialiser then omits the field entirely, which is cleaner than
    // a `comment: ""` line.
    let normalised = comment.and_then(|c| {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });

    // Mutate the host-OS variant. Returns `StepNotFound` when
    // `step_index` is past the end so the Hub can surface a useful
    // error rather than silently writing nothing.
    let steps = host_os_steps_mut(&mut recipe.os_steps);
    let Some(steps) = steps else {
        return Err(StorageError::NoStepsForHostOs);
    };
    let Some(step) = steps.get_mut(step_index) else {
        return Err(StorageError::StepNotFound {
            index: step_index,
            len: steps.len(),
        });
    };
    set_step_comment(step, normalised);

    // Validate before writing so a malformed save (we shouldn't be able
    // to produce one from a comment edit, but the cost of running the
    // validator is zero) is caught with a useful error instead of
    // landing in the YAML.
    crate::recipes::validate_recipe(&recipe)
        .map_err(|err| StorageError::Validation(err.to_string()))?;

    let yaml = serde_yaml_ng::to_string(&recipe)
        .map_err(|err| StorageError::Serialise(err.to_string()))?;
    fs::write(&path, yaml)?;
    Ok(())
}

/// Mutable reference to the host-OS step list inside `os_steps`.
/// Returns `None` when the host-OS variant isn't populated — the
/// caller maps that to `NoStepsForHostOs`.
fn host_os_steps_mut(os_steps: &mut crate::recipes::OsSteps) -> Option<&mut Vec<crate::recipes::Step>> {
    #[cfg(target_os = "windows")]
    {
        os_steps.windows.as_mut()
    }
    #[cfg(target_os = "macos")]
    {
        os_steps.macos.as_mut()
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        os_steps.linux.as_mut()
    }
}

/// Set the `comment:` field on a `Step` regardless of variant. Every
/// variant carries an optional `comment` field with identical
/// semantics; this function centralises the per-variant pattern match
/// so the storage layer doesn't have to know each variant's shape.
fn set_step_comment(step: &mut crate::recipes::Step, value: Option<String>) {
    use crate::recipes::Step::*;
    match step {
        KeyChord { comment, .. }
        | TypeUnicode { comment, .. }
        | ClickLabel { comment, .. }
        | FocusWindow { comment, .. }
        | WaitForWindow { comment, .. }
        | WaitMs { comment, .. }
        | WaitForFocusChange { comment, .. }
        | ScreenshotToClipboard { comment, .. }
        | ClipboardSet { comment, .. }
        | ClipboardGetInto { comment, .. }
        | RunShell { comment, .. }
        | OpenUrl { comment, .. }
        | OpenApp { comment, .. } => *comment = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::recipe_tools::{BUNDLED_RECIPES_ENV, USER_RECIPES_ENV};
    use std::sync::Mutex;

    /// The recipe-dir env vars are process-wide globals; the storage
    /// tests must not run in parallel or they'd race each other's
    /// fixtures. The `mcp::server` integration tests in
    /// `tests/lashon_mcp_stdio.rs` use the same env vars but run in a
    /// separate process — they don't share this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn setup_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        let bundled = tempfile::tempdir().expect("bundled tempdir");
        let user = tempfile::tempdir().expect("user tempdir");
        std::env::set_var(BUNDLED_RECIPES_ENV, bundled.path());
        std::env::set_var(USER_RECIPES_ENV, user.path());
        (bundled, user)
    }

    fn write_recipe(parent: &Path, dir_name: &str, id: &str, body: &str) {
        let dir = parent.join(dir_name);
        fs::create_dir_all(&dir).expect("recipe dir");
        let yaml = body.replace("{id}", id);
        fs::write(dir.join("recipe.yaml"), yaml).expect("write recipe.yaml");
    }

    const MINIMAL_YAML: &str = r#"
version: 1
id: {id}
name: Minimal recipe
description: Smallest legal recipe for tests.
permissions:
  - keyboard.type
os_steps:
  windows:
    - type: wait_ms
      ms: 0
  macos:
    - type: wait_ms
      ms: 0
  linux:
    - type: wait_ms
      ms: 0
"#;

    #[test]
    fn collect_lists_bundled_and_user_with_user_winning() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, user) = setup_dirs();
        write_recipe(bundled.path(), "shared_dir", "shared-id", MINIMAL_YAML);
        write_recipe(bundled.path(), "only_bundled", "only-bundled", MINIMAL_YAML);
        write_recipe(user.path(), "shared", "shared-id", MINIMAL_YAML);
        write_recipe(user.path(), "only-user", "only-user", MINIMAL_YAML);

        let rows = collect_hub_listings();
        assert_eq!(rows.len(), 3, "shared-id deduped to user row only");
        let shared = rows.iter().find(|r| r.id == "shared-id").unwrap();
        assert_eq!(shared.source, "user", "user dir wins precedence");
        assert!(rows
            .iter()
            .any(|r| r.id == "only-bundled" && r.source == "bundled"));
        assert!(rows
            .iter()
            .any(|r| r.id == "only-user" && r.source == "user"));
    }

    #[test]
    fn collect_includes_parse_error_rows() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_bundled, user) = setup_dirs();
        write_recipe(
            user.path(),
            "broken",
            "x",
            "not: valid: yaml: at all: oops:",
        );

        let rows = collect_hub_listings();
        let broken = rows.iter().find(|r| r.parse_error.is_some());
        assert!(broken.is_some(), "broken file surfaces as an error row");
        let broken = broken.unwrap();
        assert_eq!(
            broken.id, "broken",
            "id synthesised from dir name on parse failure"
        );
    }

    #[test]
    fn find_recipe_matches_by_id_not_dir_name() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, _user) = setup_dirs();
        // Dir uses underscores, id uses hyphens — the actual layout
        // in recipes/starters/. find_recipe_by_id must walk-and-match
        // on the parsed id, not the dir name.
        write_recipe(
            bundled.path(),
            "lock_workstation",
            "lock-workstation",
            MINIMAL_YAML,
        );
        let (path, source) = find_recipe_by_id("lock-workstation").unwrap();
        assert_eq!(source, "bundled");
        assert!(path.ends_with("lock_workstation/recipe.yaml"));
    }

    #[test]
    fn duplicate_appends_custom_then_numbered() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, user) = setup_dirs();
        write_recipe(
            bundled.path(),
            "lock_workstation",
            "lock-workstation",
            MINIMAL_YAML,
        );

        let new_id = duplicate_to_user("lock-workstation").unwrap();
        assert_eq!(new_id, "lock-workstation-custom");
        assert!(user
            .path()
            .join("lock-workstation-custom/recipe.yaml")
            .is_file());

        let new_id_2 = duplicate_to_user("lock-workstation").unwrap();
        assert_eq!(new_id_2, "lock-workstation-custom-2");

        let new_id_3 = duplicate_to_user("lock-workstation").unwrap();
        assert_eq!(new_id_3, "lock-workstation-custom-3");
    }

    #[test]
    fn duplicate_writes_new_id_into_yaml() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, user) = setup_dirs();
        write_recipe(
            bundled.path(),
            "lock_workstation",
            "lock-workstation",
            MINIMAL_YAML,
        );

        let new_id = duplicate_to_user("lock-workstation").unwrap();
        let body = fs::read_to_string(user.path().join(&new_id).join("recipe.yaml")).unwrap();
        let parsed: Recipe = serde_yaml_ng::from_str(&body).unwrap();
        assert_eq!(parsed.id, new_id);
    }

    #[test]
    fn delete_bundled_refused() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, _user) = setup_dirs();
        write_recipe(
            bundled.path(),
            "lock_workstation",
            "lock-workstation",
            MINIMAL_YAML,
        );

        let err = delete_user_recipe("lock-workstation").unwrap_err();
        assert!(matches!(err, StorageError::BundledNotWritable { .. }));
    }

    #[test]
    fn delete_user_removes_the_directory() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_bundled, user) = setup_dirs();
        write_recipe(
            user.path(),
            "lock-workstation-custom",
            "lock-workstation-custom",
            MINIMAL_YAML,
        );
        assert!(user.path().join("lock-workstation-custom").is_dir());

        delete_user_recipe("lock-workstation-custom").unwrap();
        assert!(!user.path().join("lock-workstation-custom").exists());
    }

    #[test]
    fn update_comment_sets_and_clears_through_validate_roundtrip() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_bundled, user) = setup_dirs();
        write_recipe(user.path(), "with-comment", "with-comment", MINIMAL_YAML);

        // Set a comment.
        update_recipe_comment(
            "with-comment",
            0,
            Some("Slack cold-launch needs this".to_string()),
        )
        .expect("set comment succeeds");
        let body = fs::read_to_string(user.path().join("with-comment").join("recipe.yaml")).unwrap();
        assert!(
            body.contains("Slack cold-launch needs this"),
            "comment must appear in the YAML: {body}"
        );

        // Empty / whitespace-only normalises to no comment.
        update_recipe_comment("with-comment", 0, Some("   ".to_string()))
            .expect("empty-string clears");
        let body = fs::read_to_string(user.path().join("with-comment").join("recipe.yaml")).unwrap();
        assert!(
            !body.contains("Slack cold-launch needs this"),
            "comment must be removed: {body}"
        );
    }

    #[test]
    fn update_comment_refuses_bundled_recipe() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (bundled, _user) = setup_dirs();
        write_recipe(bundled.path(), "untouchable", "untouchable", MINIMAL_YAML);

        let err = update_recipe_comment("untouchable", 0, Some("nope".to_string())).unwrap_err();
        assert!(matches!(err, StorageError::BundledNotWritable { .. }));
    }

    #[test]
    fn update_comment_rejects_out_of_range_step() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let (_bundled, user) = setup_dirs();
        write_recipe(user.path(), "tiny", "tiny", MINIMAL_YAML);

        // Fixture has 1 step (the `wait_ms`). Index 5 is past the end.
        let err = update_recipe_comment("tiny", 5, Some("ghost".to_string())).unwrap_err();
        match err {
            StorageError::StepNotFound { index, len } => {
                assert_eq!(index, 5);
                assert_eq!(len, 1);
            }
            other => panic!("expected StepNotFound, got {other:?}"),
        }
    }
}
