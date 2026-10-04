//! The one creature validator (docs/adr/0041). Hot reload, Studio saves,
//! import and agent writes all go through [`validate_creature`]; nothing
//! else turns bytes into a [`Creature`].
//!
//! `serde_json` with `deny_unknown_fields` rejects the structural mistakes
//! (unknown or missing fields, wrong types, names outside the fixed sets).
//! This module adds what the types cannot say:
//!
//! - the file size cap and the schema version
//! - every number inside its [`Bounds`]
//! - a kebab-case id that can name a folder on every OS, and display names
//!   that are plain text
//! - a palm wider than its arm
//! - the lamp inside the body, at least as large as the body is tall, and
//!   never under an eye, wherever the engine moves it
//! - the eyes inside the body
//! - gestures that need a prop only in the state that has it
//!
//! Every issue is collected, not only the first, so an author sees them all
//! at once. Each one reads in Hebrew and in English.

use std::fmt;

use icu_properties::props::{
    DefaultIgnorableCodePoint, Emoji, EmojiModifier, ExtendedPictographic, GeneralCategory,
};
use icu_properties::{CodePointMapData, CodePointSetData};

use super::schema::{
    Bounds, Creature, Eyes, Gesture, Lamp, ARM_RADIUS, ARM_SOFTNESS, BODY_RADIUS_X, BODY_RADIUS_Y,
    BODY_WOBBLE, EYE_GAZE_BEND, EYE_RADIUS_X, EYE_RADIUS_Y, EYE_SCALE_MAX, EYE_SHIFT_X,
    EYE_SHIFT_Y, EYE_X, EYE_Y, ID_CHARS, LAMP_RADIUS, LAMP_X, LAMP_Y, MAX_FILE_BYTES, NAME_CHARS,
    PALM_RADIUS, PALM_SOFTNESS, SCHEMA_VERSION,
};

/// The lamp's centre must sit inside this fraction of the body ellipse, so
/// it stays clear of the outline and of the taskbar floor.
const LAMP_INNER: f64 = 0.4;
/// The eyes must sit inside this fraction of the body ellipse.
const EYES_INNER: f64 = 0.85;

/// Why a name is not plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    TooLong,
    /// A character that doesn't show as itself: a control or format
    /// character, a line or paragraph separator, or one Unicode makes
    /// invisible by default (a zero-width space, a bidi embedding, override
    /// or isolate, a tag character, a filler). Emoji sequences keep their
    /// joiners and variation selectors.
    HiddenCharacter,
}

/// One specific complaint about a creature file.
#[derive(Debug, Clone, PartialEq)]
pub enum CreatureIssue {
    TooLarge {
        bytes: usize,
    },
    /// Not JSON, or not shaped like a creature (serde's message).
    Malformed {
        detail: String,
    },
    UnsupportedSchema {
        found: u32,
    },
    InvalidId {
        id: String,
    },
    /// The id is a device name Windows reserves, and the id is also the
    /// creature's folder name.
    ReservedId {
        id: String,
    },
    InvalidName {
        lang: &'static str,
        problem: NameProblem,
    },
    OutOfRange {
        field: &'static str,
        value: f64,
        bounds: Bounds,
    },
    PalmNotWiderThanArm,
    LampOutsideBody,
    LampSmallerThanBody,
    LampUnderEye,
    EyesOutsideBody,
    GestureNeedsProp {
        state: &'static str,
        gesture: &'static str,
    },
}

