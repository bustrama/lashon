//! Helpers shared by the recipe-management MCP tools in
//! [`crate::mcp::server`]. The tool functions themselves live on the
//! `OttidMcpServer` impl block decorated with `#[tool_router]` —
//! `rmcp`'s macro merges across all `#[tool]` methods in that block,
//! so this file is helpers only.

use std::fs;
use std::path::{Path, PathBuf};

use crate::recipes::Recipe;

/// Env var the Tauri shell sets when it spawns `ottid-mcp` so the
/// stdio binary doesn't have to guess where the bundled starters
/// landed on a packaged install (`%PROGRAMFILES%\Ottid\recipes\`
/// on Windows, etc.).
pub const BUNDLED_RECIPES_ENV: &str = "OTTID_BUNDLED_RECIPES_DIR";

/// Env var to override the per-user recipes dir. Set by integration
/// tests; in production the binary uses
/// [`user_data_local_dir`]-derived defaults.
pub const USER_RECIPES_ENV: &str = "OTTID_USER_RECIPES_DIR";

/// Where to find the bundled starter recipes. Resolution order:
/// `$OTTID_BUNDLED_RECIPES_DIR` → cargo-dev fallback
/// (`CARGO_MANIFEST_DIR/../../recipes/starters`).
pub fn bundled_recipes_dir() -> PathBuf {
    if let Some(path) = std::env::var_os(BUNDLED_RECIPES_ENV) {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/starters")
}

/// Where to read + write per-user recipes. Resolution order:
/// `$OTTID_USER_RECIPES_DIR` → `<data_local_dir>/ottid/recipes/`.
///
/// `data_local_dir()` resolves to:
/// - Windows: `%LOCALAPPDATA%\ottid\recipes\`
/// - macOS:   `~/Library/Application Support/ottid/recipes/`
/// - Linux:   `$XDG_DATA_HOME/ottid/recipes/` (or `~/.local/share/...`)
///
/// Recipes written before the rename live in `<data_local_dir>/lashon/`;
/// the first resolve moves them here (docs/adr/0042), when the carry-over
/// is on (docs/adr/0046).
pub fn user_recipes_dir() -> PathBuf {
    if let Some(path) = std::env::var_os(USER_RECIPES_ENV) {
        return PathBuf::from(path);
    }
    recipes_dir_under(&user_data_local_dir(), crate::legacy::carry_over_enabled())
}

/// `<base>/ottid/recipes`, after moving the pre-rename recipes into it
/// when `carry_over` is on.
fn recipes_dir_under(base: &Path, carry_over: bool) -> PathBuf {
    let dir = base.join("ottid").join("recipes");
    if !carry_over {
        return dir;
    }
    let legacy = base.join(crate::legacy::LEGACY_DIR_NAME);
    match crate::legacy::adopt_dir(&legacy.join("recipes"), &dir) {
        Ok(crate::legacy::Adopted::Nothing) => {}
        Ok(adopted) => {
            tracing::info!(?adopted, "recipes: adopted the pre-rename recipes dir");
            crate::legacy::remove_dir_if_empty(&legacy);
        }
        Err(err) => {
            tracing::warn!(error = %err, "recipes: could not adopt the pre-rename recipes dir")
        }
    }
    dir
}

#[cfg(target_os = "windows")]
fn user_data_local_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn user_data_local_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Application Support"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn user_data_local_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// One row of the `list_recipes` response.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecipeListing {
    /// Recipe id (kebab-case, matches `Recipe::id` in the YAML).
    pub id: String,
    /// One-line description for cascade matching / Hub display.
    pub description: String,
    /// `"starter"` (bundled, read-only) or `"user"` (per-user, writable).
    pub source: String,
    /// On-disk path to the `recipe.yaml`. Stable for the process
    /// lifetime; not stable across reinstalls.
    pub path: String,
}

/// Walk the bundled + user directories and collect a listing. Errors
/// reading individual files are demoted to skipped rows with a
/// tracing warning — one bad recipe must not break discovery.
pub fn collect_listings() -> Vec<RecipeListing> {
    let mut out = Vec::new();
    for (dir, source) in [
        (bundled_recipes_dir(), "starter"),
        (user_recipes_dir(), "user"),
    ] {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(err) => {
                tracing::debug!(dir = %dir.display(), %source, "skip listing: {err}");
                continue;
            }
        };
        for entry in entries.flatten() {
            let recipe_yaml = entry.path().join("recipe.yaml");
            if !recipe_yaml.is_file() {
                continue;
            }
            let body = match fs::read_to_string(&recipe_yaml) {
                Ok(b) => b,
                Err(err) => {
                    tracing::warn!(path = %recipe_yaml.display(), "read failed: {err}");
                    continue;
                }
            };
            let recipe: Recipe = match serde_yaml_ng::from_str(&body) {
                Ok(r) => r,
                Err(err) => {
                    tracing::warn!(path = %recipe_yaml.display(), "parse failed: {err}");
                    continue;
                }
            };
            out.push(RecipeListing {
                id: recipe.id,
                description: recipe.description,
                source: source.to_string(),
                path: recipe_yaml.to_string_lossy().into_owned(),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Locate a recipe by id. Per-user wins over bundled when both exist
/// — that's the same precedence the Hub Recipes browser uses (Phase
/// 1d) and the M9 story's open-question 5 default.
pub fn find_recipe_path(id: &str) -> Option<PathBuf> {
    let user = user_recipes_dir().join(id).join("recipe.yaml");
    if user.is_file() {
        return Some(user);
    }
    let bundled = bundled_recipes_dir().join(id).join("recipe.yaml");
    if bundled.is_file() {
        return Some(bundled);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECIPE: &str = "id: send-message\nintents: [\"שלח הודעה\"]\n";

    fn seed_legacy_recipe(base: &Path) -> PathBuf {
        let yaml = base.join("lashon/recipes/send_message/recipe.yaml");
        fs::create_dir_all(yaml.parent().unwrap()).unwrap();
        fs::write(&yaml, RECIPE).unwrap();
        yaml
    }

    #[test]
    fn pre_rename_recipes_stay_put_without_the_carry_over() {
        let base = tempfile::tempdir().unwrap();
        let old = seed_legacy_recipe(base.path());

        let dir = recipes_dir_under(base.path(), false);
        assert_eq!(dir, base.path().join("ottid/recipes"));
        assert_eq!(fs::read_to_string(&old).unwrap(), RECIPE);
        assert!(!dir.exists());
    }

    #[test]
    fn pre_rename_recipes_move_with_the_carry_over() {
        let base = tempfile::tempdir().unwrap();
        seed_legacy_recipe(base.path());

        let dir = recipes_dir_under(base.path(), true);
        assert_eq!(
            fs::read_to_string(dir.join("send_message/recipe.yaml")).unwrap(),
            RECIPE
        );
        // The emptied `lashon/` parent is tidied away too.
        assert!(!base.path().join("lashon").exists());
    }
}
