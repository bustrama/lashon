//! `scroll` — synthetic wheel scroll at the mouse cursor, optionally
//! after first moving the cursor over a named UIA region.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Default scroll amount in wheel clicks. Three matches the OS default
/// scroll-velocity used by hands-on testers — small enough not to skip
/// past a single item in a list, large enough to feel responsive.
const DEFAULT_AMOUNT: i32 = 3;
const MAX_AMOUNT: i32 = 50;

pub struct Scroll;

impl Scroll {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Scroll {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for Scroll {
    fn name(&self) -> &str {
        "scroll"
    }

    fn description(&self) -> &str {
        "Scroll the foreground window in the given direction. `amount` \
         defaults to 3 wheel clicks (capped at 50). When `target` is \
         provided, the mouse first moves to the centre of the UIA \
         element with that visible label — useful for scrolling one of \
         several lists in the same window (a sidebar, a chat pane, the \
         main content area)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"],
                    "description": "Which way the content scrolls. `up`/`down` are vertical; `left`/`right` are horizontal."
                },
                "amount": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_AMOUNT,
                    "description": "Number of wheel clicks. Defaults to 3."
                },
                "target": {
                    "type": "string",
                    "description": "Optional UIA label to move the cursor over before scrolling. Case-insensitive substring match."
                }
            },
            "required": ["direction"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("scroll: missing required `direction` argument"))?;
            let direction = match direction.to_lowercase().as_str() {
                "up" => ScrollDir::Up,
                "down" => ScrollDir::Down,
                "left" => ScrollDir::Left,
                "right" => ScrollDir::Right,
                other => {
                    return Err(anyhow!(
                        "scroll: invalid direction `{other}`; expected up/down/left/right"
                    ));
                }
            };
            let amount = args
                .get("amount")
                .and_then(|v| v.as_i64())
                .map(|v| v.clamp(1, MAX_AMOUNT as i64) as i32)
                .unwrap_or(DEFAULT_AMOUNT);
            let target = args.get("target").and_then(|v| v.as_str());
            scroll(direction, amount, target)?;
            Ok(ToolResult {
                content: format!("scrolled {} by {amount} clicks", direction.as_str()),
                display_summary: Some(format!("גלילה {} ({amount})", direction.as_he())),
            })
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum ScrollDir {
    Up,
    Down,
    Left,
    Right,
}

impl ScrollDir {
    fn as_str(&self) -> &'static str {
        match self {
            ScrollDir::Up => "up",
            ScrollDir::Down => "down",
            ScrollDir::Left => "left",
            ScrollDir::Right => "right",
        }
    }
    fn as_he(&self) -> &'static str {
        match self {
            ScrollDir::Up => "למעלה",
            ScrollDir::Down => "למטה",
            ScrollDir::Left => "שמאלה",
            ScrollDir::Right => "ימינה",
        }
    }
}

#[cfg(target_os = "windows")]
fn scroll(direction: ScrollDir, amount: i32, target: Option<&str>) -> Result<()> {
    use enigo::{Axis, Coordinate, Enigo, Mouse, Settings};
    if let Some(label) = target {
        if let Some((cx, cy)) = locate_element_center(label)? {
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| anyhow!("scroll: cannot open input device: {e}"))?;
            enigo
                .move_mouse(cx, cy, Coordinate::Abs)
                .map_err(|e| anyhow!("scroll: move_mouse: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
    }
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("scroll: cannot open input device: {e}"))?;
    let (axis, length) = match direction {
        // enigo's positive vertical scroll is "down" on Windows (the wheel
        // rotates "away from the user"); flip the sign for "up".
        ScrollDir::Down => (Axis::Vertical, amount),
        ScrollDir::Up => (Axis::Vertical, -amount),
        ScrollDir::Right => (Axis::Horizontal, amount),
        ScrollDir::Left => (Axis::Horizontal, -amount),
    };
    enigo
        .scroll(length, axis)
        .map_err(|e| anyhow!("scroll: scroll failed: {e}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn locate_element_center(needle: &str) -> Result<Option<(i32, i32)>> {
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
            return Err(anyhow!("scroll: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("scroll: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(None);
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("scroll: ElementFromHandle: {e}"))?;
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("scroll: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("scroll: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("scroll: Length: {e}"))?;
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
            return Ok(Some((cx, cy)));
        }
    }
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
fn scroll(_direction: ScrollDir, _amount: i32, _target: Option<&str>) -> Result<()> {
    Err(anyhow!(
        "scroll: not yet implemented on this OS. The Phase-2 tool is \
         Windows-only; macOS and Linux land alongside the other UIA \
         tools."
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
        assert_eq!(Scroll.name(), "scroll");
        assert!(!Scroll.requires_confirmation(&json!({"direction": "down"})));
    }

    #[test]
    fn parameters_describe_enum_and_optionals() {
        let p = Scroll.parameters();
        assert_eq!(p["properties"]["direction"]["type"], "string");
        assert_eq!(p["properties"]["amount"]["type"], "integer");
        assert_eq!(p["required"][0], "direction");
    }

    #[test]
    fn missing_direction_errors() {
        let err = rt()
            .block_on(Scroll.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("direction"));
    }

    #[test]
    fn invalid_direction_errors() {
        let err = rt()
            .block_on(Scroll.execute(&json!({"direction": "diagonal"})))
            .err()
            .expect("invalid direction must error");
        assert!(err.to_string().contains("invalid direction"));
    }
}
