//! Post-STT transcript helpers — runs between the speech-to-text
//! engine and the recipe cascade + LLM planner so every downstream
//! consumer sees the *corrected* text.
//!
//! Today the only helper is [`apply_aliases`], the deterministic
//! word-substitution layer that fixes recurring STT misrecognitions
//! the user shouldn't have to live with:
//!
//! - **"claude" → "cloud"** — Whisper biases toward the common English
//!   noun over the rare proper noun on English-leaning audio.
//! - **Contact-name homonyms** — "Kookie" / "Kuki", "Tom" / "Tum",
//!   any user-specific name that consistently lands wrong.
//! - **Hebrew transliteration drift** — "קלאוד" → "קלוד" for the same
//!   proper noun.
//!
//! The aliases are user-supplied (Hub: "Voice corrections") and
//! persist in `settings.json` as `stt.word_aliases`. The Tauri shell
//! reads them on each take + applies before the cascade / LLM
//! dispatch path runs.
//!
//! ## Substitution semantics
//!
//! - **Case-insensitive matching.** `Cloud`, `CLOUD`, `cloud` all
//!   match an alias declared as `cloud`.
//! - **Word-boundary respect.** Only whole tokens substitute —
//!   `cloudy` stays `cloudy`, never becomes `claudey`.
//! - **Capitalization heuristic on output.** If the input token was
//!   ALL-UPPERCASE → output is uppercased. Capitalised → titlecased.
//!   Otherwise lowercase. Hebrew has no case so this is a no-op for
//!   Hebrew tokens.
//! - **Punctuation passes through.** `Hi, cloud!` becomes
//!   `Hi, claude!` — the leading `Hi, ` and trailing `!` are
//!   preserved verbatim.
//! - **Empty alias map is a free pass.** No allocation, no walk.

use std::collections::HashMap;

/// Replace every alias-keyed token in `text` with its mapped value.
/// See module docs for the substitution semantics. `aliases` keys
/// may be in any case; lookup is case-insensitive. Empty input or
/// empty map both return `text` unchanged (the empty-map path
/// short-circuits without allocation).
pub fn apply_aliases(text: &str, aliases: &HashMap<String, String>) -> String {
    if aliases.is_empty() || text.is_empty() {
        return text.to_string();
    }
    // Lowercase the lookup keys once. The values are kept verbatim;
    // the capitalisation heuristic in `replace_token` decides how to
    // render them per occurrence.
    let lookup: HashMap<String, String> = aliases
        .iter()
        .map(|(k, v)| (k.to_lowercase(), v.clone()))
        .collect();

    let mut out = String::with_capacity(text.len());
    let mut token = String::new();

    let flush = |out: &mut String, token: &mut String, lookup: &HashMap<String, String>| {
        if token.is_empty() {
            return;
        }
        match lookup.get(&token.to_lowercase()) {
            Some(replacement) => out.push_str(&match_case(token, replacement)),
            None => out.push_str(token),
        }
        token.clear();
    };

    for ch in text.chars() {
        // A "word character" is anything alphabetic or numeric in the
        // Unicode sense — covers ASCII letters, Hebrew letters, digits,
        // accented Latin, CJK, etc. Everything else is a separator that
        // breaks the token (whitespace, punctuation, symbols).
        if is_word_char(ch) {
            token.push(ch);
        } else {
            flush(&mut out, &mut token, &lookup);
            out.push(ch);
        }
    }
    flush(&mut out, &mut token, &lookup);
    out
}

