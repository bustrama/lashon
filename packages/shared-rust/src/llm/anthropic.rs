//! Anthropic Messages API impl of `LLMProvider` (docs/adr/0019). Anthropic is
//! the lone vendor with its own wire format; every OpenAI-compatible
//! provider (OpenAI, Groq, DeepSeek, Mistral, Together AI, OpenRouter,
//! MiniMax, Ollama) is served by the parameterised `OpenAiCompatLlmProvider`.
//!
//! The translation between Lashon's vendor-neutral `Msg`/`Tool` and the
//! Messages API wire format lives entirely in this file — callers never see
//! Anthropic-specific structs.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    BoxFuture, Completion, ContentBlock, LLMProvider, Msg, MsgContent, RemoteModels, Role, Token,
    TokenStream, Tool, Usage, DEFAULT_MAX_TOKENS, REMOTE_MODELS_CAP,
};
use crate::provider::{Confidence, ProviderError};

/// Default endpoint. Override via `LLMProviderConfig.base_url` for
/// Anthropic-compatible proxies or the `llm.anthropic.base_url` setting.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// The Messages API version Lashon was authored against (the `anthropic-version`
/// header is required on every call). Bump together with any wire-format
/// migration; pinned so a vendor change does not surprise an installed Lashon.
pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Default model id. Aligned with the user's repo guidance — Claude Sonnet 4.6
/// is the best general-purpose coding model on the family today.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Static catalogue of available models. The Hub renders this list in the
/// model-picker dropdown. Order matters — first entry is the default in
/// the picker when no explicit `llm.anthropic.model` is saved.
pub const AVAILABLE_MODELS: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-opus-4-7",
    "claude-haiku-4-5-20251001",
];

/// The Anthropic provider. Constructed lazily — the HTTP client is cheap,
/// the API key is read from the keychain on each `chat` call so a key
/// rotation in the Hub takes effect without restarting the registry.
pub struct AnthropicLlmProvider {
    http: Client,
    base_url: String,
    model: String,
    /// Optional override for the keychain key name — used by tests with
    /// `MockAnthropicLlmProvider` so a real `read_key` call is never made.
    key_name: String,
}

impl AnthropicLlmProvider {
    /// Construct with the production defaults. The base URL and model can be
    /// overridden by the Hub picker; callers that need a custom base call
    /// `with_base_url`.
    pub fn new() -> Self {
        Self {
            http: build_client(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            key_name: "llm.anthropic".to_string(),
        }
    }

    /// Replace the base URL (e.g. an enterprise gateway).
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        if !url.is_empty() {
            self.base_url = url;
        }
        self
    }

    /// Replace the model id sent on the wire.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        if !model.is_empty() {
            self.model = model;
        }
        self
    }
}

