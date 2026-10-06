//! Where the overlay window goes.
//!
//! Ottid stands in one of three placements (docs/design-system.md):
//!
//! - **Taskbar**: on the bottom edge of the work area, which is the
//!   taskbar's top edge when the taskbar is at the bottom of the screen.
//! - **Float**: anywhere on the desktop.
//! - **Ceiling**: hanging from the top edge of the work area.
//!
//! The creature draws inside a fixed-size **stage**. The overlay window is
//! the stage plus room for the island (bubbles, the approval card) on one
//! side of it, above the stage or below it. All the screen math happens in
//! physical pixels on one monitor; the frontend gets a logical [`Frame`].
//!
//! The **anchor** is the stage's centre in physical screen pixels. It is
//! what gets persisted: with the placement it fixes the monitor and, for the
//! taskbar and the ceiling, the horizontal position.

use serde::{Deserialize, Serialize};

use super::geometry::{clamp, Point, Rect};

/// The overlay window's size, in logical pixels. The stage takes the bottom
/// (or top) [`STAGE_H`]; the rest is room for the island, sized for the
/// approval card with its full command.
pub const WINDOW_W: f64 = 420.0;
pub const WINDOW_H: f64 = 520.0;

/// The creature's stage, in logical pixels: wide enough for a sideways reach
/// at the largest pose scale, tall enough for a hop above a raised hand.
pub const STAGE_W: f64 = 220.0;
pub const STAGE_H: f64 = 120.0;

/// One design unit, in logical pixels (docs/design-system.md).
pub const UNIT: f64 = 1.5;

/// How close, in logical pixels, the stage's edge must come to the bottom or
/// top of the work area for a drop to snap to the taskbar or the ceiling.
pub const SNAP_DISTANCE: f64 = 40.0;

/// Distance from the floor line up to the body's centre, in units.
const TASKBAR_CENTER_UNITS: f64 = 11.0;
/// Distance from the ceiling line down to the body's centre, in units: the
/// hanging body is 1.08 × 20.2 units tall above its centre, plus 11 units of
/// arm.
const CEILING_CENTER_UNITS: f64 = 33.0;
/// The eyes sit this far below the body's centre, in units (negative: above).
const EYE_UNITS: f64 = -1.3;

/// Where Ottid stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    #[default]
    Taskbar,
    Float,
    Ceiling,
    Left,
    Right,
}

impl Placement {
    pub const ALL: [Placement; 5] = [
        Placement::Taskbar,
        Placement::Float,
        Placement::Ceiling,
        Placement::Left,
        Placement::Right,
    ];

    /// The settings and wire code: `taskbar`, `float` or `ceiling`.
    pub fn code(self) -> &'static str {
        match self {
            Placement::Taskbar => "taskbar",
            Placement::Float => "float",
            Placement::Ceiling => "ceiling",
            Placement::Left => "left",
            Placement::Right => "right",
        }
    }

    pub fn from_code(code: &str) -> Option<Placement> {
        Placement::ALL.into_iter().find(|p| p.code() == code)
    }

    /// The body's centre inside the stage, in logical pixels.
    pub fn body_center(self) -> Point {
        let cx = STAGE_W / 2.0;
        match self {
            Placement::Taskbar => Point::new(cx, STAGE_H - TASKBAR_CENTER_UNITS * UNIT),
            Placement::Ceiling => Point::new(cx, CEILING_CENTER_UNITS * UNIT),
            Placement::Float => Point::new(cx, STAGE_H / 2.0),
            Placement::Left => Point::new(29.0 * UNIT + 4.0, STAGE_H * 0.53),
            Placement::Right => Point::new(STAGE_W - 29.0 * UNIT - 4.0, STAGE_H * 0.53),
        }
    }

    /// The eyes inside the stage, in logical pixels: where gaze is measured
    /// from.
    pub fn eye(self) -> Point {
        let c = self.body_center();
        Point::new(c.x, c.y + EYE_UNITS * UNIT)
    }
}

/// One display, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    /// The whole display.
    pub bounds: Rect,
    /// The display minus the taskbar, dock or panels.
    pub work: Rect,
    /// Physical pixels per logical pixel.
    pub scale: f64,
}

