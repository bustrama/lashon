//! `show_notification` — fire a desktop notification via `notify-rust`
//! (Win32 toast / macOS NSUserNotification / Linux libnotify). Lives in
//! lashon-core for parity with the rest of the catalogue — the Tauri
//! shell stays thin per `.claude/rules/architecture.md`.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct ShowNotification;

impl ShowNotification {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShowNotification {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ShowNotification {
    fn name(&self) -> &str {
        "show_notification"
    }

    fn description(&self) -> &str {
        "Show a desktop notification. The OS surfaces it in the system \
         notification centre (Action Center on Windows, Notification \
         Center on macOS, libnotify on Linux). Use to remind the user \
         of something asynchronously or to confirm a long action has \
         finished."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Headline. Hebrew is supported."
                },
                "body": {
                    "type": "string",
                    "description": "Body text. Hebrew is supported. Keep it short — many notification UIs truncate aggressively."
                }
            },
            "required": ["title", "body"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("show_notification: missing required `title` argument"))?;
            let body = args
                .get("body")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("show_notification: missing required `body` argument"))?;
            match notify_rust::Notification::new()
                .summary(title)
                .body(body)
                .show()
            {
                Ok(_) => Ok(ToolResult {
                    content: format!("notification shown: {title}"),
                    display_summary: Some("שלחתי התראה".into()),
                }),
                Err(e) => Ok(ToolResult::error(format!("show_notification: {e}"))),
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
        assert_eq!(ShowNotification.name(), "show_notification");
        assert!(!ShowNotification.requires_confirmation(&json!({"title": "x", "body": "y"})));
    }

    #[test]
    fn missing_args_error() {
        let err = rt()
            .block_on(ShowNotification.execute(&json!({})))
            .err()
            .expect("missing args must error");
        assert!(err.to_string().contains("title"));
        let err = rt()
            .block_on(ShowNotification.execute(&json!({"title": "x"})))
            .err()
            .expect("missing body must error");
        assert!(err.to_string().contains("body"));
    }

    #[test]
    fn parameters_describe_required_fields() {
        let p = ShowNotification.parameters();
        assert_eq!(p["properties"]["title"]["type"], "string");
        assert_eq!(p["properties"]["body"]["type"], "string");
        let required = p["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
    }
}