impl Default for AnthropicLlmProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn build_client() -> Client {
    Client::builder()
        .user_agent(concat!("lashon/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client construction never fails for our config")
}

impl LLMProvider for AnthropicLlmProvider {
    fn chat<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<Completion>> {
        Box::pin(async move {
            let key = crate::keychain::read_key(&self.key_name)?.ok_or_else(|| {
                ProviderError::KeyNotFound {
                    provider: "anthropic".into(),
                }
            })?;
            let request = build_request(&self.model, messages, tools);
            let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
            let response = self
                .http
                .post(&url)
                .header("x-api-key", &key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header("content-type", "application/json")
                .json(&request)
                .send()
                .await
                .context("anthropic Messages API request")?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(http_error("anthropic", status.as_u16(), &body).into());
            }
            let parsed: AnthropicResponse = serde_json::from_str(&body)
                .with_context(|| format!("parsing Anthropic response: {body}"))?;
            Ok(response_to_completion(parsed))
        })
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<TokenStream<'a>>> {
        // M7 callers (the Hub "test prompt" button) use `chat`; we satisfy
        // the trait by buffering the non-streaming response into a one-token
        // stream. Native SSE support is a follow-up that lands together with
        // M8's Chat mode.
        Box::pin(async move {
            let completion = self.chat(messages, tools).await?;
            let text = completion.content.to_plain_text();
            let stream = futures::stream::iter(vec![
                Ok(Token::Text(text)),
                Ok(Token::Finish {
                    reason: completion.finish_reason,
                    usage: completion.usage,
                }),
            ]);
            Ok(Box::pin(stream) as TokenStream<'a>)
        })
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn display_name_key(&self) -> &str {
        "provider.llm.anthropic"
    }

    fn supports_tool_use(&self) -> bool {
        true
    }

    fn supports_hebrew(&self) -> Confidence {
        // Claude handles Hebrew at near-native quality
        // (docs/stories/m7-provider-mux.md Hebrew handling table).
        Confidence::Excellent
    }

    fn context_window(&self) -> usize {
        // 200k tokens across the Claude 4.x family. Strictly informational.
        200_000
    }

    fn is_local(&self) -> bool {
        false
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    fn available_models(&self) -> Vec<String> {
        AVAILABLE_MODELS.iter().map(|s| (*s).to_string()).collect()
    }

    fn has_api_key(&self) -> bool {
        crate::keychain::has_key(&self.key_name)
    }

    fn fetch_remote_models<'a>(&'a self) -> BoxFuture<'a, Result<RemoteModels>> {
        Box::pin(async move {
            let key = crate::keychain::read_key(&self.key_name)?.ok_or_else(|| {
                ProviderError::KeyNotFound {
                    provider: "anthropic".into(),
                }
            })?;
            let url = format!("{}/v1/models", self.base_url.trim_end_matches('/'));
            let response = self
                .http
                .get(&url)
                .header("x-api-key", &key)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .send()
                .await
                .context("anthropic /v1/models request")?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(http_error("anthropic", status.as_u16(), &body).into());
            }
            let parsed: AnthropicModelsListResponse = serde_json::from_str(&body)
                .with_context(|| format!("parsing Anthropic /v1/models: {body}"))?;
            let total = parsed.data.len();
            // Anthropic's `/v1/models` is already chat-only (no embeddings
            // or image models on the platform). Sort by `created_at`
            // descending so newest snapshots come first, then cap at the
            // shared limit — even Anthropic's list will outgrow 30
            // eventually as dated model snapshots accumulate.
            let mut entries: Vec<AnthropicModelEntry> = parsed.data;
            entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            let models: Vec<String> = entries
                .into_iter()
                .take(REMOTE_MODELS_CAP)
                .map(|e| e.id)
                .collect();
            Ok(RemoteModels { models, total })
        })
    }
}

/// Anthropic-specific `/v1/models` payload — `created_at` is an ISO 8601
/// string (the OpenAI shape uses a Unix integer in `created`), so a
/// separate parser keeps the two cleanly typed.
#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicModelsListResponse {
    pub data: Vec<AnthropicModelEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicModelEntry {
    pub id: String,
    /// Optional — older deployments may omit it. Sorting falls through
    /// to lexicographic on the id when the timestamp is missing.
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Build an HTTP-flavoured `ProviderError`. Status 401/403 → `Unauthorized`,
/// 429 → `RateLimited`, everything else → opaque `Http`. The vendor's
/// response body is folded into the user-visible message so a 401 toast
/// reads e.g. "anthropic rejected the request (401): invalid x-api-key"
/// rather than the bare status code.
fn http_error(provider: &str, status: u16, body: &str) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Unauthorized {
            provider: provider.to_string(),
            status,
            message: extract_message(body, 256),
        },
        429 => ProviderError::RateLimited {
            provider: provider.to_string(),
            message: extract_message(body, 256),
        },
        _ => ProviderError::Http {
            provider: provider.to_string(),
            status,
            body: truncate(body, 1024),
        },
    }
}

