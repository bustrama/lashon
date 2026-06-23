//! `list_files` — directory listing under an allowed root, optionally
//! filtered by a shell-style glob (`*.txt`, `screenshot*.png`).

use anyhow::{anyhow, Result};
use glob::Pattern;
use serde_json::{json, Value};

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Cap on the number of entries we surface. A user's Downloads folder
/// with 5 000 files would otherwise drown the LLM in noise; the model
/// can ask the user for a more specific glob.
const MAX_ENTRIES: usize = 200;

pub struct ListFiles;

impl ListFiles {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListFiles {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ListFiles {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List entries in a directory under the user's home directory or \
         the OS temp folder. Optionally filtered by a shell-style glob \
         like `*.png`. Returns up to 200 entries; longer listings get a \
         truncation marker. Use to discover the file the user just \
         mentioned (`'the screenshot in my Downloads'`)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or `~`-prefixed directory path. Must resolve under the user's home directory or the OS temp folder."
                },
                "pattern": {
                    "type": "string",
                    "description": "Optional shell-style glob (`*.gguf`, `screenshot*`). Matched against the file name only, not the full path."
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
                .ok_or_else(|| anyhow!("list_files: missing required `path` argument"))?;
            let pattern = args.get("pattern").and_then(|v| v.as_str());
            let safe = match resolve_safe_path(path_str) {
                Ok(p) => p,
                Err(e) => return Ok(ToolResult::error(format!("list_files: {e}"))),
            };
            if !safe.is_dir() {
                return Ok(ToolResult::error(format!(
                    "list_files: `{}` is not a directory",
                    safe.display()
                )));
            }
            let matcher = match pattern {
                Some(p) => match Pattern::new(p) {
                    Ok(pat) => Some(pat),
                    Err(e) => {
                        return Ok(ToolResult::error(format!(
                            "list_files: invalid glob `{p}`: {e}"
                        )));
                    }
                },
                None => None,
            };
            let read_dir = match std::fs::read_dir(&safe) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "list_files: cannot read `{}`: {e}",
                        safe.display()
                    )));
                }
            };
            let mut entries: Vec<String> = Vec::new();
            let mut truncated = false;
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(m) = matcher.as_ref() {
                    if !m.matches(&name) {
                        continue;
                    }
                }
                let kind = match entry.file_type() {
                    Ok(t) if t.is_dir() => "d",
                    Ok(t) if t.is_symlink() => "l",
                    _ => "f",
                };
                entries.push(format!("{kind} {name}"));
                if entries.len() >= MAX_ENTRIES {
                    truncated = true;
                    break;
                }
            }
            entries.sort();
            let count = entries.len();
            let mut content = entries.join("\n");
            if truncated {
                content.push_str(&format!("\n(truncated at {MAX_ENTRIES} entries)"));
            }
            if content.is_empty() {
                content = String::from("(no entries)");
            }
            Ok(ToolResult {
                content,
                display_summary: Some(format!("מצאתי {count} פריטים")),
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
        assert_eq!(ListFiles.name(), "list_files");
        assert!(!ListFiles.requires_confirmation(&json!({})));
    }

    #[test]
    fn missing_path_argument_errors() {
        let err = rt()
            .block_on(ListFiles.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("path"));
    }

    #[test]
    fn refuses_path_outside_allowed_roots() {
        let bad = if cfg!(target_os = "windows") {
            r"C:\Windows\System32"
        } else {
            "/etc"
        };
        let result = rt()
            .block_on(ListFiles.execute(&json!({"path": bad})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
    }

    #[test]
    fn lists_a_temp_dir_and_filters_by_glob() {
        let dir = std::env::temp_dir().join("lashon-list_files-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alpha.txt"), "a").unwrap();
        std::fs::write(dir.join("beta.md"), "b").unwrap();
        std::fs::write(dir.join("gamma.txt"), "c").unwrap();
        let result_all = rt()
            .block_on(ListFiles.execute(&json!({"path": dir.to_str().unwrap()})))
            .unwrap();
        assert!(result_all.content.contains("alpha.txt"));
        assert!(result_all.content.contains("beta.md"));

        let result_glob = rt()
            .block_on(
                ListFiles.execute(&json!({"path": dir.to_str().unwrap(), "pattern": "*.txt"})),
            )
            .unwrap();
        assert!(result_glob.content.contains("alpha.txt"));
        assert!(result_glob.content.contains("gamma.txt"));
        assert!(!result_glob.content.contains("beta.md"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_glob() {
        let dir = std::env::temp_dir();
        let result = rt()
            .block_on(
                ListFiles.execute(&json!({"path": dir.to_str().unwrap(), "pattern": "[invalid"})),
            )
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("glob"));
    }
}
