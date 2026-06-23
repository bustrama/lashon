//! `file_write` — atomic write of UTF-8 text into a file under one of
//! the allowed roots. Creates parent directories on demand; writes to a
//! temp sibling then renames so a partial write cannot leave a
//! half-truncated file in place.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct FileWrite;

impl FileWrite {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FileWrite {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for FileWrite {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write text content to a file under the user's home directory or \
         the OS temp folder. Overwrites the file if it exists. Parent \
         directories are created on demand. The write is atomic — a \
         crash mid-write leaves either the old content or the new, \
         never a partial mix. Paths outside the allowed roots are \
         refused. Requires user confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or `~`-prefixed path. Must resolve under the user's home directory or the OS temp folder."
                },
                "content": {
                    "type": "string",
                    "description": "The UTF-8 text to write. Replaces the file's current content in full."
                }
            },
            "required": ["path", "content"]
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
                .ok_or_else(|| anyhow!("file_write: missing required `path` argument"))?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("file_write: missing required `content` argument"))?;
            let safe = match resolve_safe_path(path_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("file_write: {e}"))),
            };
            if let Some(parent) = safe.parent() {
                if !parent.as_os_str().is_empty() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return Ok(ToolResult::error(format!(
                            "file_write: cannot create parent `{}`: {e}",
                            parent.display()
                        )));
                    }
                }
            }
            // Atomic write: tmp file in the same directory, then rename.
            // Same-directory is important — cross-volume renames silently
            // degrade to copy+remove and lose atomicity guarantees.
            let tmp = match temp_sibling(&safe) {
                Ok(t) => t,
                Err(e) => return Ok(ToolResult::error(format!("file_write: {e}"))),
            };
            if let Err(e) = std::fs::write(&tmp, content.as_bytes()) {
                let _ = std::fs::remove_file(&tmp);
                return Ok(ToolResult::error(format!(
                    "file_write: tmp write failed: {e}"
                )));
            }
            if let Err(e) = std::fs::rename(&tmp, &safe) {
                let _ = std::fs::remove_file(&tmp);
                return Ok(ToolResult::error(format!("file_write: rename failed: {e}")));
            }
            let bytes = content.len();
            Ok(ToolResult {
                content: format!("wrote {bytes} bytes to {}", safe.display()),
                display_summary: Some(format!("כתבתי {bytes} בייטים")),
            })
        })
    }
}

/// Generate a same-directory `.tmp.<rand>` sibling so the rename is
/// guaranteed to stay on the same volume.
fn temp_sibling(target: &std::path::Path) -> Result<std::path::PathBuf> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", target.display()))?;
    let stem = target
        .file_name()
        .ok_or_else(|| anyhow!("path has no file name: {}", target.display()))?
        .to_string_lossy()
        .into_owned();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(parent.join(format!(".{stem}.lashon.tmp.{nanos}")))
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
        assert_eq!(FileWrite.name(), "file_write");
        assert!(FileWrite.requires_confirmation(&json!({})));
    }

    #[test]
    fn missing_args_error() {
        let err = rt()
            .block_on(FileWrite.execute(&json!({})))
            .err()
            .expect("missing args must error");
        assert!(err.to_string().contains("path"));
        let err = rt()
            .block_on(FileWrite.execute(&json!({"path": "x"})))
            .err()
            .expect("missing content must error");
        assert!(err.to_string().contains("content"));
    }

    #[test]
    fn refuses_path_outside_allowed_roots() {
        let bad = if cfg!(target_os = "windows") {
            r"C:\Windows\System32\evil.txt"
        } else {
            "/etc/lashon-evil"
        };
        let result = rt()
            .block_on(FileWrite.execute(&json!({"path": bad, "content": "x"})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("outside the allowed roots"));
    }

    #[test]
    fn writes_round_trip_in_temp() {
        let path = std::env::temp_dir().join("lashon-file_write-test.txt");
        let _ = std::fs::remove_file(&path);
        let result = rt()
            .block_on(
                FileWrite
                    .execute(&json!({"path": path.to_str().unwrap(), "content": "שלום world\n"})),
            )
            .unwrap();
        assert!(!result.content.starts_with("error:"));
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, "שלום world\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn creates_parent_dirs_on_demand() {
        let dir = std::env::temp_dir().join("lashon-file_write-nested-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("a/b/c/deep.txt");
        let result = rt()
            .block_on(FileWrite.execute(&json!({"path": path.to_str().unwrap(), "content": "x"})))
            .unwrap();
        assert!(!result.content.starts_with("error:"), "{}", result.content);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