impl CreatureIssue {
    pub fn message_en(&self) -> String {
        use CreatureIssue::*;
        match self {
            TooLarge { bytes } => {
                format!("the file is {bytes} bytes; a creature file is at most {MAX_FILE_BYTES}")
            }
            Malformed { detail } => format!("not a valid creature file: {detail}"),
            UnsupportedSchema { found } => format!(
                "schema version {found} is not supported; this version of Ottid reads \
                 {SCHEMA_VERSION}"
            ),
            InvalidId { id } => format!(
                "id {id:?} must be 1–{} characters of a–z, 0–9 and '-', starting with a letter",
                ID_CHARS.max
            ),
            ReservedId { id } => format!(
                "id {id:?} is a device name Windows reserves (con, prn, aux, nul, com0–com9, \
                 lpt0–lpt9), and the id is also the creature's folder name; pick another"
            ),
            InvalidName { lang, problem } => match problem {
                NameProblem::Empty => format!("name.{lang} is empty"),
                NameProblem::TooLong => {
                    format!("name.{lang} is longer than {} characters", NAME_CHARS.max)
                }
                NameProblem::HiddenCharacter => format!(
                    "name.{lang} contains an invisible or control character (such as a \
                     zero-width space, a line break or a bidi override); use plain text"
                ),
            },
            OutOfRange {
                field,
                value,
                bounds,
            } => format!(
                "{field} is {value}; it must be between {} and {}",
                bounds.min, bounds.max
            ),
            PalmNotWiderThanArm => {
                "hands.palm_radius must be at least 1 larger than hands.arm_radius".to_string()
            }
            LampOutsideBody => "the lamp must sit near the middle of the body".to_string(),
            LampSmallerThanBody => {
                "lamp.radius must be at least body.radius_y, so the lamp fills the body".to_string()
            }
            LampUnderEye => {
                "the lamp's centre must not be under an eye, wherever the eyes look".to_string()
            }
            EyesOutsideBody => "the eyes must sit inside the body".to_string(),
            GestureNeedsProp { state, gesture } => format!(
                "poses.{state} cannot be {gesture:?}: that gesture needs the notepad, which only \
                 dictation has"
            ),
        }
    }

    pub fn message_he(&self) -> String {
        use CreatureIssue::*;
        match self {
            TooLarge { bytes } => {
                format!("גודל הקובץ {bytes} בתים. קובץ יצור מוגבל ל-{MAX_FILE_BYTES} בתים")
            }
            Malformed { detail } => format!("זה לא קובץ יצור תקין: {detail}"),
            UnsupportedSchema { found } => format!(
                "גרסת הסכמה {found} אינה נתמכת. הגרסה הזו של אוטיד קוראת את גרסה {SCHEMA_VERSION}"
            ),
            InvalidId { id } => format!(
                "המזהה {id:?} צריך להכיל 1–{} תווים מתוך a–z, ‏0–9 ו-'-', ולהתחיל באות",
                ID_CHARS.max
            ),
            ReservedId { id } => format!(
                "המזהה {id:?} הוא שם התקן ש-Windows שומרת לעצמה (con, prn, aux, nul, com0–com9, \
                 lpt0–lpt9), והמזהה הוא גם שם התיקייה של היצור. בחרו מזהה אחר"
            ),
            InvalidName { lang, problem } => match problem {
                NameProblem::Empty => format!("השם name.{lang} ריק"),
                NameProblem::TooLong => {
                    format!("השם name.{lang} ארוך מ-{} תווים", NAME_CHARS.max)
                }
                NameProblem::HiddenCharacter => format!(
                    "השם name.{lang} מכיל תו בלתי נראה או תו בקרה (כמו רווח ברוחב אפס, ירידת \
                     שורה או תו כיווניות). כתבו טקסט רגיל"
                ),
            },
            OutOfRange {
                field,
                value,
                bounds,
            } => format!(
                "הערך של {field} הוא {value}. הוא צריך להיות בין {} ל-{}",
                bounds.min, bounds.max
            ),
            PalmNotWiderThanArm => {
                "hands.palm_radius צריך להיות גדול לפחות ב-1 מ-hands.arm_radius".to_string()
            }
            LampOutsideBody => "המנורה צריכה לשבת קרוב למרכז הגוף".to_string(),
            LampSmallerThanBody => {
                "lamp.radius צריך להיות לפחות body.radius_y, כדי שהמנורה תמלא את הגוף".to_string()
            }
            LampUnderEye => "מרכז המנורה לא יכול להיות מתחת לעין, לא משנה לאן העיניים מסתכלות".to_string(),
            EyesOutsideBody => "העיניים צריכות להיות בתוך הגוף".to_string(),
            GestureNeedsProp { state, gesture } => format!(
                "poses.{state} לא יכול להיות {gesture:?}: המחווה הזו צריכה את הפנקס, ורק להכתבה יש פנקס"
            ),
        }
    }
}

impl fmt::Display for CreatureIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message_en())
    }
}

/// A rejected creature file, with every issue found.
#[derive(Debug, Clone, PartialEq)]
pub struct CreatureError {
    pub issues: Vec<CreatureIssue>,
}

impl CreatureError {
    fn from_issues(issues: Vec<CreatureIssue>) -> Self {
        Self { issues }
    }
}

impl fmt::Display for CreatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let messages: Vec<String> = self.issues.iter().map(CreatureIssue::message_en).collect();
        write!(f, "invalid creature: {}", messages.join("; "))
    }
}

impl std::error::Error for CreatureError {}

