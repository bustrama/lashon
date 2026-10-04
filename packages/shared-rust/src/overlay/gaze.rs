// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: the eyes-follow-the-cursor mapping in windows/src/island/island.ts.
// See THIRD-PARTY-NOTICES.

//! Where the creature looks.
//!
//! Gaze is a direction, not a point: each axis runs from -1 to 1, x to the
//! right and y down. A `tanh` of the cursor's offset from the eyes makes the
//! eyes follow a nearby cursor closely and saturate smoothly for a far one,
//! so they never snap to the edge.

use serde::Serialize;

use super::geometry::Point;

/// The offset, in logical pixels, at which the gaze reaches `tanh(1)` ≈ 0.76
/// of its range.
pub const GAZE_FALLOFF: f64 = 220.0;

/// A gaze direction, each axis in `[-1, 1]` (x right, y down).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Gaze {
    pub x: f64,
    pub y: f64,
}

/// Look from `eye` toward `target`, both in the same pixel space.
/// `falloff` is in that space too; pass `GAZE_FALLOFF × scale` for physical
/// pixels. A degenerate input looks straight ahead.
pub fn gaze_toward(eye: Point, target: Point, falloff: f64) -> Gaze {
    if !(eye.is_finite() && target.is_finite() && falloff.is_finite() && falloff > 0.0) {
        return Gaze::default();
    }
    Gaze {
        x: ((target.x - eye.x) / falloff).tanh(),
        y: ((target.y - eye.y) / falloff).tanh(),
    }
}

impl Gaze {
    /// Rounded to `step`, so the poll emits a change only when the eyes would
    /// visibly move.
    pub fn quantized(self, step: f64) -> Gaze {
        if !(step.is_finite() && step > 0.0) {
            return self;
        }
        Gaze {
            x: (self.x / step).round() * step,
            y: (self.y / step).round() * step,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_straight_ahead_at_itself() {
        let eye = Point::new(500.0, 500.0);
        assert_eq!(gaze_toward(eye, eye, GAZE_FALLOFF), Gaze::default());
    }

    #[test]
    fn follows_the_axes_x_right_y_down() {
        let eye = Point::new(0.0, 0.0);
        let g = gaze_toward(eye, Point::new(100.0, -100.0), 100.0);
        assert!((g.x - 1f64.tanh()).abs() < 1e-12);
        assert!((g.y + 1f64.tanh()).abs() < 1e-12);
    }

    #[test]
    fn stays_in_range_for_far_targets() {
        let eye = Point::new(0.0, 0.0);
        let g = gaze_toward(eye, Point::new(1e9, -1e9), GAZE_FALLOFF);
        assert!(g.x <= 1.0 && g.x > 0.99);
        assert!(g.y >= -1.0 && g.y < -0.99);
    }

    #[test]
    fn degenerate_inputs_look_ahead() {
        let eye = Point::new(0.0, 0.0);
        let far = Point::new(50.0, 50.0);
        assert_eq!(gaze_toward(eye, far, 0.0), Gaze::default());
        assert_eq!(gaze_toward(eye, far, f64::NAN), Gaze::default());
        assert_eq!(
            gaze_toward(Point::new(f64::NAN, 0.0), far, 1.0),
            Gaze::default()
        );
    }

    #[test]
    fn quantize_snaps_to_steps() {
        let g = Gaze {
            x: 0.123,
            y: -0.987,
        }
        .quantized(0.05);
        assert!((g.x - 0.10).abs() < 1e-9);
        assert!((g.y + 1.0).abs() < 1e-9);
        let same = Gaze { x: 0.3, y: 0.3 };
        assert_eq!(same.quantized(0.0), same);
    }
}
