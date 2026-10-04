//! Keeps the installer's resource lists and the runtime's idea of where
//! those resources land in step (ADR-0049).
//!
//! `ottid-mcp` and the starter recipes ship in the full edition only. The
//! free edition's `tauri.free.conf.json` lists its resources in full, because
//! Tauri replaces an array when it merges configs rather than adding to it.
//! So a resource added to `tauri.conf.json` reaches the free installer
//! unless the free list leaves it out, and nothing else would notice.
//!
//! Run with:
//!
//! ```text
//! cargo test -p ottid-core --test bundle_layout
//! ```

#![cfg(feature = "mcp-server")]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use ottid_core::mcp::recipe_tools::{BUNDLED_RECIPES_RESOURCE_DIR, MCP_RESOURCE_DIR};
use serde_json::Value;

/// The `bundle.resources` entry for the starters, as `tauri.conf.json`
/// spells it: relative to `src-tauri/`, so three levels up to the repo root.
const STARTERS_RESOURCE: &str = "../../../recipes/starters/**/*";

/// The entries only the full edition ships.
const FULL_ONLY: [&str; 3] = [
    "binaries/llama-server/**/*",
    "binaries/ottid-mcp/**/*",
    STARTERS_RESOURCE,
];

fn src_tauri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src-tauri")
}

/// The `bundle.resources` array of a Tauri config file.
fn resources(conf: &str) -> BTreeSet<String> {
    let text = fs::read_to_string(src_tauri().join(conf)).unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    json["bundle"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("{conf} has no bundle.resources array"))
        .iter()
        .map(|entry| entry.as_str().unwrap().to_owned())
        .collect()
}

/// Where Tauri puts the files an array entry matches, relative to the
/// resource dir and without the trailing `/**/*`: each `..` becomes `_up_`
/// (`tauri_utils::resources::resource_relpath`).
fn installed_dir(entry: &str) -> String {
    let dir = entry.strip_suffix("/**/*").expect("a `dir/**/*` glob");
    Path::new(dir)
        .components()
        .map(|c| match c {
            Component::ParentDir => "_up_".to_owned(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[test]
fn the_full_edition_ships_ottid_mcp_and_the_starters_where_the_runtime_looks() {
    let full = resources("tauri.conf.json");
    for entry in FULL_ONLY {
        assert!(full.contains(entry), "tauri.conf.json lacks `{entry}`");
    }
    assert_eq!(
        installed_dir(STARTERS_RESOURCE),
        BUNDLED_RECIPES_RESOURCE_DIR
    );
    assert_eq!(installed_dir("binaries/ottid-mcp/**/*"), MCP_RESOURCE_DIR);
}

#[test]
fn the_free_edition_ships_the_full_list_minus_the_command_mode_resources() {
    let full = resources("tauri.conf.json");
    let free = resources("tauri.free.conf.json");
    let full_only: BTreeSet<String> = FULL_ONLY.iter().map(|s| (*s).to_owned()).collect();
    let dropped: BTreeSet<String> = full.difference(&free).cloned().collect();
    assert_eq!(
        dropped, full_only,
        "the free list must drop exactly the full-only resources"
    );
    let invented: Vec<&String> = free.difference(&full).collect();
    assert!(
        invented.is_empty(),
        "the free list ships {invented:?}, which the full list does not"
    );
}

/// Tauri fails the build of a glob that matches nothing, and CI checks the
/// Tauri crate on a clean checkout, where nothing has staged `ottid-mcp`.
/// A tracked `.gitkeep` keeps `binaries/ottid-mcp/**/*` resolvable.
#[test]
fn the_staging_dir_resolves_on_a_clean_checkout() {
    let keep = src_tauri().join("binaries/ottid-mcp/.gitkeep");
    assert!(keep.is_file(), "{} is missing", keep.display());
}