/// Parse and check a creature file. The only way in.
pub fn validate_creature(raw: &str) -> Result<Creature, CreatureError> {
    if raw.len() > MAX_FILE_BYTES {
        return Err(CreatureError::from_issues(vec![CreatureIssue::TooLarge {
            bytes: raw.len(),
        }]));
    }
    let creature: Creature = serde_json::from_str(raw).map_err(|err| {
        CreatureError::from_issues(vec![CreatureIssue::Malformed {
            detail: err.to_string(),
        }])
    })?;
    let issues = check(&creature);
    if issues.is_empty() {
        Ok(creature)
    } else {
        Err(CreatureError::from_issues(issues))
    }
}

/// The semantic checks on an already-parsed creature.
fn check(c: &Creature) -> Vec<CreatureIssue> {
    let mut issues = Vec::new();

    if c.schema != SCHEMA_VERSION {
        issues.push(CreatureIssue::UnsupportedSchema { found: c.schema });
    }
    if !is_valid_id(&c.id) {
        issues.push(CreatureIssue::InvalidId { id: c.id.clone() });
    } else if is_reserved_on_windows(&c.id) {
        issues.push(CreatureIssue::ReservedId { id: c.id.clone() });
    }
    for (lang, name) in [("he", &c.name.he), ("en", &c.name.en)] {
        if let Some(problem) = name_problem(name) {
            issues.push(CreatureIssue::InvalidName { lang, problem });
        }
    }

    let ranged = [
        ("body.radius_x", c.body.radius_x, BODY_RADIUS_X),
        ("body.radius_y", c.body.radius_y, BODY_RADIUS_Y),
        ("body.wobble", c.body.wobble, BODY_WOBBLE),
        ("hands.arm_radius", c.hands.arm_radius, ARM_RADIUS),
        ("hands.palm_radius", c.hands.palm_radius, PALM_RADIUS),
        ("hands.arm_softness", c.hands.arm_softness, ARM_SOFTNESS),
        ("hands.palm_softness", c.hands.palm_softness, PALM_SOFTNESS),
        ("eyes.radius_x", c.eyes.radius_x, EYE_RADIUS_X),
        ("eyes.radius_y", c.eyes.radius_y, EYE_RADIUS_Y),
        ("eyes.x", c.eyes.x, EYE_X),
        ("eyes.y", c.eyes.y, EYE_Y),
        ("lamp.x", c.lamp.x, LAMP_X),
        ("lamp.y", c.lamp.y, LAMP_Y),
        ("lamp.radius", c.lamp.radius, LAMP_RADIUS),
    ];
    let mut in_range = true;
    for (field, value, bounds) in ranged {
        if !bounds.contains(value) {
            in_range = false;
            issues.push(CreatureIssue::OutOfRange {
                field,
                value,
                bounds,
            });
        }
    }

    // The cross-field checks assume sane numbers; with a value out of range
    // they would only repeat the same complaint in other words.
    if in_range {
        let (rx, ry) = (c.body.radius_x, c.body.radius_y);
        if c.hands.palm_radius < c.hands.arm_radius + 1.0 {
            issues.push(CreatureIssue::PalmNotWiderThanArm);
        }
        if ellipse_norm(c.lamp.x, c.lamp.y, rx, ry) > LAMP_INNER {
            issues.push(CreatureIssue::LampOutsideBody);
        }
        if c.lamp.radius < ry {
            issues.push(CreatureIssue::LampSmallerThanBody);
        }
        if lamp_under_an_eye(&c.lamp, &c.eyes) {
            issues.push(CreatureIssue::LampUnderEye);
        }
        let outer_x = c.eyes.x + c.eyes.radius_x;
        let eye_top = c.eyes.y - c.eyes.radius_y;
        let eye_bottom = c.eyes.y + c.eyes.radius_y;
        let eyes_inside = ellipse_norm(outer_x, c.eyes.y, rx, ry) <= EYES_INNER
            && ellipse_norm(c.eyes.x, eye_top, rx, ry) <= EYES_INNER
            && ellipse_norm(c.eyes.x, eye_bottom, rx, ry) <= EYES_INNER;
        if !eyes_inside {
            issues.push(CreatureIssue::EyesOutsideBody);
        }
    }

    for (state, gesture) in c.poses.entries() {
        if gesture == Gesture::Write && state != "dictation" {
            issues.push(CreatureIssue::GestureNeedsProp {
                state,
                gesture: gesture.name(),
            });
        }
    }

    issues
}

