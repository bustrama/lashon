//! `type_text` — type text at the focused cursor. Reuses Phase 1's
//! `inject::inject_text`, which already handles the Hebrew clipboard
//! path with combining-mark integrity (`docs/architecture.md`,
//! `.claude/rules/hebrew.md`).

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::inject::inject_text;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct TypeText;

impl TypeText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TypeText {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for TypeText {
    fn name(&self) -> &str {
        "type_text"
    }

    fn description(&self) -> &str {
        "Type text at the user's current cursor position. The text is \
         inserted into whatever app is focused — supports Hebrew with \
         correct RTL ordering."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "The text to type. Hebrew is supported."
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
                .ok_or_else(|| anyhow!("type_text: missing required `text` argument"))?;
            // The injection itself is synchronous and runs quickly enough
            // that we don't bother offloading to a blocking pool.
            inject_text(text)?;
            // Don't include the text in the user-visible summary —
            // dictation content is private (`.claude/rules/security.md`),
            // and the LLM already echoed an action description.
            Ok(ToolResult {
                content: format!("typed {} chars", text.chars().count()),
                display_summary: Some(format!("הקלדתי {} תווים", text.chars().count())),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_describe_text_argument() {
        let params = TypeText.parameters();
        assert_eq!(params["properties"]["text"]["type"], "string");
        assert_eq!(params["required"][0], "text");
    }

    #[test]
    fn missing_text_argument_errors() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(TypeText.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("text"));
    }

    #[test]
    fn metadata_matches_spec() {
        let tool = TypeText::new();
        assert_eq!(tool.name(), "type_text");
        assert!(!tool.requires_confirmation(&json!({"text": "anything"})));
    }
}
