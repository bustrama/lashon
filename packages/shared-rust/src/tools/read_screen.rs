//! `read_screen` — enumerate every top-level window and sample each
//! one's visible UIA labels into a single text snapshot. Distinct from
//! `read_active_window_text`, which is foreground-only: the model uses
//! `read_screen` when it needs to know *what other apps are open* and
//! roughly what's in them ("is Slack showing a notification?", "does
//! the explorer window have my Downloads tab open?").

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Hard cap on the total snapshot. 4 KB matches the per-tool result
/// budget the rest of the OS-control tranche uses; bigger snapshots
/// drown the LLM's context.
const MAX_OUTPUT_BYTES: usize = 4096;
/// Per-window UIA descendants we even iterate. Big Electron apps
/// (Slack with 200 channels, Discord with hundreds of servers) would
/// otherwise dominate the snapshot.
const MAX_LABELS_PER_WINDOW: usize = 40;
/// Cap on the number of windows we walk. Most desktops have <30
/// visible top-level windows; the rest are background.
const MAX_WINDOWS: usize = 50;

pub struct ReadScreen;

impl ReadScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ReadScreen {
    fn name(&self) -> &str {
        "read_screen"
    }

    fn description(&self) -> &str {
        "Snapshot every visible top-level window — title plus a sample of \
         its on-screen UIA labels. Use when you need to know what's open \
         across the desktop (not just the focused window): 'is Slack \
         already open?', 'which explorer window has my Downloads?', \
         'do I have any browser tabs about X?'. Distinct from \
         `read_active_window_text`, which is foreground-only. Capped at \
         ~4 KB total."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            match snapshot_all_windows() {
                Ok(text) => {
                    let window_count = text.matches("\nWindow:").count() + 1;
                    Ok(ToolResult {
                        content: text,
                        display_summary: Some(format!("נסרקו {window_count} חלונות")),
                    })
                }
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn snapshot_all_windows() -> Result<String> {
    use std::sync::Mutex;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };

    struct Collected {
        hwnds: Vec<HWND>,
        titles: Vec<String>,
    }
    let state = Mutex::new(Collected {
        hwnds: Vec::new(),
        titles: Vec::new(),
    });

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &*(lparam.0 as *const Mutex<Collected>) };
        let Ok(mut state) = state.lock() else {
            return BOOL(1);
        };
        if state.hwnds.len() >= MAX_WINDOWS {
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
        state.hwnds.push(hwnd);
        state.titles.push(title);
        BOOL(1)
    }

    let lparam = LPARAM(&state as *const Mutex<Collected> as isize);
    unsafe {
        let _ = EnumWindows(Some(callback), lparam);
    }
    let collected = state
        .lock()
        .map_err(|e| anyhow!("read_screen: state lock poisoned: {e}"))?;
    let mut out = String::with_capacity(MAX_OUTPUT_BYTES.min(1024));
    for (hwnd, title) in collected.hwnds.iter().zip(collected.titles.iter()) {
        let block = window_block(*hwnd, title);
        if out.len() + block.len() > MAX_OUTPUT_BYTES {
            out.push_str("…(truncated)\n");
            break;
        }
        out.push_str(&block);
    }
    if out.is_empty() {
        out.push_str("(no visible windows)\n");
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
fn window_block(hwnd: windows::Win32::Foundation::HWND, title: &str) -> String {
    use windows::core::BOOL;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants,
    };

    let mut block = format!(
        "Window: {}\n",
        if title.is_empty() {
            "(untitled)"
        } else {
            title
        }
    );
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return block;
        }
        let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        else {
            return block;
        };
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return block;
        };
        let _ = (&root as &IUIAutomationElement,);
        let Ok(condition) = automation.CreateTrueCondition() else {
            return block;
        };
        let Ok(candidates) = root.FindAll(TreeScope_Descendants, &condition) else {
            return block;
        };
        let Ok(count) = candidates.Length() else {
            return block;
        };
        let scan_cap = (count as usize).min(MAX_LABELS_PER_WINDOW);
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(title.to_string());
        let mut emitted = 0usize;
        for i in 0..count {
            if emitted >= scan_cap {
                break;
            }
            let Ok(elem) = candidates.GetElement(i) else {
                continue;
            };
            let off_screen: BOOL = elem.CurrentIsOffscreen().unwrap_or(BOOL(1));
            if off_screen.as_bool() {
                continue;
            }
            let Ok(name) = elem.CurrentName() else {
                continue;
            };
            let name = name.to_string();
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: String = if trimmed.chars().count() > 80 {
                let mut s: String = trimmed.chars().take(80).collect();
                s.push('…');
                s
            } else {
                trimmed.to_string()
            };
            if !seen.insert(entry.clone()) {
                continue;
            }
            block.push_str("  - ");
            block.push_str(&entry);
            block.push('\n');
            emitted += 1;
        }
    }
    block
}

#[cfg(not(target_os = "windows"))]
fn snapshot_all_windows() -> Result<String> {
    Err(anyhow!(
        "read_screen: not yet implemented on this OS. The Phase-2 tool is \
         Windows-only (EnumWindows + UI Automation); macOS / Linux land later."
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
        assert_eq!(ReadScreen.name(), "read_screen");
        assert!(!ReadScreen.requires_confirmation(&json!({})));
    }

    #[test]
    fn parameters_are_object_with_no_required_fields() {
        let p = ReadScreen.parameters();
        assert_eq!(p["type"], "object");
        assert!(p["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extra_args_are_ignored() {
        let result = rt().block_on(ReadScreen.execute(&json!({"junk": 1})));
        assert!(result.is_ok());
    }
}
