//! `lashon-recipe` — CLI driver for the M9 Phase 1b recipe runtime.
//!
//! Locates a recipe by id under either the bundled starters
//! (`recipes/starters/` in dev / `<install>/recipes/` packaged) or the
//! per-user dir (`<data-local>/lashon/recipes/`), parses + validates
//! it, fills slots from `--<key>=<value>` argv flags, and executes via
//! `lashon_core::recipes::execute_recipe`. Run-shell steps are denied
//! unless `--allow-shell` is passed (matches the safe default the MCP
//! `run_recipe` tool will adopt in its follow-up PR).
//!
//! Usage:
//!
//! ```text
//! lashon-recipe send-discord-message --recipient=kuki --body="hi there"
//! lashon-recipe lock-workstation
//! lashon-recipe batch-rename-files --directory=C:\tmp \
//!     --pattern="*.txt" --find=old --replace=new --allow-shell
//! lashon-recipe --list
//! ```
//!
//! Lives next to `lashon-mcp` (the stdio MCP server binary, ADR-0028)
//! under the `mcp-server` feature only because both are
//! optional-by-feature; the recipe runtime itself is feature-free.
//! Reuses the same `LASHON_BUNDLED_RECIPES_DIR` / `LASHON_USER_RECIPES_DIR`
//! env-var overrides ADR-0028 introduced.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

use lashon_core::recipes::{
    execute_recipe, AlwaysAllow, AlwaysDeny, ConfirmDecision, ConfirmHandler, Recipe, RuntimeError,
};

/// `LASHON_BUNDLED_RECIPES_DIR` (with cargo-dev fallback) — mirrors
/// the constant the MCP server uses so the two binaries find the same
/// starters when both are installed alongside the Tauri shell.
fn bundled_recipes_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("LASHON_BUNDLED_RECIPES_DIR") {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/starters")
}

fn user_recipes_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("LASHON_USER_RECIPES_DIR") {
        return PathBuf::from(path);
    }
    user_data_local_dir().join("lashon").join("recipes")
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

/// Resolve a recipe by id. Walks the bundled + user dirs and matches
/// on the recipe's `id:` field rather than the directory name —
/// `recipes/starters/send_discord_message/` ships with
/// `id: send-discord-message`, so naive `dir == id` lookup misses.
/// Same precedence the Hub uses (per-user wins over bundled).
fn find_recipe_path(id: &str) -> Option<PathBuf> {
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
            if recipe.id == id {
                return Some(yaml);
            }
        }
    }
    None
}

fn list_all() -> Result<()> {
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for (dir, source) in [
        (bundled_recipes_dir(), "starter"),
        (user_recipes_dir(), "user"),
    ] {
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
            rows.push((recipe.id, source.to_string(), recipe.description));
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    if rows.is_empty() {
        eprintln!(
            "no recipes found under {} or {}",
            bundled_recipes_dir().display(),
            user_recipes_dir().display()
        );
        return Ok(());
    }
    for (id, source, description) in rows {
        println!("{id}  [{source}]  {description}");
    }
    Ok(())
}

fn print_usage() {
    eprintln!(
        "lashon-recipe — run a Lashon recipe from the CLI

usage:
  lashon-recipe <id> [--key=value ...]
  lashon-recipe --list

flags:
  --list           list installed recipes (bundled + per-user)
  --allow-shell    permit run_shell steps (default: deny — safer)
  --help, -h       this message

env vars:
  LASHON_BUNDLED_RECIPES_DIR   override the bundled starters dir
  LASHON_USER_RECIPES_DIR      override the per-user recipes dir

example:
  lashon-recipe send-discord-message --recipient=kuki --body=\"hi\""
    );
}

/// CLI confirmation handler — `AlwaysAllow` for shell when the user
/// passed `--allow-shell`, `AlwaysDeny` otherwise. (Interactive
/// stdin-prompt confirmation could go here in the future; for the v1
/// CLI a clear opt-in flag is the safer default than nudging the user
/// to mash <enter> per step.)
struct CliConfirm {
    allow_shell: bool,
}

impl ConfirmHandler for CliConfirm {
    fn confirm(&self, prompt: &str) -> ConfirmDecision {
        if self.allow_shell {
            eprintln!("[allow-shell] running: {prompt}");
            ConfirmDecision::Allow
        } else {
            ConfirmDecision::Deny
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        std::process::exit(if args.is_empty() { 2 } else { 0 });
    }
    if args.iter().any(|a| a == "--list") {
        return list_all();
    }

    let mut allow_shell = false;
    let mut id: Option<String> = None;
    let mut slots: HashMap<String, String> = HashMap::new();

    for arg in args {
        if arg == "--allow-shell" {
            allow_shell = true;
        } else if let Some(rest) = arg.strip_prefix("--") {
            // `--key=value` slot. `--key value` (separate args) is not
            // supported — keeps the parser tiny and unambiguous.
            let (key, value) = rest
                .split_once('=')
                .ok_or_else(|| anyhow!("flag {arg:?} needs the --key=value form"))?;
            slots.insert(key.to_string(), value.to_string());
        } else if id.is_none() {
            id = Some(arg);
        } else {
            bail!("unexpected positional arg {arg:?} — only one recipe id is allowed");
        }
    }

    let id = id.ok_or_else(|| anyhow!("missing recipe id; pass it as the first argument"))?;
    let path = find_recipe_path(&id)
        .ok_or_else(|| anyhow!("recipe {id:?} not found in bundled or user dirs"))?;
    let body = fs::read_to_string(&path)?;
    let recipe: Recipe = serde_yaml_ng::from_str(&body)?;

    eprintln!(
        "lashon-recipe: running {id} ({} steps, allow_shell={allow_shell})",
        recipe
            .os_steps
            .windows
            .as_ref()
            .map(|s| s.len())
            .unwrap_or(0)
    );

    let confirm: Box<dyn ConfirmHandler> = if allow_shell {
        Box::new(AlwaysAllow)
    } else {
        Box::new(CliConfirm { allow_shell })
    };
    // The `AlwaysAllow` branch above only kicks in when the user
    // explicitly passed `--allow-shell` — there's no path where a
    // surprise shell command runs without an opt-in.
    let _ = AlwaysDeny; // silence unused-import warning on this branch

    match execute_recipe(&recipe, slots, confirm.as_ref()).await {
        Ok(run) => {
            eprintln!(
                "lashon-recipe: done — {} steps executed",
                run.steps_executed
            );
            Ok(())
        }
        Err(RuntimeError::Denied { kind, .. }) => {
            bail!("{kind} step denied — pass --allow-shell to permit run_shell steps")
        }
        Err(err) => Err(err.into()),
    }
}
