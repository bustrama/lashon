//! `list_open_windows` — cheap title-only listing of every visible
//! top-level window. Where `read_screen` walks the UIA tree (slower,
//! richer), `list_open_windows` just hits `EnumWindows` + `GetWindowText`
//! and pairs the title with the owning process's image name.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

const MAX_LINES: usize = 100;

pub struct ListOpenWindows;

impl ListOpenWindows {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListOpenWindows {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ListOpenWindows {
    fn name(&self) -> &str {
        "list_open_windows"
    }

    fn description(&self) -> &str {
        "List every visible top-level window — one line per window, \
         formatted `<title> (process)`. Much cheaper than `read_screen` \
         when all you need is to know what's open. Use to answer \
         'do I have Chrome open?' or to pick a target for \
         `focus_window`."
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
            match list() {
                Ok(lines) => {
                    let count = lines.len();
                    let body = if lines.is_empty() {
                        "(no visible windows)".to_string()
                    } else {
                        lines.join("\n")
                    };
                    Ok(ToolResult {
                        content: body,
                        display_summary: Some(format!("{count} חלונות פתוחים")),
                    })
                }
                Err(e) => Ok(ToolResult::error(e.to_string())),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn list() -> Result<Vec<String>> {
    use std::sync::Mutex;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    struct Acc {
        out: Vec<String>,
    }
    let state = Mutex::new(Acc { out: Vec::new() });

    extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &*(lparam.0 as *const Mutex<Acc>) };
        let Ok(mut state) = state.lock() else {
            return BOOL(1);
        };
        if state.out.len() >= MAX_LINES {
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
        let mut pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        let proc_name = process_image_name(pid).unwrap_or_else(|| "?".to_string());
        state.out.push(format!("{title} ({proc_name})"));
        BOOL(1)
    }

    let lparam = LPARAM(&state as *const Mutex<Acc> as isize);
    unsafe {
        let _ = EnumWindows(Some(callback), lparam);
    }
    let acc = state
        .lock()
        .map_err(|e| anyhow!("list_open_windows: state lock poisoned: {e}"))?;
    Ok(acc.out.clone())
}

#[cfg(target_os = "windows")]
fn process_image_name(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return None;
        };
        let mut buf: Vec<u16> = vec![0u16; MAX_PATH as usize];
        let mut len: u32 = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        if res.is_err() || len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(
            std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(path),
        )
    }
}

#[cfg(not(target_os = "windows"))]
fn list() -> Result<Vec<String>> {
    Err(anyhow!(
        "list_open_windows: not yet implemented on this OS. The Phase-2 \
         tool is Windows-only; macOS / Linux land later."
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
        assert_eq!(ListOpenWindows.name(), "list_open_windows");
        assert!(!ListOpenWindows.requires_confirmation(&json!({})));
    }

    #[test]
    fn extra_args_are_ignored() {
        let result = rt().block_on(ListOpenWindows.execute(&json!({"junk": "v"})));
        assert!(result.is_ok());
    }
}
