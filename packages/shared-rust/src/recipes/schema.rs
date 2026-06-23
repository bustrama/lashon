//! Recipe schema — Rust types that double as the JSON Schema source.
//!
//! Every type derives `serde::Serialize` + `serde::Deserialize` (parse + write
//! back) and `schemars::JsonSchema` (auto-generated JSON Schema export).
//! `validate::validate_recipe` layers semantic checks on top of what serde
//! enforces structurally (parameter references, OS coverage, permission
//! declarations).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Recipe-schema version. v1 is the format documented in
/// `docs/stories/m9-recipes.md`; a future v2 would bump this and the
/// runtime would dispatch on the field.
pub const SCHEMA_VERSION: u32 = 1;

/// A `recipe.yaml`. Top-level fields are split between identity (the
/// Agent-Skills-shaped envelope) and behaviour (Goose-shaped
/// `parameters:` + Lashon-shaped `os_steps:`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    /// Recipe-schema version. Must be [`SCHEMA_VERSION`] for v1 recipes.
    pub version: u32,
    /// Stable kebab-case id — used as the on-disk directory name, the
    /// import key, and the cascade-match key. Validated to match
    /// `[a-z][a-z0-9-]*`.
    pub id: String,
    /// Human-readable name shown in the Hub Recipes browser.
    pub name: String,
    /// One-line description of what the recipe does. Lives in both the
    /// browser list and the LLM intent classifier's prompt
    /// (`recipes::intent`, Phase 1c).
    pub description: String,
    /// Optional longer description (markdown). Shown on hover / in the
    /// recipe detail pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_description: Option<String>,
    /// Recipe author / maintainer. Free-text; not parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Recipe semantic version, independent of [`SCHEMA_VERSION`]. The
    /// Hub uses it to detect "newer than installed" upgrades for
    /// bundled recipes. Defaults to `"1.0.0"`.
    #[serde(default = "default_recipe_version")]
    pub recipe_version: String,
    /// Tags the Hub groups by (`messaging`, `media`, `productivity`, …).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Natural-language phrases the intent cascade matches against.
    /// `{param}` placeholders are substituted with extracted slot values.
    /// Both Hebrew and English entries are conventional.
    #[serde(default)]
    pub intents: Vec<String>,
    /// Goose-compatible parameter declarations. Order is preserved so
    /// the Hub slot-fill modal can render fields in author intent.
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    /// Declared permissions. Descriptive in v1 — the runtime does not
    /// reject a step that exceeds them. Phase M11+ may enforce. Known
    /// values: `keyboard.type`, `app.focus`, `app.open`, `clipboard`,
    /// `screenshot`, `file.write`, `shell.run`, `network`, `destructive`.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Per-OS step list. At least one variant must be populated; the
    /// runtime selects the host-OS variant at execution time.
    pub os_steps: OsSteps,
}

/// Goose-style parameter declaration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    /// Slot name. Validated to match `[a-z][a-z0-9_]*` and to be
    /// referenced (via `{{ key }}`) somewhere in the step list.
    pub key: String,
    /// Value shape, used by the Hub slot-fill modal to pick a control.
    pub input_type: ParameterType,
    /// Whether the slot must be filled, may be left empty, or always
    /// prompts the user even when a default is set.
    pub requirement: ParameterRequirement,
    /// Human-readable description. Shown in the Hub modal as the
    /// field label hint.
    pub description: String,
    /// Optional default value. Type must be compatible with
    /// `input_type`; the validator does not enforce this in v1 (the
    /// runtime coerces at slot-fill time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// Parameter value shape. Mirrors the Goose enum verbatim — extending
/// it is a separate spec change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParameterType {
    String,
    Number,
    Boolean,
    File,
    Date,
}

/// Whether the slot is required, optional, or always-prompt.
/// `UserPrompt` is the "ignore any default; ask every run" mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParameterRequirement {
    Required,
    Optional,
    UserPrompt,
}

/// Per-OS step variants. At least one of `windows` / `macos` / `linux`
/// must be `Some(_)` and non-empty. v1 runtime only honours `windows`
/// (per ADR-0028 scope), but the other two slots are spec'd so authors
/// can declare them now and the runtime gains coverage later.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OsSteps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<Step>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<Vec<Step>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<Step>>,
}