/// Pull the most-useful message out of a vendor error body. Both
/// Anthropic and OpenAI-shaped errors put a `{"error": {"message": "..."}}`
/// envelope around the human-readable detail; if we find one, we surface
/// just that. Otherwise we truncate the raw body — better to show
/// something than nothing on a 401.
fn extract_message(body: &str, max_chars: usize) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(msg) = value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return truncate(msg, max_chars);
        }
        if let Some(msg) = value.get("message").and_then(|m| m.as_str()) {
            return truncate(msg, max_chars);
        }
    }
    truncate(body, max_chars)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max_chars).collect();
        result.push('…');
        result
    }
}

// --- Wire-format translation -------------------------------------------------

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicResponseBlock>,
    model: String,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicResponseBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    /// A few tool-use error shapes Anthropic may emit — captured here so
    /// `serde` does not bail on the response. Treated as plain assistant
    /// text downstream.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

/// Translate the vendor-neutral request into the Anthropic Messages shape.
/// Public-in-crate so the unit tests exercise it directly.
fn build_request(model: &str, messages: &[Msg], tools: &[Tool]) -> AnthropicRequest {
    // System messages get hoisted to the top-level `system` field —
    // Anthropic does not accept `role: "system"` in the messages array.
    let mut system_parts: Vec<String> = Vec::new();
    let mut anthropic_messages: Vec<AnthropicMessage> = Vec::new();
    for msg in messages {
        match msg.role {
            Role::System => {
                system_parts.push(msg.content.to_plain_text());
            }
            Role::Tool => {
                // A `Role::Tool` message is a tool result. Anthropic carries
                // tool results as a `tool_result` block inside a `user`
                // message — we synthesise that here.
                if let MsgContent::ToolResult { call_id, content } = &msg.content {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".into(),
                        content: AnthropicMessageContent::Blocks(vec![
                            AnthropicContentBlock::ToolResult {
                                tool_use_id: call_id.clone(),
                                content: content.clone(),
                            },
                        ]),
                    });
                }
            }
            Role::User | Role::Assistant => {
                let role = if msg.role == Role::User {
                    "user"
                } else {
                    "assistant"
                };
                let content = match &msg.content {
                    MsgContent::Text { text } => AnthropicMessageContent::Text(text.clone()),
                    MsgContent::ToolResult { call_id, content } => {
                        AnthropicMessageContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                            tool_use_id: call_id.clone(),
                            content: content.clone(),
                        }])
                    }
                    MsgContent::Blocks { blocks } => AnthropicMessageContent::Blocks(
                        blocks
                            .iter()
                            .map(|block| match block {
                                ContentBlock::Text { text } => {
                                    AnthropicContentBlock::Text { text: text.clone() }
                                }
                                ContentBlock::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => AnthropicContentBlock::ToolUse {
                                    id: id.clone(),
                                    name: name.clone(),
                                    input: arguments.clone(),
                                },
                            })
                            .collect(),
                    ),
                };
                anthropic_messages.push(AnthropicMessage {
                    role: role.into(),
                    content,
                });
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    let tools = if tools.is_empty() {
        None
    } else {
        Some(
            tools
                .iter()
                .map(|tool| AnthropicTool {
                    name: tool.name.clone(),
                    description: tool.description.clone(),
                    input_schema: tool.parameters.clone(),
                })
                .collect(),
        )
    };

    AnthropicRequest {
        model: model.to_string(),
        max_tokens: DEFAULT_MAX_TOKENS,
        system,
        messages: anthropic_messages,
        tools,
    }
}

