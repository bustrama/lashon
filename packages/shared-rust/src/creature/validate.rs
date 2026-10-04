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
//! - a kebab-case id, and display names that are plain text
//! - a palm wider than its arm
//! - the lamp inside the body, at least as large as the body is tall, and
//!   never under an eye, wherever the engine moves it
//! - the eyes inside the body
//! - gestures that need a prop only in the state that has it
//!
//! Every issue is collected, not only the first, so an author sees them all
//! at once. Each one reads in Hebrew and in English.

use std::fmt;

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
    /// A control character, or a bidi embedding, override or isolate.
    ControlCharacter,
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
            InvalidName { lang, problem } => match problem {
                NameProblem::Empty => format!("name.{lang} is empty"),
                NameProblem::TooLong => {
                    format!("name.{lang} is longer than {} characters", NAME_CHARS.max)
                }
                NameProblem::ControlCharacter => format!(
                    "name.{lang} contains a control character or a bidi override; use plain text"
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
            InvalidName { lang, problem } => match problem {
                NameProblem::Empty => format!("השם name.{lang} ריק"),
                NameProblem::TooLong => {
                    format!("השם name.{lang} ארוך מ-{} תווים", NAME_CHARS.max)
                }
                NameProblem::ControlCharacter => format!(
                    "השם name.{lang} מכיל תו בקרה או תו כיווניות. כתבו טקסט רגיל"
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

fn name_problem(name: &str) -> Option<NameProblem> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Some(NameProblem::Empty);
    }
    if name.chars().count() as f64 > NAME_CHARS.max {
        return Some(NameProblem::TooLong);
    }
    // Explicit embeddings, overrides and isolates can make a name display as
    // something else. The marks (U+200E, U+200F) are harmless and allowed.
    let spoofing = |c: char| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    if name.chars().any(|c| c.is_control() || spoofing(c)) {
        return Some(NameProblem::ControlCharacter);
    }
    None
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
                    problem: NameProblem::ControlCharacter
                },
            ]
        );

        // Hebrew with an English word and a right-to-left mark is fine.
        let mut v = default_value();
        v["name"]["he"] = serde_json::json!("בלובי \u{200F}Blob");
        assert!(validate_value(&v).is_ok());
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
