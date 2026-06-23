//! `double_click` — find a UI element by its visible label and
//! double-click its centre. The two clicks land ~50 ms apart so the
//! OS's double-click detector accepts them as a pair on every default
//! configuration we've measured.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct DoubleClick;

impl DoubleClick {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DoubleClick {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for DoubleClick {
    fn name(&self) -> &str {
        "double_click"
    }

    fn description(&self) -> &str {
        "Double-click a UI element in the foreground window by its visible \
         text label. Same target rules as `click_element`. Use to open a \
         file from Explorer, expand a tree row, or trigger a double-click \
         affordance that a single click does not."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Substring of the element's visible label / accessible Name. Case-insensitive."
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
                .ok_or_else(|| anyhow!("double_click: missing required `text` argument"))?;
            if needle.trim().is_empty() {
                return Err(anyhow!("double_click: `text` must not be empty"));
            }
            match double_click_by_name(needle)? {
                Some(matched) => Ok(ToolResult {
                    content: format!("double-clicked element `{matched}`"),
                    display_summary: Some(format!("דאבל-קליק על `{matched}`")),
                }),
                None => Ok(ToolResult::error(format!(
                    "no element with label containing `{needle}` was found in the foreground window"
                ))),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn double_click_by_name(needle: &str) -> Result<Option<String>> {
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
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("double_click: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("double_click: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!("double_click: no foreground window"));
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("double_click: ElementFromHandle: {e}"))?;
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("double_click: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("double_click: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("double_click: Length: {e}"))?;
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
                Ok(b) => b.to_string(),
                Err(_) => continue,
            };
            if name.is_empty() || !name.to_lowercase().contains(&needle_lower) {
                continue;
            }
            let rect = match elem.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rect.right <= rect.left || rect.bottom <= rect.top {
                continue;
            }
            let cx = rect.left + (rect.right - rect.left) / 2;
            let cy = rect.top + (rect.bottom - rect.top) / 2;
            double_click_at(cx, cy)?;
            std::thread::sleep(Duration::from_millis(80));
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

#[cfg(target_os = "windows")]
fn double_click_at(x: i32, y: i32) -> Result<()> {
    use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("double_click: cannot open input device: {e}"))?;
    enigo
        .move_mouse(x, y, Coordinate::Abs)
        .map_err(|e| anyhow!("double_click: move_mouse: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| anyhow!("double_click: first click: {e}"))?;
    // ~50 ms is well inside Windows' default `GetDoubleClickTime`
    // (500 ms) and macOS's typical 300 ms, so a paired click registers
    // as a real double-click on every default configuration.
    std::thread::sleep(std::time::Duration::from_millis(50));
    enigo
        .button(Button::Left, Direction::Click)
        .map_err(|e| anyhow!("double_click: second click: {e}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn double_click_by_name(_needle: &str) -> Result<Option<String>> {
    Err(anyhow!(
        "double_click: not yet implemented on this OS. The Phase-2 tool \
         is Windows-only (UI Automation); macOS AXUIElement and Linux \
         AT-SPI land later."
    ))
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
        assert_eq!(DoubleClick.name(), "double_click");
        assert!(!DoubleClick.requires_confirmation(&json!({"text": "x"})));
    }

    #[test]
    fn missing_text_argument_errors() {
        let err = rt()
            .block_on(DoubleClick.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("text"));
    }

    #[test]
    fn empty_text_argument_errors() {
        let err = rt()
            .block_on(DoubleClick.execute(&json!({"text": "   "})))
            .err()
            .expect("blank text must error");
        assert!(err.to_string().contains("empty"));
    }
}
