//! `file_delete` — remove a single file under one of the allowed roots.
//! Refuses directories in this PR; the LLM has to tell the user to
//! delete folders manually until a follow-up adds a recursive variant
//! gated on extra confirmation.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct FileDelete;

impl FileDelete {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileDelete {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for FileDelete {
    fn name(&self) -> &str {
        "file_delete"
    }

    fn description(&self) -> &str {
        "Delete a single file under the user's home directory or the OS \
         temp folder. Refuses directories — the user must remove folders \
         manually. Paths outside the allowed roots are refused. Requires \
         user confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or `~`-prefixed path to a single file. Must resolve under the user's home directory or the OS temp folder."
                }
            },
            "required": ["path"]
        })
    }

    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let path_str = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("file_delete: missing required `path` argument"))?;
            let safe = match resolve_safe_path(path_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("file_delete: {e}"))),
            };
            let meta = match std::fs::symlink_metadata(&safe) {
                Ok(m) => m,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "file_delete: cannot stat `{}`: {e}",
                        safe.display()
                    )));
                }
            };
            if meta.is_dir() {
                return Ok(ToolResult::error(format!(
                    "file_delete: `{}` is a directory; this tool only \
                     removes single files",
                    safe.display()
                )));
            }
            if let Err(e) = std::fs::remove_file(&safe) {
                return Ok(ToolResult::error(format!(
                    "file_delete: remove failed: {e}"
                )));
            }
            Ok(ToolResult {
                content: format!("deleted {}", safe.display()),
                display_summary: Some(format!(
                    "מחקתי את {}",
                    safe.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| safe.display().to_string())
                )),
            })
        })
    }
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
        assert_eq!(FileDelete.name(), "file_delete");
        assert!(FileDelete.requires_confirmation(&json!({})));
    }

    #[test]
    fn missing_path_argument_errors() {
        let err = rt()
            .block_on(FileDelete.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn refuses_path_outside_allowed_roots() {
        let bad = if cfg!(target_os = "windows") {
            r"C:\Windows\System32\notepad.exe"
        } else {
            "/etc/hostname"
        };
        let result = rt()
            .block_on(FileDelete.execute(&json!({"path": bad})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("outside the allowed roots"));
    }

    #[test]
    fn refuses_directories() {
        let dir = std::env::temp_dir().join("lashon-file_delete-dir-test");
        let _ = std::fs::create_dir_all(&dir);
        let result = rt()
            .block_on(FileDelete.execute(&json!({"path": dir.to_str().unwrap()})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("directory"));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn deletes_a_temp_file() {
        let path = std::env::temp_dir().join("lashon-file_delete-target.txt");
        std::fs::write(&path, "to-go").unwrap();
        let result = rt()
            .block_on(FileDelete.execute(&json!({"path": path.to_str().unwrap()})))
            .unwrap();
        assert!(!result.content.starts_with("error:"), "{}", result.content);
        assert!(!path.exists());
    }
}
