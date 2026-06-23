//! `click_element` — find and click a UI element by its visible label.
//!
//! Walks the foreground window's UI Automation tree, looks for an
//! element whose Name property contains the user's `text` (case
//! insensitive), then mouse-clicks the centre of its bounding box. The
//! LLM uses this for the "press the Play button in Spotify" step of a
//! tool chain that the bare keyboard tools (`press_keys`,
//! `type_text`) can't reach.
//!
//! Windows: UI Automation v3 via the `windows` crate (CUIAutomation
//! COM object, IUIAutomationElement::FindAll with a TrueCondition,
//! filtered in-process). Mouse-click via `enigo` so the click looks
//! identical to a human one — many apps respond differently to
//! `InvokePattern.Invoke()` vs a synthetic mouse-down, and the click
//! path works on a wider variety of controls.
//!
//! macOS / Linux ship as Phase-1 stubs alongside `open_app` and
//! `focus_window`; the real impls (AXUIElement, AT-SPI) land later.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct ClickElement;

impl ClickElement {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClickElement {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ClickElement {
    fn name(&self) -> &str {
        "click_element"
    }

    fn description(&self) -> &str {
        "Click a UI element in the foreground window by its visible \
         text label. Matches the element's accessibility Name property \
         using case-insensitive substring search. Use for buttons / \
         links / list items that can't be reached with a keyboard \
         shortcut — e.g. the Play button in a Spotify search result. \
         Tip: chain `wait_ms` first so the UI has time to render before \
         the click."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Substring of the element's visible label / accessible Name. Case-insensitive. e.g. \"Play\", \"Submit\", \"Imagine Dragons\"."
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
                .ok_or_else(|| anyhow!("click_element: missing required `text` argument"))?;
            if needle.trim().is_empty() {
                return Err(anyhow!("click_element: `text` must not be empty"));
            }
            match click_by_name(needle)? {
                Some(matched) => Ok(ToolResult {
                    content: format!("clicked element `{matched}`"),
                    display_summary: Some(format!("הקלקתי על `{matched}`")),
                }),
                None => Ok(ToolResult::error(format!(
                    "no element with label containing `{needle}` was found in the foreground window"
                ))),
            }
        })
    }
}

/// Search the foreground window's UI Automation tree for the first
/// element whose Name property contains `needle` (case-insensitive)
/// and click its bounding-rect centre. Returns:
/// - `Ok(Some(matched_name))` when an element was found and clicked.
/// - `Ok(None)` when no element matched.
/// - `Err(_)` when the UIA / mouse plumbing itself errored.
#[cfg(target_os = "windows")]
fn click_by_name(needle: &str) -> Result<Option<String>> {
    use std::time::Duration;
    use windows::core::BOOL;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let needle_lower = needle.to_lowercase();

    unsafe {
        // CoInitializeEx is idempotent per thread; the dispatcher pool
        // doesn't guarantee a fixed thread, so we re-arm on each call.
        // RPC_E_CHANGED_MODE is the harmless "already initialised with
        // a different model" case — bail in that case and proceed.
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("click_element: CoInitializeEx failed: {init:?}"));
        }

        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("click_element: CoCreateInstance(CUIAutomation): {e}"))?;

        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!("click_element: no foreground window"));
        }

        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("click_element: ElementFromHandle: {e}"))?;

        // A TRUE condition: every descendant element. We filter in
        // Rust because UIA's PropertyCondition is exact-match, not
        // substring, so we'd need a less ergonomic OrCondition tree.
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("click_element: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("click_element: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("click_element: Length: {e}"))?;

        for i in 0..count {
            let elem = match candidates.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            // Skip elements that aren't on screen (off-screen or hidden).
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
            if !name.to_lowercase().contains(&needle_lower) {
                continue;
            }
            // Bounding rect → centre.
            let rect = match elem.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => continue,
            };
            let cx = rect.left + (rect.right - rect.left) / 2;
            let cy = rect.top + (rect.bottom - rect.top) / 2;
            // Some screen modes report (0,0,0,0) for elements that
            // technically pass IsOffscreen=false — skip those too.
            if rect.right <= rect.left || rect.bottom <= rect.top {
                continue;
            }
            click_at(cx, cy)?;
            // A tiny settle before returning so the LLM's next
            // tool_result reads as "we already moved" rather than
            // "still racing the click".
            std::thread::sleep(Duration::from_millis(80));
            // Truncate the matched name so a window-title-like Name
            // doesn't flood the tongue's flash.
            let summary = if name.chars().count() > 64 {
                let mut s: String = name.chars().take(64).collect();
                s.push('…');
                s
            } else {
                name
            };
            return Ok(Some(summary));
        }
    }
    Ok(None)
}

/// Move the mouse to `(x, y)` (screen coords, virtual desktop) and
/// click the left button once. enigo's coordinate system is the same
/// virtual-screen origin UIA reports, so no DPI dance needed.
#[cfg(target_os = "windows")]
fn click_at(x: i32, y: i32) -> Result<()> {
    use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("click_element: cannot open input device: {e}"))?;
    enigo
        .move_mouse(x, y, Coordinate::Abs)
        .map_err(|e| anyhow!("click_element: move_mouse: {e}"))?;
    // A brief pause between move and click so apps tracking
    // OnMouseEnter / hover state register the cursor before the down/up.
    std::thread::sleep(std::time::Duration::from_millis(40));
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| anyhow!("click_element: button click: {e}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn click_by_name(_needle: &str) -> Result<Option<String>> {
    Err(anyhow!(
        "click_element: not yet implemented on this OS. The Phase-1.1 \
         impl is Windows-only (UI Automation); macOS AXUIElement and \
         Linux AT-SPI land later."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = ClickElement::new();
        assert_eq!(tool.name(), "click_element");
        assert!(!tool.requires_confirmation(&json!({"text": "Play"})));
    }

    #[test]
    fn parameters_describe_text_argument() {
        let params = ClickElement.parameters();
        assert_eq!(params["properties"]["text"]["type"], "string");
        assert_eq!(params["required"][0], "text");
    }

    #[test]
    fn missing_text_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(ClickElement.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("text"));
    }

    #[test]
    fn empty_text_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(ClickElement.execute(&json!({"text": "   "})))
            .err()
            .expect("blank text must error");
        assert!(err.to_string().contains("empty"));
    }
}
