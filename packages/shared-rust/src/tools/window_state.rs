//! `minimize_window` / `maximize_window` / `close_window` — change a
//! window's state. Targets the foreground window when `title` is
//! omitted; otherwise looks up the first visible window whose title
//! contains the substring.
//!
//! `close_window` is the only destructive one — it posts `WM_CLOSE`,
//! which most apps treat as "user clicked the X" (so they'll show a
//! save-changes prompt rather than vanishing). Still wraps under the
//! confirmation modal because the LLM may pick the wrong window and a
//! mistaken close still costs the user.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::tool::{LashonTool, ToolResult};

#[derive(Debug, Clone, Copy)]
enum WindowAction {
    Minimize,
    Maximize,
    Close,
}

impl WindowAction {
    fn name(self) -> &'static str {
        match self {
            WindowAction::Minimize => "minimize_window",
            WindowAction::Maximize => "maximize_window",
            WindowAction::Close => "close_window",
        }
    }
    fn verb_he(self) -> &'static str {
        match self {
            WindowAction::Minimize => "מזערתי",
            WindowAction::Maximize => "הגדלתי",
            WindowAction::Close => "סגרתי",
        }
    }
}

pub struct MinimizeWindow;
pub struct MaximizeWindow;
pub struct CloseWindow;

impl MinimizeWindow {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MinimizeWindow {
    fn default() -> Self {
        Self::new()
    }
}
impl MaximizeWindow {
    pub fn new() -> Self {
        Self
    }
}
impl Default for MaximizeWindow {
    fn default() -> Self {
        Self::new()
    }
}
impl CloseWindow {
    pub fn new() -> Self {
        Self
    }
}
impl Default for CloseWindow {
    fn default() -> Self {
        Self::new()
    }
}

fn parameters_with_optional_title() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {
                "type": "string",
                "description": "Optional case-insensitive substring of the window's title. When omitted, targets the foreground window."
            }
        }
    })
}

fn run<'a>(action: WindowAction, args: &'a Value) -> Result<ToolResult> {
    let title = args.get("title").and_then(|v| v.as_str());
    // A platform-stub error (e.g. on the non-Windows runners that compile
    // this tool but don't implement Win32 window state) becomes a
    // `ToolResult::error` so the dispatcher can feed it back to the LLM,
    // matching the pattern `read_active_window_text` uses. Returning a
    // raw `Err` would surface as a panic in the `is_ok()` tests.
    match apply(action, title) {
        Ok(Some(matched_title)) => Ok(ToolResult {
            content: format!(
                "{} `{matched_title}`",
                match action {
                    WindowAction::Minimize => "minimized",
                    WindowAction::Maximize => "maximized",
                    WindowAction::Close => "closed",
                }
            ),
            display_summary: Some(format!(
                "{} את `{}`",
                action.verb_he(),
                truncate(&matched_title)
            )),
        }),
        Ok(None) => Ok(ToolResult::error(format!(
            "{}: no matching window was found",
            action.name()
        ))),
        Err(e) => Ok(ToolResult::error(format!("{}: {e}", action.name()))),
    }
}

fn truncate(title: &str) -> String {
    if title.chars().count() > 40 {
        let mut s: String = title.chars().take(40).collect();
        s.push('…');
        s
    } else {
        title.to_string()
    }
}

impl LashonTool for MinimizeWindow {
    fn name(&self) -> &str {
        "minimize_window"
    }
    fn description(&self) -> &str {
        "Minimize a window. Targets the foreground window when `title` is \
         omitted; otherwise the first visible window whose title contains \
         the substring (case-insensitive)."
    }
    fn parameters(&self) -> Value {
        parameters_with_optional_title()
    }
    fn execute<'a>(&'a self, args: &'a Value) -> crate::llm::BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move { run(WindowAction::Minimize, args) })
    }
}

impl LashonTool for MaximizeWindow {
    fn name(&self) -> &str {
        "maximize_window"
    }
    fn description(&self) -> &str {
        "Maximize a window (toggle to its largest non-fullscreen size). \
         Same target rules as `minimize_window`."
    }
    fn parameters(&self) -> Value {
        parameters_with_optional_title()
    }
    fn execute<'a>(&'a self, args: &'a Value) -> crate::llm::BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move { run(WindowAction::Maximize, args) })
    }
}

