//! The overlay window's geometry: where it goes, which parts of it take the
//! mouse, and where the creature looks.
//!
//! The overlay is a transparent, always-on-top window that never takes
//! focus and lets clicks through everywhere except over what it draws
//! ([ADR-0040](../../../../docs/adr/0040-the-overlay-becomes-a-living-creature.md)).
//! This module is the GUI-free half: plain geometry, unit-tested here. The
//! Tauri shell (`apps/desktop/src-tauri/src/overlay.rs`) runs the cursor
//! poll and moves the real window.

pub mod drag;
pub mod foreground;
pub mod gaze;
pub mod geometry;
pub mod hit_test;
pub mod keyboard;
pub mod placement;
pub mod pointer;
pub mod regions;

pub use drag::Drags;
pub use foreground::{Foreground, Handback};
pub use gaze::{gaze_toward, Gaze, GAZE_FALLOFF};
pub use geometry::{Point, Rect};
pub use hit_test::{decide, ClickThrough, Decision, Sample, HIT_MARGIN};
pub use placement::{
    default_anchor, layout, legacy_anchor, pick_monitor, snap, switch_anchor, Frame, IslandSide,
    Layout, Monitor, Placement,
};
pub use regions::{Region, RegionError, RegionSet, Stamp};
