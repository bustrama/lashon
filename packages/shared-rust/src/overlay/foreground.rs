//! Giving the foreground back after the overlay's right-click menu.
//!
//! The overlay never takes focus: it is non-activating, so the user's app
//! keeps the keyboard and dictation types into it. A popup menu is the
//! exception. Windows only closes a popup when the user clicks away if its
//! owner is the foreground window, so the menu library brings the overlay to
//! the front before it opens the menu. After the menu closes, the overlay is
//! still in front, and the next dictation would type into it.
//!
//! [`Foreground::remember`] notes which window was in front before the menu,
//! and [`Foreground::give_back`] puts it back afterwards. It only undoes the
//! overlay's own activation: if the user clicked into another app to close
//! the menu, that app stays in front.
//!
//! An item that opens a window (Settings, the tutorial, the logs folder)
//! keeps the front instead. Handing it to the user's app first would flash
//! that app, and the new window would then have to take the front from
//! another process, which Windows may refuse. [`Handback`] holds the
//! give-back until the item picked is known.

use std::sync::Mutex;

/// The window that was in front before the overlay's menu opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Foreground(isize);

impl Foreground {
    /// The foreground window now, or `None` when there is none or this
    /// platform has no direct read.
    pub fn remember() -> Option<Self> {
        imp::foreground().map(Self)
    }

    /// Bring this window to the front again if the overlay, whose native
    /// handle is `overlay`, is still in front. Returns whether it did.
    pub fn give_back(self, overlay: isize) -> bool {
        if !should_give_back(self.0, imp::foreground(), overlay) {
            return false;
        }
        let given = imp::bring_to_front(self.0);
        if !given {
            // The next dictation would type into the overlay. No window
            // titles: they can hold document names.
            tracing::warn!("could not give the foreground back after the overlay's menu");
        }
        given
    }
}

/// A give-back waiting for the overlay menu's outcome.
///
/// [`arm`](Self::arm) it before the menu opens. Once the item picked is
/// known, [`settle`](Self::settle) it exactly once; later calls find nothing
/// to do.
#[derive(Debug, Default)]
pub struct Handback(Mutex<Option<(Foreground, isize)>>);

impl Handback {
    /// Hold a give-back to `previous` from the overlay, whose native handle
    /// is `overlay`.
    pub fn arm(&self, previous: Foreground, overlay: isize) {
        *self.lock() = Some((previous, overlay));
    }

    /// Settle the give-back held, if any: drop it when the item picked
    /// keeps the front (it opens a window), and give the front back
    /// otherwise. Returns whether the front was given back.
    pub fn settle(&self, keep_front: bool) -> bool {
        let Some((previous, overlay)) = self.lock().take() else {
            return false;
        };
        !keep_front && previous.give_back(overlay)
    }

    /// Whether a give-back is held.
    pub fn is_armed(&self) -> bool {
        self.lock().is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Foreground, isize)>> {
        // The value is a plain pair, always whole: a panic elsewhere while
        // the lock was held leaves nothing half-written.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Whether to hand the foreground from the overlay back to `previous`.
///
/// Only while the overlay is still in front: anything else in front got
/// there after the menu (the user clicked it, or a menu item opened it).
/// Never "back" to the overlay itself.
fn should_give_back(previous: isize, now: Option<isize>, overlay: isize) -> bool {
    now == Some(overlay) && previous != overlay
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, IsWindow, SetForegroundWindow,
    };

    pub fn foreground() -> Option<isize> {
        // SAFETY: takes no arguments and touches no memory of ours.
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
    }

    pub fn bring_to_front(window: isize) -> bool {
        let hwnd = HWND(window as *mut _);
        // SAFETY: both calls take a window handle by value. `IsWindow`
        // accepts any value, and a handle that has since been destroyed
        // makes it return false before `SetForegroundWindow` sees it.
        unsafe { IsWindow(Some(hwnd)).as_bool() && SetForegroundWindow(hwnd).as_bool() }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn foreground() -> Option<isize> {
        None
    }

    pub fn bring_to_front(_window: isize) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: isize = 0x1010;
    const OVERLAY: isize = 0x2020;
    const OTHER: isize = 0x3030;

    #[test]
    fn hands_the_foreground_back_from_the_overlay() {
        assert!(should_give_back(APP, Some(OVERLAY), OVERLAY));
    }

    #[test]
    fn leaves_an_app_the_user_picked_meanwhile_in_front() {
        // The user closed the menu by clicking into another app.
        assert!(!should_give_back(APP, Some(OTHER), OVERLAY));
        // The app came back to the front by itself.
        assert!(!should_give_back(APP, Some(APP), OVERLAY));
        // Nothing is in front (the desktop is switching).
        assert!(!should_give_back(APP, None, OVERLAY));
    }

    #[test]
    fn never_gives_the_foreground_to_the_overlay() {
        assert!(!should_give_back(OVERLAY, Some(OVERLAY), OVERLAY));
    }

    // The handles below are made up and never in front, so `give_back`
    // stops at `should_give_back` and no real window is touched.

    #[test]
    fn a_handback_holds_nothing_until_armed() {
        let handback = Handback::default();
        assert!(!handback.is_armed());
        assert!(!handback.settle(false));
    }

    #[test]
    fn an_item_that_opens_a_window_drops_the_handback() {
        let handback = Handback::default();
        handback.arm(Foreground(APP), OVERLAY);
        assert!(handback.is_armed());
        assert!(!handback.settle(true));
        assert!(!handback.is_armed());
    }

    #[test]
    fn a_handback_settles_once() {
        // The menu event can reach the handler twice, and the fallback for
        // a menu closed without a pick always runs after it.
        let handback = Handback::default();
        handback.arm(Foreground(APP), OVERLAY);
        handback.settle(false);
        assert!(!handback.is_armed());
        assert!(!handback.settle(false));
    }

    #[test]
    fn a_new_menu_replaces_a_handback_still_held() {
        let handback = Handback::default();
        handback.arm(Foreground(APP), OVERLAY);
        handback.arm(Foreground(OTHER), OVERLAY);
        assert_eq!(*handback.lock(), Some((Foreground(OTHER), OVERLAY)));
    }
}
