//! `new_browser_tab` — open the default browser at `url` (or
//! `about:blank` when omitted). Uses the `open` crate (already a
//! Phase-1 dep for `open_url`); on platforms where the browser is
//! running, `start` / `open` / `xdg-open` produces a new tab in the
//! existing window — which is the user's intent.

use anyhow::Result;
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

const BLANK: &str = "about:blank";

pub struct NewBrowserTab;

impl NewBrowserTab {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NewBrowserTab {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for NewBrowserTab {
    fn name(&self) -> &str {
        "new_browser_tab"
    }

    fn description(&self) -> &str {
        "Open a new browser tab. Defaults to `about:blank`; an optional \
         `url` lands the tab on a specific page. Adds a tab to the \
         already-running browser when one exists, otherwise launches \
         the default browser."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Optional destination URL — `https://…`, `mailto:…`, etc. Defaults to `about:blank`."
                }
            }
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(BLANK);
            match open::that(url) {
                Ok(_) => Ok(ToolResult {
                    content: format!("opened new tab at {url}"),
                    display_summary: Some("פתחתי לשונית חדשה".into()),
                }),
                Err(e) => Ok(ToolResult::error(format!(
                    "new_browser_tab: failed to open `{url}`: {e}"
                ))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(NewBrowserTab.name(), "new_browser_tab");
        assert!(!NewBrowserTab.requires_confirmation(&json!({})));
    }

    #[test]
    fn parameters_have_optional_url() {
        let p = NewBrowserTab.parameters();
        assert_eq!(p["type"], "object");
        // `url` is optional — no `required` field.
        assert!(p.get("required").is_none());
        assert_eq!(p["properties"]["url"]["type"], "string");
    }

    // We don't actually invoke `open::that` in tests — it would spawn a
    // real browser tab on every CI run. The trait shape is what matters.
}
