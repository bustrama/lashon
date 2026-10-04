//! Creature schema — Rust types that double as the JSON Schema source
//! (docs/adr/0041).
//!
//! A creature file is data only. It picks numbers inside fixed bounds and
//! names from fixed sets; it carries no colour values, no markup and nothing
//! the engine would run. The bounds live here as constants so the JSON
//! Schema export, the validator and the docs read the same numbers.
//!
//! What a creature never controls (ADR-0041): the state machine, event
//! timing, the props, and the lamp's colour and strength. It only places the
//! lamp inside its body, no smaller than [`LAMP_RADIUS`]'s minimum.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Creature-schema version. v1 is the kit tier for one body with two hands
/// (docs/adr/0045); freeform path data and more parts arrive with later
/// versions.
pub const SCHEMA_VERSION: u32 = 1;

/// The largest creature file the validator reads, in bytes.
pub const MAX_FILE_BYTES: usize = 16 * 1024;

/// An inclusive numeric bound, in design units unless noted. One design unit
/// is 1.5 CSS px at the overlay's default scale (docs/design-system.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: f64,
    pub max: f64,
}

impl Bounds {
    const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    pub fn contains(self, value: f64) -> bool {
        value.is_finite() && value >= self.min && value <= self.max
    }
}

// The bounds keep every creature inside the overlay's 220 × 120 CSS px stage
// in all three placements, and its lamp visible.
pub const BODY_RADIUS_X: Bounds = Bounds::new(24.0, 33.0);
pub const BODY_RADIUS_Y: Bounds = Bounds::new(16.0, 23.0);
/// The resting outline wobble, as a fraction of the body's size.
pub const BODY_WOBBLE: Bounds = Bounds::new(0.0, 0.04);
pub const ARM_RADIUS: Bounds = Bounds::new(1.5, 4.0);
pub const PALM_RADIUS: Bounds = Bounds::new(3.0, 6.5);
/// How far an arm's fillet reaches into the body (the smooth-union `k`).
pub const ARM_SOFTNESS: Bounds = Bounds::new(1.0, 8.0);
pub const PALM_SOFTNESS: Bounds = Bounds::new(1.0, 5.0);
pub const EYE_RADIUS_X: Bounds = Bounds::new(2.0, 4.5);
pub const EYE_RADIUS_Y: Bounds = Bounds::new(2.5, 6.5);
pub const EYE_X: Bounds = Bounds::new(4.0, 12.0);
pub const EYE_Y: Bounds = Bounds::new(-8.0, 3.0);
pub const LAMP_X: Bounds = Bounds::new(-6.0, 6.0);
pub const LAMP_Y: Bounds = Bounds::new(-4.0, 8.0);
/// The lamp's glow radius. The minimum is the smallest lamp the engine will
/// draw; a creature cannot shrink it further.
pub const LAMP_RADIUS: Bounds = Bounds::new(18.0, 40.0);
/// Characters in a display name, per language.
pub const NAME_CHARS: Bounds = Bounds::new(1.0, 32.0);
/// Characters in an id.
pub const ID_CHARS: Bounds = Bounds::new(1.0, 40.0);

/// How far the engine moves an eye from where a creature puts it. The lamp
/// must stay clear of an eye wherever it goes, so the validator needs these;
/// the engine (`lib/creature/engine/eyes.ts`) clamps to them, and the schema
/// publishes them on `Eyes` as `x-ottid-eye-motion`, which the frontend's
/// tests compare with the engine's.
///
/// The widest an eye gets (a startle), as a scale of its radii.
pub const EYE_SCALE_MAX: f64 = 1.25;
/// How far an eye shifts toward the gaze, per unit of `radius_x` and of
/// `radius_y`, at full bend.
pub const EYE_SHIFT_X: f64 = 0.72;
pub const EYE_SHIFT_Y: f64 = 0.4;
/// The gaze, clamped to ±1, bends through `sin(gaze × EYE_GAZE_BEND)`.
pub const EYE_GAZE_BEND: f64 = 0.9;