/// `[a-z][a-z0-9-]*`, within [`ID_CHARS`].
fn is_valid_id(id: &str) -> bool {
    let len = id.chars().count() as f64;
    let mut chars = id.chars();
    ID_CHARS.contains(len)
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A device name Windows reserves: no file or folder can take it, in any
/// case or with any extension. A valid id is lowercase letters, digits and
/// hyphens, so only the bare names can occur. Windows also reserves `COM`
/// and `LPT` with a superscript digit, which an id can't hold.
fn is_reserved_on_windows(id: &str) -> bool {
    matches!(id, "con" | "prn" | "aux" | "nul")
        || matches!(id.as_bytes(), [b'c', b'o', b'm', d] | [b'l', b'p', b't', d] if d.is_ascii_digit())
}

fn name_problem(name: &str) -> Option<NameProblem> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some(NameProblem::Empty);
    }
    if name.chars().count() as f64 > NAME_CHARS.max {
        return Some(NameProblem::TooLong);
    }
    if has_hidden_character(name) {
        return Some(NameProblem::HiddenCharacter);
    }
    None
}

/// Whether a name holds a character that doesn't show as itself.
///
/// - Control and format characters (general categories Cc and Cf), and the
///   line and paragraph separators (Zl, Zp), which break a name across
///   lines. Format characters include the zero-width spaces and joiners,
///   the bidi embeddings, overrides and isolates that make a name display as
///   something else, and the interlinear annotation characters.
/// - Unicode's Default_Ignorable_Code_Point characters, which render as
///   nothing: besides most format characters, the variation selectors, the
///   combining grapheme joiner, tag characters and fillers.
/// - U+2800 BRAILLE PATTERN BLANK, a symbol whose glyph is empty: it passes
///   for a space it isn't. It is the one such character no Unicode property
///   marks.
///
/// Allowed among these:
/// - the left-to-right and right-to-left marks (U+200E, U+200F): they are
///   how a Hebrew name with an English word in it reads right;
/// - emoji as an emoji picker types them (Unicode TS #51): a text or emoji
///   variation selector (U+FE0E, U+FE0F) right after an emoji character, as
///   in ❤️, and a zero-width joiner between two emoji, as in 👩‍💻 or 🏳️‍🌈.
///   Tag characters stay out, even in the flag sequences that use them:
///   they can carry text no one sees.
fn has_hidden_character(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    (0..chars.len()).any(|i| {
        let before = i.checked_sub(1).map(|j| chars[j]);
        is_hidden(chars[i], before, chars.get(i + 1).copied())
    })
}

/// [`has_hidden_character`] for one character, between `before` and `after`.
fn is_hidden(c: char, before: Option<char>, after: Option<char>) -> bool {
    const MARKS: [char; 2] = ['\u{200E}', '\u{200F}'];
    const BRAILLE_BLANK: char = '\u{2800}';
    let unseen = matches!(
        CodePointMapData::<GeneralCategory>::new().get(c),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::LineSeparator
            | GeneralCategory::ParagraphSeparator
    ) || CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(c)
        || c == BRAILLE_BLANK;
    if !unseen || MARKS.contains(&c) {
        return false;
    }
    let emoji = |c: char| CodePointSetData::new::<Emoji>().contains(c);
    let pictographic = |c: char| CodePointSetData::new::<ExtendedPictographic>().contains(c);
    match c {
        // A variation selector shows its emoji as text or as an emoji.
        '\u{FE0E}' | '\u{FE0F}' => !before.is_some_and(emoji),
        // A joiner makes one emoji of two: after an emoji (with its skin
        // tone or presentation selector) and before a pictograph.
        '\u{200D}' => {
            let joins_from = before.is_some_and(|b| {
                pictographic(b)
                    || CodePointSetData::new::<EmojiModifier>().contains(b)
                    || b == '\u{FE0F}'
            });
            !(joins_from && after.is_some_and(pictographic))
        }
        _ => true,
    }
}

