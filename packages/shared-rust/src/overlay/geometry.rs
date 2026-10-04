//! Points and rectangles for the overlay's screen geometry.
//!
//! Everything here is plain `f64`. Callers say which pixel space a value is
//! in (physical screen pixels, physical window pixels, or logical CSS
//! pixels); the types don't encode it.

use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// A point, or a 2D offset.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }
}

/// An axis-aligned rectangle: top-left corner plus size.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Half-open on the right and bottom edges, so two rectangles that touch
    /// never both claim the same pixel.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Grown by `by` on every side.
    pub fn inflate(&self, by: f64) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            self.width + 2.0 * by,
            self.height + 2.0 * by,
        )
    }

    /// Every coordinate multiplied by `k` (a change of pixel scale).
    pub fn scaled(&self, k: f64) -> Rect {
        Rect::new(self.x * k, self.y * k, self.width * k, self.height * k)
    }

    /// Finite, with a size that isn't negative.
    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width >= 0.0
            && self.height >= 0.0
    }

    /// Squared distance from `p` to the nearest point of the rectangle; zero
    /// inside it.
    pub fn distance_sq_to(&self, p: Point) -> f64 {
        let dx = (self.x - p.x).max(p.x - self.right()).max(0.0);
        let dy = (self.y - p.y).max(p.y - self.bottom()).max(0.0);
        dx * dx + dy * dy
    }
}

/// `v` clamped to `[lo, hi]`. Unlike `f64::clamp` it never panics: when the
/// range is empty (`hi < lo`, e.g. a window wider than its screen) the
/// result is `lo`.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo {
        lo
    } else {
        v.max(lo).min(hi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(Point::new(10.0, 20.0)));
        assert!(r.contains(Point::new(39.9, 59.9)));
        assert!(!r.contains(Point::new(40.0, 30.0)));
        assert!(!r.contains(Point::new(20.0, 60.0)));
        assert!(!r.contains(Point::new(9.9, 30.0)));
    }

    #[test]
    fn touching_rects_never_share_a_point() {
        let left = Rect::new(0.0, 0.0, 10.0, 10.0);
        let right = Rect::new(10.0, 0.0, 10.0, 10.0);
        let edge = Point::new(10.0, 5.0);
        assert!(!left.contains(edge));
        assert!(right.contains(edge));
    }

    #[test]
    fn negative_coordinates_work() {
        // A display left of, or above, the primary one has negative
        // physical coordinates.
        let r = Rect::new(-1920.0, -200.0, 1920.0, 1080.0);
        assert!(r.contains(Point::new(-1.0, -1.0)));
        assert!(r.contains(Point::new(-1920.0, -200.0)));
        assert!(!r.contains(Point::new(0.0, 0.0)));
        assert_eq!(r.center(), Point::new(-960.0, 340.0));
    }

    #[test]
    fn inflate_and_scale() {
        let r = Rect::new(10.0, 10.0, 20.0, 20.0);
        assert_eq!(r.inflate(5.0), Rect::new(5.0, 5.0, 30.0, 30.0));
        assert_eq!(r.scaled(1.5), Rect::new(15.0, 15.0, 30.0, 30.0));
    }

    #[test]
    fn validity() {
        assert!(Rect::new(0.0, 0.0, 0.0, 0.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, -1.0, 5.0).is_valid());
        assert!(!Rect::new(f64::NAN, 0.0, 1.0, 1.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, f64::INFINITY, 1.0).is_valid());
    }

    #[test]
    fn distance_to_rect() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(r.distance_sq_to(Point::new(5.0, 5.0)), 0.0);
        assert_eq!(r.distance_sq_to(Point::new(13.0, 5.0)), 9.0);
        assert_eq!(r.distance_sq_to(Point::new(-3.0, -4.0)), 25.0);
    }

    #[test]
    fn clamp_never_panics() {
        assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
        assert_eq!(clamp(-5.0, 0.0, 10.0), 0.0);
        assert_eq!(clamp(15.0, 0.0, 10.0), 10.0);
        assert_eq!(clamp(15.0, 10.0, 0.0), 10.0);
    }

    #[test]
    fn point_arithmetic() {
        let a = Point::new(3.0, 4.0);
        let b = Point::new(1.0, -1.0);
        assert_eq!(a + b, Point::new(4.0, 3.0));
        assert_eq!(a - b, Point::new(2.0, 5.0));
        assert!(!Point::new(f64::NAN, 0.0).is_finite());
    }
}
