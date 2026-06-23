//! `focus_window` — bring a window with a matching title to the front.
//!
//! Windows: `EnumWindows` + `GetWindowTextW`, substring-match (case
//! insensitive) on the title, then `SetForegroundWindow` on the first
//! hit. The LLM's typical chain is `open_app("spotify")` → wait →
//! `focus_window("spotify")` → `press_keys("Ctrl+L")` → `type_text(...)`.
//!
//! macOS / Linux stub the same as `open_app`.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct FocusWindow;

impl FocusWindow {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FocusWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for FocusWindow {
    fn name(&self) -> &str {
        "focus_window"
    }

    fn description(&self) -> &str {
        "Bring a window with a matching title to the front. The match is \
         case-insensitive substring. Use after `open_app` when the LLM \
         wants to type into a freshly-launched app."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "A substring of the window's title — e.g. `spotify`, `chrome`, `notepad`. Case-insensitive."
                }
            },
            "required": ["title"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("focus_window: missing required `title` argument"))?;
            if focus(title)? {
                Ok(ToolResult {
                    content: format!("focused window matching `{title}`"),
                    display_summary: Some(format!("התמקדתי בחלון {title}")),
                })
            } else {
                Ok(ToolResult::error(format!(
                    "no window with title containing `{title}` is open"
                )))
            }
        })
    }
}

/// Try to bring an existing window matching `title_substring` to the
/// front. Returns `Ok(true)` when one was found and focused. Exposed
/// to other tools (notably `open_app`, which calls it first to skip
/// re-launching an app that is already open) — keep the signature
/// stable.
#[cfg(target_os = "windows")]
pub(crate) fn try_focus(title_substring: &str) -> Result<bool> {
    focus(title_substring)
}

/// Same on non-Windows: a stub that returns `Ok(false)` so callers
/// (open_app) can fall through to the launch path. Doesn't error,
/// since the caller may have a perfectly good fallback.
#[cfg(not(target_os = "windows"))]
pub(crate) fn try_focus(_title_substring: &str) -> Result<bool> {
    Ok(false)
}

/// Returns `true` when a matching window was found and brought to the
/// front; `false` when nothing matched (the LLM can suggest the user
/// open the app first, or try a different title).
#[cfg(target_os = "windows")]
fn focus(title_substring: &str) -> Result<bool> {
    use std::sync::Mutex;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible, SetForegroundWindow,
    };
    const TRUE: BOOL = BOOL(1);
    const FALSE: BOOL = BOOL(0);

    struct State {
        needle: String,
        match_hwnd: Option<HWND>,
    }
    let state = Mutex::new(State {
        needle: title_substring.to_lowercase(),
        match_hwnd: None,
    });

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` was set by `EnumWindows` from a `&Mutex<State>`
        // owned by the calling stack frame; the closure lives for the
        // entire `EnumWindows` call.
        let state = unsafe { &*(lparam.0 as *const Mutex<State>) };
        let Ok(mut state) = state.lock() else {
            return TRUE;
        };
        // Already found one — stop walking. EnumWindows respects FALSE.
        if state.match_hwnd.is_some() {
            return FALSE;
        }
        // Skip windows that have no on-screen presence.
        let visible = unsafe { IsWindowVisible(hwnd) };
        if !visible.as_bool() {
            return TRUE;
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return TRUE;
        }
        let mut buf: Vec<u16> = vec![0u16; (len as usize) + 1];
        let read = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if read <= 0 {
            return TRUE;
        }
        let title = String::from_utf16_lossy(&buf[..read as usize]).to_lowercase();
        if title.contains(&state.needle) {
            state.match_hwnd = Some(hwnd);
            return FALSE; // stop
        }
        TRUE
    }

    let lparam = LPARAM(&state as *const Mutex<State> as isize);
    unsafe {
        // EnumWindows returns Err(_) when our callback returns FALSE to
        // signal the early-exit path; that is not an error for us.
        let _ = EnumWindows(Some(callback), lparam);
    }
    let hwnd = state
        .lock()
        .map(|s| s.match_hwnd)
        .map_err(|e| anyhow!("focus_window: state lock poisoned: {e}"))?;
    let Some(hwnd) = hwnd else { return Ok(false) };
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(true)
}

#[cfg(not(target_os = "windows"))]
fn focus(_title_substring: &str) -> Result<bool> {
    Err(anyhow!(
        "focus_window: not yet implemented on this OS. The Phase-1 \
         tool supports Windows; macOS AXUIElement and Linux wmctrl land in M8.2."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = FocusWindow::new();
        assert_eq!(tool.name(), "focus_window");
        assert!(!tool.requires_confirmation(&json!({"title": "spotify"})));
    }

    #[test]
    fn parameters_describe_title_argument() {
        let params = FocusWindow.parameters();
        assert_eq!(params["properties"]["title"]["type"], "string");
        assert_eq!(params["required"][0], "title");
    }

    #[test]
    fn missing_title_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(FocusWindow.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("title"));
    }
}