/// A `creature.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Creature {
    /// Creature-schema version. Must be [`SCHEMA_VERSION`].
    #[schemars(range(min = SCHEMA_VERSION, max = SCHEMA_VERSION))]
    pub schema: u32,
    /// Stable kebab-case id, `[a-z][a-z0-9-]*`. Also the directory name under
    /// `creatures/`.
    #[schemars(regex(pattern = r"^[a-z][a-z0-9-]*$"), length(min = 1, max = 40))]
    pub id: String,
    /// The name the user gave the creature, per UI language.
    pub name: CreatureName,
    pub body: Body,
    pub hands: Hands,
    pub eyes: Eyes,
    pub lamp: Lamp,
    /// The gesture the creature makes in each state.
    pub poses: Poses,
}

/// A display name in each UI language. Plain text: no control characters,
/// line breaks or invisible characters (Unicode's default-ignorable ones,
/// such as zero-width spaces, bidi overrides and tag characters). The
/// left-to-right and right-to-left marks are allowed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatureName {
    #[schemars(length(min = 1, max = 32))]
    pub he: String,
    #[schemars(length(min = 1, max = 32))]
    pub en: String,
}

/// The body: an ellipse whose outline wobbles through angular harmonics 2, 3
/// and 5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Body {
    /// Half-width of the ellipse.
    #[schemars(range(min = BODY_RADIUS_X.min, max = BODY_RADIUS_X.max))]
    pub radius_x: f64,
    /// Half-height of the ellipse.
    #[schemars(range(min = BODY_RADIUS_Y.min, max = BODY_RADIUS_Y.max))]
    pub radius_y: f64,
    /// The resting wobble of the outline, as a fraction of the body's size.
    /// The voice level and pokes add to it.
    #[schemars(range(min = BODY_WOBBLE.min, max = BODY_WOBBLE.max))]
    pub wobble: f64,
    pub colour: BodyColour,
}

/// Two dough hands: a thin arm capsule ending in a round palm, fused into the
/// body by smooth union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hands {
    #[schemars(range(min = ARM_RADIUS.min, max = ARM_RADIUS.max))]
    pub arm_radius: f64,
    /// At least one unit wider than the arm, so the hand reads as a hand.
    #[schemars(range(min = PALM_RADIUS.min, max = PALM_RADIUS.max))]
    pub palm_radius: f64,
    #[schemars(range(min = ARM_SOFTNESS.min, max = ARM_SOFTNESS.max))]
    pub arm_softness: f64,
    #[schemars(range(min = PALM_SOFTNESS.min, max = PALM_SOFTNESS.max))]
    pub palm_softness: f64,
}

/// Two eyes, mirrored about the midline. Positions are from the body's
/// centre, y down. The engine scales and shifts them as `x-ottid-eye-motion`
/// says, and the lamp must stay clear of them wherever they go.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-ottid-eye-motion" = {
    "scale_max": EYE_SCALE_MAX,
    "shift_x": EYE_SHIFT_X,
    "shift_y": EYE_SHIFT_Y,
    "gaze_bend": EYE_GAZE_BEND
}))]
pub struct Eyes {
    pub style: EyeStyle,
    pub colour: EyeColour,
    #[schemars(range(min = EYE_RADIUS_X.min, max = EYE_RADIUS_X.max))]
    pub radius_x: f64,
    #[schemars(range(min = EYE_RADIUS_Y.min, max = EYE_RADIUS_Y.max))]
    pub radius_y: f64,
    /// Each eye's distance from the midline.
    #[schemars(range(min = EYE_X.min, max = EYE_X.max))]
    pub x: f64,
    #[schemars(range(min = EYE_Y.min, max = EYE_Y.max))]
    pub y: f64,
}

/// Where the lamp sits inside the body. Its colour and strength are the
/// engine's: they always show the state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lamp {
    #[schemars(range(min = LAMP_X.min, max = LAMP_X.max))]
    pub x: f64,
    #[schemars(range(min = LAMP_Y.min, max = LAMP_Y.max))]
    pub y: f64,
    /// The glow's radius. At least the body's half-height.
    #[schemars(range(min = LAMP_RADIUS.min, max = LAMP_RADIUS.max))]
    pub radius: f64,
}

/// Body colours: names of design tokens, never colour values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BodyColour {
    /// `--creature-charcoal`.
    Charcoal,
}

