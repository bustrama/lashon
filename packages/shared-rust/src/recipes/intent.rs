//! M9 Phase 1c — intent cascade.
//!
//! Pre-LLM matcher that routes natural-language commands to a recipe
//! when one fits. The dispatcher calls [`CascadeMatcher::match_intent`]
//! before invoking the full LLM planner; on a hit, the runtime
//! (`crate::recipes::runtime`) executes the recipe deterministically
//! in 0–1 turns. On a miss, the dispatcher falls through to the
//! existing planner unchanged.
//!
//! ## Tiers (Phase 1c v1)
//!
//! - **Tier 1 — Regex** (`RegexMatcher`). Each `Recipe::intents`
//!   phrase becomes an anchored, case-insensitive regex with `{slot}`
//!   tokens translated to non-greedy named captures. ~10 µs per
//!   recipe. Catches the high-confidence "user said exactly the
//!   declared phrase" path.
//! - **Tier 2 — Embedding match.** *Deferred.* Needs
//!   multilingual-E5-small (~120 MB Tauri resource) + an inference
//!   path. Will land alongside the model bundling decision (open
//!   question 3 in `docs/stories/m9-recipes.md`).
//! - **Tier 3 — LLM classifier.** *Deferred.* Reuses the local Qwen
//!   to pick the matching recipe and extract slot values in one
//!   structured-JSON response. Lands after Phase 1c v1 ships so we
//!   can measure tier 1 hit rate on real transcripts and size the
//!   prompt-engineering cost accordingly.
//! - **Tier 4 — LLM full planner.** The existing
//!   `command_mode::dispatch` path. The cascade returning `None`
//!   means "didn't match — let the planner handle it."

use std::collections::HashMap;

use regex_lite::Regex;

use super::schema::Recipe;

/// A successful intent match. The dispatcher hands [`recipe_id`] +
/// [`args`] to [`super::runtime::execute_recipe`]; [`tier`] is a
/// tracing aid so we can measure cascade hit-rates per tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedIntent {
    pub recipe_id: String,
    pub args: HashMap<String, String>,
    pub tier: MatchTier,
}

/// Which cascade tier produced a match. Used in tracing and
/// (eventually) the Hub Recipes tab's "last match" diagnostic pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchTier {
    Regex,
    // LlmClassifier — added in the v1.1 follow-up.
}

impl MatchTier {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchTier::Regex => "regex",
        }
    }
}

/// Cascade-shaped matcher trait. v1 carries only [`RegexMatcher`];
/// the LLM classifier (v1.1) and embedding tier will implement the
/// same trait, and [`CascadeMatcher`] will run them in order.
pub trait IntentMatcher: Send + Sync {
    fn match_intent(&self, transcript: &str, recipes: &[Recipe]) -> Option<MatchedIntent>;
}

/// Runs a sequence of [`IntentMatcher`]s in priority order, returning
/// the first match (or `None`). Cheap to construct; the matchers
/// themselves carry any state.
pub struct CascadeMatcher {
    tiers: Vec<Box<dyn IntentMatcher>>,
}

impl CascadeMatcher {
    pub fn new(tiers: Vec<Box<dyn IntentMatcher>>) -> Self {
        Self { tiers }
    }

    /// Phase 1c v1 default — regex tier only. The signature already
    /// accepts an expansion when tier 3 lands without breaking
    /// callers.
    pub fn default_phase_1c_v1() -> Self {
        Self::new(vec![Box::new(RegexMatcher::new())])
    }

    pub fn match_intent(&self, transcript: &str, recipes: &[Recipe]) -> Option<MatchedIntent> {
        for tier in &self.tiers {
            if let Some(matched) = tier.match_intent(transcript, recipes) {
                tracing::info!(
                    target: "lashon::recipes::intent",
                    recipe = %matched.recipe_id,
                    tier = matched.tier.as_str(),
                    "intent cascade match"
                );
                return Some(matched);
            }
        }
        None
    }
}

impl Default for CascadeMatcher {
    fn default() -> Self {
        Self::default_phase_1c_v1()
    }
}

/// Tier 1 matcher: anchor each recipe.intents phrase as a regex with
/// `{slot}` tokens replaced by non-greedy named captures, walk every
/// recipe's phrases against the transcript, return the first hit.
///
/// Stateless — the regex compilation is per-call. Future optimisation
/// could cache a compiled regex set per recipe set; v1 keeps the
/// pipeline simple because regex compilation is sub-millisecond at
/// this scale (each recipe has ~3 short patterns).
pub struct RegexMatcher;

