//! Creatures as data (docs/adr/0041, docs/adr/0045).
//!
//! A creature is one declarative `creature.json`: body radii, hands, eyes,
//! where its lamp sits, and a gesture per state. It contains nothing that
//! runs. The overlay's engine draws it; the engine, not the file, owns the
//! state machine, the props and the lamp's colour and strength.
//!
//! - [`schema`] holds the types, which are also the JSON Schema source
//!   (`creatures/schema/ottid-creature.schema.json`, kept in sync by
//!   `tests/creature_schema_snapshot.rs`).
//! - [`validate`] is the one validator every creature file goes through.
//!
//! The default creature, `ottid`, is bundled into the binary and cannot be
//! modified. It is the first example of the format, not a special case: it
//! passes through the same validator.

pub mod schema;
pub mod validate;

pub use schema::{Creature, Gesture, SCHEMA_VERSION};
pub use validate::{validate_creature, CreatureError, CreatureIssue};

/// The bundled default creature, as shipped. The frontend bundles the same
/// file (`apps/desktop/src/lib/creature/data.ts`).
pub const DEFAULT_CREATURE_JSON: &str = include_str!("../../../../creatures/ottid/creature.json");

/// The bundled default creature, validated.
pub fn default_creature() -> Creature {
    // The file is compiled in and covered by `default_creature_is_valid`, so
    // a failure here is a build defect, not a user error.
    validate_creature(DEFAULT_CREATURE_JSON).expect("the bundled default creature is valid")
}
