//! Dragging the creature: from the frontend's start to its end.
//!
//! Start and end reach the shell as two async commands, and the runtime may
//! handle them in either order. An end handled before its start found no drag
//! to end, and the start then began one nobody would end: the window followed
//! the cursor until the poll saw the button up, which it can only see on
//! Windows.
//!
//! So each drag gets a token. Starting returns it, the frontend sends its end
//! with it once the start has answered, and an end only ends the drag with
//! its token. A late end can't drop a newer drag either.

use super::geometry::Point;

/// A drag ends by itself once the button has been seen up this many poll
/// ticks in a row, in case the webview never reports the release.
pub const RELEASED_TICKS: u8 = 2;

/// The drag in progress, if any.
#[derive(Debug, Default)]
pub struct Drags {
    current: Option<Drag>,
    last_token: u64,
}

#[derive(Debug)]
struct Drag {
    token: u64,
    /// Stage centre minus cursor at the start of the drag.
    offset: Point,
    released: u8,
}

impl Drags {
    /// Start a drag, replacing any in progress. Returns its token.
    pub fn start(&mut self, offset: Point) -> u64 {
        self.last_token = self.last_token.wrapping_add(1);
        self.current = Some(Drag {
            token: self.last_token,
            offset,
            released: 0,
        });
        self.last_token
    }

    /// End the drag with `token`. Returns whether it was the one in
    /// progress; an end for an older drag changes nothing.
    pub fn end(&mut self, token: u64) -> bool {
        if self.current.as_ref().is_some_and(|d| d.token == token) {
            self.current = None;
            true
        } else {
            false
        }
    }

    /// Drop the drag in progress, whichever it is (a placement was picked
    /// from the menu).
    pub fn cancel(&mut self) {
        self.current = None;
    }

    pub fn is_active(&self) -> bool {
        self.current.is_some()
    }

    /// Stage centre minus cursor at the start of the drag in progress.
    pub fn offset(&self) -> Option<Point> {
        self.current.as_ref().map(|d| d.offset)
    }

    /// The poll read the primary button: `Some(down)`, or `None` where the
    /// platform can't tell. Returns the token of a drag that should end
    /// because the button has been up for [`RELEASED_TICKS`] ticks.
    pub fn observe_button(&mut self, down: Option<bool>) -> Option<u64> {
        let drag = self.current.as_mut()?;
        drag.released = match down {
            Some(false) => drag.released.saturating_add(1),
            _ => 0,
        };
        (drag.released >= RELEASED_TICKS).then_some(drag.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offset() -> Point {
        Point::new(3.0, -4.0)
    }

    #[test]
    fn an_end_ends_its_own_drag() {
        let mut drags = Drags::default();
        let token = drags.start(offset());
        assert!(drags.is_active());
        assert_eq!(drags.offset(), Some(offset()));
        assert!(drags.end(token));
        assert!(!drags.is_active());
        assert!(!drags.end(token), "a second end is a no-op");
    }

    #[test]
    fn a_late_end_cannot_drop_a_newer_drag() {
        let mut drags = Drags::default();
        let first = drags.start(offset());
        // The first drag's end is still in flight when the second starts.
        let second = drags.start(offset());
        assert_ne!(first, second);
        assert!(!drags.end(first));
        assert!(drags.is_active());
        assert!(drags.end(second));
    }

    #[test]
    fn an_end_without_a_drag_changes_nothing() {
        let mut drags = Drags::default();
        assert!(!drags.end(1));
        let token = drags.start(offset());
        assert_eq!(token, 1);
        assert!(drags.is_active());
    }

    #[test]
    fn the_poll_ends_a_drag_once_the_button_stays_up() {
        let mut drags = Drags::default();
        let token = drags.start(offset());
        assert_eq!(drags.observe_button(Some(true)), None);
        assert_eq!(drags.observe_button(Some(false)), None);
        // A press again resets the count.
        assert_eq!(drags.observe_button(Some(true)), None);
        assert_eq!(drags.observe_button(Some(false)), None);
        assert_eq!(drags.observe_button(Some(false)), Some(token));
    }

    #[test]
    fn a_platform_without_a_button_read_leaves_the_end_to_the_frontend() {
        let mut drags = Drags::default();
        drags.start(offset());
        for _ in 0..10 {
            assert_eq!(drags.observe_button(None), None);
        }
        assert!(drags.is_active());
    }

    #[test]
    fn cancel_drops_any_drag() {
        let mut drags = Drags::default();
        let token = drags.start(offset());
        drags.cancel();
        assert!(!drags.is_active());
        assert!(!drags.end(token));
        assert_eq!(drags.observe_button(Some(false)), None);
    }
}
