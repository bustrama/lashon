//! Snapshot test — keeps the committed creature JSON Schema in sync with the
//! Rust types. Reads `creatures/schema/ottid-creature.schema.json`,
//! re-derives it from `Creature`, and asserts equality. On drift the test
//! fails with the instruction to regenerate:
//!
//! ```text
//! cargo test -p ottid-core --test creature_schema_snapshot -- --ignored regenerate
//! ```
//!
//! The committed file is the public contract (docs/adr/0041): agents and the
//! local LLM fill it with schema-constrained decoding, and the frontend's
//! gesture library is checked against its gesture names.

use std::fs;
use std::path::{Path, PathBuf};

use ottid_core::creature::Creature;

/// `<repo>/creatures/schema/ottid-creature.schema.json`.
fn schema_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../creatures/schema/ottid-creature.schema.json")
}

fn generated_schema_json() -> String {
    let schema = schemars::schema_for!(Creature);
    let value = serde_json::to_value(&schema).expect("schema must serialise");
    serde_json::to_string_pretty(&value).expect("pretty-print schema") + "\n"
}

#[test]
fn committed_schema_matches_rust_types() {
    let path = schema_path();
    let committed = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "{} not found: {err} — run `cargo test -p ottid-core --test \
             creature_schema_snapshot -- --ignored regenerate` to seed it",
            path.display()
        )
    });
    let generated = generated_schema_json();
    if committed != generated {
        let committed_value: serde_json::Value =
            serde_json::from_str(&committed).expect("committed schema is valid JSON");
        let generated_value: serde_json::Value =
            serde_json::from_str(&generated).expect("generated schema is valid JSON");
        assert_eq!(
            committed_value, generated_value,
            "JSON Schema drift — regenerate with `cargo test -p ottid-core \
             --test creature_schema_snapshot -- --ignored regenerate`"
        );
        panic!(
            "JSON Schema content matches but formatting differs — \
             regenerate with `cargo test -p ottid-core --test \
             creature_schema_snapshot -- --ignored regenerate`"
        );
    }
}

/// Every bundled creature under `creatures/<id>/creature.json` passes the
/// validator and lives in the directory named after its id.
#[test]
fn bundled_creatures_are_valid() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../creatures");
    let mut seen = 0;
    for entry in fs::read_dir(&root).expect("creatures/ exists") {
        let dir = entry.expect("dir entry").path();
        let file = dir.join("creature.json");
        if !file.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&file).expect("read creature.json");
        let creature = ottid_core::creature::validate_creature(&raw)
            .unwrap_or_else(|err| panic!("{}: {err}", file.display()));
        assert_eq!(
            dir.file_name().and_then(|n| n.to_str()),
            Some(creature.id.as_str()),
            "{} must live in a directory named after its id",
            file.display()
        );
        seen += 1;
    }
    assert!(
        seen >= 1,
        "no bundled creatures found under {}",
        root.display()
    );
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
