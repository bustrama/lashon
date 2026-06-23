//! Snapshot test — keeps the committed JSON Schema export in sync with
//! the Rust types. Reads `recipes/schema/lashon-recipe.schema.json`,
//! re-derives it from `Recipe`, and asserts equality. On drift the
//! test fails with the instruction to regenerate:
//!
//! ```text
//! cargo test -p lashon-core --test recipe_schema_snapshot -- --ignored regenerate
//! ```
//!
//! The committed file is the canonical contract for external
//! consumers — the Hub creator UI, Lashon-as-MCP-server clients, and
//! third-party recipe authoring tools all read it without having to
//! depend on the Rust crate.

use std::fs;
use std::path::{Path, PathBuf};

use lashon_core::recipes::Recipe;

/// `<repo>/recipes/schema/lashon-recipe.schema.json`.
fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/schema/lashon-recipe.schema.json")
}

/// Generate the schema and serialise it to a canonical pretty form so
/// the committed file stays diff-friendly. The pretty width matches
/// the project's prevailing `rustfmt` line width (100) by virtue of
/// `serde_json::to_string_pretty`'s default 2-space indent.
fn generated_schema_json() -> String {
    let schema = schemars::schema_for!(Recipe);
    let value = serde_json::to_value(&schema).expect("schema must serialise");
    serde_json::to_string_pretty(&value).expect("pretty-print schema") + "\n"
}

#[test]
fn committed_schema_matches_rust_types() {
    let path = schema_path();
    let committed = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{} not found: {err} — run `cargo test -p lashon-core --test \
             recipe_schema_snapshot -- --ignored regenerate` to seed it",
            path.display()
        )
    });
    let generated = generated_schema_json();

    if committed != generated {
        // serde_json::Value comparison gives a clearer signal than raw
        // string diff when the only change is field ordering.
        let committed_value: serde_json::Value =
            serde_json::from_str(&committed).expect("committed schema is valid JSON");
        let generated_value: serde_json::Value =
            serde_json::from_str(&generated).expect("generated schema is valid JSON");
        assert_eq!(
            committed_value, generated_value,
            "JSON Schema drift — regenerate with `cargo test -p lashon-core \
             --test recipe_schema_snapshot -- --ignored regenerate`"
        );
        // Identical content, different formatting — still surface as a
        // failure so the on-disk pretty-print stays deterministic.
        panic!(
            "JSON Schema content matches but formatting differs — \
             regenerate with `cargo test -p lashon-core --test \
             recipe_schema_snapshot -- --ignored regenerate`"
        );
    }
}

#[test]
#[ignore = "writes a file; run explicitly to regenerate the committed schema"]
fn regenerate() {
    let path = schema_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create schema dir");
    }
    fs::write(&path, generated_schema_json()).expect("write schema file");
    eprintln!("wrote {}", path.display());
}
