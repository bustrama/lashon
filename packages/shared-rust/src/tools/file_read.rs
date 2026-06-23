//! `file_read` — return the UTF-8 contents of a file under one of the
//! allowed roots. Caps the response at `MAX_BYTES` so a huge log file
//! cannot blow the LLM's context window.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// 32 KB matches the dispatcher's per-tool result budget. A larger
/// response would dwarf the rest of the conversation and trip the LLM's
/// context cap; a smaller one would force the model to chain reads,
/// which is worse UX for log-tail-style use cases.
const MAX_BYTES: usize = 32 * 1024;

pub struct FileRead;

impl FileRead {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileRead {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for FileRead {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file from the user's home directory or the OS \
         temp folder. Returns the file contents as text (up to 32 KB; \
         longer files are tail-truncated with a marker). Paths outside \
         the allowed roots (system directories, removable drives, etc.) \
         are refused. Use for inspecting the user's own notes, configs, \
         and scratch files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or `~`-prefixed path. Must resolve under the user's home directory or the OS temp folder."
                }
            },
            "required": ["path"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let path_str = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("file_read: missing required `path` argument"))?;
            let safe = match resolve_safe_path(path_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("file_read: {e}"))),
            };
            let bytes = match std::fs::read(&safe) {
                Ok(b) => b,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "file_read: cannot read `{}`: {e}",
                        safe.display()
                    )));
                }
            };
            let total_bytes = bytes.len();
            let (text, truncated) = if total_bytes > MAX_BYTES {
                let tail = &bytes[total_bytes - MAX_BYTES..];
                let s = String::from_utf8_lossy(tail).into_owned();
                (s, true)
            } else {
                let s = String::from_utf8_lossy(&bytes).into_owned();
                (s, false)
            };
            let content = if truncated {
                format!("(truncated; showing the last {MAX_BYTES} of {total_bytes} bytes)\n{text}")
            } else {
                text
            };
            Ok(ToolResult {
                content,
                display_summary: Some(format!("קראתי {total_bytes} בייטים")),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(FileRead.name(), "file_read");
        assert!(!FileRead.requires_confirmation(&json!({"path": "x"})));
    }

    #[test]
    fn missing_path_argument_errors() {
        let err = rt()
            .block_on(FileRead.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn refuses_path_outside_allowed_roots() {
        let bad = if cfg!(target_os = "windows") {
            r"C:\Windows\System32\drivers\etc\hosts"
        } else {
            "/etc/passwd"
        };
        let result = rt()
            .block_on(FileRead.execute(&json!({"path": bad})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("outside the allowed roots"));
    }

    #[test]
    fn reads_a_temp_file_round_trip() {
        let path = std::env::temp_dir().join("lashon-file_read-test.txt");
        let _ = std::fs::remove_file(&path);
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all("שלום, world\n".as_bytes()).unwrap();
        }
        let result = rt()
            .block_on(FileRead.execute(&json!({"path": path.to_str().unwrap()})))
            .unwrap();
        assert!(result.content.contains("שלום"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn truncates_files_over_the_cap() {
        let path = std::env::temp_dir().join("lashon-file_read-big.txt");
        let _ = std::fs::remove_file(&path);
        let body = "A".repeat(MAX_BYTES + 1024);
        std::fs::write(&path, body.as_bytes()).unwrap();
        let result = rt()
            .block_on(FileRead.execute(&json!({"path": path.to_str().unwrap()})))
            .unwrap();
        assert!(result.content.starts_with("(truncated"));
        let _ = std::fs::remove_file(&path);
    }
}
