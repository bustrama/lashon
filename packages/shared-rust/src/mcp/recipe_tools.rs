//! Helpers shared by the recipe-management MCP tools in
//! [`crate::mcp::server`]. The tool functions themselves live on the
//! `OttidMcpServer` impl block decorated with `#[tool_router]` —
//! `rmcp`'s macro merges across all `#[tool]` methods in that block,
//! so this file is helpers only.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::recipes::Recipe;

/// Env var that points the recipe code at the bundled starters. A packaged
/// Ottid sets it for itself at start-up, from the resource dir Tauri
/// resolves (`configure_recipes_env` in the Tauri shell), so the runtime
/// never depends on a path baked in at compile time. A developer, or an MCP
/// host's config, may set it to use another starters tree. `ottid-mcp` runs
/// as the host's child process, not Ottid's, so an installed one needs no
/// env var: it finds the starters from its own location
/// ([`bundled_recipes_dir`]).
pub const BUNDLED_RECIPES_ENV: &str = "OTTID_BUNDLED_RECIPES_DIR";

/// Where the installer puts the bundled starters, relative to the resource
/// dir (the folder holding `ottid.exe` on Windows). `tauri.conf.json` lists
/// them as `../../../recipes/starters/**/*`, and Tauri stores each `..` of a
/// resource path as `_up_`. `tests/bundle_layout.rs` keeps this in step with
/// that entry.
pub const BUNDLED_RECIPES_RESOURCE_DIR: &str = "_up_/_up_/_up_/recipes/starters";

/// Where the installer puts `ottid-mcp`, relative to the resource dir:
/// `binaries/ottid-mcp/ottid-mcp[.exe]`, beside the `binaries/ottid-stt`
/// sidecar (ADR-0049). Matches the `binaries/ottid-mcp/**/*` resource.
pub const MCP_RESOURCE_DIR: &str = "binaries/ottid-mcp";

/// Env var to override the per-user recipes dir. Set by integration
/// tests; in production the binary uses
/// [`user_data_local_dir`]-derived defaults.
pub const USER_RECIPES_ENV: &str = "OTTID_USER_RECIPES_DIR";

/// The starters the installer put under `resource_dir`, or `None` when this
/// build shipped none: a free edition, or a run from a checkout.
pub fn starters_in_resource_dir(resource_dir: &Path) -> Option<PathBuf> {
    // Joined part by part so the path reads with the OS's own separators
    // (`list_recipes` shows it to the agent host).
    let dir = BUNDLED_RECIPES_RESOURCE_DIR
        .split('/')
        .fold(resource_dir.to_path_buf(), |dir, part| dir.join(part));
    dir.is_dir().then_some(dir)
}

/// The resource dir an installed `ottid-mcp` sits in, read from the binary's
/// own path: `<resource dir>/binaries/ottid-mcp/ottid-mcp[.exe]`. `None` for
/// a binary anywhere else, such as a `cargo build` in `target/`. Windows
/// paths are case-insensitive, so a host config that spells the folders
/// differently still matches.
fn mcp_resource_dir(exe: &Path) -> Option<&Path> {
    let mut dir = exe.parent()?;
    for want in Path::new(MCP_RESOURCE_DIR).components().rev() {
        if !dir.file_name()?.eq_ignore_ascii_case(want.as_os_str()) {
            return None;
        }
        dir = dir.parent()?;
    }
    Some(dir)
}

/// Where to find the bundled starter recipes. Resolution order:
/// 1. `$OTTID_BUNDLED_RECIPES_DIR`;
/// 2. the starters installed beside this binary, when it is the installed
///    `ottid-mcp` (see [`mcp_resource_dir`]);
/// 3. the checkout's `recipes/starters`, found through
///    `CARGO_MANIFEST_DIR`. That path is baked in at compile time, so it
///    serves `cargo run` and the tests, never a shipped build.
pub fn bundled_recipes_dir() -> PathBuf {
    resolve_bundled_recipes_dir(
        std::env::var_os(BUNDLED_RECIPES_ENV),
        std::env::current_exe().ok().as_deref(),
    )
}

