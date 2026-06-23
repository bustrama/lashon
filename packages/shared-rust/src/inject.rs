//! System-wide text injection — places transcribed text at the cursor.
//!
//! Hebrew must always be injected via the clipboard, never typed per-codepoint
//! (docs/roadmap.md §1.6). M0–M2 take the clipboard path for all text; the
//! UIA / direct-type fast path for Latin text is a later refinement.

use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// How long to wait after pasting before restoring the clipboard, so the
/// target app has consumed the paste (docs/roadmap.md §1.6).
const RESTORE_DELAY: Duration = Duration::from_millis(250);

/// True if `text` contains Hebrew, including presentation forms — the
/// docs/roadmap.md §1.6 codepoint ranges U+0590–U+05FF and U+FB1D–U+FB4F.
pub fn contains_hebrew(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{0590}'..='\u{05FF}' | '\u{FB1D}'..='\u{FB4F}'))
}

/// A snapshot of the clipboard, taken before injection and restored after.
///
/// `arboard` can read and write text and images; a clipboard holding anything
/// else — a file list, say — is `Other`, which it cannot capture, so injection
/// leaves the transcript in its place.
enum ClipboardSnapshot {
    Text(String),
    Image(arboard::ImageData<'static>),
    Other,
}

impl ClipboardSnapshot {
    /// Capture whatever the clipboard currently holds.
    fn capture(clipboard: &mut arboard::Clipboard) -> Self {
        if let Ok(text) = clipboard.get_text() {
            Self::Text(text)
        } else if let Ok(image) = clipboard.get_image() {
            Self::Image(image)
        } else {
            Self::Other
        }
    }

    /// Put the captured contents back. Best-effort: the paste has already
    /// landed, so a restore failure must not fail the injection.
    fn restore(self, clipboard: &mut arboard::Clipboard) {
        let _ = match self {
            Self::Text(text) => clipboard.set_text(text),
            Self::Image(image) => clipboard.set_image(image),
            Self::Other => Ok(()),
        };
    }
}

/// Inject `text` at the current cursor position via the clipboard: snapshot the
/// clipboard, set the text, synthesize the paste shortcut, then restore the
/// original clipboard contents — text or image alike (docs/roadmap.md §1.6).
///
/// Blocks briefly (see `RESTORE_DELAY`); call it off the UI thread.
pub fn inject_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let mut clipboard = arboard::Clipboard::new().context("opening the clipboard")?;
    let saved = ClipboardSnapshot::capture(&mut clipboard);

    clipboard
        .set_text(text)
        .context("writing the transcript to the clipboard")?;
    paste()?;

    thread::sleep(RESTORE_DELAY);
    saved.restore(&mut clipboard);
    Ok(())
}

/// Synthesize the platform paste shortcut — Ctrl+V, or Cmd+V on macOS.
fn paste() -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).context("initialising input synthesis")?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    // On Windows, send `VK_V` directly (`Key::V`) instead of going through
    // `Key::Unicode('v')`. The Unicode path calls `VkKeyScan('v')` which
    // returns -1 under a Hebrew keyboard layout — the physical V key
    // produces ה there, no key maps to 'v'. enigo then falls back to
    // `KEYEVENTF_UNICODE`, which emits `WM_CHAR 'v'` rather than
    // `WM_KEYDOWN VK_V`. Most apps interpret Ctrl+V via the keydown
    // path, so `WM_CHAR` while Ctrl is held does not trigger paste and
    // the transcript just sits on the clipboard. `Key::V` is a
    // Windows-only enigo variant that bypasses the scan-code lookup;
    // macOS and Linux take the Unicode path safely.
    #[cfg(target_os = "windows")]
    let v_key = Key::V;
    #[cfg(not(target_os = "windows"))]
    let v_key = Key::Unicode('v');

    enigo
        .key(modifier, Direction::Press)
        .context("paste: modifier down")?;
    enigo
        .key(v_key, Direction::Click)
        .context("paste: v")?;
    enigo
        .key(modifier, Direction::Release)
        .context("paste: modifier up")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hebrew_including_mixed_text() {
        assert!(contains_hebrew("שלום"));
        assert!(contains_hebrew("יש לי meeting מחר"));
    }

    #[test]
    fn ignores_text_without_hebrew() {
        assert!(!contains_hebrew("hello world"));
        assert!(!contains_hebrew("12345 !?."));
        assert!(!contains_hebrew(""));
    }
}