/// Whether an eye can cover the lamp's centre anywhere the engine moves it:
/// scaled up to [`EYE_SCALE_MAX`] and shifted toward the gaze by up to
/// [`EYE_SHIFT_X`] and [`EYE_SHIFT_Y`] of its radii, at full bend.
///
/// The eye's centre ranges over a rectangle around where the creature puts
/// it, so the closest it gets to the lamp is that place moved as far toward
/// the lamp as the rectangle allows. The eye narrows a little as it looks
/// aside; the full-size eye at every shift is a slightly larger, safe bound.
fn lamp_under_an_eye(lamp: &Lamp, eyes: &Eyes) -> bool {
    let bend = EYE_GAZE_BEND.sin();
    let reach_x = EYE_SHIFT_X * bend * eyes.radius_x;
    let reach_y = EYE_SHIFT_Y * bend * eyes.radius_y;
    let (rx, ry) = (eyes.radius_x * EYE_SCALE_MAX, eyes.radius_y * EYE_SCALE_MAX);
    let closest = |d: f64, reach: f64| (d.abs() - reach).max(0.0);
    [-eyes.x, eyes.x].iter().any(|&ex| {
        let dx = closest(lamp.x - ex, reach_x);
        let dy = closest(lamp.y - eyes.y, reach_y);
        ellipse_norm(dx, dy, rx, ry) <= 1.0
    })
}

