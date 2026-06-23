//! `press_keys` — fire a keyboard chord (`Ctrl+L`, `Enter`, `Alt+F4`).
//! Uses `enigo` (already a Phase-1 dep) and supports the same chord
//! syntax the Hub's hotkey field already validates — `Ctrl+Win+.`, etc.

use anyhow::{anyhow, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct PressKeys;

impl PressKeys {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PressKeys {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for PressKeys {
    fn name(&self) -> &str {
        "press_keys"
    }

    fn description(&self) -> &str {
        "Send a keyboard shortcut to the focused app. Use `+` to separate \
         modifiers and the key — e.g. `Ctrl+L` to focus a browser's URL bar, \
         `Alt+Tab` to switch windows, `Enter` to confirm, `Ctrl+S` to save."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keys": {
                    "type": "string",
                    "description": "The chord, e.g. `Ctrl+L`, `Alt+Tab`, `Enter`, `F1`, `Ctrl+Shift+T`. Modifiers: Ctrl/Control, Alt/Option, Shift, Win/Cmd/Super."
                }
            },
            "required": ["keys"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let chord = args
                .get("keys")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("press_keys: missing required `keys` argument"))?;
            execute_chord(chord)?;
            Ok(ToolResult {
                content: format!("pressed {chord}"),
                display_summary: Some(format!("הקשתי {chord}")),
            })
        })
    }
}

/// Send a keyboard chord (`Ctrl+L`, `Alt+F4`, `Enter`). Shared between
/// the `press_keys` LLM tool (above) and the M9 recipe runtime's
/// `key_chord` step, so a recipe and the dispatcher both press keys
/// the exact same way.
pub fn execute_chord(chord: &str) -> Result<()> {
    let (mods, key) = parse_chord(chord)?;
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("press_keys: cannot open input device: {e}"))?;
    // Press modifiers down → tap the main key → release modifiers.
    // `enigo` is synchronous; the tap completes in microseconds.
    for m in &mods {
        enigo
            .key(*m, Direction::Press)
            .map_err(|e| anyhow!("press_keys: failed to press {m:?}: {e}"))?;
    }
    let key_press_result = enigo.key(key, Direction::Click);
    for m in mods.iter().rev() {
        let _ = enigo.key(*m, Direction::Release);
    }
    key_press_result.map_err(|e| anyhow!("press_keys: chord failed: {e}"))
}

/// Parse a `Ctrl+Win+.` style chord into a list of modifier keys + the
/// final non-modifier key. Single-character keys are wrapped in
/// `Key::Unicode`; named keys (`Enter`, `Tab`, `F5`, …) are mapped to
/// the matching `enigo::Key` variant. Case-insensitive on names.
fn parse_chord(chord: &str) -> Result<(Vec<Key>, Key)> {
    let parts: Vec<&str> = chord.split('+').map(str::trim).collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return Err(anyhow!("press_keys: empty key chord `{chord}`"));
    }
    let (last, init) = parts
        .split_last()
        .ok_or_else(|| anyhow!("press_keys: empty chord"))?;
    let mut mods: Vec<Key> = Vec::with_capacity(init.len());
    for token in init {
        mods.push(modifier_key(token)?);
    }
    let key = main_key(last)?;
    Ok((mods, key))
}

fn modifier_key(token: &str) -> Result<Key> {
    Ok(match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Key::Control,
        "alt" | "option" | "opt" => Key::Alt,
        "shift" => Key::Shift,
        "win" | "cmd" | "meta" | "super" => Key::Meta,
        other => return Err(anyhow!("press_keys: unknown modifier `{other}`")),
    })
}

