//! `read_active_window_text` — flatten the foreground window's
//! visible UIA labels into a newline-separated text snapshot.
//!
//! Lets the LLM verify state mid-chain without a screenshot model.
//! Concrete need: in the WhatsApp flow
//! (`open_app(whatsapp) → wait_for_window → … → click_element(קוקי)`),
//! the model wants to confirm that the contact list has rendered
//! and that "קוקי" actually appears before clicking. Without this
//! tool the model either guesses (risky) or blindly retries
//! `wait_for_element` until it times out.
//!
//! The output is capped (`MAX_OUTPUT_BYTES`) so a huge UIA tree
//! (Slack with 200 channels, file explorer with hundreds of items)
//! doesn't blow the LLM's context window. Duplicates are dropped —
//! one entry per distinct visible label.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Hard cap on the returned text. 4 KB is enough for a typical
/// app's visible chrome (~150 labels at ~25 chars each) and leaves
/// plenty of headroom inside the dispatcher's 4 K-token request cap.
const MAX_OUTPUT_BYTES: usize = 4096;

/// Hard cap on labels we even consider before dedupe/truncate.
/// Walking a giant UIA tree is cheap (`FindAll` is a single call)
/// but iterating descendant names isn't — bound it for predictable
/// latency.
const MAX_LABELS_SCANNED: usize = 2000;

pub struct ReadActiveWindowText;

impl ReadActiveWindowText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadActiveWindowText {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ReadActiveWindowText {
    fn name(&self) -> &str {
        "read_active_window_text"
    }

    fn description(&self) -> &str {
        "Return a snapshot of the visible text labels in the currently \
         focused window — window title plus on-screen UIA element names, \
         one per line, deduplicated, truncated to ~4 KB. Use this when \
         you need to verify state before clicking: 'did the search \
         results render?', 'is the contact list showing?', 'did the \
         settings dialog open?'. Pair with `click_element` once you've \
         confirmed the target is visible. Cheap (~50 ms); call freely \
         between steps."
    }

    fn parameters(&self) -> Value {
        // No arguments — the foreground window is whatever has focus
        // right now. Keeping the schema as `{ type: object, properties:
        // {} }` matches `clipboard_get`'s arg-less shape.
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            match snapshot_foreground() {
                Ok(text) => {
                    let line_count = text.lines().count();
                    Ok(ToolResult {
                        content: text,
                        display_summary: Some(format!("נקראו {line_count} תוויות")),
                    })
                }
                Err(err) => Ok(ToolResult::error(err.to_string())),
            }
        })
    }
}

/// Collect visible labels from the foreground window's UIA tree.
/// First line is `Window: <title>`; subsequent lines are unique
/// element names in tree order, truncated to `MAX_OUTPUT_BYTES`.
#[cfg(target_os = "windows")]
fn snapshot_foreground() -> Result<String> {
    use std::collections::HashSet;

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
            return Err(anyhow!(
                "read_active_window_text: CoInitializeEx failed: {init:?}"
            ));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("read_active_window_text: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(String::from("Window: (no focused window)"));
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("read_active_window_text: ElementFromHandle: {e}"))?;
        let title = root
            .CurrentName()
            .map(|b| b.to_string())
            .unwrap_or_default();

        let mut out = String::with_capacity(MAX_OUTPUT_BYTES.min(1024));
        out.push_str("Window: ");
        out.push_str(if title.is_empty() {
            "(untitled)"
        } else {
            &title
        });
        out.push('\n');

        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("read_active_window_text: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("read_active_window_text: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("read_active_window_text: Length: {e}"))?;

        let scan_cap = (count as usize).min(MAX_LABELS_SCANNED);
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(title.clone());
        for i in 0..scan_cap as i32 {
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
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Long single labels (e.g. a paragraph of body text in a
            // chat window) would bloat the output and confuse the
            // model. Cap each entry; the model can still ask for
            // more by running the tool again after acting.
            let entry: String = if trimmed.chars().count() > 200 {
                let mut s: String = trimmed.chars().take(200).collect();
                s.push('…');
                s
            } else {
                trimmed.to_string()
            };
            if !seen.insert(entry.clone()) {
                continue;
            }
            // Cheap predictive truncation — adding this entry plus a
            // newline must fit under the byte cap, otherwise we
            // append a marker and stop.
            if out.len() + entry.len() + 1 > MAX_OUTPUT_BYTES {
                out.push_str("…(truncated)\n");
                break;
            }
            out.push_str(&entry);
            out.push('\n');
        }
        Ok(out)
    }
}

#[cfg(not(target_os = "windows"))]
fn snapshot_foreground() -> Result<String> {
    Err(anyhow!(
        "read_active_window_text: not yet implemented on this OS. \
         macOS AXUIElement and Linux AT-SPI land alongside the other \
         UIA tools later."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        let tool = ReadActiveWindowText::new();
        assert_eq!(tool.name(), "read_active_window_text");
        assert!(!tool.requires_confirmation(&json!({})));
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn parameters_are_object_with_no_required_fields() {
        let params = ReadActiveWindowText.parameters();
        assert_eq!(params["type"], "object");
        assert!(params["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn extra_args_are_ignored_and_do_not_error() {
        // additionalProperties: false in the schema; behaviour at
        // runtime is "we accept and discard". The dispatcher relies
        // on this — some LLMs slip in placeholder args.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(ReadActiveWindowText.execute(&json!({"junk": "ignored"})));
        // On non-Windows the platform stub returns ToolResult::error
        // (Ok). On Windows it returns a real snapshot. Either way, no
        // panic, no anyhow::Err from arg parsing.
        assert!(result.is_ok());
    }
}