impl LashonTool for CloseWindow {
    fn name(&self) -> &str {
        "close_window"
    }
    fn description(&self) -> &str {
        "Close a window — equivalent to clicking the title-bar X. Apps \
         with unsaved changes will typically prompt the user. Same \
         target rules as `minimize_window`. Requires user confirmation."
    }
    fn parameters(&self) -> Value {
        parameters_with_optional_title()
    }
    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }
    fn execute<'a>(&'a self, args: &'a Value) -> crate::llm::BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move { run(WindowAction::Close, args) })
    }
}

#[cfg(target_os = "windows")]
fn apply(action: WindowAction, title: Option<&str>) -> Result<Option<String>> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, PostMessageW, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, WM_CLOSE,
    };

    let (hwnd, matched_title): (HWND, String) = match title {
        Some(needle) => match find_visible_window(needle)? {
            Some(pair) => pair,
            None => return Ok(None),
        },
        None => {
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                return Ok(None);
            }
            (hwnd, foreground_title(hwnd).unwrap_or_default())
        }
    };
    unsafe {
        match action {
            WindowAction::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            WindowAction::Maximize => {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            WindowAction::Close => {
                PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))
                    .map_err(|e| anyhow!("close_window: PostMessageW: {e}"))?;
            }
        }
    }
    Ok(Some(matched_title))
}

#[cfg(target_os = "windows")]
fn foreground_title(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf: Vec<u16> = vec![0u16; (len as usize) + 1];
        let read = GetWindowTextW(hwnd, &mut buf);
        if read <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..read as usize]))
    }
}

#[cfg(target_os = "windows")]
fn find_visible_window(needle: &str) -> Result<Option<(windows::Win32::Foundation::HWND, String)>> {
    use std::sync::Mutex;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    struct State {
        needle: String,
        found: Option<(HWND, String)>,
    }
    let state = Mutex::new(State {
        needle: needle.to_lowercase(),
        found: None,
    });

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &*(lparam.0 as *const Mutex<State>) };
        let Ok(mut state) = state.lock() else {
            return BOOL(1);
        };
        if state.found.is_some() {
            return BOOL(0);
        }
        let visible = unsafe { IsWindowVisible(hwnd) };
        if !visible.as_bool() {
            return BOOL(1);
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len <= 0 {
            return BOOL(1);
        }
        let mut buf: Vec<u16> = vec![0u16; (len as usize) + 1];
        let read = unsafe { GetWindowTextW(hwnd, &mut buf) };
        if read <= 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..read as usize]);
        if title.to_lowercase().contains(&state.needle) {
            state.found = Some((hwnd, title));
            return BOOL(0);
        }
        BOOL(1)
    }

    let lparam = LPARAM(&state as *const Mutex<State> as isize);
    unsafe {
        let _ = EnumWindows(Some(callback), lparam);
    }
    let state = state
        .lock()
        .map_err(|e| anyhow!("find_visible_window: state lock poisoned: {e}"))?;
    Ok(state.found.clone())
}

#[cfg(not(target_os = "windows"))]
fn apply(action: WindowAction, _title: Option<&str>) -> Result<Option<String>> {
    Err(anyhow!("{}: not yet implemented on this OS", action.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(MinimizeWindow.name(), "minimize_window");
        assert_eq!(MaximizeWindow.name(), "maximize_window");
        assert_eq!(CloseWindow.name(), "close_window");
        assert!(!MinimizeWindow.requires_confirmation(&json!({})));
        assert!(!MaximizeWindow.requires_confirmation(&json!({})));
        assert!(CloseWindow.requires_confirmation(&json!({})));
    }

    #[test]
    fn parameters_have_optional_title() {
        let p = MinimizeWindow.parameters();
        assert_eq!(p["type"], "object");
        // No `required` field — `title` is optional.
        assert!(p.get("required").is_none());
        assert_eq!(p["properties"]["title"]["type"], "string");
    }

    #[test]
    fn extra_args_pass_through() {
        // The model occasionally emits unused keys; we must not error.
        let result = rt().block_on(MinimizeWindow.execute(&json!({"junk": 1})));
        assert!(result.is_ok());
    }
}
