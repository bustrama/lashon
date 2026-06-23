//! `wait_for_element` — poll the foreground window's UIA tree until a
//! matching element appears on screen.
//!
//! Used by the LLM between a search-and-click chain step. Concretely:
//! after `press_keys("Enter")` runs a Spotify search, the search
//! results need a fraction of a second to render. Instead of guessing
//! `wait_ms(800)`, the model emits
//! `wait_for_element({ text: "Play", timeout_ms: 3000 })` and the
//! dispatcher polls UIA at 150 ms intervals until the Play button is
//! visible and enabled — or times out cleanly.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;
/// 60 s matches `wait_for_window`'s cap so a chain can wait for the
/// outer window then a slow-to-render inner element (a contact list,
/// a search box, a chat compose area) without padding individual
/// timeouts. The dispatcher's 3-minute `TAKE_BUDGET` is the global
/// backstop.
const MAX_TIMEOUT_MS: u64 = 60_000;
/// 150 ms is enough for the user not to notice latency while keeping
/// UIA-tree traversals (which can be expensive on heavy Electron apps
/// like Slack / Discord) out of the way of the foreground app.
const POLL_INTERVAL_MS: u64 = 150;

pub struct WaitForElement;

impl WaitForElement {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaitForElement {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for WaitForElement {
    fn name(&self) -> &str {
        "wait_for_element"
    }

    fn description(&self) -> &str {
        "Wait until an element whose visible label (accessibility Name) \
         contains the given substring is on-screen in the foreground \
         window. Returns when the element appears, or errors after \
         `timeout_ms`. **Prefer this over `wait_ms`** before a \
         `click_element` call — it adapts to the actual UI render \
         time. Typical use: search bar → Enter → \
         `wait_for_element({text: \"Play\"})` → `click_element(\"Play\")`."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Substring of the element's visible label / accessible Name (case-insensitive)."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 100,
                    "maximum": MAX_TIMEOUT_MS,
                    "description": "How long to wait. Defaults to 5000 ms; capped at 60000 ms."
                }
            },
            "required": ["text"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let needle = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("wait_for_element: missing required `text` argument"))?;
            if needle.trim().is_empty() {
                return Err(anyhow!("wait_for_element: `text` must not be empty"));
            }
            let timeout = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS);

            let started = Instant::now();
            let needle_lower = needle.to_lowercase();
            loop {
                match foreground_has_element(&needle_lower) {
                    Ok(Some(matched)) => {
                        let elapsed = started.elapsed().as_millis() as u64;
                        return Ok(ToolResult {
                            content: format!("element `{matched}` appeared after {elapsed} ms"),
                            display_summary: Some(format!("`{matched}` הופיע ({elapsed} מ\"ש)")),
                        });
                    }
                    Ok(None) => {}
                    // Platform stub or transient COM/UIA failure — surface
                    // it as an error-shaped ToolResult so the dispatch task
                    // doesn't panic on a non-Windows build.
                    Err(err) => return Ok(ToolResult::error(err.to_string())),
                }
                if started.elapsed() >= Duration::from_millis(timeout) {
                    return Ok(ToolResult::error(format!(
                        "no element with label containing `{needle}` appeared within {timeout} ms"
                    )));
                }
                tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
            }
        })
    }
}

/// Returns the matched name of the first on-screen UIA descendant of
/// the foreground window whose Name contains `needle_lower`, or
/// `Ok(None)` when nothing matches yet. Mirrors `click_element`'s
/// walk; kept separate so the polling loop stays clean.
#[cfg(target_os = "windows")]
fn foreground_has_element(needle_lower: &str) -> Result<Option<String>> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("wait_for_element: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("wait_for_element: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(None);
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("wait_for_element: ElementFromHandle: {e}"))?;
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("wait_for_element: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("wait_for_element: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("wait_for_element: Length: {e}"))?;
        for i in 0..count {
            let elem = match candidates.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let off_screen: BOOL = elem.CurrentIsOffscreen().unwrap_or(BOOL(1));
            if off_screen.as_bool() {
                continue;
            }
            let name = match elem.CurrentName() {
                Ok(bstr) => bstr.to_string(),
                Err(_) => continue,
            };
            if name.is_empty() {
                continue;
            }
            if name.to_lowercase().contains(needle_lower) {
                return Ok(Some(if name.chars().count() > 64 {
                    let mut s: String = name.chars().take(64).collect();
                    s.push('…');
                    s
                } else {
                    name
                }));
            }
        }
    }
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
fn foreground_has_element(_needle_lower: &str) -> Result<Option<String>> {
    Err(anyhow!(
        "wait_for_element: not yet implemented on this OS. macOS AXUIElement \
         and Linux AT-SPI land later."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = WaitForElement::new();
        assert_eq!(tool.name(), "wait_for_element");
        assert!(!tool.requires_confirmation(&json!({"text": "Play"})));
    }

    #[test]
    fn parameters_describe_text_and_timeout() {
        let params = WaitForElement.parameters();
        assert_eq!(params["properties"]["text"]["type"], "string");
        assert_eq!(params["properties"]["timeout_ms"]["type"], "integer");
        assert_eq!(params["required"][0], "text");
    }

    #[test]
    fn missing_text_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(WaitForElement.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("text"));
    }
}
