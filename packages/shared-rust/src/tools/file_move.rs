//! `file_move` — rename a file from one allowed-root path to another.
//! Falls back to copy + delete when the rename would cross volumes
//! (Windows: different drive letters; Linux: different mount points).

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct FileMove;

impl FileMove {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileMove {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for FileMove {
    fn name(&self) -> &str {
        "file_move"
    }

    fn description(&self) -> &str {
        "Move or rename a file. Both source and destination must resolve \
         under the user's home directory or the OS temp folder. The \
         operation is `rename` when both sides are on the same volume, \
         otherwise `copy + delete` (still atomic per-file: the source is \
         removed only after the destination write succeeds). Requires \
         user confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "from": {
                    "type": "string",
                    "description": "Source file path. Must exist and resolve under an allowed root."
                },
                "to": {
                    "type": "string",
                    "description": "Destination path. Parent directories are NOT auto-created — use `file_write` first if you need a new tree."
                }
            },
            "required": ["from", "to"]
        })
    }

    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let from_str = args
                .get("from")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("file_move: missing required `from` argument"))?;
            let to_str = args
                .get("to")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("file_move: missing required `to` argument"))?;
            let from = match resolve_safe_path(from_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("file_move: from: {e}"))),
            };
            let to = match resolve_safe_path(to_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("file_move: to: {e}"))),
            };
            let meta = match std::fs::symlink_metadata(&from) {
                Ok(m) => m,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "file_move: cannot stat `{}`: {e}",
                        from.display()
                    )));
                }
            };
            if meta.is_dir() {
                return Ok(ToolResult::error(format!(
                    "file_move: `{}` is a directory; this tool only \
                     moves single files",
                    from.display()
                )));
            }
            match std::fs::rename(&from, &to) {
                Ok(()) => Ok(ToolResult {
                    content: format!("moved {} → {}", from.display(), to.display()),
                    display_summary: Some(format!(
                        "העברתי את {}",
                        from.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| from.display().to_string())
                    )),
                }),
                Err(_) => {
                    // Cross-volume — fall back to copy + delete. We only
                    // remove the source after the copy survives.
                    if let Err(e) = std::fs::copy(&from, &to) {
                        return Ok(ToolResult::error(format!(
                            "file_move: cross-volume copy failed: {e}"
                        )));
                    }
                    if let Err(e) = std::fs::remove_file(&from) {
                        // Destination already exists; warn rather than
                        // erroring so the user isn't left with a dupe
                        // and no way to know.
                        return Ok(ToolResult {
                            content: format!(
                                "copied {} → {}, but could not remove source: {e}",
                                from.display(),
                                to.display()
                            ),
                            display_summary: Some("הועתק; מחיקה נכשלה".into()),
                        });
                    }
                    Ok(ToolResult {
                        content: format!(
                            "moved (copy+delete) {} → {}",
                            from.display(),
                            to.display()
                        ),
                        display_summary: Some("העברתי בין כוננים".into()),
                    })
                }
            }
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
        assert_eq!(FileMove.name(), "file_move");
        assert!(FileMove.requires_confirmation(&json!({})));
    }

    #[test]
    fn missing_args_error() {
        let err = rt()
            .block_on(FileMove.execute(&json!({})))
            .err()
            .expect("missing args must error");
        assert!(err.to_string().contains("from"));
        let err = rt()
            .block_on(FileMove.execute(&json!({"from": "x"})))
            .err()
            .expect("missing to must error");
        assert!(err.to_string().contains("to"));
    }

    #[test]
    fn refuses_paths_outside_allowed_roots() {
        let from = if cfg!(target_os = "windows") {
            r"C:\Windows\System32\notepad.exe"
        } else {
            "/etc/hostname"
        };
        let tmp = std::env::temp_dir().join("lashon-file_move-target");
        let result = rt()
            .block_on(FileMove.execute(&json!({"from": from, "to": tmp.to_str().unwrap()})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("outside the allowed roots"));
    }

    #[test]
    fn moves_a_temp_file() {
        let from = std::env::temp_dir().join("lashon-file_move-src.txt");
        let to = std::env::temp_dir().join("lashon-file_move-dst.txt");
        let _ = std::fs::remove_file(&from);
        let _ = std::fs::remove_file(&to);
        std::fs::write(&from, "payload").unwrap();
        let result = rt()
            .block_on(
                FileMove
                    .execute(&json!({"from": from.to_str().unwrap(), "to": to.to_str().unwrap()})),
            )
            .unwrap();
        assert!(!result.content.starts_with("error:"), "{}", result.content);
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");
        let _ = std::fs::remove_file(&to);
    }
}
