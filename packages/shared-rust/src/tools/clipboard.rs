//! `clipboard_get` / `clipboard_set` — read or write the system clipboard.
//! `arboard` is already a Phase-1 dep (the dictation injector uses it
//! for the Hebrew clipboard path); we reuse it here.

use anyhow::{anyhow, Result};
use arboard::Clipboard;
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Read the clipboard's current text and feed it back to the LLM.
pub struct ClipboardGet;

impl ClipboardGet {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardGet {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ClipboardGet {
    fn name(&self) -> &str {
        "clipboard_get"
    }

    fn description(&self) -> &str {
        "Read the current text content of the system clipboard. Useful \
         when the user says 'paraphrase what's on the clipboard' or 'send \
         the clipboard to John'."
    }

    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let mut clipboard = Clipboard::new()
                .map_err(|e| anyhow!("clipboard_get: cannot open clipboard: {e}"))?;
            match clipboard.get_text() {
                Ok(text) => {
                    // Silent — the LLM consumes the content; the user
                    // doesn't need a flash echoing their own clipboard.
                    Ok(ToolResult::silent(text))
                }
                Err(arboard::Error::ContentNotAvailable) => {
                    Ok(ToolResult::silent("(clipboard is empty or non-text)"))
                }
                Err(e) => Err(anyhow!("clipboard_get: read failed: {e}")),
            }
        })
    }
}

/// Replace the clipboard's text content.
pub struct ClipboardSet;

impl ClipboardSet {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardSet {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ClipboardSet {
    fn name(&self) -> &str {
        "clipboard_set"
    }

    fn description(&self) -> &str {
        "Write text to the system clipboard. Useful when the user says \
         'copy this to clipboard' or as a step before pasting elsewhere."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to copy to the clipboard."
                }
            },
            "required": ["text"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("clipboard_set: missing required `text` argument"))?;
            let mut clipboard = Clipboard::new()
                .map_err(|e| anyhow!("clipboard_set: cannot open clipboard: {e}"))?;
            clipboard
                .set_text(text.to_string())
                .map_err(|e| anyhow!("clipboard_set: write failed: {e}"))?;
            Ok(ToolResult {
                content: format!("copied {} chars", text.chars().count()),
                display_summary: Some("העתקתי ללוח".into()),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_set_requires_text() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(ClipboardSet.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("text"));
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(ClipboardGet.name(), "clipboard_get");
        assert_eq!(ClipboardSet.name(), "clipboard_set");
        assert!(!ClipboardGet.requires_confirmation(&json!({})));
        assert!(!ClipboardSet.requires_confirmation(&json!({"text": "x"})));
    }

    #[test]
    fn clipboard_get_has_empty_object_schema() {
        let params = ClipboardGet.parameters();
        assert_eq!(params["type"], "object");
        // No required fields.
        assert!(params.get("required").is_none());
    }
}