impl Monitor {
    /// A sane scale, and a work area inside the display. Platforms
    /// occasionally report a zero or missing work area; the whole display
    /// stands in for it.
    pub fn sanitized(self) -> Monitor {
        let scale = if self.scale.is_finite() && self.scale > 0.0 {
            self.scale
        } else {
            1.0
        };
        let work = if self.work.is_valid() && self.work.width > 0.0 && self.work.height > 0.0 {
            self.work
        } else {
            self.bounds
        };
        Monitor {
            bounds: self.bounds,
            work,
            scale,
        }
    }
}

/// Which side of the stage the island opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandSide {
    Above,
    Below,
}

/// What the frontend needs to lay itself out, in logical pixels relative to
/// the window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub placement: Placement,
    /// True while the user drags Ottid: the creature hangs from the cursor
    /// rather than standing in its placement.
    pub dragging: bool,
    /// The window's size.
    pub width: f64,
    pub height: f64,
    /// The stage inside the window.
    pub stage: Rect,
    /// Where the island opens.
    pub island: IslandSide,
    pub scale: f64,
}

/// A computed layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The window, in physical screen pixels. Whole pixels.
    pub window: Rect,
    /// The stage, in physical screen pixels.
    pub stage: Rect,
    pub monitor: Monitor,
    pub frame: Frame,
}

impl Layout {
    /// The creature's eyes, in physical screen pixels.
    pub fn eye(&self) -> Point {
        let eye = self.frame.placement.eye();
        let s = self.monitor.scale;
        Point::new(self.stage.x + eye.x * s, self.stage.y + eye.y * s)
    }
}

/// Lay out the window for `placement` on `monitor`, with the stage centred
/// as close to `anchor` as the placement allows.
///
/// While `dragging`, the stage follows the anchor freely (like Float), and
/// the frame says so.
pub fn layout(placement: Placement, anchor: Point, monitor: &Monitor, dragging: bool) -> Layout {
    let monitor = monitor.sanitized();
    let scale = monitor.scale;
    let work = monitor.work;
    let anchor = if anchor.is_finite() {
        anchor
    } else {
        work.center()
    };

    let stage_w = STAGE_W * scale;
    let stage_h = STAGE_H * scale;
    let effective = if dragging {
        Placement::Float
    } else {
        placement
    };

    // Whole pixels, so the window rounded around the stage still holds it.
    let stage_x = match effective {
        Placement::Left => work.x.ceil(),
        Placement::Right => (work.right() - stage_w).floor(),
        _ => clamp(
            (anchor.x - stage_w / 2.0).round(),
            work.x.ceil(),
            (work.right() - stage_w).floor(),
        ),
    };
    let stage_y = match effective {
        Placement::Taskbar => (work.bottom() - stage_h).floor(),
        Placement::Ceiling => work.y.ceil(),
        Placement::Float | Placement::Left | Placement::Right => clamp(
            (anchor.y - stage_h / 2.0).round(),
            work.y.ceil(),
            (work.bottom() - stage_h).floor(),
        ),
    };
    let stage = Rect::new(stage_x, stage_y, stage_w, stage_h);

    let win_w = (WINDOW_W * scale).round().min(work.width.floor()).max(1.0);
    let win_h = (WINDOW_H * scale).round().min(work.height.floor()).max(1.0);
    let island_h = win_h - stage_h;

    let side = match effective {
        Placement::Taskbar => IslandSide::Above,
        Placement::Ceiling => IslandSide::Below,
        Placement::Float | Placement::Left | Placement::Right => {
            let room_above = stage.y - work.y;
            let room_below = work.bottom() - stage.bottom();
            if room_above >= island_h || room_above >= room_below {
                IslandSide::Above
            } else {
                IslandSide::Below
            }
        }
    };

    let want_y = match side {
        IslandSide::Above => stage.y - island_h,
        IslandSide::Below => stage.y,
    };
    let win_x = clamp(
        (stage.center().x - win_w / 2.0).round(),
        work.x.ceil(),
        (work.right() - win_w).floor(),
    );
    let win_y = clamp(
        want_y.round(),
        work.y.ceil(),
        (work.bottom() - win_h).floor(),
    );
    let window = Rect::new(win_x, win_y, win_w, win_h);

    let frame = Frame {
        placement,
        dragging,
        width: win_w / scale,
        height: win_h / scale,
        stage: Rect::new(
            (stage.x - window.x) / scale,
            (stage.y - window.y) / scale,
            STAGE_W,
            STAGE_H,
        ),
        island: side,
        scale,
    };

    Layout {
        window,
        stage,
        monitor,
        frame,
    }
}

