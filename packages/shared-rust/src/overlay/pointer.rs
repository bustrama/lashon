// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: `cursor_physical` and `left_button_down` in
// windows/src-tauri/src/platform/windows.rs. See THIRD-PARTY-NOTICES.

//! The global cursor, read straight from the OS.
//!
//! The cursor poll runs on its own thread at 60 Hz. Reading the cursor
//! through the GUI toolkit costs a round trip to the main thread every tick,
//! and the tick stalls whenever that thread is busy. On Windows the OS
//! answers from any thread, so the poll asks it directly. Elsewhere these
//! return `None` and the caller falls back to the toolkit.

use super::geometry::Point;

/// The cursor in physical screen pixels, or `None` when this platform has no
/// direct read (or the read failed).
///
/// Physical only because the process is per-monitor DPI aware (the GUI
/// toolkit opts in at start-up); a DPI-unaware process would get virtualised
/// coordinates.
pub fn cursor_physical() -> Option<Point> {
    imp::cursor_physical()
}

/// Whether the primary mouse button is held right now, or `None` when this
/// platform can't tell. Respects swapped buttons.
pub fn primary_button_down() -> Option<bool> {
    imp::primary_button_down()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SM_SWAPBUTTON};

    use super::Point;

    pub fn cursor_physical() -> Option<Point> {
        let mut p = POINT::default();
        // SAFETY: `p` is a valid, writable POINT for the duration of the call.
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some(Point::new(p.x as f64, p.y as f64))
    }

    pub fn primary_button_down() -> Option<bool> {
        // SAFETY: both calls take plain values and touch no memory of ours.
        let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
        let key = if swapped { VK_RBUTTON } else { VK_LBUTTON };
        let state = unsafe { GetAsyncKeyState(key.0 as i32) };
        Some((state as u16 & 0x8000) != 0)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Point;

    pub fn cursor_physical() -> Option<Point> {
        None
    }

    pub fn primary_button_down() -> Option<bool> {
        None
    }
}