fn main_key(token: &str) -> Result<Key> {
    Ok(match token.to_ascii_lowercase().as_str() {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "space" => Key::Space,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        // Function keys F1–F12.
        f if f.starts_with('f') && f[1..].parse::<u32>().is_ok() => {
            let n: u32 = f[1..].parse().unwrap();
            match n {
                1 => Key::F1,
                2 => Key::F2,
                3 => Key::F3,
                4 => Key::F4,
                5 => Key::F5,
                6 => Key::F6,
                7 => Key::F7,
                8 => Key::F8,
                9 => Key::F9,
                10 => Key::F10,
                11 => Key::F11,
                12 => Key::F12,
                _ => return Err(anyhow!("press_keys: function key F{n} not supported")),
            }
        }
        single if single.chars().count() == 1 => {
            char_to_key(single.chars().next().expect("len 1"))
        }
        other => return Err(anyhow!("press_keys: unknown key `{other}`")),
    })
}

/// Map a single-character chord key (the `l` in `Ctrl+L`) to an `enigo::Key`.
///
/// On Windows, ASCII letters and digits return the `Key::A`..`Key::Z` /
/// `Key::Num0`..`Key::Num9` variants — these map straight to `VK_*` so the
/// chord synthesises as a real `WM_KEYDOWN`. The default `Key::Unicode`
/// path goes through `VkKeyScan`, which returns -1 for Latin letters under
/// a Hebrew layout (the physical L key types ך) and falls back to
/// `KEYEVENTF_UNICODE` → `WM_CHAR`, so `Ctrl+L`, `Ctrl+K`, `Ctrl+S` … all
/// silently no-op. See `inject::paste()` for the same fix on Ctrl+V.
fn char_to_key(c: char) -> Key {
    #[cfg(target_os = "windows")]
    {
        const ALPHA: [Key; 26] = [
            Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I,
            Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R,
            Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z,
        ];
        const DIGITS: [Key; 10] = [
            Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4,
            Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9,
        ];
        if c.is_ascii_lowercase() {
            return ALPHA[(c as u8 - b'a') as usize];
        }
        if c.is_ascii_digit() {
            return DIGITS[(c as u8 - b'0') as usize];
        }
    }
    Key::Unicode(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_chord() {
        let (mods, key) = parse_chord("Ctrl+L").unwrap();
        assert!(matches!(mods.as_slice(), [Key::Control]));
        // Windows uses the native `VK_L` variant so the chord survives a
        // Hebrew keyboard layout — see `char_to_key` for the rationale.
        #[cfg(target_os = "windows")]
        assert!(matches!(key, Key::L));
        #[cfg(not(target_os = "windows"))]
        assert!(matches!(key, Key::Unicode('l')));
    }

    #[test]
    fn parses_digit_chord() {
        let (_, key) = parse_chord("Ctrl+1").unwrap();
        #[cfg(target_os = "windows")]
        assert!(matches!(key, Key::Num1));
        #[cfg(not(target_os = "windows"))]
        assert!(matches!(key, Key::Unicode('1')));
    }

    #[test]
    fn parses_modifier_aliases() {
        let (mods, _) = parse_chord("Cmd+Option+Shift+S").unwrap();
        assert!(matches!(mods.as_slice(), [Key::Meta, Key::Alt, Key::Shift]));
    }

    #[test]
    fn parses_named_key() {
        let (_, key) = parse_chord("Enter").unwrap();
        assert!(matches!(key, Key::Return));
    }

    #[test]
    fn parses_function_key() {
        let (_, key) = parse_chord("Alt+F4").unwrap();
        assert!(matches!(key, Key::F4));
    }

    #[test]
    fn rejects_empty_chord() {
        assert!(parse_chord("").is_err());
        assert!(parse_chord("Ctrl+").is_err());
    }

    #[test]
    fn rejects_unknown_modifier() {
        assert!(parse_chord("Hyper+A").is_err());
    }

    #[test]
    fn metadata_matches_spec() {
        let tool = PressKeys::new();
        assert_eq!(tool.name(), "press_keys");
        assert!(!tool.requires_confirmation(&json!({"keys": "Ctrl+S"})));
    }
}