/// The placement a dropped stage snaps to: the taskbar when its bottom is
/// near the bottom of the work area, the ceiling when its top is near the
/// top, otherwise Float.
pub fn snap(stage: &Rect, monitor: &Monitor) -> Placement {
    let monitor = monitor.sanitized();
    let reach = SNAP_DISTANCE * monitor.scale;
    if (monitor.work.bottom() - stage.bottom()).abs() <= reach {
        Placement::Taskbar
    } else if (stage.y - monitor.work.y).abs() <= reach {
        Placement::Ceiling
    } else if (stage.x - monitor.work.x).abs() <= reach {
        Placement::Left
    } else if (monitor.work.right() - stage.right()).abs() <= reach {
        Placement::Right
    } else {
        Placement::Float
    }
}

/// The monitor for `p`: the one containing it, else the nearest one.
/// `None` only when there are no monitors.
pub fn pick_monitor(p: Point, monitors: &[Monitor]) -> Option<usize> {
    if !p.is_finite() {
        return if monitors.is_empty() { None } else { Some(0) };
    }
    monitors
        .iter()
        .position(|m| m.bounds.contains(p))
        .or_else(|| {
            monitors
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.bounds
                        .distance_sq_to(p)
                        .total_cmp(&b.bounds.distance_sq_to(p))
                })
                .map(|(i, _)| i)
        })
}

/// Where a first run puts Ottid: on the taskbar, towards the right, near the
/// notification area and away from the centre of the user's work.
pub fn default_anchor(monitor: &Monitor) -> Point {
    let monitor = monitor.sanitized();
    let work = monitor.work;
    let stage_h = STAGE_H * monitor.scale;
    Point::new(work.x + work.width * 0.85, work.bottom() - stage_h / 2.0)
}

/// Where the old overlay drew its mark's centre, in logical pixels from the
/// window's top-left: 16 px of padding plus half the 221 px mark stage
/// across, 14 px plus half the stage down.
const LEGACY_MARK_CENTER: Point = Point::new(126.5, 124.5);

/// The anchor for a position saved by the old overlay window
/// (`tongue.position`, its top-left in physical pixels): the centre of the
/// mark it drew.
pub fn legacy_anchor(top_left: Point, scale: f64) -> Point {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    Point::new(
        top_left.x + LEGACY_MARK_CENTER.x * scale,
        top_left.y + LEGACY_MARK_CENTER.y * scale,
    )
}

