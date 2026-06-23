//! `drag` — find two UI elements by their visible labels and drag the
//! first onto the second. Useful for reordering a Spotify queue, moving
//! a file into a folder in Explorer, dragging a tab from one window to
//! another.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct Drag;

impl Drag {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Drag {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for Drag {
    fn name(&self) -> &str {
        "drag"
    }

    fn description(&self) -> &str {
        "Drag a UI element from one labelled position to another within \
         the foreground window. Both labels are resolved via the UIA \
         tree (case-insensitive substring on the accessibility Name); \
         the mouse left-button is held while moving the cursor between \
         their centres. Use to reorder lists or drop items into trays."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": {
                    "type": "string",
                    "description": "Substring of the element to grab. Case-insensitive."
                },
                "to": {
                    "type": "string",
                    "description": "Substring of the element to drop onto. Case-insensitive."
                }
            },
            "required": ["from", "to"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let from = args
                .get("from")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("drag: missing required `from` argument"))?;
            let to = args
                .get("to")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("drag: missing required `to` argument"))?;
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(anyhow!("drag: `from`/`to` must not be empty"));
            }
            match drag_by_names(from, to)? {
                DragOutcome::Done { from_name, to_name } => Ok(ToolResult {
                    content: format!("dragged `{from_name}` onto `{to_name}`"),
                    display_summary: Some(format!("גרירה `{from_name}` → `{to_name}`")),
                }),
                DragOutcome::MissingFrom => Ok(ToolResult::error(format!(
                    "no element with label containing `{from}` was found"
                ))),
                DragOutcome::MissingTo => Ok(ToolResult::error(format!(
                    "no element with label containing `{to}` was found"
                ))),
            }
        })
    }
}

enum DragOutcome {
    Done { from_name: String, to_name: String },
    MissingFrom,
    MissingTo,
}

#[cfg(target_os = "windows")]
fn drag_by_names(from: &str, to: &str) -> Result<DragOutcome> {
    use enigo::{Button, Coordinate, Direction, Enigo, Mouse, Settings};
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

    let from_lower = from.to_lowercase();
    let to_lower = to.to_lowercase();
    let (from_pt, to_pt, from_name, to_name) = unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("drag: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("drag: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!("drag: no foreground window"));
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("drag: ElementFromHandle: {e}"))?;
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("drag: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("drag: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("drag: Length: {e}"))?;
        let mut from_pt: Option<(i32, i32, String)> = None;
        let mut to_pt: Option<(i32, i32, String)> = None;
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
            if name.is_empty() {
                continue;
            }
            let lower = name.to_lowercase();
            let rect = match elem.CurrentBoundingRectangle() {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rect.right <= rect.left || rect.bottom <= rect.top {
                continue;
            }
            let cx = rect.left + (rect.right - rect.left) / 2;
            let cy = rect.top + (rect.bottom - rect.top) / 2;
            if from_pt.is_none() && lower.contains(&from_lower) {
                from_pt = Some((cx, cy, name.clone()));
            } else if to_pt.is_none() && lower.contains(&to_lower) {
                to_pt = Some((cx, cy, name.clone()));
            }
            if from_pt.is_some() && to_pt.is_some() {
                break;
            }
        }
        let Some(from_pt) = from_pt else {
            return Ok(DragOutcome::MissingFrom);
        };
        let Some(to_pt) = to_pt else {
            return Ok(DragOutcome::MissingTo);
        };
        (
            (from_pt.0, from_pt.1),
            (to_pt.0, to_pt.1),
            from_pt.2,
            to_pt.2,
        )
    };

    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow!("drag: cannot open input device: {e}"))?;
    enigo
        .move_mouse(from_pt.0, from_pt.1, Coordinate::Abs)
        .map_err(|e| anyhow!("drag: move to from: {e}"))?;
    std::thread::sleep(Duration::from_millis(60));
    enigo
        .button(Button::Left, Direction::Press)
        .map_err(|e| anyhow!("drag: button press: {e}"))?;
    std::thread::sleep(Duration::from_millis(40));
    // Some apps treat instantaneous moves as a click — step the cursor
    // halfway first so the drag-and-drop machinery engages.
    let mx = (from_pt.0 + to_pt.0) / 2;
    let my = (from_pt.1 + to_pt.1) / 2;
    enigo
        .move_mouse(mx, my, Coordinate::Abs)
        .map_err(|e| anyhow!("drag: midpoint move: {e}"))?;
    std::thread::sleep(Duration::from_millis(20));
    enigo
        .move_mouse(to_pt.0, to_pt.1, Coordinate::Abs)
        .map_err(|e| anyhow!("drag: move to to: {e}"))?;
    std::thread::sleep(Duration::from_millis(40));
    enigo
        .button(Button::Left, Direction::Release)
        .map_err(|e| anyhow!("drag: button release: {e}"))?;
    Ok(DragOutcome::Done {
        from_name: truncate_for_summary(&from_name),
        to_name: truncate_for_summary(&to_name),
    })
}

fn truncate_for_summary(name: &str) -> String {
    if name.chars().count() > 40 {
        let mut s: String = name.chars().take(40).collect();
        s.push('…');
        s
    } else {
        name.to_string()
    }
}

#[cfg(not(target_os = "windows"))]
fn drag_by_names(_from: &str, _to: &str) -> Result<DragOutcome> {
    Err(anyhow!(
        "drag: not yet implemented on this OS. The Phase-2 tool is \
         Windows-only (UI Automation); macOS AXUIElement and Linux \
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
        assert_eq!(Drag.name(), "drag");
        assert!(!Drag.requires_confirmation(&json!({"from": "a", "to": "b"})));
    }

    #[test]
    fn missing_args_error() {
        let err = rt()
            .block_on(Drag.execute(&json!({})))
            .err()
            .expect("missing args must error");
        assert!(err.to_string().contains("from"));
        let err = rt()
            .block_on(Drag.execute(&json!({"from": "x"})))
            .err()
            .expect("missing to must error");
        assert!(err.to_string().contains("to"));
    }

    #[test]
    fn empty_args_error() {
        let err = rt()
            .block_on(Drag.execute(&json!({"from": " ", "to": "x"})))
            .err()
            .expect("blank from must error");
        assert!(err.to_string().contains("empty"));
    }
}