/// What counts as a "word character" for token boundaries. Splits on
/// whitespace + punctuation; keeps letters + digits + the underscore
/// (so identifiers like `foo_bar` aren't split). Hebrew Aleph–Tav
/// (U+05D0..U+05EA) qualify via `is_alphabetic`.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Re-render `replacement` with the case pattern of `template`. Three
/// patterns recognised, in order of specificity:
///
/// - ALL-UPPERCASE (`CLOUD`) → uppercase the replacement (`CLAUDE`)
/// - Capitalised (`Cloud`) → titlecase the replacement (`Claude`)
/// - Anything else (including all-lowercase, Hebrew, mixed) → use the
///   replacement verbatim. The author who typed `claude` as the
///   alias value gets exactly `claude`; if they typed `Claude` they
///   get `Claude`. This is the right default for proper nouns where
///   the alias map is the source of truth.
fn match_case(template: &str, replacement: &str) -> String {
    if replacement.is_empty() {
        return String::new();
    }
    // Quick scan: collect the alphabetic chars only — digits / hyphens
    // / underscores don't carry case information.
    let alpha: Vec<char> = template.chars().filter(|c| c.is_alphabetic()).collect();
    if alpha.is_empty() {
        return replacement.to_string();
    }
    let all_upper = alpha.iter().all(|c| c.is_uppercase());
    if all_upper && alpha.len() > 1 {
        // "CLOUD" → uppercase replacement. Single-letter all-upper
        // ("A") is treated as titlecase below — it could be either.
        return replacement.to_uppercase();
    }
    let first = alpha[0];
    let rest_lower = alpha[1..].iter().all(|c| c.is_lowercase());
    if first.is_uppercase() && rest_lower {
        // "Cloud" → titlecase replacement. Walk replacement chars,
        // uppercase the first alphabetic, leave the rest as authored.
        let mut out = String::with_capacity(replacement.len());
        let mut capitalised = false;
        for c in replacement.chars() {
            if !capitalised && c.is_alphabetic() {
                for up in c.to_uppercase() {
                    out.push(up);
                }
                capitalised = true;
            } else {
                out.push(c);
            }
        }
        return out;
    }
    // Otherwise (all-lowercase, mixed, Hebrew with no case) — use the
    // replacement verbatim.
    replacement.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn empty_aliases_returns_text_unchanged() {
        let out = apply_aliases("hello cloud", &HashMap::new());
        assert_eq!(out, "hello cloud");
    }

    #[test]
    fn empty_text_returns_empty() {
        let out = apply_aliases("", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "");
    }

    #[test]
    fn replaces_isolated_token() {
        let out = apply_aliases("ask cloud about it", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "ask claude about it");
    }

    #[test]
    fn does_not_replace_substring() {
        let out = apply_aliases("cloudy with cloudbase", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "cloudy with cloudbase");
    }

    #[test]
    fn case_all_lower_keeps_alias_value_verbatim() {
        let out = apply_aliases("cloud", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "claude");
    }

    #[test]
    fn case_titlecase_input_titlecases_replacement() {
        let out = apply_aliases("Cloud", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "Claude");
    }

    #[test]
    fn case_all_upper_input_uppercases_replacement() {
        let out = apply_aliases("CLOUD", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "CLAUDE");
    }

    #[test]
    fn punctuation_around_token_is_preserved() {
        let out = apply_aliases("Hi, cloud! How are you, cloud?", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "Hi, claude! How are you, claude?");
    }

    #[test]
    fn multiple_aliases_apply_in_one_pass() {
        let map = aliases(&[("cloud", "claude"), ("kookie", "kuki")]);
        let out = apply_aliases("send hi to kookie and cloud", &map);
        assert_eq!(out, "send hi to kuki and claude");
    }

    #[test]
    fn alias_key_is_case_insensitive() {
        // Author wrote the key as `Cloud`; matcher still catches all
        // case variants because lookup lowercases the key on init.
        let map = aliases(&[("Cloud", "claude")]);
        assert_eq!(apply_aliases("cloud", &map), "claude");
        assert_eq!(apply_aliases("CLOUD", &map), "CLAUDE");
        assert_eq!(apply_aliases("Cloud", &map), "Claude");
    }

    #[test]
    fn hebrew_token_replaces_without_case_change() {
        let map = aliases(&[("קלאוד", "קלוד")]);
        let out = apply_aliases("שלח לקלאוד הודעה", &map);
        // Hebrew tokens are split on whitespace (which is the only
        // separator here) — but "לקלאוד" includes a `ל` prefix so it
        // *doesn't* match the bare `קלאוד` alias. This is the
        // documented limitation: users either declare the prefixed
        // form too, or write recipes that match the bare form.
        assert_eq!(out, "שלח לקלאוד הודעה");
    }

    #[test]
    fn hebrew_bare_token_replaces() {
        let map = aliases(&[("קלאוד", "קלוד")]);
        let out = apply_aliases("שלח קלאוד הודעה", &map);
        assert_eq!(out, "שלח קלוד הודעה");
    }

    #[test]
    fn unknown_token_unchanged() {
        let out = apply_aliases("hello world", &aliases(&[("cloud", "claude")]));
        assert_eq!(out, "hello world");
    }

    #[test]
    fn alias_value_with_internal_caps_passes_through_on_lowercase_input() {
        // Author wants the replacement to carry "Claude" (the brand
        // capitalisation) even when the input is lowercase. The
        // verbatim-on-lowercase rule honours that: input "cloud" →
        // output "Claude" because that's what the author authored.
        let map = aliases(&[("cloud", "Claude")]);
        let out = apply_aliases("ask cloud", &map);
        assert_eq!(out, "ask Claude");
    }

    #[test]
    fn token_starts_at_string_start() {
        let map = aliases(&[("cloud", "claude")]);
        assert_eq!(apply_aliases("cloud says hi", &map), "claude says hi");
    }

    #[test]
    fn token_at_string_end() {
        let map = aliases(&[("cloud", "claude")]);
        assert_eq!(apply_aliases("say hi to cloud", &map), "say hi to claude");
    }
}