/// A single OS-UI primitive. The `type:` discriminator carries the
/// variant tag in YAML.
///
/// Lashon-specific design notes:
/// - `rtl_safe: true` on a `type_unicode` step routes via the clipboard
///   path instead of synthetic keypresses — Electron apps mangle
///   synthetic BiDi otherwise (`.claude/rules/hebrew.md`).
/// - `click_label.ocr_fallback: true` declares intent; Phase 1 doesn't
///   ship the OCR adapter, Phase 2 does. The validator accepts the
///   flag now so authors don't have to bump the schema later.
/// - `run_shell` always requires the `shell.run` permission and gates
///   on the same confirmation modal as the Command-mode `run_command`
///   tool. v1 honours `timeout_ms` and `capture_into`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Press a key chord. `keys` is the chord in order — modifiers
    /// first, target last. Common chords:
    /// `["Control", "K"]`, `["Control", "Shift", "Enter"]`.
    KeyChord {
        keys: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Type Unicode text, with `{{ param }}` interpolation.
    /// Set `rtl_safe: true` for Hebrew + Electron — routes through the
    /// clipboard so the target app sees a `WM_PASTE`, not synthetic
    /// keypresses.
    TypeUnicode {
        text: String,
        #[serde(default)]
        rtl_safe: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Click a UI element by its accessibility label (UIA on Windows).
    /// `window` narrows the search to a single window when set.
    /// `ocr_fallback: true` signals authors' intent; Phase 2 wires
    /// the actual OCR adapter.
    ClickLabel {
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
        #[serde(default)]
        ocr_fallback: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Focus the first window whose title contains `title_contains`.
    /// `process` narrows the match to a specific executable when set —
    /// useful when multiple apps' windows share a substring.
    FocusWindow {
        title_contains: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        process: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Block until a window matching `title_contains` exists, or
    /// timeout. Used to bridge cold-launch latency for Electron apps
    /// where `focus_window` immediately after `open_app` would miss.
    WaitForWindow {
        title_contains: String,
        #[serde(default = "default_window_timeout_ms")]
        timeout_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Sleep for a fixed duration. Use sparingly — prefer
    /// `wait_for_window` for state-driven waits. Authors reach for
    /// this when the target signal is invisible to the OS (an
    /// in-app animation, a network round-trip).
    WaitMs {
        ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Wait until keyboard focus moves to a *different* UIA element.
    /// Snapshots the currently-focused element's runtime id when the
    /// step starts; polls every 50 ms until the focused element has
    /// a different id, or errors on timeout.
    ///
    /// The cheap state-driven companion to `wait_ms` for **native
    /// Win32 apps** (Notepad, File Explorer, settings dialogs, native
    /// installers) where focus genuinely tracks across discrete
    /// controls. After a key chord that opens a modal, the modal's
    /// input typically receives focus within ~50 ms and the poll
    /// returns instantly.
    ///
    /// **Electron apps don't benefit.** Chrome/Electron expose their
    /// entire WebView as a single UIA "chrome window" element —
    /// `GetFocusedElement` returns the same runtime id whether
    /// focus is on the chat sidebar or a freshly-opened modal's
    /// input. Discord, Slack, Telegram Desktop, WhatsApp, VS Code,
    /// Cursor, Notion, etc. all hit this. Use `wait_ms` for those.
    ///
    /// Also doesn't help when focus stays put — typing into a
    /// search box debounces a list filter but keeps focus on the
    /// input; for that, keep `wait_ms`.
    WaitForFocusChange {
        #[serde(default = "default_focus_change_timeout_ms")]
        timeout_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Take a screenshot and place it on the clipboard. `region` of
    /// `None` captures the foreground window; a `Region` captures
    /// just that rectangle.
    ScreenshotToClipboard {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<Region>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Set the clipboard to a (possibly interpolated) text value.
    ClipboardSet {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Read the current clipboard contents into a recipe-local
    /// variable named `var`. Later steps reference it via
    /// `{{ var }}`, same as parameter slots.
    ClipboardGetInto {
        var: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Run a shell command. Requires the `shell.run` permission;
    /// the confirmation modal gates execution at runtime. The
    /// command string is interpolated.
    ///
    /// `dry_run: true` skips actual execution — the runtime logs the
    /// interpolated command and binds `capture_into` (if set) to a
    /// "(dry-run)" placeholder. Designed for recipe authors testing a
    /// new shell step without side effects; the Hub Steps panel shows
    /// a small rose-italic "dry-run בלבד · no changes" annotation so
    /// the user can see at a glance the command won't actually fire.
    RunShell {
        command: String,
        #[serde(default = "default_shell_timeout_ms")]
        timeout_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capture_into: Option<String>,
        /// When `true`, the runtime does NOT actually run the command;
        /// it just records what *would* have run. Default `false`.
        #[serde(default)]
        dry_run: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Open a URL in the user's default browser.
    OpenUrl {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },

    /// Launch an application by name (Windows: the start-menu name or
    /// path; macOS: bundle id or app name; Linux: desktop-file name).
    OpenApp {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment: Option<String>,
    },
}

/// Pixel rectangle for `screenshot_to_clipboard` region capture.
/// Coordinates are physical pixels on the primary display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

fn default_recipe_version() -> String {
    "1.0.0".to_string()
}

fn default_window_timeout_ms() -> u32 {
    5_000
}

fn default_shell_timeout_ms() -> u32 {
    30_000
}

/// 2 s is the right default for `wait_for_focus_change`: most modal /
/// menu / contextual-input transitions land in well under 200 ms;
/// 2 s catches a cold Electron app without burning real time on the
/// happy path. Authors can override per step (`timeout_ms: 5000` for
/// a known-slow app, etc.).
fn default_focus_change_timeout_ms() -> u32 {
    2_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smallest legal recipe — proves the structural defaults work.
    #[test]
    fn minimal_recipe_parses() {
        let yaml = r#"
version: 1
id: minimal
name: Minimal recipe
description: The smallest legal recipe — one focus step on Windows.
os_steps:
  windows:
    - type: focus_window
      title_contains: Notepad
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).expect("minimal recipe must parse");
        assert_eq!(recipe.version, SCHEMA_VERSION);
        assert_eq!(recipe.id, "minimal");
        assert_eq!(recipe.recipe_version, "1.0.0");
        assert!(recipe.parameters.is_empty());
        assert!(recipe.permissions.is_empty());
        let windows = recipe.os_steps.windows.expect("windows variant present");
        assert_eq!(windows.len(), 1);
        match &windows[0] {
            Step::FocusWindow { title_contains, .. } => assert_eq!(title_contains, "Notepad"),
            other => panic!("expected FocusWindow, got {other:?}"),
        }
    }

    /// Every step variant deserialises from its tagged form. Exhaustive
    /// — if a new variant is added without a test row here, this fails.
    #[test]
    fn every_step_variant_parses() {
        let yaml = r#"
version: 1
id: kitchen-sink
name: Every step variant
description: Exercises every step tag the schema spec'd.
parameters:
  - key: recipient
    input_type: string
    requirement: required
    description: Whoever
  - key: body
    input_type: string
    requirement: required
    description: The body
permissions:
  - keyboard.type
  - app.focus
  - shell.run
  - destructive
os_steps:
  windows:
    - type: key_chord
      keys: [Control, K]
    - type: type_unicode
      text: "{{ recipient }}"
      rtl_safe: true
    - type: click_label
      label: Send
      window: Discord
      ocr_fallback: true
    - type: focus_window
      title_contains: Discord
      process: Discord.exe
    - type: wait_for_window
      title_contains: Discord
      timeout_ms: 8000
    - type: wait_ms
      ms: 250
    - type: screenshot_to_clipboard
      region: { x: 10, y: 20, width: 100, height: 200 }
    - type: clipboard_set
      text: "hi"
    - type: clipboard_get_into
      var: stash
    - type: run_shell
      command: "echo {{ body }}"
      timeout_ms: 10000
      capture_into: out
    - type: open_url
      url: https://example.com
    - type: open_app
      name: Notepad
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).expect("kitchen-sink recipe must parse");
        let steps = recipe.os_steps.windows.expect("windows variant");
        assert_eq!(steps.len(), 12, "every variant covered");
    }

    /// `deny_unknown_fields` catches typos in the top-level shape —
    /// a misspelt `descripton:` shouldn't silently drop the description.
    #[test]
    fn unknown_top_level_field_rejected() {
        let yaml = r#"
version: 1
id: typo-trap
name: Typo trap
description: Has an extra field.
extra_field: oops
os_steps:
  windows: []
"#;
        let err = serde_yaml_ng::from_str::<Recipe>(yaml)
            .expect_err("unknown field must reject")
            .to_string();
        assert!(
            err.contains("extra_field") || err.contains("unknown field"),
            "rejection must name the field: {err}"
        );
    }

    /// JSON Schema generation runs without panicking and produces a
    /// schema object naming the recipe at the root.
    #[test]
    fn json_schema_generates() {
        let schema = schemars::schema_for!(Recipe);
        let json = serde_json::to_value(&schema).expect("schema must serialise to JSON");
        assert!(json.is_object(), "schema is a JSON object");
        // The root schema's `title` field comes from the Rust type name
        // by default — `schemars` derives "Recipe".
        let title = json
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        assert_eq!(title, "Recipe");
    }
}
