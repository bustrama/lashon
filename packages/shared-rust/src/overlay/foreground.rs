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
        should_give_back(self.0, imp::foreground(), overlay) && imp::bring_to_front(self.0)
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
}
