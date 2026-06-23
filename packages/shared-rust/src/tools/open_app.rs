//! `open_app` — launch an application by name.
//!
//! Windows: shells out to `cmd /c start "" "<name>"`. The `start` command
//! resolves the name against the App Paths registry, the Start Menu, and
//! the user's PATH, so users can say "open VS Code", "open spotify",
//! "open chrome" and get the same behaviour as typing the name into the
//! Run dialog. Bonus: `start` also accepts URI schemes (spotify://,
//! mailto:, etc.), so an LLM that emits `spotify` lands on the Spotify
//! desktop app even when its `.exe` isn't on PATH.
//!
//! macOS / Linux ship as stubs in Phase 1 — they return a clear error
//! the LLM can read back to the user; the real impls (`open -a` on
//! macOS, `.desktop` lookup on Linux) land alongside the M8 cross-platform
//! verification step.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct OpenApp;

impl OpenApp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcome of `launch` — the LLM and the user-visible flash both
/// distinguish "the app is already open and we just focused it" from
/// "we spawned a new instance". The former is more common than I
/// expected — Spotify, Chrome, Discord etc. stay open across user
/// sessions.
enum LaunchOutcome {
    /// A window matching the name was already on the desktop; we
    /// brought it to the front instead of relaunching.
    FocusedExisting,
    /// `cmd /c start` spawned a fresh instance / activated a tray
    /// icon / opened a registered URI handler.
    Launched,
}

impl LashonTool for OpenApp {
    fn name(&self) -> &str {
        "open_app"
    }

    fn description(&self) -> &str {
        "Launch an application by name, or bring it to the front if it \
         is already running. Idempotent — if a window with a matching \
         title is already on the desktop the tool focuses that window \
         instead of starting a second instance. The name can be a \
         short common label (`vscode`, `spotify`, `chrome`, `notepad`, \
         `calc`) or the full executable name (`code.exe`)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The app name. Lowercased common names work — vscode, spotify, chrome, notepad, calc, firefox, slack, discord."
                }
            },
            "required": ["name"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("open_app: missing required `name` argument"))?;
            match launch(name).await? {
                LaunchOutcome::FocusedExisting => Ok(ToolResult {
                    content: format!("focused already-running {name}"),
                    display_summary: Some(format!("התמקדתי ב-{name} (כבר פתוח)")),
                }),
                LaunchOutcome::Launched => Ok(ToolResult {
                    content: format!("launched {name}"),
                    display_summary: Some(format!("פתחתי את {name}")),
                }),
            }
        })
    }
}

#[cfg(target_os = "windows")]
async fn launch(name: &str) -> Result<LaunchOutcome> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    // CREATE_NO_WINDOW so the cmd.exe console flash never appears.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // 1) Already open? Bring it to the front instead of relaunching.
    //    `try_focus` is a `pub(crate)` shim into focus_window's UIA
    //    walk — same case-insensitive substring match. We try both
    //    the user-supplied name (which usually matches the window
    //    title — "spotify" → "Spotify Free") and the resolved alias
    //    when it's a different string (handles "vscode" → "Code").
    if super::focus_window::try_focus(name).unwrap_or(false) {
        return Ok(LaunchOutcome::FocusedExisting);
    }
    let resolved = resolve_alias(name).unwrap_or(name);
    if resolved != name && super::focus_window::try_focus(resolved).unwrap_or(false) {
        return Ok(LaunchOutcome::FocusedExisting);
    }

    // 2) Not open — `cmd /c start "" "<name>"`. The empty `""` is the
    //    window-title placeholder `start` requires when its first arg
    //    is quoted.
    Command::new("cmd")
        .args(["/c", "start", "", resolved])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| anyhow!("open_app: cmd start failed: {e}"))?;
    Ok(LaunchOutcome::Launched)
}

#[cfg(not(target_os = "windows"))]
async fn launch(name: &str) -> Result<LaunchOutcome> {
    // macOS / Linux ship as Phase-1 stubs. The LLM gets a clean error
    // back so it can either tell the user the platform isn't supported
    // yet or pick a different tool.
    Err(anyhow!(
        "open_app: not yet implemented on this OS for `{name}`. The Phase-1 \
         tool supports Windows; macOS `open -a` and Linux `.desktop` \
         lookup land in M8.2."
    ))
}

/// Map common short labels users might say to the names Windows' `start`
/// command actually resolves. Anything not in this table is forwarded
/// verbatim — `start` handles a lot on its own.
#[cfg(target_os = "windows")]
fn resolve_alias(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "vscode" | "vs code" | "code" => "code",
        "chrome" => "chrome",
        "firefox" => "firefox",
        "edge" => "msedge",
        "notepad" => "notepad",
        "calculator" | "calc" => "calc",
        "explorer" | "file explorer" => "explorer",
        "spotify" => "spotify:",
        "slack" => "slack://",
        "discord" => "discord:",
        "telegram" => "tg:",
        "whatsapp" => "whatsapp:",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = OpenApp::new();
        assert_eq!(tool.name(), "open_app");
        assert!(!tool.requires_confirmation(&json!({"name": "vscode"})));
    }

    #[test]
    fn parameters_describe_name_argument() {
        let params = OpenApp.parameters();
        assert_eq!(params["properties"]["name"]["type"], "string");
        assert_eq!(params["required"][0], "name");
    }

    #[test]
    fn missing_name_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(OpenApp.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("name"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn alias_table_handles_common_labels() {
        assert_eq!(resolve_alias("vscode"), Some("code"));
        assert_eq!(resolve_alias("VS Code"), Some("code"));
        assert_eq!(resolve_alias("spotify"), Some("spotify:"));
        assert_eq!(resolve_alias("nonexistent app"), None);
    }
}
