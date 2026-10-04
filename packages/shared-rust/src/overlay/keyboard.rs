//! The keyboard, read straight from the OS.
//!
//! The approval card's Allow hotkey allows only while its whole chord is
//! held ([ADR-0048](../../../../docs/adr/0048-the-approval-card.md)). The
//! hotkey's events can't show that on their own: on Windows, global-hotkey
//! reports the release from a thread that watches only the main key, so
//! letting go of Ctrl or Shift goes unseen, and a lost release would read as
//! a hold. So the hold asks the OS which keys are down. Elsewhere this
//! returns `None`, and the keyboard can't allow.

/// Whether Ctrl, Shift and Y, the keys of
/// [`ALLOW_ACCELERATOR`](crate::approval::ALLOW_ACCELERATOR), are all
/// physically down right now, or `None` when this platform can't tell.
///
/// Y is the virtual key `VK_Y` that global-hotkey registers for `KeyY`, so
/// this reads the same key the hotkey fired on, whatever the layout.
pub fn allow_chord_down() -> Option<bool> {
    imp::allow_chord_down()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_SHIFT, VK_Y,
    };

    fn down(key: VIRTUAL_KEY) -> bool {
        // SAFETY: takes a plain value and touches no memory of ours.
        let state = unsafe { GetAsyncKeyState(key.0 as i32) };
        (state as u16 & 0x8000) != 0
    }

    pub fn allow_chord_down() -> Option<bool> {
        // Reads as up when another desktop (the lock screen, a UAC prompt)
        // has the keyboard, which keeps the hold from allowing.
        Some(down(VK_CONTROL) && down(VK_SHIFT) && down(VK_Y))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn allow_chord_down() -> Option<bool> {
        None
    }
}
