//! `read_browser_url` — walk the foreground window's UIA tree looking
//! for an Edit-class element whose value parses as a URL. Chrome, Edge,
//! Firefox and Brave all expose their address bar this way; the
//! heuristic also catches DuckDuckGo's URL pill and a handful of
//! Electron browsers.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct ReadBrowserUrl;

impl ReadBrowserUrl {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReadBrowserUrl {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ReadBrowserUrl {
    fn name(&self) -> &str {
        "read_browser_url"
    }

    fn description(&self) -> &str {
        "Return the URL of the foreground browser tab. Walks the UIA \
         tree for an editable element whose value looks like a URL — \
         works for Chrome, Edge, Firefox, and Brave. Errors when the \
         foreground window is not a browser or the address bar is \
         hidden."
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
            match find_url() {
                Ok(Some(url)) => Ok(ToolResult {
                    content: url.clone(),
                    display_summary: Some(format!("ה-URL: {}", short(&url))),
                }),
                Ok(None) => Ok(ToolResult::error(
                    "read_browser_url: no URL-shaped address bar found in the foreground window"
                        .to_string(),
                )),
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

fn short(url: &str) -> String {
    let trimmed = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let head = trimmed.split('/').next().unwrap_or(trimmed);
    if head.chars().count() > 40 {
        let mut s: String = head.chars().take(40).collect();
        s.push('…');
        s
    } else {
        head.to_string()
    }
}

#[cfg(target_os = "windows")]
fn find_url() -> Result<Option<String>> {
    use windows::core::{Interface, BOOL};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
        TreeScope_Descendants, UIA_ValuePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // UIA_EditControlTypeId — the address bar in every Chromium / Gecko
    // browser is an Edit-class element. Hard-coded so we don't add a
    // dependency on the UIA pattern enums beyond what we already use.
    const UIA_EDIT_CONTROL_TYPE_ID: i32 = 50004;

    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("read_browser_url: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("read_browser_url: CoCreateInstance: {e}"))?;
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err(anyhow!("read_browser_url: no foreground window"));
        }
        let root: IUIAutomationElement = automation
            .ElementFromHandle(hwnd)
            .map_err(|e| anyhow!("read_browser_url: ElementFromHandle: {e}"))?;
        let condition = automation
            .CreateTrueCondition()
            .map_err(|e| anyhow!("read_browser_url: CreateTrueCondition: {e}"))?;
        let candidates = root
            .FindAll(TreeScope_Descendants, &condition)
            .map_err(|e| anyhow!("read_browser_url: FindAll: {e}"))?;
        let count = candidates
            .Length()
            .map_err(|e| anyhow!("read_browser_url: Length: {e}"))?;
        for i in 0..count {
            let elem = match candidates.GetElement(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let off_screen: BOOL = elem.CurrentIsOffscreen().unwrap_or(BOOL(1));
            if off_screen.as_bool() {
                continue;
            }
            // The address bar's Name is the user-facing localised label
            // (e.g. "שורת הכתובת"). We instead probe by control type —
            // every Chromium / Gecko browser exposes the address bar as
            // an Edit element — and pull the displayed value via the
            // ValuePattern.
            let control_type = elem.CurrentControlType().unwrap_or_default();
            if control_type.0 != UIA_EDIT_CONTROL_TYPE_ID {
                continue;
            }
            let Ok(pattern_iunk) = elem.GetCurrentPattern(UIA_ValuePatternId) else {
                continue;
            };
            let Ok(value_pattern) = pattern_iunk.cast::<IUIAutomationValuePattern>() else {
                continue;
            };
            let Ok(bstr) = value_pattern.CurrentValue() else {
                continue;
            };
            let s = bstr.to_string();
            if looks_like_url(&s) {
                return Ok(Some(s));
            }
        }
    }
    Ok(None)
}

fn looks_like_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    // Easy positives.
    if s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("file://")
        || s.starts_with("about:")
    {
        return true;
    }
    // Bare host like `example.com` or `example.com/path` — most browsers
    // store the address bar in this shape when the URL has been
    // edited-without-submit.
    if s.contains(' ') {
        return false;
    }
    let head = s.split('/').next().unwrap_or(s);
    // Heuristic: contains a dot, no whitespace, no Hebrew (the address
    // bar value is always ASCII even if the URL is a punycoded IDN).
    head.contains('.') && head.is_ascii()
}

#[cfg(not(target_os = "windows"))]
fn find_url() -> Result<Option<String>> {
    Err(anyhow!(
        "read_browser_url: not yet implemented on this OS — Windows-only in M8.2"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(ReadBrowserUrl.name(), "read_browser_url");
        assert!(!ReadBrowserUrl.requires_confirmation(&json!({})));
    }

    #[test]
    fn looks_like_url_heuristic() {
        assert!(looks_like_url("https://example.com"));
        assert!(looks_like_url("http://example.com/path"));
        assert!(looks_like_url("example.com"));
        assert!(looks_like_url("about:blank"));
        assert!(!looks_like_url(""));
        assert!(!looks_like_url("hello world"));
        assert!(!looks_like_url("שלום"));
    }

    #[test]
    fn short_url_helper_trims_scheme_and_path() {
        assert_eq!(short("https://example.com/foo"), "example.com");
        assert_eq!(short("http://example.com"), "example.com");
    }
}
