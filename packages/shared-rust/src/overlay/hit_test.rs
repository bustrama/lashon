// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: the click-through decision of the cursor poll in
// windows/src-tauri/src/island.rs. See THIRD-PARTY-NOTICES.

//! The click-through decision.
//!
//! The overlay window is transparent and much larger than what it draws.
//! The OS can't see per-pixel transparency through WebView2, so the window
//! either takes every click inside its rectangle or none of them. A poll
//! thread reads the cursor every tick and flips that switch: the window
//! takes the mouse only while the cursor is over an interactive region.
//!
//! The decision and the OS call must happen in the same tick as the cursor
//! read, on one thread that owns the switch. When the decision is made one
//! IPC hop away, ticks overlap, flag changes land out of order, and a cached
//! flag drifts from the real window. Clicks on interactive elements then
//! fall through to the app underneath. [`ClickThrough`] is that single
//! owner's bookkeeping.

use super::geometry::Point;
use super::regions::RegionSet;

/// How far outside a region the window already takes the mouse, in logical
/// pixels. A moving cursor crosses several pixels per tick, so without a
/// margin the first pixels of a button would still click through.
pub const HIT_MARGIN: f64 = 8.0;

/// One cursor reading, all in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// The cursor, in physical screen pixels.
    pub cursor: Point,
    /// The window's top-left, in physical screen pixels.
    pub origin: Point,
    /// The window's current scale factor.
    pub scale: f64,
}

impl Sample {
    /// The cursor in physical pixels relative to the window's top-left.
    pub fn local(&self) -> Point {
        self.cursor - self.origin
    }
}

/// What the window should do this tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    /// Take the mouse (`true`) or let it through (`false`).
    pub accept: bool,
    /// The region under the cursor, if any.
    pub region: Option<String>,
}

/// Decide from one sample.
///
/// `holding` is true while the overlay itself holds the pointer (a drag of
/// the creature is in progress). The window then keeps the mouse wherever
/// the cursor is, so the press can't be released over another app.
pub fn decide(regions: &RegionSet, sample: &Sample, holding: bool) -> Decision {
    let margin = HIT_MARGIN * sanitize_scale(sample.scale);
    let region = if sample.cursor.is_finite() && sample.origin.is_finite() {
        regions
            .hit(sample.local(), sample.scale, margin)
            .map(str::to_owned)
    } else {
        None
    };
    Decision {
        accept: holding || region.is_some(),
        region,
    }
}

fn sanitize_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// The poll's record of the window's click-through flag.
///
/// The OS is only called when the wanted state differs from the recorded
/// one. After anything that may have reset the real flag behind our back
/// (a resize, a move to another display, the window being shown again), call
/// [`invalidate`](Self::invalidate) and the next tick re-applies it.
#[derive(Debug, Clone, Default)]
pub struct ClickThrough {
    ignoring: Option<bool>,
}

impl ClickThrough {
    /// Record the wanted state. Returns `Some(ignore)` when the OS must be
    /// told, `None` when the window is already right.
    pub fn update(&mut self, accept: bool) -> Option<bool> {
        let ignore = !accept;
        if self.ignoring == Some(ignore) {
            None
        } else {
            self.ignoring = Some(ignore);
            Some(ignore)
        }
    }

    /// Forget the recorded state, so the next [`update`](Self::update)
    /// always calls the OS.
    pub fn invalidate(&mut self) {
        self.ignoring = None;
    }

    /// The OS call failed: the recorded state is unknown again.
    pub fn failed(&mut self) {
        self.invalidate();
    }

    pub fn is_ignoring(&self) -> Option<bool> {
        self.ignoring
    }
}

#[cfg(test)]
mod tests {
    use super::super::geometry::Rect;
    use super::super::regions::Region;
    use super::*;

    fn regions(list: &[(&str, Rect)], scale: f64) -> RegionSet {
        let mut set = RegionSet::default();
        set.replace(
            list.iter()
                .map(|(id, rect)| Region {
                    id: (*id).to_string(),
                    rect: *rect,
                })
                .collect(),
            scale,
        )
        .unwrap();
        set
    }