/// The anchor to use when the user switches placement from a menu.
///
/// The taskbar and the ceiling only keep the horizontal position. Going to
/// Float from one of them moves the stage off the edge, so the creature
/// visibly floats instead of bobbing on the line it just left.
pub fn switch_anchor(from: &Layout, to: Placement) -> Point {
    let center = from.stage.center();
    let lift = 2.0 * SNAP_DISTANCE * from.monitor.scale;
    match (from.frame.placement, to) {
        (Placement::Taskbar, Placement::Float) => Point::new(center.x, center.y - lift),
        (Placement::Ceiling, Placement::Float) => Point::new(center.x, center.y + lift),
        (Placement::Left, Placement::Float) => Point::new(center.x + lift, center.y),
        (Placement::Right, Placement::Float) => Point::new(center.x - lift, center.y),
        _ => center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920 × 1080 display at `scale` with a 48-logical-px taskbar at the
    /// bottom, its top-left at `(x, y)`.
    fn monitor(x: f64, y: f64, scale: f64) -> Monitor {
        let w = 1920.0 * scale;
        let h = 1080.0 * scale;
        let taskbar = 48.0 * scale;
        Monitor {
            bounds: Rect::new(x, y, w, h),
            work: Rect::new(x, y, w, h - taskbar),
            scale,
        }
    }

    fn inside(inner: &Rect, outer: &Rect) -> bool {
        const EPS: f64 = 1e-6;
        inner.x >= outer.x - EPS
            && inner.y >= outer.y - EPS
            && inner.right() <= outer.right() + EPS
            && inner.bottom() <= outer.bottom() + EPS
    }

    #[test]
    fn taskbar_stands_on_the_work_area_bottom() {
        let m = monitor(0.0, 0.0, 1.0);
        let l = layout(Placement::Taskbar, Point::new(960.0, 500.0), &m, false);
        assert_eq!(l.stage.bottom(), m.work.bottom());
        assert_eq!(l.stage.center().x, 960.0);
        assert_eq!(l.window.bottom(), m.work.bottom());
        assert_eq!(l.window.width, WINDOW_W);
        assert_eq!(l.window.height, WINDOW_H);
        assert_eq!(l.frame.island, IslandSide::Above);
        assert_eq!(l.frame.stage, Rect::new(100.0, 400.0, STAGE_W, STAGE_H));
    }

    #[test]
    fn ceiling_hangs_from_the_work_area_top() {
        let m = monitor(0.0, 0.0, 1.0);
        let l = layout(Placement::Ceiling, Point::new(400.0, 900.0), &m, false);
        assert_eq!(l.stage.y, m.work.y);
        assert_eq!(l.window.y, m.work.y);
        assert_eq!(l.frame.island, IslandSide::Below);
        assert_eq!(l.frame.stage.y, 0.0);
        assert_eq!(l.stage.center().x, 400.0);
    }

    #[test]
    fn float_centres_the_stage_on_the_anchor() {
        let m = monitor(0.0, 0.0, 1.0);
        let anchor = Point::new(900.0, 600.0);
        let l = layout(Placement::Float, anchor, &m, false);
        assert_eq!(l.stage.center(), anchor);
        assert_eq!(l.frame.island, IslandSide::Above);
        assert_eq!(l.frame.stage.y, WINDOW_H - STAGE_H);
    }

    #[test]
    fn float_near_the_top_opens_the_island_below() {
        let m = monitor(0.0, 0.0, 1.0);
        let l = layout(Placement::Float, Point::new(900.0, 120.0), &m, false);
        assert_eq!(l.frame.island, IslandSide::Below);
        assert_eq!(l.window.y, l.stage.y);
        assert_eq!(l.frame.stage.y, 0.0);
    }

    #[test]
    fn edges_clamp_the_stage_and_the_window() {
        let m = monitor(0.0, 0.0, 1.0);
        let l = layout(Placement::Taskbar, Point::new(5.0, 0.0), &m, false);
        assert_eq!(l.stage.x, 0.0);
        assert_eq!(l.window.x, 0.0);
        // The window can't centre on the stage at the edge: the stage sits
        // off-centre in it, and the frame says where.
        assert_eq!(l.frame.stage.x, 0.0);

        let r = layout(Placement::Taskbar, Point::new(5000.0, 0.0), &m, false);
        assert_eq!(r.stage.right(), m.work.right());
        assert_eq!(r.window.right(), m.work.right());
        assert_eq!(r.frame.stage.x, WINDOW_W - STAGE_W);
    }

    #[test]
    fn scale_is_physical_on_screen_and_logical_in_the_frame() {
        let m = monitor(0.0, 0.0, 1.5);
        let l = layout(Placement::Taskbar, Point::new(1440.0, 0.0), &m, false);
        assert_eq!(l.window.width, 630.0);
        assert_eq!(l.window.height, 780.0);
        assert_eq!(l.stage.width, 330.0);
        assert_eq!(l.frame.width, WINDOW_W);
        assert_eq!(l.frame.height, WINDOW_H);
        assert_eq!(l.frame.scale, 1.5);
        assert!((l.frame.stage.y - (WINDOW_H - STAGE_H)).abs() < 1e-9);
    }

    #[test]
    fn a_display_left_of_and_above_the_primary_works() {
        let m = monitor(-1920.0, -300.0, 1.0);
        let l = layout(Placement::Taskbar, Point::new(-960.0, 0.0), &m, false);
        assert_eq!(l.stage.bottom(), m.work.bottom());
        assert!(inside(&l.window, &m.work));
        assert_eq!(l.stage.center().x, -960.0);
    }

    #[test]
    fn a_tiny_work_area_shrinks_the_window_to_fit() {
        let m = Monitor {
            bounds: Rect::new(0.0, 0.0, 300.0, 400.0),
            work: Rect::new(0.0, 0.0, 300.0, 400.0),
            scale: 1.0,
        };
        let l = layout(Placement::Float, Point::new(150.0, 200.0), &m, false);
        assert_eq!(l.window, Rect::new(0.0, 0.0, 300.0, 400.0));
        assert!(inside(&l.stage, &l.window));
    }

    #[test]
    fn the_window_stays_in_the_work_area_and_holds_the_stage() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let m = monitor(-700.0, 120.0, scale);
            for placement in Placement::ALL {
                for dragging in [false, true] {
                    for ix in -2..=12 {
                        for iy in -2..=12 {
                            let anchor = Point::new(
                                m.bounds.x + m.bounds.width * ix as f64 / 10.0,
                                m.bounds.y + m.bounds.height * iy as f64 / 10.0,
                            );
                            let l = layout(placement, anchor, &m, dragging);
                            assert!(inside(&l.window, &m.work), "{l:?}");
                            assert!(inside(&l.stage, &l.window), "{l:?}");
                            assert_eq!(l.window.x.fract(), 0.0);
                            assert_eq!(l.window.y.fract(), 0.0);
                            assert_eq!(l.window.width.fract(), 0.0);
                            assert_eq!(l.window.height.fract(), 0.0);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dragging_follows_the_anchor_like_float() {
        let m = monitor(0.0, 0.0, 1.0);
        let anchor = Point::new(700.0, 500.0);
        let l = layout(Placement::Taskbar, anchor, &m, true);
        assert_eq!(l.stage.center(), anchor);
        assert!(l.frame.dragging);
        // The frame still names the placement the drag started from.
        assert_eq!(l.frame.placement, Placement::Taskbar);
    }

    #[test]
    fn bad_monitor_data_is_survivable() {
        let m = Monitor {
            bounds: Rect::new(0.0, 0.0, 1920.0, 1080.0),
            work: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: f64::NAN,
        };
        let l = layout(Placement::Taskbar, Point::new(f64::NAN, 3.0), &m, false);
        assert_eq!(l.monitor.scale, 1.0);
        assert_eq!(l.stage.bottom(), 1080.0);
        assert_eq!(l.stage.center().x, 960.0);
    }

    #[test]
    fn snap_picks_the_nearest_edge() {
        let m = monitor(0.0, 0.0, 1.0);
        let at = |y: f64| Rect::new(500.0, y, STAGE_W, STAGE_H);
        let bottom = m.work.bottom();
        assert_eq!(snap(&at(bottom - STAGE_H), &m), Placement::Taskbar);
        assert_eq!(snap(&at(bottom - STAGE_H - 39.0), &m), Placement::Taskbar);
        assert_eq!(snap(&at(bottom - STAGE_H - 41.0), &m), Placement::Float);
        assert_eq!(snap(&at(0.0), &m), Placement::Ceiling);
        assert_eq!(snap(&at(40.0), &m), Placement::Ceiling);
        assert_eq!(snap(&at(41.0), &m), Placement::Float);
        assert_eq!(snap(&at(500.0), &m), Placement::Float);
    }

    #[test]
    fn snap_distance_scales() {
        let m = monitor(0.0, 0.0, 2.0);
        let stage = Rect::new(0.0, 70.0, STAGE_W * 2.0, STAGE_H * 2.0);
        assert_eq!(snap(&stage, &m), Placement::Ceiling);
    }

    #[test]
    fn snapping_a_layout_keeps_its_placement() {
        // Dropping Ottid where it already stands never changes the placement.
        let m = monitor(0.0, 0.0, 1.25);
        for placement in Placement::ALL {
            let anchor = Point::new(800.0, 600.0);
            let l = layout(placement, anchor, &m, false);
            assert_eq!(snap(&l.stage, &m), placement, "{placement:?}");
        }
    }

    #[test]
    fn pick_monitor_prefers_containing_then_nearest() {
        let left = monitor(-1920.0, 0.0, 1.0);
        let right = monitor(0.0, 0.0, 1.5);
        let all = [left, right];
        assert_eq!(pick_monitor(Point::new(-5.0, 10.0), &all), Some(0));
        assert_eq!(pick_monitor(Point::new(5.0, 10.0), &all), Some(1));
        // Below both: the nearest.
        assert_eq!(pick_monitor(Point::new(100.0, 5000.0), &all), Some(1));
        assert_eq!(pick_monitor(Point::new(-1000.0, -500.0), &all), Some(0));
        assert_eq!(pick_monitor(Point::new(0.0, 0.0), &[]), None);
        assert_eq!(pick_monitor(Point::new(f64::NAN, 0.0), &all), Some(0));
    }

    #[test]
    fn default_anchor_is_on_the_taskbar_to_the_right() {
        let m = monitor(0.0, 0.0, 1.0);
        let a = default_anchor(&m);
        let l = layout(Placement::Taskbar, a, &m, false);
        assert_eq!(l.stage.center(), a);
        assert!(a.x > m.work.center().x);
        assert!(m.work.contains(a));
    }

    #[test]
    fn legacy_anchor_is_the_old_mark_centre() {
        assert_eq!(
            legacy_anchor(Point::new(100.0, 200.0), 1.0),
            Point::new(226.5, 324.5)
        );
        assert_eq!(
            legacy_anchor(Point::new(100.0, 200.0), 2.0),
            Point::new(353.0, 449.0)
        );
        assert_eq!(
            legacy_anchor(Point::new(0.0, 0.0), 0.0),
            Point::new(126.5, 124.5)
        );
    }

    #[test]
    fn switching_to_float_lifts_off_the_edge() {
        let m = monitor(0.0, 0.0, 1.0);
        let task = layout(Placement::Taskbar, Point::new(900.0, 0.0), &m, false);
        let a = switch_anchor(&task, Placement::Float);
        let float = layout(Placement::Float, a, &m, false);
        assert_eq!(snap(&float.stage, &m), Placement::Float);
        assert!(float.stage.center().y < task.stage.center().y);
        assert_eq!(float.stage.center().x, 900.0);

        let ceiling = layout(Placement::Ceiling, Point::new(900.0, 0.0), &m, false);
        let b = switch_anchor(&ceiling, Placement::Float);
        let float = layout(Placement::Float, b, &m, false);
        assert_eq!(snap(&float.stage, &m), Placement::Float);
        assert!(float.stage.center().y > ceiling.stage.center().y);

        // The other switches keep the stage's centre.
        assert_eq!(
            switch_anchor(&task, Placement::Ceiling),
            task.stage.center()
        );
    }

    #[test]
    fn eyes_follow_the_placement() {
        let m = monitor(0.0, 0.0, 1.0);
        let task = layout(Placement::Taskbar, Point::new(900.0, 0.0), &m, false);
        let eye = task.eye();
        assert_eq!(eye.x, 900.0);
        // 11 units above the floor, then 1.3 more to the eyes.
        assert!((m.work.bottom() - eye.y - 12.3 * UNIT).abs() < 1e-9);

        let ceiling = layout(Placement::Ceiling, Point::new(900.0, 0.0), &m, false);
        assert!((ceiling.eye().y - (33.0 - 1.3) * UNIT).abs() < 1e-9);
    }

    #[test]
    fn placement_codes_round_trip() {
        for p in Placement::ALL {
            assert_eq!(Placement::from_code(p.code()), Some(p));
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.code()));
            assert_eq!(serde_json::from_str::<Placement>(&json).unwrap(), p);
        }
        assert_eq!(Placement::from_code("TASKBAR"), None);
        assert_eq!(Placement::from_code(""), None);
        assert_eq!(Placement::default(), Placement::Taskbar);
    }

    #[test]
    fn side_docking_preserves_height_and_stays_inside_scaled_monitors() {
        for scale in [1.0, 1.25, 2.0] {
            let m = monitor(-1920.0, -100.0, scale);
            for p in [Placement::Left, Placement::Right] {
                let anchor = Point::new(m.work.center().x, m.work.center().y);
                let l = layout(p, anchor, &m, false);
                assert!(inside(&l.window, &m.work));
                assert_eq!(l.stage.center().y, anchor.y);
                assert_eq!(snap(&l.stage, &m), p);
                if p == Placement::Left {
                    assert_eq!(l.stage.x, m.work.x);
                } else {
                    assert_eq!(l.stage.right(), m.work.right());
                }
                let free = layout(
                    Placement::Float,
                    switch_anchor(&l, Placement::Float),
                    &m,
                    false,
                );
                assert_eq!(snap(&free.stage, &m), Placement::Float);
            }
        }
    }

    #[test]
    fn frame_serializes_camel_case() {
        let m = monitor(0.0, 0.0, 1.0);
        let l = layout(Placement::Float, Point::new(900.0, 600.0), &m, false);
        let v = serde_json::to_value(&l.frame).unwrap();
        assert_eq!(v["placement"], "float");
        assert_eq!(v["dragging"], false);
        assert_eq!(v["island"], "above");
        assert_eq!(v["width"], WINDOW_W);
        assert_eq!(v["stage"]["width"], STAGE_W);
    }
}