/// [`bundled_recipes_dir`] with the env value and the running binary's path
/// passed in, so it can be tested.
fn resolve_bundled_recipes_dir(env: Option<OsString>, exe: Option<&Path>) -> PathBuf {
    if let Some(path) = env {
        return PathBuf::from(path);
    }
    if let Some(dir) = exe
        .and_then(mcp_resource_dir)
        .and_then(starters_in_resource_dir)
    {
        return dir;
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
    // No uninstall deletes this dir, even when a per-user install shares the
    // folder (`%LOCALAPPDATA%\Ottid` is `ottid` to case-insensitive Windows;
    // the installer's default is `%LOCALAPPDATA%\Programs\Ottid`). Tauri's
    // NSIS uninstaller deletes only the files it installed, by name, and
    // removes folders only when empty. Its one recursive delete is the
    // opt-in "delete app data" box: it targets the `app.ottid.desktop` dirs,
    // is never shown under `/S` or `/P`, and is ignored under `/UPDATE`. So
    // the silent Lashon uninstall in `windows/hooks.nsh` leaves
    // `lashon/recipes` for the move below.
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

    /// An installed resource dir: `ottid-mcp` under `binaries/` and the
    /// starters under `_up_/`. Returns the resource dir.
    fn install_layout() -> tempfile::TempDir {
        let res = tempfile::tempdir().unwrap();
        fs::create_dir_all(res.path().join(BUNDLED_RECIPES_RESOURCE_DIR)).unwrap();
        fs::create_dir_all(res.path().join(MCP_RESOURCE_DIR)).unwrap();
        res
    }

    fn dev_fallback() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/starters")
    }

    #[test]
    fn an_installed_mcp_finds_the_starters_beside_it() {
        let res = install_layout();
        let exe = res.path().join(MCP_RESOURCE_DIR).join("ottid-mcp.exe");
        assert_eq!(
            resolve_bundled_recipes_dir(None, Some(&exe)),
            res.path().join(BUNDLED_RECIPES_RESOURCE_DIR)
        );
    }

    #[test]
    fn the_installed_folders_may_be_spelled_in_any_case() {
        let res = install_layout();
        let exe = res.path().join("Binaries/OTTID-MCP/ottid-mcp.exe");
        assert_eq!(
            resolve_bundled_recipes_dir(None, Some(&exe)),
            res.path().join(BUNDLED_RECIPES_RESOURCE_DIR)
        );
    }

    #[test]
    fn the_env_var_beats_the_installed_starters() {
        let res = install_layout();
        let exe = res.path().join(MCP_RESOURCE_DIR).join("ottid-mcp.exe");
        let custom = OsString::from("/somewhere/else");
        assert_eq!(
            resolve_bundled_recipes_dir(Some(custom), Some(&exe)),
            PathBuf::from("/somewhere/else")
        );
    }

    #[test]
    fn a_binary_outside_the_install_layout_uses_the_checkout() {
        // A cargo build in `target/` with a starters dir two levels up must
        // not be mistaken for an installed one.
        let res = install_layout();
        let exe = res.path().join("target/debug/ottid-mcp.exe");
        assert_eq!(
            resolve_bundled_recipes_dir(None, Some(&exe)),
            dev_fallback()
        );
        assert_eq!(resolve_bundled_recipes_dir(None, None), dev_fallback());
    }

    #[test]
    fn an_install_without_starters_falls_back_rather_than_pointing_at_nothing() {
        let res = tempfile::tempdir().unwrap();
        fs::create_dir_all(res.path().join(MCP_RESOURCE_DIR)).unwrap();
        let exe = res.path().join(MCP_RESOURCE_DIR).join("ottid-mcp.exe");
        assert_eq!(
            resolve_bundled_recipes_dir(None, Some(&exe)),
            dev_fallback()
        );
    }

    #[test]
    fn starters_in_a_resource_dir_are_found_only_when_shipped() {
        let res = install_layout();
        assert_eq!(
            starters_in_resource_dir(res.path()),
            Some(res.path().join(BUNDLED_RECIPES_RESOURCE_DIR))
        );
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(starters_in_resource_dir(bare.path()), None);
    }
}
