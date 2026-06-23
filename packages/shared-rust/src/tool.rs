//! The Command-mode tool abstraction (`docs/roadmap.md §2.2`).
//!
//! Each native action Lashon can take in Command mode — opening an app,
//! typing text, copying to the clipboard, opening a URL — implements
//! `LashonTool`. The registry collects them; `command_mode` serialises
//! their schemas into the `llm::Tool` shape the active LLM provider
//! expects and dispatches the tool calls the model emits.
//!
//! The trait is deliberately small. Each tool is a pure `Send + Sync`
//! handle with:
//!
//! - `name()` / `description()` — what the LLM sees.
//! - `parameters()` — a JSON Schema describing the args; both Anthropic
//!   (`input_schema`) and OpenAI (`function.parameters`) accept this.
//! - `execute(args)` — runs the action; returns a `ToolResult` whose
//!   `content` is fed back to the LLM as the `tool_result` message.
//! - `requires_confirmation(args)` — whether the dispatcher must gate on
//!   a user yes/no before executing (docs/roadmap.md §2.6).
//!
//! Tools never hold their own state across calls. Anything cross-call
//! (e.g. the M12 `remember` tool's SQLite handle) lives behind an `Arc`
//! inside the tool struct.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::llm::{BoxFuture, Tool as LlmTool};

/// The result of running a tool. `content` is fed back to the LLM as the
/// `tool_result` message content; `display_summary` is what the tongue
/// flashes to the user (short, Hebrew-friendly).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// What the LLM sees on its next turn. Stringified — JSON, plain
    /// text, or a status report. Never carries the user's raw audio.
    pub content: String,
    /// A short human-readable summary the tongue flashes — e.g.
    /// `"פתחתי את VS Code"`. `None` means the tool ran but had nothing
    /// presentable to show.
    pub display_summary: Option<String>,
}

impl ToolResult {
    /// Convenience: build a `ToolResult` from a single string used for
    /// both the LLM `content` and the user-visible flash.
    pub fn ok(text: impl Into<String>) -> Self {
        let text: String = text.into();
        Self {
            content: text.clone(),
            display_summary: Some(text),
        }
    }

    /// Convenience: build a tool-result that the user need not see —
    /// `clipboard_get` for instance reads silently and the LLM uses the
    /// content to compose the next step.
    pub fn silent(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            display_summary: None,
        }
    }

    /// Convenience: an error report that the LLM can read back and try
    /// to repair from. The dispatcher already logs and toasts errors
    /// out-of-band; this is the message the model sees.
    pub fn error(message: impl Into<String>) -> Self {
        let message: String = message.into();
        Self {
            content: format!("error: {message}"),
            display_summary: Some(format!("שגיאה: {message}")),
        }
    }
}

/// A confirmation decision returned by the user via the tongue's modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfirmDecision {
    Allow,
    Deny,
}

/// A tool Lashon can call in Command mode. Concrete impls live under
/// `lashon-core::tools::*` (`open_app`, `type_text`, `press_keys`,
/// `clipboard_get`, `clipboard_set`, `open_url`, `web_search`,
/// `focus_window`).
pub trait LashonTool: Send + Sync {
    /// The wire name the LLM uses to invoke this tool. Conventions:
    /// snake_case, English, stable across versions (the model picks
    /// tools by name).
    fn name(&self) -> &str;

    /// A one-line description the LLM reads when deciding whether to
    /// call this tool. English; the LLM translates as it needs to.
    fn description(&self) -> &str;

    /// JSON Schema for the `arguments` object. Both Anthropic and OpenAI
    /// accept JSON Schema here; the same `Value` is forwarded verbatim.
    fn parameters(&self) -> Value;

    /// Whether this call must be gated on user confirmation
    /// (docs/roadmap.md §2.6). The Phase-1 toolset returns `false`
    /// uniformly; destructive tools (file_delete, shutdown, send_message)
    /// in M8.2 override to `true`.
    fn requires_confirmation(&self, _args: &Value) -> bool {
        false
    }

    /// Run the action. `args` is the model's `tool_calls[].function.arguments`
    /// (OpenAI) or `tool_use.input` (Anthropic), both parsed to `Value`.
    /// Errors bubble back to the dispatcher, which feeds them to the LLM
    /// as a `tool_result` with `content: "error: …"`.
    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>>;
}