/// The point's "radius" in an ellipse: 1 on the outline, 0 at the centre.
fn ellipse_norm(x: f64, y: f64, rx: f64, ry: f64) -> f64 {
    ((x / rx).powi(2) + (y / ry).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::DEFAULT_CREATURE_JSON;

    fn default_value() -> serde_json::Value {
        serde_json::from_str(DEFAULT_CREATURE_JSON).unwrap()
    }

    fn validate_value(value: &serde_json::Value) -> Result<Creature, CreatureError> {
        validate_creature(&serde_json::to_string(value).unwrap())
    }

    fn issues_of(value: &serde_json::Value) -> Vec<CreatureIssue> {
        validate_value(value)
            .expect_err("should be rejected")
            .issues
    }

    #[test]
    fn default_creature_is_valid() {
        let creature = validate_creature(DEFAULT_CREATURE_JSON).expect("default creature");
        assert_eq!(creature.id, "ottid");
        assert_eq!(creature.name.he, "אוטיד");
        assert_eq!(creature.poses.dictation, Gesture::Write);
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut v = default_value();
        v["script"] = serde_json::json!("alert(1)");
        assert!(matches!(
            issues_of(&v).as_slice(),
            [CreatureIssue::Malformed { .. }]
        ));

        let mut v = default_value();
        v["lamp"]["colour"] = serde_json::json!("#000000");
        assert!(matches!(
            issues_of(&v).as_slice(),
            [CreatureIssue::Malformed { .. }]
        ));
    }

    #[test]
    fn rejects_colour_values_and_unknown_gestures() {
        let mut v = default_value();
        v["body"]["colour"] = serde_json::json!("#14181c");
        assert!(matches!(
            issues_of(&v).as_slice(),
            [CreatureIssue::Malformed { .. }]
        ));

        let mut v = default_value();
        v["poses"]["idle"] = serde_json::json!("moonwalk");
        assert!(matches!(
            issues_of(&v).as_slice(),
            [CreatureIssue::Malformed { .. }]
        ));
    }

    #[test]
    fn rejects_missing_states() {
        let mut v = default_value();
        v["poses"].as_object_mut().unwrap().remove("agent-done");
        assert!(matches!(
            issues_of(&v).as_slice(),
            [CreatureIssue::Malformed { .. }]
        ));
    }

    #[test]
    fn rejects_files_over_the_size_cap() {
        let padded = format!("{DEFAULT_CREATURE_JSON}{}", " ".repeat(MAX_FILE_BYTES));
        assert!(matches!(
            validate_creature(&padded).unwrap_err().issues.as_slice(),
            [CreatureIssue::TooLarge { .. }]
        ));
    }

    #[test]
    fn rejects_other_schema_versions() {
        let mut v = default_value();
        v["schema"] = serde_json::json!(2);
        assert_eq!(
            issues_of(&v),
            vec![CreatureIssue::UnsupportedSchema { found: 2 }]
        );
    }

    #[test]
    fn rejects_bad_ids() {
        let long = "a".repeat(41);
        for id in ["", "Ottid", "9lives", "my creature", "a/b", long.as_str()] {
            let mut v = default_value();
            v["id"] = serde_json::json!(id);
            assert_eq!(
                issues_of(&v),
                vec![CreatureIssue::InvalidId { id: id.to_string() }],
                "id {id:?}"
            );
        }
        let mut v = default_value();
        v["id"] = serde_json::json!("my-creature-2");
        assert!(validate_value(&v).is_ok());
    }

    #[test]
    fn rejects_ids_windows_reserves_for_devices() {
        let digits = ('0'..='9').flat_map(|d| [format!("com{d}"), format!("lpt{d}")]);
        let reserved: Vec<String> = ["con", "prn", "aux", "nul"]
            .map(String::from)
            .into_iter()
            .chain(digits)
            .collect();
        assert_eq!(reserved.len(), 24);
        for id in &reserved {
            let mut v = default_value();
            v["id"] = serde_json::json!(id);
            assert_eq!(
                issues_of(&v),
                vec![CreatureIssue::ReservedId { id: id.clone() }],
                "id {id:?}"
            );
        }
        // Only the bare names are reserved.
        for id in ["console", "com", "com10", "lpt-1", "nul-2"] {
            let mut v = default_value();
            v["id"] = serde_json::json!(id);
            assert!(validate_value(&v).is_ok(), "id {id:?}");
        }
    }

    #[test]
    fn names_are_plain_text_in_both_languages() {
        let mut v = default_value();
        v["name"]["he"] = serde_json::json!("   ");
        v["name"]["en"] = serde_json::json!("Evil\u{202E}god");
        assert_eq!(
            issues_of(&v),
            vec![
                CreatureIssue::InvalidName {
                    lang: "he",
                    problem: NameProblem::Empty
                },
                CreatureIssue::InvalidName {
                    lang: "en",
                    problem: NameProblem::HiddenCharacter
                },
            ]
        );

        // Hebrew with an English word and a right-to-left mark is fine.
        let mut v = default_value();
        v["name"]["he"] = serde_json::json!("בלובי \u{200F}Blob");
        assert!(validate_value(&v).is_ok());
    }

    #[test]
    fn names_hold_no_invisible_characters() {
        let hidden = [
            ("zero-width space", "אוטי\u{200B}ד"),
            ("zero-width joiner", "Ot\u{200D}tid"),
            ("word joiner", "Ot\u{2060}tid"),
            ("byte order mark", "\u{FEFF}Ottid"),
            ("soft hyphen", "Ot\u{00AD}tid"),
            ("line separator", "Ot\u{2028}tid"),
            ("paragraph separator", "Ottid\u{2029}"),
            ("arabic letter mark", "אוטיד\u{061C}"),
            ("isolate", "\u{2067}Ottid\u{2069}"),
            ("tag characters", "Ottid\u{E0041}\u{E007F}"),
            ("variation selector", "Ottid\u{FE0F}"),
            ("hangul filler", "\u{3164}"),
            ("only invisible", "\u{200B}\u{200B}"),
            ("control", "Ot\u{0007}tid"),
            ("interlinear annotation anchor", "Ot\u{FFF9}tid"),
            ("interlinear annotation separator", "Ot\u{FFFA}tid"),
            ("interlinear annotation terminator", "Ottid\u{FFFB}"),
            ("braille pattern blank", "Ot\u{2800}tid"),
            ("only braille pattern blanks", "\u{2800}\u{2800}"),
            ("joiner from an emoji to a letter", "🦦\u{200D}Ottid"),
            ("joiner from a letter to an emoji", "Ottid\u{200D}🦦"),
            ("joiner after the last emoji", "Ottid 👩\u{200D}"),
            ("emoji selector on a letter", "Ot\u{FE0F}tid"),
            ("text selector on a letter", "אוטיד\u{FE0E}"),
            (
                "tag characters in a flag",
                "🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
            ),
        ];
        for (what, name) in hidden {
            let mut v = default_value();
            v["name"]["en"] = serde_json::json!(name);
            assert_eq!(
                issues_of(&v),
                vec![CreatureIssue::InvalidName {
                    lang: "en",
                    problem: NameProblem::HiddenCharacter
                }],
                "{what}"
            );
        }
    }

    #[test]
    fn names_keep_what_hebrew_and_english_text_needs() {
        let plain = [
            "אוֹטִיד",                // niqqud: combining marks that show
            "\u{200E}Ottid אוטיד",  // a left-to-right mark
            "בלובי \u{200F}Blob",   // a right-to-left mark
            "Ottid 2 — the Otter!", // punctuation and spaces
            "Ottid 🦦",             // an emoji on its own
        ];
        for name in plain {
            let mut v = default_value();
            v["name"]["he"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn names_keep_emoji_as_the_emoji_panel_types_them() {
        let emoji = [
            ("a heart", "Ottid \u{2764}\u{FE0F}"),
            ("a sun", "\u{2600}\u{FE0F} שמשי"),
            ("a smile as text", "\u{263A}\u{FE0E}"),
            ("a keycap", "1\u{FE0F}\u{20E3}"),
            ("a coder", "\u{1F469}\u{200D}\u{1F4BB} Coder"),
            (
                "a coder with a skin tone",
                "\u{1F469}\u{1F3FD}\u{200D}\u{1F4BB}",
            ),
            ("a family", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
            ("a rainbow flag", "\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}"),
            ("a polar bear", "\u{1F43B}\u{200D}\u{2744}\u{FE0F}"),
            ("a flag of letters", "\u{1F1EE}\u{1F1F1}"),
        ];
        for (what, name) in emoji {
            let mut v = default_value();
            v["name"]["en"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "{what}: {name:?}");
        }
    }

    #[test]
    fn names_keep_every_hebrew_letter_point_and_accent() {
        // Every letter, and every combining mark of the Hebrew block on a
        // letter: the niqqud (vowel points, dagesh, shin and sin dots, rafe,
        // meteg) and the cantillation accents.
        let gc = CodePointMapData::<GeneralCategory>::new();
        let hebrew = '\u{0591}'..='\u{05F4}';
        let letters: String = hebrew
            .clone()
            .filter(|&c| gc.get(c) == GeneralCategory::OtherLetter)
            .collect();
        // 27 letters, the yod triangle and 3 Yiddish ligatures.
        assert_eq!(letters.chars().count(), 31, "{letters}");
        let marks: Vec<char> = hebrew
            .filter(|&c| gc.get(c) == GeneralCategory::NonspacingMark)
            .collect();
        assert!(marks.contains(&'\u{05B4}') && marks.contains(&'\u{0596}'));
        for chunk in letters.chars().collect::<Vec<_>>().chunks(16) {
            let name: String = chunk.iter().collect();
            let mut v = default_value();
            v["name"]["he"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "{name:?}");
        }
        for mark in marks {
            let name = format!("\u{05D0}{mark}\u{05D1}");
            let mut v = default_value();
            v["name"]["he"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "U+{:04X}", mark as u32);
        }
        // A word with points, an accent and a maqaf, as written.
        let mut v = default_value();
        v["name"]["he"] = serde_json::json!(
            "\u{05D1}\u{05BC}\u{05B0}\u{05E8}\u{05B5}\u{05D0}\u{05E9}\u{05C1}\u{05B4}\u{0596}\u{05D9}\u{05EA}\u{05BE}\u{05D0}"
        );
        assert!(validate_value(&v).is_ok());
    }

    #[test]
    fn names_keep_every_hebrew_punctuation_mark() {
        // The maqaf, paseq, sof pasuq, nun hafukha, geresh and gershayim.
        let gc = CodePointMapData::<GeneralCategory>::new();
        let marks: Vec<char> = ('\u{0591}'..='\u{05F4}')
            .filter(|&c| {
                matches!(
                    gc.get(c),
                    GeneralCategory::DashPunctuation | GeneralCategory::OtherPunctuation
                )
            })
            .collect();
        assert_eq!(
            marks,
            ['\u{05BE}', '\u{05C0}', '\u{05C3}', '\u{05C6}', '\u{05F3}', '\u{05F4}']
        );
        for mark in marks {
            let name = format!("\u{05D0}{mark}\u{05D1}");
            let mut v = default_value();
            v["name"]["he"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "U+{:04X}", mark as u32);
        }
        // As written: a geresh in a borrowed sound, gershayim in an acronym.
        for name in ["צ\u{05F3}יפס", "צה\u{05F4}ל"] {
            let mut v = default_value();
            v["name"]["he"] = serde_json::json!(name);
            assert!(validate_value(&v).is_ok(), "{name:?}");
        }
    }

    #[test]
    fn collects_every_out_of_range_number() {
        let mut v = default_value();
        v["body"]["radius_x"] = serde_json::json!(80);
        v["lamp"]["radius"] = serde_json::json!(4);
        let issues = issues_of(&v);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues
            .iter()
            .all(|i| matches!(i, CreatureIssue::OutOfRange { .. })));
    }

    #[test]
    fn the_lamp_cannot_shrink_below_the_body() {
        let mut v = default_value();
        v["body"]["radius_y"] = serde_json::json!(22);
        v["lamp"]["radius"] = serde_json::json!(19);
        assert_eq!(issues_of(&v), vec![CreatureIssue::LampSmallerThanBody]);
    }

    #[test]
    fn the_lamp_stays_near_the_middle_of_the_body() {
        let mut v = default_value();
        v["lamp"]["x"] = serde_json::json!(6);
        v["lamp"]["y"] = serde_json::json!(8);
        assert_eq!(issues_of(&v), vec![CreatureIssue::LampOutsideBody]);
    }

    #[test]
    fn the_lamp_is_never_under_an_eye() {
        let mut v = default_value();
        v["eyes"]["x"] = serde_json::json!(4);
        v["eyes"]["y"] = serde_json::json!(3);
        v["eyes"]["radius_y"] = serde_json::json!(6);
        v["lamp"]["x"] = serde_json::json!(4);
        v["lamp"]["y"] = serde_json::json!(4);
        assert_eq!(issues_of(&v), vec![CreatureIssue::LampUnderEye]);
    }

    #[test]
    fn the_lamp_is_never_under_an_eye_wherever_it_looks() {
        // Clear of the eyes at rest, even startled (the old check passed it),
        // but an eye looking toward the midline slides over the lamp.
        let mut v = default_value();
        v["eyes"]["x"] = serde_json::json!(12);
        v["eyes"]["radius_x"] = serde_json::json!(4);
        v["lamp"]["x"] = serde_json::json!(6);
        v["lamp"]["y"] = serde_json::json!(0);
        let creature: Creature = serde_json::from_value(v.clone()).unwrap();
        let startled_at_rest = ellipse_norm(
            creature.lamp.x - creature.eyes.x,
            creature.lamp.y - creature.eyes.y,
            creature.eyes.radius_x * EYE_SCALE_MAX,
            creature.eyes.radius_y * EYE_SCALE_MAX,
        );
        assert!(startled_at_rest > 1.0, "{startled_at_rest}");
        assert_eq!(issues_of(&v), vec![CreatureIssue::LampUnderEye]);

        // The same lamp is clear once the eyes can't reach it.
        v["lamp"]["x"] = serde_json::json!(0);
        assert!(validate_value(&v).is_ok());
    }

    #[test]
    fn eyes_stay_inside_the_body() {
        let mut v = default_value();
        v["body"]["radius_x"] = serde_json::json!(24);
        v["body"]["radius_y"] = serde_json::json!(16);
        v["eyes"]["x"] = serde_json::json!(12);
        v["eyes"]["y"] = serde_json::json!(-8);
        v["eyes"]["radius_y"] = serde_json::json!(6.5);
        assert_eq!(issues_of(&v), vec![CreatureIssue::EyesOutsideBody]);
    }

    #[test]
    fn palms_are_wider_than_arms() {
        let mut v = default_value();
        v["hands"]["arm_radius"] = serde_json::json!(4);
        v["hands"]["palm_radius"] = serde_json::json!(4.5);
        assert_eq!(issues_of(&v), vec![CreatureIssue::PalmNotWiderThanArm]);
    }

    #[test]
    fn write_is_only_for_dictation() {
        let mut v = default_value();
        v["poses"]["command"] = serde_json::json!("write");
        assert_eq!(
            issues_of(&v),
            vec![CreatureIssue::GestureNeedsProp {
                state: "command",
                gesture: "write"
            }]
        );
        // Dictation may pick another gesture; the notepad stays an engine prop.
        let mut v = default_value();
        v["poses"]["dictation"] = serde_json::json!("ready");
        assert!(validate_value(&v).is_ok());
    }

    #[test]
    fn every_issue_reads_in_both_languages() {
        let issues = [
            CreatureIssue::TooLarge { bytes: 99_999 },
            CreatureIssue::Malformed {
                detail: "expected value at line 1 column 1".into(),
            },
            CreatureIssue::UnsupportedSchema { found: 9 },
            CreatureIssue::InvalidId { id: "X".into() },
            CreatureIssue::ReservedId { id: "con".into() },
            CreatureIssue::InvalidName {
                lang: "he",
                problem: NameProblem::TooLong,
            },
            CreatureIssue::OutOfRange {
                field: "body.wobble",
                value: 1.0,
                bounds: BODY_WOBBLE,
            },
            CreatureIssue::PalmNotWiderThanArm,
            CreatureIssue::LampOutsideBody,
            CreatureIssue::LampSmallerThanBody,
            CreatureIssue::LampUnderEye,
            CreatureIssue::EyesOutsideBody,
            CreatureIssue::GestureNeedsProp {
                state: "idle",
                gesture: "write",
            },
        ];
        let hebrew = |c: char| ('\u{05D0}'..='\u{05EA}').contains(&c);
        for issue in issues {
            assert!(!issue.message_en().is_empty());
            assert!(
                issue.message_he().chars().any(hebrew),
                "{issue:?} has no Hebrew message"
            );
        }
    }
}