impl RegexMatcher {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RegexMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentMatcher for RegexMatcher {
    fn match_intent(&self, transcript: &str, recipes: &[Recipe]) -> Option<MatchedIntent> {
        let normalised = normalise_transcript(transcript);
        for recipe in recipes {
            for phrase in &recipe.intents {
                if let Some(args) = try_match_phrase(&normalised, phrase) {
                    return Some(MatchedIntent {
                        recipe_id: recipe.id.clone(),
                        args,
                        tier: MatchTier::Regex,
                    });
                }
            }
        }
        None
    }
}

/// Light transcript cleanup before matching: collapse runs of
/// whitespace into single spaces, trim outer whitespace + trailing
/// punctuation. STT outputs sometimes include a final period or
/// double-space; the user shouldn't have to think about those.
fn normalise_transcript(transcript: &str) -> String {
    let trimmed = transcript.trim().trim_end_matches(['.', '!', '?']);
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Convert an intent phrase like `"send {body} to {recipient} in
/// discord"` into a compiled anchored case-insensitive regex with
/// named captures, then try to match `text`. On success return the
/// named captures as a slot map.
///
/// `regex_lite`'s syntax is the standard Rust regex flavour minus the
/// heavy Unicode tables, which is plenty for slot extraction. The
/// non-greedy `.+?` per slot prevents one slot from swallowing the
/// remainder of the line.
fn try_match_phrase(text: &str, phrase: &str) -> Option<HashMap<String, String>> {
    let (pattern, slot_names) = phrase_to_regex(phrase)?;
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(text)?;
    let mut out = HashMap::new();
    for name in slot_names {
        let value = captures.name(&name)?.as_str().trim().to_string();
        if value.is_empty() {
            return None;
        }
        out.insert(name, value);
    }
    Some(out)
}

/// Translate `"send {body} to {recipient} in discord"` into a regex
/// `^(?i)\s*send\s+(?P<body>.+?)\s+to\s+(?P<recipient>.+?)\s+in\s+discord\s*$`
/// plus the ordered list of slot names. Returns `None` if the same
/// slot name appears twice (a regex with duplicate named captures
/// fails to compile and is almost certainly a recipe authoring
/// mistake).
fn phrase_to_regex(phrase: &str) -> Option<(String, Vec<String>)> {
    let mut pattern = String::from("(?i)^\\s*");
    let mut slots: Vec<String> = Vec::new();
    let mut seen_slots: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut chars = phrase.chars().peekable();
    let mut literal_buf = String::new();

    while let Some(c) = chars.next() {
        if c == '{' {
            // Flush any accumulated literal text first.
            if !literal_buf.is_empty() {
                pattern.push_str(&compile_literal(&literal_buf));
                literal_buf.clear();
            }
            let mut name = String::new();
            while let Some(&next) = chars.peek() {
                if next == '}' {
                    chars.next();
                    break;
                }
                chars.next();
                name.push(next);
            }
            let name = name.trim().to_string();
            if name.is_empty() {
                return None;
            }
            if !seen_slots.insert(name.clone()) {
                return None;
            }
            pattern.push_str(&format!("(?P<{name}>.+?)"));
            slots.push(name);
        } else {
            literal_buf.push(c);
        }
    }
    if !literal_buf.is_empty() {
        pattern.push_str(&compile_literal(&literal_buf));
    }
    pattern.push_str("\\s*$");
    Some((pattern, slots))
}

/// Escape regex meta-characters in a literal phrase chunk and treat
/// any internal whitespace as `\s+` so the matcher tolerates extra
/// spaces / single-vs-multi-space the STT might emit.
fn compile_literal(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len() + 8);
    let mut prev_was_space = false;
    for ch in literal.chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                out.push_str("\\s+");
                prev_was_space = true;
            }
        } else {
            prev_was_space = false;
            if matches!(
                ch,
                '.' | '(' | ')' | '[' | ']' | '+' | '?' | '*' | '|' | '\\' | '^' | '$'
            ) {
                out.push('\\');
            }
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::{OsSteps, Parameter, ParameterRequirement, ParameterType, Recipe, Step};

    fn recipe_with_intents(id: &str, intents: Vec<&str>) -> Recipe {
        Recipe {
            version: 1,
            id: id.to_string(),
            name: id.to_string(),
            description: format!("Fixture for {id}"),
            long_description: None,
            author: None,
            recipe_version: "1.0.0".into(),
            tags: vec![],
            intents: intents.into_iter().map(String::from).collect(),
            parameters: vec![
                Parameter {
                    key: "recipient".into(),
                    input_type: ParameterType::String,
                    requirement: ParameterRequirement::Required,
                    description: "Recipient".into(),
                    default: None,
                },
                Parameter {
                    key: "body".into(),
                    input_type: ParameterType::String,
                    requirement: ParameterRequirement::Required,
                    description: "Body".into(),
                    default: None,
                },
            ],
            permissions: vec![],
            os_steps: OsSteps {
                windows: Some(vec![Step::WaitMs {
                    ms: 0,
                    comment: None,
                }]),
                macos: None,
                linux: None,
            },
        }
    }

    #[test]
    fn regex_matches_exact_intent_with_two_slots() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents(
            "send-discord-message",
            vec!["send {body} to {recipient} in discord"],
        )];
        let m = matcher
            .match_intent("send hello world to kuki in discord", &recipes)
            .expect("two-slot intent should match");
        assert_eq!(m.recipe_id, "send-discord-message");
        assert_eq!(m.tier, MatchTier::Regex);
        assert_eq!(m.args.get("body").map(String::as_str), Some("hello world"));
        assert_eq!(m.args.get("recipient").map(String::as_str), Some("kuki"));
    }

    #[test]
    fn regex_is_case_insensitive() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents(
            "send-discord-message",
            vec!["send {body} to {recipient} in discord"],
        )];
        assert!(matcher
            .match_intent("SEND HI to KUKI in Discord", &recipes)
            .is_some());
    }

    #[test]
    fn regex_strips_trailing_punctuation_and_whitespace() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents(
            "send-discord-message",
            vec!["send {body} to {recipient} in discord"],
        )];
        assert!(matcher
            .match_intent("  send hi to kuki in discord.  ", &recipes)
            .is_some());
    }

    #[test]
    fn regex_returns_none_when_no_recipe_matches() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents(
            "send-discord-message",
            vec!["send {body} to {recipient} in discord"],
        )];
        assert!(matcher
            .match_intent("open visual studio code", &recipes)
            .is_none());
    }

    #[test]
    fn regex_first_recipe_in_iteration_order_wins() {
        let matcher = RegexMatcher::new();
        let recipes = vec![
            recipe_with_intents("first", vec!["do {x}"]),
            recipe_with_intents("second", vec!["do {x}"]),
        ];
        let m = matcher
            .match_intent("do thing", &recipes)
            .expect("ambiguous match still returns one");
        assert_eq!(m.recipe_id, "first");
    }

    #[test]
    fn regex_rejects_empty_slot_values() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents("send", vec!["send {body}"])];
        // The non-greedy `.+?` requires at least one non-whitespace
        // char; this transcript provides only the literal prefix.
        assert!(matcher.match_intent("send   ", &recipes).is_none());
    }

    #[test]
    fn cascade_default_uses_regex_tier_only() {
        let cascade = CascadeMatcher::default_phase_1c_v1();
        let recipes = vec![recipe_with_intents(
            "lock-workstation",
            vec!["lock the screen", "lock my computer"],
        )];
        let m = cascade
            .match_intent("lock the screen", &recipes)
            .expect("cascade should match through the regex tier");
        assert_eq!(m.tier, MatchTier::Regex);
        assert_eq!(m.recipe_id, "lock-workstation");
        assert!(m.args.is_empty(), "this fixture has no slots");
    }

    #[test]
    fn phrase_to_regex_rejects_duplicate_slot_names() {
        assert!(phrase_to_regex("set {x} to {x}").is_none());
    }

    #[test]
    fn phrase_to_regex_handles_no_slots() {
        let (pattern, slots) = phrase_to_regex("lock the screen").unwrap();
        assert!(slots.is_empty());
        let regex = Regex::new(&pattern).unwrap();
        assert!(regex.is_match("lock the screen"));
        assert!(regex.is_match("Lock The Screen"));
    }

    #[test]
    fn compile_literal_escapes_regex_metacharacters() {
        let compiled = compile_literal("a.b+c");
        assert_eq!(compiled, "a\\.b\\+c");
    }

    #[test]
    fn compile_literal_collapses_runs_of_whitespace() {
        let compiled = compile_literal("hi   there");
        assert_eq!(compiled, "hi\\s+there");
    }

    #[test]
    fn hebrew_intent_phrase_matches() {
        let matcher = RegexMatcher::new();
        let recipes = vec![recipe_with_intents(
            "send-discord-message",
            vec!["שלח לדיסקורד ל{recipient} {body}"],
        )];
        let m = matcher
            .match_intent("שלח לדיסקורד לקוקי היי", &recipes)
            .expect("Hebrew intent should match");
        assert_eq!(m.args.get("recipient").map(String::as_str), Some("קוקי"));
        assert_eq!(m.args.get("body").map(String::as_str), Some("היי"));
    }
}