    fn sample(cx: f64, cy: f64, ox: f64, oy: f64, scale: f64) -> Sample {
        Sample {
            cursor: Point::new(cx, cy),
            origin: Point::new(ox, oy),
            scale,
        }
    }

    #[test]
    fn accepts_only_over_a_region() {
        let set = regions(&[("creature", Rect::new(100.0, 400.0, 80.0, 60.0))], 1.0);
        // Window at (1000, 500): the creature is at (1100, 900) on screen.
        let over = decide(&set, &sample(1120.0, 920.0, 1000.0, 500.0, 1.0), false);
        assert!(over.accept);
        assert_eq!(over.region.as_deref(), Some("creature"));

        let away = decide(&set, &sample(1010.0, 510.0, 1000.0, 500.0, 1.0), false);
        assert!(!away.accept);
        assert_eq!(away.region, None);
    }

    #[test]
    fn the_margin_follows_the_scale() {
        let set = regions(&[("island", Rect::new(150.0, 150.0, 30.0, 30.0))], 1.5);
        // 10 physical px left of the region at 150 %: inside the 12 px margin.
        let near = decide(&set, &sample(140.0, 160.0, 0.0, 0.0, 1.5), false);
        assert!(near.accept);
        // At 100 % with the same report rescaled (the region is now at 100 px)
        // and an 8 px margin, 10 px off is outside.
        let far = decide(&set, &sample(90.0, 110.0, 0.0, 0.0, 1.0), false);
        assert!(!far.accept);
    }

    #[test]
    fn a_window_on_a_negative_display_works() {
        let set = regions(&[("creature", Rect::new(10.0, 10.0, 50.0, 50.0))], 1.0);
        let s = sample(-1890.0, -170.0, -1920.0, -200.0, 1.0);
        assert!(decide(&set, &s, false).accept);
    }

    #[test]
    fn holding_keeps_the_mouse_anywhere() {
        let set = regions(&[("creature", Rect::new(0.0, 0.0, 10.0, 10.0))], 1.0);
        let s = sample(500.0, 500.0, 0.0, 0.0, 1.0);
        assert!(!decide(&set, &s, false).accept);
        let held = decide(&set, &s, true);
        assert!(held.accept);
        assert_eq!(held.region, None);
    }

    #[test]
    fn no_regions_means_click_through() {
        let set = RegionSet::default();
        assert!(!decide(&set, &sample(5.0, 5.0, 0.0, 0.0, 1.0), false).accept);
    }

    #[test]
    fn garbage_samples_never_accept() {
        let set = regions(&[("creature", Rect::new(0.0, 0.0, 100.0, 100.0))], 1.0);
        let nan = sample(f64::NAN, 5.0, 0.0, 0.0, 1.0);
        assert!(!decide(&set, &nan, false).accept);
        let bad_scale = sample(5.0, 5.0, 0.0, 0.0, f64::NAN);
        // A bad scale falls back to 1, so the cursor still hits.
        assert!(decide(&set, &bad_scale, false).accept);
    }

    #[test]
    fn click_through_only_calls_the_os_on_change() {
        let mut ct = ClickThrough::default();
        assert_eq!(ct.is_ignoring(), None);
        // The first decision is always applied.
        assert_eq!(ct.update(false), Some(true));
        assert_eq!(ct.update(false), None);
        assert_eq!(ct.update(true), Some(false));
        assert_eq!(ct.update(true), None);
        assert_eq!(ct.is_ignoring(), Some(false));
    }

    #[test]
    fn invalidate_forces_the_next_call() {
        let mut ct = ClickThrough::default();
        ct.update(true);
        ct.invalidate();
        assert_eq!(ct.update(true), Some(false));
        ct.failed();
        assert_eq!(ct.update(true), Some(false));
    }
}