/// Translate an Anthropic response into the vendor-neutral `Completion`.
/// Public-in-crate so the unit tests exercise it directly.
fn response_to_completion(response: AnthropicResponse) -> Completion {
    let blocks: Vec<ContentBlock> = response
        .content
        .into_iter()
        .filter_map(|block| match block {
            AnthropicResponseBlock::Text { text } => Some(ContentBlock::Text { text }),
            AnthropicResponseBlock::ToolUse { id, name, input } => Some(ContentBlock::ToolCall {
                id,
                name,
                arguments: input,
            }),
            AnthropicResponseBlock::Other => None,
        })
        .collect();

    // If the model returned a single text block, surface it as plain text —
    // the common Hub "test prompt" path is then a one-string read.
    let content = match blocks.as_slice() {
        [ContentBlock::Text { text }] => MsgContent::text(text.clone()),
        _ => MsgContent::Blocks { blocks },
    };

    Completion {
        content,
        model: response.model,
        usage: response.usage.map(|u| Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        }),
        finish_reason: response.stop_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_metadata_matches_spec() {
        let provider = AnthropicLlmProvider::new();
        assert_eq!(provider.name(), "anthropic");
        assert_eq!(provider.display_name_key(), "provider.llm.anthropic");
        assert!(provider.supports_tool_use());
        assert_eq!(provider.supports_hebrew(), Confidence::Excellent);
        assert_eq!(provider.context_window(), 200_000);
        assert!(!provider.is_local());
        assert_eq!(provider.default_model(), DEFAULT_MODEL);
        let models = provider.available_models();
        assert!(models.iter().any(|m| m == DEFAULT_MODEL));
    }

    #[test]
    fn build_request_hoists_system_messages_to_top_level() {
        let messages = [
            Msg::system("You are Lashon — a Hebrew voice assistant."),
            Msg::user("שלום"),
        ];
        let request = build_request(DEFAULT_MODEL, &messages, &[]);
        assert_eq!(
            request.system.as_deref(),
            Some("You are Lashon — a Hebrew voice assistant.")
        );
        assert_eq!(request.messages.len(), 1);
    }

    #[test]
    fn build_request_concatenates_multiple_system_messages() {
        let messages = [
            Msg::system("first system note"),
            Msg::system("second system note"),
            Msg::user("hi"),
        ];
        let request = build_request(DEFAULT_MODEL, &messages, &[]);
        assert_eq!(
            request.system.as_deref(),
            Some("first system note\n\nsecond system note")
        );
    }

    #[test]
    fn build_request_serialises_tool_use_block() {
        let messages = [Msg {
            role: Role::Assistant,
            content: MsgContent::Blocks {
                blocks: vec![ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "open_app".into(),
                    arguments: json!({"name": "vscode"}),
                }],
            },
        }];
        let request = build_request(DEFAULT_MODEL, &messages, &[]);
        let json_value = serde_json::to_value(&request).unwrap();
        let blocks = &json_value["messages"][0]["content"];
        assert_eq!(blocks[0]["type"], "tool_use");
        assert_eq!(blocks[0]["id"], "call_1");
        assert_eq!(blocks[0]["name"], "open_app");
        assert_eq!(blocks[0]["input"]["name"], "vscode");
    }

    #[test]
    fn build_request_serialises_tool_result_block_under_user_role() {
        let messages = [Msg {
            role: Role::Tool,
            content: MsgContent::ToolResult {
                call_id: "call_1".into(),
                content: "the app opened".into(),
            },
        }];
        let request = build_request(DEFAULT_MODEL, &messages, &[]);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["messages"][0]["role"], "user");
        let content = &json_value["messages"][0]["content"][0];
        assert_eq!(content["type"], "tool_result");
        assert_eq!(content["tool_use_id"], "call_1");
        assert_eq!(content["content"], "the app opened");
    }

    #[test]
    fn build_request_serialises_tools() {
        let tools = [Tool {
            name: "open_app".into(),
            description: "Open an app by name".into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"]
            }),
        }];
        let request = build_request(DEFAULT_MODEL, &[Msg::user("test")], &tools);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["tools"][0]["name"], "open_app");
        assert_eq!(json_value["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn response_to_completion_handles_text_only() {
        let response = AnthropicResponse {
            content: vec![AnthropicResponseBlock::Text {
                text: "שלום עולם".into(),
            }],
            model: DEFAULT_MODEL.into(),
            stop_reason: Some("end_turn".into()),
            usage: Some(AnthropicUsage {
                input_tokens: 8,
                output_tokens: 12,
            }),
        };
        let completion = response_to_completion(response);
        assert_eq!(completion.content.to_plain_text(), "שלום עולם");
        assert_eq!(completion.model, DEFAULT_MODEL);
        assert_eq!(completion.finish_reason.as_deref(), Some("end_turn"));
        assert_eq!(
            completion.usage,
            Some(Usage {
                input_tokens: 8,
                output_tokens: 12
            })
        );
        // A single text block becomes `MsgContent::Text`, not `Blocks`.
        assert!(matches!(completion.content, MsgContent::Text { .. }));
    }

    #[test]
    fn response_to_completion_handles_mixed_text_and_tool_call() {
        let response = AnthropicResponse {
            content: vec![
                AnthropicResponseBlock::Text {
                    text: "Opening VS Code".into(),
                },
                AnthropicResponseBlock::ToolUse {
                    id: "call_1".into(),
                    name: "open_app".into(),
                    input: json!({"name": "vscode"}),
                },
            ],
            model: DEFAULT_MODEL.into(),
            stop_reason: Some("tool_use".into()),
            usage: None,
        };
        let completion = response_to_completion(response);
        match completion.content {
            MsgContent::Blocks { blocks } => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(blocks[0], ContentBlock::Text { .. }));
                assert!(matches!(blocks[1], ContentBlock::ToolCall { .. }));
            }
            _ => panic!("expected blocks"),
        }
    }

    #[test]
    fn response_to_completion_with_empty_content_yields_empty_blocks() {
        let response = AnthropicResponse {
            content: vec![],
            model: DEFAULT_MODEL.into(),
            stop_reason: None,
            usage: None,
        };
        let completion = response_to_completion(response);
        assert_eq!(completion.content.to_plain_text(), "");
    }

    #[test]
    fn http_error_classifies_status_codes() {
        match http_error("anthropic", 401, "bad key") {
            ProviderError::Unauthorized { message, .. } => {
                assert!(message.contains("bad key"));
            }
            _ => panic!("401 must be Unauthorized"),
        }
        match http_error("anthropic", 429, "slow down") {
            ProviderError::RateLimited { message, .. } => {
                assert!(message.contains("slow down"));
            }
            _ => panic!("429 must be RateLimited"),
        }
        match http_error("anthropic", 500, "boom") {
            ProviderError::Http { status, body, .. } => {
                assert_eq!(status, 500);
                assert!(body.contains("boom"));
            }
            _ => panic!("500 must be Http"),
        }
    }

    #[test]
    fn http_error_extracts_anthropic_message_envelope() {
        // Real-world Anthropic 401 body shape:
        //   {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        match http_error("anthropic", 401, body) {
            ProviderError::Unauthorized { message, .. } => {
                assert_eq!(message, "invalid x-api-key");
            }
            _ => panic!("401 must be Unauthorized"),
        }
    }

    #[test]
    fn http_error_falls_back_to_raw_body_when_not_json() {
        match http_error("anthropic", 401, "plain text 401 body") {
            ProviderError::Unauthorized { message, .. } => {
                assert_eq!(message, "plain text 401 body");
            }
            _ => panic!("401 must be Unauthorized"),
        }
    }

    // Integration test — only runs locally with a real key. CI never sees
    // `LASHON_LLM_ANTHROPIC_KEY` (`.claude/rules/security.md`).
    #[test]
    #[ignore = "needs LASHON_LLM_ANTHROPIC_KEY in the environment"]
    fn live_anthropic_hebrew_round_trip() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let provider = AnthropicLlmProvider::new();
        let completion = runtime
            .block_on(provider.chat(&[Msg::user("שלום, תוכל לענות בעברית?")], &[]))
            .expect("anthropic call must succeed with a real key");
        let text = completion.content.to_plain_text();
        // A simple smoke check — the response should contain at least one
        // Hebrew codepoint.
        assert!(text.chars().any(|c| ('\u{0590}'..='\u{05FF}').contains(&c)));
    }
}
