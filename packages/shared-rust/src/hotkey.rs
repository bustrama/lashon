//! Validation for global-shortcut accelerator strings.
//!
//! The Settings Hub lets the user rebind Lashon's dictation hotkey; the chord
//! it captures becomes a Tauri accelerator string such as `Control+Space`.
//! Deciding whether such a string is acceptable is real logic, so it lives
//! here in `lashon-core` with its tests rather than in the GUI shell — the
//! Tauri crate only wraps `validate_accelerator` in a command.

/// Why an accelerator string was rejected. Each variant has a stable `code`
/// the Settings Hub maps to a localized message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyError {
    /// No chord at all.
    Empty,
    /// A chord with no modifier (e.g. a bare `Space`). A global shortcut with
    /// no modifier swallows that key in every application.
    NoModifier,
    /// A chord with modifiers but no ordinary key (e.g. `Control+Shift`).
    NoKey,
    /// A non-final token is not a recognised modifier — a malformed chord.
    Malformed,
    /// An OS-reserved chord the operating system intercepts before any app, so
    /// binding it would simply never fire.
    Reserved,
}

impl HotkeyError {
    /// A stable, machine-readable reason code. The Hub localizes it via the
    /// `hub.shortcuts.invalid.<code>` catalog keys.
    pub fn code(self) -> &'static str {
        match self {
            HotkeyError::Empty => "empty",
            HotkeyError::NoModifier => "no-modifier",
            HotkeyError::NoKey => "no-key",
            HotkeyError::Malformed => "malformed",
            HotkeyError::Reserved => "reserved",
        }
    }
}

/// OS-reserved chords, as canonical signatures (see `canonical_signature`).
/// Kept deliberately small — only chords the OS itself intercepts, so binding
/// them could never work: the Windows lock-screen and secure-attention chords.
const RESERVED: &[&str] = &["super+l", "alt+ctrl+delete"];

/// Map a modifier token (any accepted spelling) to its canonical name, or
/// `None` if the token is not a modifier. `CommandOrControl` collapses to
/// `ctrl`: on Windows — Lashon's target — it resolves to Control.
fn canonical_modifier(token: &str) -> Option<&'static str> {
    match token.to_ascii_lowercase().as_str() {
        "control" | "ctrl" | "commandorcontrol" | "cmdorctrl" => Some("ctrl"),
        "alt" | "option" | "altgr" => Some("alt"),
        "shift" => Some("shift"),
        "super" | "meta" | "command" | "cmd" => Some("super"),
        _ => None,
    }
}

/// A canonical signature for a chord — sorted canonical modifiers and a
/// lowercased key — so equivalent spellings (`Ctrl+Alt+Del`,
/// `Alt+Control+Delete`) compare equal against `RESERVED`.
fn canonical_signature(modifiers: &[&str], key: &str) -> String {
    let mut canon: Vec<&'static str> = modifiers
        .iter()
        .filter_map(|m| canonical_modifier(m))
        .collect();
    canon.sort_unstable();
    canon.dedup();
    let key = key.to_ascii_lowercase();
    let key = if key == "del" { "delete" } else { key.as_str() };
    format!("{}+{}", canon.join("+"), key)
}

/// Validate a Tauri global-shortcut accelerator string.
///
/// A chord is acceptable when it has at least one modifier, exactly one
/// ordinary key, and is not OS-reserved. The function does not check that the
/// key name is one Tauri can register — a genuinely unknown key fails loudly
/// at `register()` time; this is the policy gate, not the parser.
pub fn validate_accelerator(accelerator: &str) -> Result<(), HotkeyError> {
    let tokens: Vec<&str> = accelerator
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();

    let Some((key, modifiers)) = tokens.split_last() else {
        return Err(HotkeyError::Empty);
    };
    if canonical_modifier(key).is_some() {
        // The final token is itself a modifier — there is no ordinary key.
        return Err(HotkeyError::NoKey);
    }
    if modifiers.is_empty() {
        return Err(HotkeyError::NoModifier);
    }
    if !modifiers.iter().all(|m| canonical_modifier(m).is_some()) {
        return Err(HotkeyError::Malformed);
    }
    if RESERVED.contains(&canonical_signature(modifiers, key).as_str()) {
        return Err(HotkeyError::Reserved);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_modified_key() {
        assert!(validate_accelerator("Control+Space").is_ok());
        assert!(validate_accelerator("CommandOrControl+Shift+D").is_ok());
        assert!(validate_accelerator("Alt+F1").is_ok());
        assert!(validate_accelerator("Super+Shift+Backslash").is_ok());
    }

    #[test]
    fn tolerates_whitespace_and_casing() {
        assert!(validate_accelerator("  control + space  ").is_ok());
        assert!(validate_accelerator("CONTROL+SPACE").is_ok());
    }

    #[test]
    fn rejects_an_empty_chord() {
        assert_eq!(validate_accelerator(""), Err(HotkeyError::Empty));
        assert_eq!(validate_accelerator("   "), Err(HotkeyError::Empty));
        assert_eq!(validate_accelerator("+"), Err(HotkeyError::Empty));
    }

    #[test]
    fn rejects_a_chord_without_a_modifier() {
        assert_eq!(validate_accelerator("Space"), Err(HotkeyError::NoModifier));
        assert_eq!(validate_accelerator("F5"), Err(HotkeyError::NoModifier));
    }

    #[test]
    fn rejects_modifiers_without_a_key() {
        assert_eq!(validate_accelerator("Control"), Err(HotkeyError::NoKey));
        assert_eq!(
            validate_accelerator("Control+Shift"),
            Err(HotkeyError::NoKey)
        );
        assert_eq!(validate_accelerator("Alt+Super"), Err(HotkeyError::NoKey));
    }

    #[test]
    fn rejects_a_malformed_chord() {
        assert_eq!(
            validate_accelerator("Control+Nonsense+D"),
            Err(HotkeyError::Malformed)
        );
    }

    #[test]
    fn rejects_os_reserved_chords() {
        assert_eq!(validate_accelerator("Super+L"), Err(HotkeyError::Reserved));
        assert_eq!(
            validate_accelerator("Control+Alt+Delete"),
            Err(HotkeyError::Reserved)
        );
        // Equivalent spellings normalize to the same reserved signature.
        assert_eq!(
            validate_accelerator("Alt+Ctrl+Del"),
            Err(HotkeyError::Reserved)
        );
        assert_eq!(validate_accelerator("Meta+L"), Err(HotkeyError::Reserved));
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(HotkeyError::Empty.code(), "empty");
        assert_eq!(HotkeyError::NoModifier.code(), "no-modifier");
        assert_eq!(HotkeyError::NoKey.code(), "no-key");
        assert_eq!(HotkeyError::Malformed.code(), "malformed");
        assert_eq!(HotkeyError::Reserved.code(), "reserved");
    }
}