/// A collection of tools, looked up by name. The dispatcher constructs
/// this once per Command-mode session and reads from it; tools are
/// stateless across calls so concurrent dispatches share the registry.
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn LashonTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Panics if the name collides — caller is
    /// configuring the registry at startup and should catch duplicates
    /// at review time.
    pub fn register(&mut self, tool: Arc<dyn LashonTool>) {
        let name = tool.name().to_string();
        if self.tools.contains_key(&name) {
            panic!("duplicate tool registration: {name}");
        }
        self.tools.insert(name, tool);
    }

    /// Look up a tool by the name the LLM emitted.
    pub fn get(&self, name: &str) -> Option<Arc<dyn LashonTool>> {
        self.tools.get(name).cloned()
    }

    /// Every registered tool, in name-sorted order.
    pub fn all(&self) -> Vec<Arc<dyn LashonTool>> {
        let mut tools: Vec<Arc<dyn LashonTool>> = self.tools.values().cloned().collect();
        tools.sort_by(|a, b| a.name().cmp(b.name()));
        tools
    }

    /// Serialise the registry into the `Vec<llm::Tool>` shape the
    /// LLMProvider trait expects. The dispatcher hands this to every
    /// `chat()` call.
    pub fn to_llm_tools(&self) -> Vec<LlmTool> {
        self.all()
            .into_iter()
            .map(|tool| LlmTool {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }

    /// Names of every registered tool — used in the system-prompt
    /// builder for fallback debugging and in tests.
    pub fn names(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .map(|t| t.name().to_string())
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A trait-conforming mock for dispatcher tests. The next-result
    /// queue is read in FIFO order so the test can stage a chain of
    /// tool calls without async plumbing.
    pub struct MockTool {
        pub name_value: &'static str,
        pub description_value: &'static str,
        pub parameters_value: Value,
        pub confirm: bool,
        pub result: ToolResult,
    }

    impl MockTool {
        pub fn echo(name: &'static str, description: &'static str) -> Self {
            Self {
                name_value: name,
                description_value: description,
                parameters_value: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"}
                    },
                    "required": ["text"]
                }),
                confirm: false,
                result: ToolResult::ok("ok"),
            }
        }
    }

    impl LashonTool for MockTool {
        fn name(&self) -> &str {
            self.name_value
        }
        fn description(&self) -> &str {
            self.description_value
        }
        fn parameters(&self) -> Value {
            self.parameters_value.clone()
        }
        fn requires_confirmation(&self, _args: &Value) -> bool {
            self.confirm
        }
        fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
            let result = self.result.clone();
            Box::pin(async move { Ok(result) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MockTool;
    use super::*;

    #[test]
    fn tool_result_ok_carries_text_in_both_fields() {
        let r = ToolResult::ok("פתחתי את VS Code");
        assert_eq!(r.content, "פתחתי את VS Code");
        assert_eq!(r.display_summary.as_deref(), Some("פתחתי את VS Code"));
    }

    #[test]
    fn tool_result_silent_has_no_display_summary() {
        let r = ToolResult::silent("clipboard content");
        assert!(r.display_summary.is_none());
        assert_eq!(r.content, "clipboard content");
    }

    #[test]
    fn tool_result_error_prefixes_content_and_translates_summary() {
        let r = ToolResult::error("path not found");
        assert!(r.content.starts_with("error:"));
        assert!(r.display_summary.unwrap().starts_with("שגיאה:"));
    }

    #[test]
    fn registry_register_and_lookup() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("echo_a", "Echo A")));
        registry.register(Arc::new(MockTool::echo("echo_b", "Echo B")));
        assert!(registry.get("echo_a").is_some());
        assert!(registry.get("echo_b").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn registry_names_are_sorted() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("z_last", "")));
        registry.register(Arc::new(MockTool::echo("a_first", "")));
        registry.register(Arc::new(MockTool::echo("m_middle", "")));
        assert_eq!(
            registry.names(),
            vec![
                "a_first".to_string(),
                "m_middle".to_string(),
                "z_last".to_string()
            ]
        );
    }

    #[test]
    #[should_panic(expected = "duplicate tool registration")]
    fn registry_panics_on_duplicate_name() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("dup", "first")));
        registry.register(Arc::new(MockTool::echo("dup", "second")));
    }

    #[test]
    fn registry_serialises_to_llm_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("open_app", "Open an app")));
        let llm_tools = registry.to_llm_tools();
        assert_eq!(llm_tools.len(), 1);
        assert_eq!(llm_tools[0].name, "open_app");
        assert_eq!(llm_tools[0].description, "Open an app");
        // The parameters round-trip — both Anthropic's `input_schema` and
        // OpenAI's `function.parameters` accept the same JSON Schema shape.
        assert_eq!(llm_tools[0].parameters["type"], "object");
    }

    #[test]
    fn mock_tool_execute_returns_canned_result() {
        let tool = MockTool::echo("echo", "Echo back");
        let args = serde_json::json!({"text": "hello"});
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(tool.execute(&args)).unwrap();
        assert_eq!(result.content, "ok");
    }
}
