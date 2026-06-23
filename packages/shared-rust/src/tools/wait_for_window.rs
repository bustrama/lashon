//! `wait_for_window` — poll for a window whose title contains a substring.
//!
//! Replaces a guessed `wait_ms(2000)` after `open_app("spotify")` with
//! a deterministic check: poll `EnumWindows` every 100 ms until a
//! visible window's title (case-insensitive) contains the user's
//! substring, or the timeout fires.
//!
//! Returns "appeared in {N} ms" so the LLM sees how long the app
//! actually took to come up — useful telemetry when chains drift.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Default timeout when the LLM doesn't specify one. Conservative —
/// covers warm Spotify / Chrome / Slack starts on any laptop.
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
/// Hard cap on the timeout the LLM may request. 60 s is sized for
/// a cold Electron-app first launch on slow hardware — WhatsApp /
/// Slack / Teams from a cold disk routinely exceed 15 s. The
/// dispatcher's own 3-minute cumulative budget (`TAKE_BUDGET`) is
/// the global backstop.
const MAX_TIMEOUT_MS: u64 = 60_000;
/// How often to re-check while waiting. 100 ms is 10 polls/sec which
/// is responsive without saturating the EnumWindows callback path.
const POLL_INTERVAL_MS: u64 = 100;

pub struct WaitForWindow;

impl WaitForWindow {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaitForWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for WaitForWindow {
    fn name(&self) -> &str {
        "wait_for_window"
    }

    fn description(&self) -> &str {
        "Wait until a window whose title contains the given substring \
         appears on screen (case-insensitive). Returns as soon as the \
         window is visible, or errors if `timeout_ms` elapses first. \
         **Prefer this over `wait_ms` after `open_app`** — it adapts \
         to the actual launch time instead of guessing."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Substring of the window's title (case-insensitive). e.g. `spotify`, `chrome`, `notepad`."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 100,
                    "maximum": MAX_TIMEOUT_MS,
                    "description": "How long to wait before giving up. Defaults to 5000 ms; capped at 60000 ms."
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
                .ok_or_else(|| anyhow!("wait_for_window: missing required `title` argument"))?;
            if title.trim().is_empty() {
                return Err(anyhow!("wait_for_window: `title` must not be empty"));
            }
            let timeout = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS);

            let started = Instant::now();
            let needle = title.to_lowercase();
            loop {
                match window_with_title_exists(&needle) {
                    Ok(true) => {
                        let elapsed = started.elapsed().as_millis() as u64;
                        return Ok(ToolResult {
                            content: format!("window `{title}` appeared after {elapsed} ms"),
                            display_summary: Some(format!("{title} זמין ({elapsed} מ\"ש)")),
                        });
                    }
                    Ok(false) => {}
                    // Platform stub or transient OS failure — surface it as
                    // an error-shaped ToolResult so the LLM can recover on
                    // the next turn rather than the dispatch task panicking.
                    Err(err) => return Ok(ToolResult::error(err.to_string())),
                }
                if started.elapsed() >= Duration::from_millis(timeout) {
                    return Ok(ToolResult::error(format!(
                        "no window with title `{title}` appeared within {timeout} ms"
                    )));
                }
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
            }
        })
    }
}

/// Whether any visible top-level window's title contains the
/// (already-lowercased) `needle`. Reuses the same `EnumWindows` walk
/// shape `focus_window` uses, just for "exists" rather than "focus".
#[cfg(target_os = "windows")]
fn window_with_title_exists(needle_lower: &str) -> Result<bool> {
    use std::sync::Mutex;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    struct State {
        needle: String,
        found: bool,
    }
    let state = Mutex::new(State {
        needle: needle_lower.to_string(),
        found: false,
    });

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` carries a borrow of the `Mutex<State>` on
        // the calling stack frame — alive for the entire EnumWindows
        // sweep.
        let state = unsafe { &*(lparam.0 as *const Mutex<State>) };
        let Ok(mut state) = state.lock() else {
            return BOOL(1);
        };
        if state.found {
            // Stop walking once we've found one.
            return BOOL(0);
        }
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
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
        let title = String::from_utf16_lossy(&buf[..read as usize]).to_lowercase();
        if title.contains(&state.needle) {
            state.found = true;
            return BOOL(0); // stop
        }
        BOOL(1)
    }

    let lparam = LPARAM(&state as *const Mutex<State> as isize);
    unsafe {
        let _ = EnumWindows(Some(callback), lparam);
    }
    let found = state
        .lock()
        .map(|s| s.found)
        .map_err(|e| anyhow!("wait_for_window: state lock poisoned: {e}"))?;
    Ok(found)
}

#[cfg(not(target_os = "windows"))]
fn window_with_title_exists(_needle_lower: &str) -> Result<bool> {
    Err(anyhow!(
        "wait_for_window: not yet implemented on this OS. macOS NSWorkspace \
         and Linux wmctrl land later."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = WaitForWindow::new();
        assert_eq!(tool.name(), "wait_for_window");
        assert!(!tool.requires_confirmation(&json!({"title": "spotify"})));
    }

    #[test]
    fn parameters_describe_title_and_timeout() {
        let params = WaitForWindow.parameters();
        assert_eq!(params["properties"]["title"]["type"], "string");
        assert_eq!(params["properties"]["timeout_ms"]["type"], "integer");
        assert_eq!(params["required"][0], "title");
    }

    #[test]
    fn missing_title_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(WaitForWindow.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("title"));
    }

    #[test]
    fn empty_title_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(WaitForWindow.execute(&json!({"title": "  "})))
            .err()
            .expect("blank title must error");
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn short_timeout_returns_error_result() {
        // No window titled `nonexistent-title-zzz-9999` exists; the tool
        // must return an error-shaped ToolResult (`Ok(...)`) rather than
        // bubbling an `Err`, so the LLM can recover on the next turn.
        //
        // The exact shape of "error" differs by platform:
        //   - Windows: the polling loop runs and times out after ~250 ms
        //     with "no window with title `...` appeared within 250 ms".
        //   - macOS / Linux: the platform stub returns immediately with
        //     a "not yet implemented" message; the loop never gets to
        //     iterate.
        //
        // Both forms start with "error:" so the assertion is cross-
        // platform. The timing assertion is Windows-only — on the
        // stub platforms the elapsed time is near zero.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let result = runtime
            .block_on(WaitForWindow.execute(&json!({
                "title": "nonexistent-title-zzz-9999",
                "timeout_ms": 250,
            })))
            .expect("tool must return Ok with an error-shaped ToolResult, not Err");
        let elapsed = started.elapsed();
        assert!(result.content.starts_with("error:"), "{}", result.content);
        #[cfg(target_os = "windows")]
        {
            // We waited at most timeout_ms + one poll interval; the lower
            // bound is timeout_ms.
            assert!(elapsed.as_millis() >= 250);
            assert!(elapsed.as_millis() < 1_000);
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Stub platforms return synchronously without ever sleeping.
            let _ = elapsed;
        }
    }
}
