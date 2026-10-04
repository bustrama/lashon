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
//! must not have the front handed to the user's app first: that would flash
//! the app, and the new window would then have to take the front from
//! another process, which Windows may refuse. [`Handback`] holds the
//! give-back until the item picked is known, so the app can run such an
//! item first and give the front back only if the overlay still holds it.

use std::sync::Mutex;

/// The window that was in front before the overlay took the front (to open
/// its menu, say).
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
            tracing::warn!("could not give the foreground back to the window that had it");
        }
        given
    }
}

/// A give-back waiting for the overlay menu's outcome.
///
/// [`arm`](Self::arm) it before the menu opens. Once the item picked is
/// known, [`settle`](Self::settle) it exactly once; later calls find nothing
/// to do. A late settlement for one menu uses [`settle_if`](Self::settle_if),
/// so it can't settle a menu opened since.
#[derive(Debug, Default)]
pub struct Handback(Mutex<Held>);

#[derive(Debug, Default)]
struct Held {
    /// The last menu armed: menus are numbered from 1.
    menus: u64,
    pending: Option<Pending>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    menu: u64,
    previous: Foreground,
    overlay: isize,
}

impl Handback {
    /// Hold a give-back to `previous` from the overlay, whose native handle
    /// is `overlay`, replacing any still held. Returns the menu's number,
    /// for [`settle_if`](Self::settle_if).
    pub fn arm(&self, previous: Foreground, overlay: isize) -> u64 {
        let mut held = self.lock();
        held.menus += 1;
        let menu = held.menus;
        held.pending = Some(Pending {
            menu,
            previous,
            overlay,
        });
        menu
    }

    /// Settle the give-back held, if any: drop it when the item picked
    /// keeps the front (it opens a window), and give the front back
    /// otherwise. Returns whether the front was given back.
    pub fn settle(&self, keep_front: bool) -> bool {
        let pending = self.lock().pending.take();
        Self::carry_out(pending, keep_front)
    }

    /// [`settle`](Self::settle), if the give-back held is menu `menu`'s.
    /// Another menu's stays held.
    pub fn settle_if(&self, menu: u64, keep_front: bool) -> bool {
        let pending = {
            let mut held = self.lock();
            match held.pending {
                Some(pending) if pending.menu == menu => held.pending.take(),
                _ => None,
            }
        };
        Self::carry_out(pending, keep_front)
    }

    /// Whether a give-back is held.
    pub fn is_armed(&self) -> bool {
        self.lock().pending.is_some()
    }

    /// Give the front back for `pending`, outside the lock.
    fn carry_out(pending: Option<Pending>, keep_front: bool) -> bool {
        match pending {
            Some(p) if !keep_front => p.previous.give_back(p.overlay),
            _ => false,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Held> {
        // The value is plain data, always whole: a panic elsewhere while the
        // lock was held leaves nothing half-written.
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
        // The fallback for a menu closed without a pick runs after the
        // handler for an item picked, and must then find nothing to do.
        let handback = Handback::default();
        handback.arm(Foreground(APP), OVERLAY);
        handback.settle(false);
        assert!(!handback.is_armed());
        assert!(!handback.settle(false));
    }

    #[test]
    fn a_new_menu_replaces_a_handback_still_held() {
        let handback = Handback::default();
        let first = handback.arm(Foreground(APP), OVERLAY);
        let second = handback.arm(Foreground(OTHER), OVERLAY);
        assert!(second > first);
        assert_eq!(
            handback.lock().pending,
            Some(Pending {
                menu: second,
                previous: Foreground(OTHER),
                overlay: OVERLAY
            })
        );
    }

    #[test]
    fn a_late_settlement_leaves_a_newer_menu_alone() {
        // The first menu closed without a pick; before its fallback ran,
        // the user opened a second one.
        let handback = Handback::default();
        let first = handback.arm(Foreground(APP), OVERLAY);
        let second = handback.arm(Foreground(APP), OVERLAY);
        assert!(!handback.settle_if(first, false));
        assert!(handback.is_armed());
        // The second menu's own settlement still finds it.
        handback.settle_if(second, false);
        assert!(!handback.is_armed());
    }

    #[test]
    fn a_settlement_for_its_own_menu_settles_it() {
        let handback = Handback::default();
        let menu = handback.arm(Foreground(APP), OVERLAY);
        handback.settle_if(menu, true);
        assert!(!handback.is_armed());
        // Already settled by the item picked: the fallback finds nothing.
        let menu = handback.arm(Foreground(APP), OVERLAY);
        handback.settle(false);
        assert!(!handback.settle_if(menu, false));
    }
}