/// Eye colours: names of design tokens, never colour values. State colours
/// are not offered, so the eyes never read as a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EyeColour {
    /// `--peach`.
    Peach,
}

/// How the eyes are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EyeStyle {
    /// Glowing ovals with lids and no pupil.
    Ember,
}

/// The engine's gesture library. Each gesture sets the hands, the lids, the
/// gaze and the body's squash; the engine supplies time, voice level and
/// placement. Gestures happen on the outline (the silhouette rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Gesture {
    /// Hands rest; nothing else.
    Rest,
    /// Rests, and now and then waves hello with a smile.
    Greet,
    /// Both hands overhead, then rest.
    Stretch,
    /// Writes on the notepad with the pencil. Dictation only: the notepad
    /// is a dictation prop.
    Write,
    /// Both hands raised, ready.
    Ready,
    /// One hand scribbles beside the body; the eyes follow it.
    Scribble,
    /// One hand scratches the head; the eyes look up.
    ScratchHead,
    /// Both hands hammer in turn.
    Hammer,
    /// Both hands open toward the user.
    Offer,
    /// A startled hop with both hands up, then one hand raised.
    Startle,
    /// Flinches; the hands drop and the eyes turn sad.
    Droop,
    /// Waves again and again, hopping.
    Beckon,
    /// Cheers with both hands up and hops.
    Cheer,
}

impl Gesture {
    pub const ALL: [Gesture; 13] = [
        Gesture::Rest,
        Gesture::Greet,
        Gesture::Stretch,
        Gesture::Write,
        Gesture::Ready,
        Gesture::Scribble,
        Gesture::ScratchHead,
        Gesture::Hammer,
        Gesture::Offer,
        Gesture::Startle,
        Gesture::Droop,
        Gesture::Beckon,
        Gesture::Cheer,
    ];

    /// The name as it appears in a creature file.
    pub fn name(self) -> &'static str {
        match self {
            Gesture::Rest => "rest",
            Gesture::Greet => "greet",
            Gesture::Stretch => "stretch",
            Gesture::Write => "write",
            Gesture::Ready => "ready",
            Gesture::Scribble => "scribble",
            Gesture::ScratchHead => "scratch-head",
            Gesture::Hammer => "hammer",
            Gesture::Offer => "offer",
            Gesture::Startle => "startle",
            Gesture::Droop => "droop",
            Gesture::Beckon => "beckon",
            Gesture::Cheer => "cheer",
        }
    }
}

/// One gesture per state. The keys are the twelve states of
/// docs/design-system.md; which state shows, and when, is the engine's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Poses {
    pub idle: Gesture,
    pub preparing: Gesture,
    pub dictation: Gesture,
    pub command: Gesture,
    pub transcribing: Gesture,
    pub thinking: Gesture,
    pub tool: Gesture,
    pub confirm: Gesture,
    pub wake: Gesture,
    pub error: Gesture,
    pub agent_needs_you: Gesture,
    pub agent_done: Gesture,
}

impl Poses {
    /// Every state with its gesture, keyed by the state's file name.
    pub fn entries(&self) -> [(&'static str, Gesture); 12] {
        [
            ("idle", self.idle),
            ("preparing", self.preparing),
            ("dictation", self.dictation),
            ("command", self.command),
            ("transcribing", self.transcribing),
            ("thinking", self.thinking),
            ("tool", self.tool),
            ("confirm", self.confirm),
            ("wake", self.wake),
            ("error", self.error),
            ("agent-needs-you", self.agent_needs_you),
            ("agent-done", self.agent_done),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesture_names_match_serde() {
        for gesture in Gesture::ALL {
            let json = serde_json::to_string(&gesture).unwrap();
            assert_eq!(json, format!("\"{}\"", gesture.name()));
        }
    }

    #[test]
    fn bounds_reject_non_finite() {
        assert!(BODY_RADIUS_X.contains(29.0));
        assert!(!BODY_RADIUS_X.contains(f64::NAN));
        assert!(!BODY_RADIUS_X.contains(f64::INFINITY));
        assert!(!BODY_RADIUS_X.contains(23.9));
    }
}
