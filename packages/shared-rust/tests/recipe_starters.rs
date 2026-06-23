//! Integration test — round-trips every bundled starter recipe through
//! the parser + validator. New recipes added under `recipes/starters/`
//! are picked up automatically by directory walk; the test fails the
//! moment one of them is structurally invalid or fails a semantic
//! check.
//!
//! Run with:
//!
//! ```text
//! cargo test -p lashon-core --test recipe_starters
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use lashon_core::recipes::{validate_recipe, IntentMatcher, Recipe, RegexMatcher};

/// `<repo>/recipes/starters`. `CARGO_MANIFEST_DIR` is
/// `<repo>/packages/shared-rust`, so the relative path traverses up two
/// levels — same pattern `model::*` uses to find the bundled models
/// tree from a unit test.
fn starters_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recipes/starters")
}

/// Collect every `recipe.yaml` under `recipes/starters/<id>/`.
fn starter_recipe_files() -> Vec<PathBuf> {
    let root = starters_dir();
    let mut out = Vec::new();
    for entry in
        fs::read_dir(&root).unwrap_or_else(|err| panic!("read {}: {err:#}", root.display()))
    {
        let entry = entry.expect("dir entry");
        let recipe = entry.path().join("recipe.yaml");
        if recipe.is_file() {
            out.push(recipe);
        }
    }
    out.sort();
    out
}

#[test]
fn every_starter_parses_and_validates() {
    let files = starter_recipe_files();
    assert!(
        files.len() >= 10,
        "expected at least 10 starter recipes (the M9 Phase 1f library), \
         found {} — check {}",
        files.len(),
        starters_dir().display()
    );

    let mut failures = Vec::new();
    for path in &files {
        let body = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(err) => {
                failures.push(format!("{}: read failed: {err}", path.display()));
                continue;
            }
        };
        let recipe: Recipe = match serde_yaml_ng::from_str(&body) {
            Ok(r) => r,
            Err(err) => {
                failures.push(format!("{}: parse failed: {err}", path.display()));
                continue;
            }
        };
        if let Err(err) = validate_recipe(&recipe) {
            failures.push(format!("{}: validation failed:\n{}", path.display(), err));
        }
    }
    assert!(
        failures.is_empty(),
        "starter recipes failed validation:\n{}",
        failures.join("\n---\n")
    );
}

#[test]
fn every_starter_has_unique_id_matching_directory_name() {
    let files = starter_recipe_files();
    let mut seen_ids = std::collections::HashSet::new();
    for path in files {
        let body = fs::read_to_string(&path).unwrap();
        let recipe: Recipe = serde_yaml_ng::from_str(&body).unwrap();
        assert!(
            seen_ids.insert(recipe.id.clone()),
            "duplicate recipe id {:?} ({})",
            recipe.id,
            path.display()
        );
        // The directory name is canonical for filesystem discovery; the
        // recipe.id is canonical for the cascade and Hub display. They
        // should agree up to kebab-vs-snake-case (directories prefer
        // snake_case so the trees sort sensibly on every OS).
        let dir_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .expect("recipe path has a parent dir name");
        let normalised_id = recipe.id.replace('-', "_");
        assert_eq!(
            normalised_id, dir_name,
            "recipe id {:?} does not match directory {:?}",
            recipe.id, dir_name
        );
    }
}

#[test]
fn every_starter_declares_at_least_one_intent_phrase() {
    let files = starter_recipe_files();
    for path in files {
        let body = fs::read_to_string(&path).unwrap();
        let recipe: Recipe = serde_yaml_ng::from_str(&body).unwrap();
        assert!(
            !recipe.intents.is_empty(),
            "{} declares no intent phrases — the cascade can't route to it",
            path.display()
        );
    }
}

/// Regression coverage for natural-Hebrew phrasings — the bug the user
/// raised was "תשלח הודעה בדיסקורד..." (future-imperative + "בדיסקורד"
/// instead of "לדיסקורד" + an inserted "הודעה") not matching. Phase 1c
/// v1's regex tier is brittle by design, so we lock the *specific
/// phrasings the v1.1 expanded-intent-list catches* against regression.
/// Adding a phrasing to a recipe's intents without exercising it here
/// lets the LLM-classifier-tier-3 follow-up silently delete the intent
/// without surfacing the loss.
#[test]
fn natural_hebrew_phrasings_match_messaging_recipes() {
    // Load every starter, hand them to the cascade matcher, and assert
    // each (utterance → expected recipe id) pair routes correctly.
    let files = starter_recipe_files();
    let recipes: Vec<Recipe> = files
        .iter()
        .map(|p| serde_yaml_ng::from_str(&fs::read_to_string(p).unwrap()).unwrap())
        .collect();
    let matcher = RegexMatcher::new();

    let cases: &[(&str, &str, Option<&str>, Option<&str>)] = &[
        // (utterance, expected recipe id, expected recipient, expected body)
        (
            "תשלח הודעה בדיסקורד לקוקי אם הוא רוצה לדבר היום",
            "send-discord-message",
            Some("קוקי"),
            Some("אם הוא רוצה לדבר היום"),
        ),
        (
            "שלח לדיסקורד לקוקי טסט",
            "send-discord-message",
            Some("קוקי"),
            Some("טסט"),
        ),
        (
            "שלח הודעה בסלאק לאליס תזכורת לפגישה",
            "send-slack-message",
            Some("אליס"),
            Some("תזכורת לפגישה"),
        ),
        (
            "תשלח בטלגרם לאמא אני בדרך",
            "send-telegram-message",
            Some("אמא"),
            Some("אני בדרך"),
        ),
        (
            "תשלח הודעה בוואצאפ לדן בקרוב מגיע",
            "send-whatsapp-message",
            Some("דן"),
            Some("בקרוב מגיע"),
        ),
        // English forms still work
        (
            "send hi to kuki in discord",
            "send-discord-message",
            Some("kuki"),
            Some("hi"),
        ),
        (
            "tell alice on slack let's sync at 3",
            "send-slack-message",
            Some("alice"),
            Some("let's sync at 3"),
        ),
    ];

    for (utterance, expected_id, expected_recipient, expected_body) in cases {
        let m = matcher.match_intent(utterance, &recipes).unwrap_or_else(|| {
            panic!("no recipe matched the utterance {utterance:?}")
        });
        assert_eq!(
            m.recipe_id, *expected_id,
            "wrong recipe for {utterance:?}"
        );
        if let Some(r) = expected_recipient {
            assert_eq!(
                m.args.get("recipient").map(String::as_str),
                Some(*r),
                "wrong recipient for {utterance:?}"
            );
        }
        if let Some(b) = expected_body {
            assert_eq!(
                m.args.get("body").map(String::as_str),
                Some(*b),
                "wrong body for {utterance:?}"
            );
        }
    }
}
