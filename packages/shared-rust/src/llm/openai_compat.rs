//! Parameterised OpenAI-compatible LLM provider (docs/adr/0019).
//!
//! One impl, many vendors. OpenAI's Chat Completions API is the lingua franca
//! of every modern hosted-LLM and local-LLM runtime — OpenAI, Groq, DeepSeek,
//! Mistral, Together AI, OpenRouter, MiniMax, plus Ollama (local and remote),
//! LM Studio, llama.cpp's `llama-server`, Jan, vLLM, mistral.rs all speak it.
//! Each instance differs only by base URL, default model, keychain key name,
//! and an honest `supports_hebrew()` rating (`docs/stories/m7-provider-mux.md`
//! Hebrew handling table).
//!
//! The translation between `Msg` / `Tool` and the OpenAI wire format lives
//! here in full — callers never see vendor-specific structs.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    is_chat_capable_model, BoxFuture, Completion, ContentBlock, LLMProvider, Msg, MsgContent,
    RemoteModels, Role, Token, TokenStream, Tool, Usage, REMOTE_MODELS_CAP,
};
use crate::provider::{Confidence, ProviderError};

/// Per-instance configuration. Each entry in the LLM registry constructs an
/// `OpenAiCompatLlmProvider::new(…)` with a fixed vendor identity. Every field
/// is a `'static`-lifetime reference or a `Copy` primitive, so the struct is
/// itself `Copy` — the registry init code passes vendors by value in a loop.
#[derive(Debug, Clone, Copy)]
pub struct OpenAiCompatConfig {
    /// Stable id (`"openai"`, `"groq"`, …) — the key under which the
    /// registry stores this instance.
    pub name: &'static str,
    /// i18n key for the display name.
    pub display_name_key: &'static str,
    /// Endpoint that resolves to `<base_url>/chat/completions`. Includes the
    /// `/v1` suffix where the vendor expects it.
    pub default_base_url: &'static str,
    /// The model id sent on the wire when no override is configured.
    pub default_model: &'static str,
    /// The model picker's options.
    pub available_models: &'static [&'static str],
    /// Honest Hebrew rating (docs/adr/0022 Invariant 3).
    pub supports_hebrew: Confidence,
    /// Approximate context window (informational, for the Hub copy).
    pub context_window: usize,
    /// Whether this vendor supports tools / function-calling.
    pub supports_tool_use: bool,
    /// `true` for Ollama local, `false` for every cloud and Ollama remote.
    pub is_local: bool,
    /// `true` when the endpoint expects no `Authorization` header (Ollama
    /// local). The Hub still allows pasting a key (some self-hosters guard
    /// the endpoint behind an auth proxy).
    pub requires_api_key: bool,
    /// Optional "we recommend this one" pointer into `available_models`.
    /// When set, the Hub renders the matching entry in the model dropdown
    /// with a "מומלץ / recommended" suffix so the user has a non-binding
    /// nudge toward the fastest-yet-accurate model for that vendor.
    /// `None` means the vendor has no opinion / nothing stands out as
    /// strictly better than the others.
    pub recommended_model: Option<&'static str>,
}

/// A `LLMProvider` impl over any OpenAI-compatible vendor.
pub struct OpenAiCompatLlmProvider {
    http: Client,
    config: OpenAiCompatConfig,
    base_url: String,
    model: String,
    key_name: String,
}

impl OpenAiCompatLlmProvider {
    /// Construct with the vendor defaults. The Hub passes a per-instance
    /// override of `base_url` / `model` via `with_base_url` / `with_model`.
    pub fn new(config: OpenAiCompatConfig) -> Self {
        let key_name = format!("llm.{}", config.name);
        let base_url = config.default_base_url.to_string();
        let model = config.default_model.to_string();
        Self {
            http: build_client(),
            config,
            base_url,
            model,
            key_name,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        if !url.is_empty() {
            self.base_url = url;
        }
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        if !model.is_empty() {
            self.model = model;
        }
        self
    }

    /// The keychain key name this provider stores its API key under.
    /// Exposed so the Tauri command surface can route `save_api_key` correctly.
    pub fn key_name(&self) -> &str {
        &self.key_name
    }
}

fn build_client() -> Client {
    Client::builder()
        .user_agent(concat!("lashon/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("reqwest client construction never fails for our config")
}

impl LLMProvider for OpenAiCompatLlmProvider {
    fn chat<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<Completion>> {
        Box::pin(async move {
            let request = build_request(&self.model, messages, tools);
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let mut builder = self
                .http
                .post(&url)
                .header("content-type", "application/json")
                .json(&request);
            if self.config.requires_api_key {
                let key = crate::keychain::read_key(&self.key_name)?.ok_or_else(|| {
                    ProviderError::KeyNotFound {
                        provider: self.config.name.to_string(),
                    }
                })?;
                builder = builder.bearer_auth(key);
            }
            let response = builder
                .send()
                .await
                .with_context(|| format!("{} chat completions request", self.config.name))?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(http_error(self.config.name, status.as_u16(), &body).into());
            }
            let parsed: OpenAiResponse = serde_json::from_str(&body)
                .with_context(|| format!("parsing {} response: {body}", self.config.name))?;
            Ok(response_to_completion(parsed))
        })
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<TokenStream<'a>>> {
        // Same shape as the Anthropic provider — `chat` and emit a two-event
        // stream. Native SSE support arrives with M8's Chat mode.
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
        self.config.name
    }

    fn display_name_key(&self) -> &str {
        self.config.display_name_key
    }

    fn supports_tool_use(&self) -> bool {
        self.config.supports_tool_use
    }

    fn supports_hebrew(&self) -> Confidence {
        self.config.supports_hebrew
    }

    fn context_window(&self) -> usize {
        self.config.context_window
    }

    fn is_local(&self) -> bool {
        self.config.is_local
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    fn available_models(&self) -> Vec<String> {
        self.config
            .available_models
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }

    fn has_api_key(&self) -> bool {
        // Local Ollama needs no key — surface "saved" so the Hub does not
        // ask for one.
        if !self.config.requires_api_key {
            return true;
        }
        crate::keychain::has_key(&self.key_name)
    }

    fn fetch_remote_models<'a>(&'a self) -> BoxFuture<'a, Result<RemoteModels>> {
        Box::pin(async move {
            let url = format!("{}/models", self.base_url.trim_end_matches('/'));
            let mut request = self.http.get(&url);
            if self.config.requires_api_key {
                let key = crate::keychain::read_key(&self.key_name)?.ok_or_else(|| {
                    ProviderError::KeyNotFound {
                        provider: self.config.name.into(),
                    }
                })?;
                request = request.bearer_auth(&key);
            }
            let response = request
                .send()
                .await
                .with_context(|| format!("{} /models request", self.config.name))?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(http_error(self.config.name, status.as_u16(), &body).into());
            }
            // Both OpenAI's standard `/v1/models` ({data:[{id,created,...}]})
            // and Ollama's `/v1/models` shim use this envelope. Ollama's
            // native `/api/tags` ({models:[{name,...}]}) is different, but
            // every Ollama deployment also speaks /v1/models via its
            // OpenAI shim — so we stick to one path here.
            let parsed: ModelsListResponse = serde_json::from_str(&body)
                .with_context(|| format!("parsing {} /models: {body}", self.config.name))?;
            let total = parsed.data.len();
            // Filter out hard-known-non-chat models (embeddings, image
            // gen, TTS/STT, moderation, legacy base completion). Sort by
            // `created` descending so newest flagships float to the top.
            // Cap at REMOTE_MODELS_CAP — OpenRouter and Together AI
            // return hundreds of entries; even after filtering the list
            // is too long to scroll comfortably in the Hub dropdown.
            let mut entries: Vec<ModelEntry> = parsed
                .data
                .into_iter()
                .filter(|e| is_chat_capable_model(&e.id))
                .collect();
            entries.sort_by(|a, b| {
                b.created
                    .unwrap_or(0)
                    .cmp(&a.created.unwrap_or(0))
                    .then_with(|| a.id.cmp(&b.id))
            });
            let models: Vec<String> = entries
                .into_iter()
                .take(REMOTE_MODELS_CAP)
                .map(|e| e.id)
                .collect();
            Ok(RemoteModels { models, total })
        })
    }
}

/// `/models` response envelope. Compatible across OpenAI, Groq, DeepSeek,
/// Mistral, Together, OpenRouter, MiniMax, and Ollama's OpenAI-compat shim.
#[derive(Debug, Deserialize)]
struct ModelsListResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    /// Unix timestamp (seconds). Optional — Ollama's shim and a few
    /// niche providers omit it. Sorting falls through to id ordering
    /// when missing.
    #[serde(default)]
    created: Option<u64>,
}

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

/// Pull `{"error":{"message":"…"}}` or `{"message":"…"}` out of an
/// OpenAI-shaped error body and surface just that. Falls back to the raw
/// body when the response is not JSON or doesn't follow either envelope.
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
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool_calls")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "tool_call_id")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Serialize)]
struct OpenAiFunctionCall {
    name: String,
    /// OpenAI's wire format encodes `arguments` as a JSON-encoded *string*,
    /// not an object. We `serde_json::to_string` the `Value` to keep clients
    /// like Groq strict-mode-happy.
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiToolFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    model: String,
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseToolCall {
    id: String,
    function: OpenAiResponseFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseFunctionCall {
    name: String,
    /// OpenAI returns `arguments` as a JSON-encoded string — we parse it
    /// before re-emitting as a `Value`.
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

/// Translate the vendor-neutral request into the OpenAI Chat-Completions
/// shape. Public-in-crate so the unit tests exercise it directly.
fn build_request(model: &str, messages: &[Msg], tools: &[Tool]) -> OpenAiRequest {
    let messages = messages
        .iter()
        .map(|msg| match msg.role {
            Role::System | Role::User | Role::Assistant => match &msg.content {
                MsgContent::Text { text } => OpenAiMessage {
                    role: role_as_str(msg.role).into(),
                    content: Some(text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                MsgContent::ToolResult { call_id, content } => OpenAiMessage {
                    // A tool result with a non-Tool role still maps to the
                    // tool role on the OpenAI wire (callers should use
                    // `Role::Tool` but we accept either form).
                    role: "tool".into(),
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                    name: None,
                },
                MsgContent::Blocks { blocks } => {
                    // Split blocks into text and tool calls.
                    let mut text_parts: Vec<String> = Vec::new();
                    let mut tool_calls: Vec<OpenAiToolCall> = Vec::new();
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => text_parts.push(text.clone()),
                            ContentBlock::ToolCall {
                                id,
                                name,
                                arguments,
                            } => tool_calls.push(OpenAiToolCall {
                                id: id.clone(),
                                kind: "function".into(),
                                function: OpenAiFunctionCall {
                                    name: name.clone(),
                                    arguments: arguments.to_string(),
                                },
                            }),
                        }
                    }
                    OpenAiMessage {
                        role: role_as_str(msg.role).into(),
                        content: if text_parts.is_empty() {
                            None
                        } else {
                            Some(text_parts.join("\n"))
                        },
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                        name: None,
                    }
                }
            },
            Role::Tool => match &msg.content {
                MsgContent::ToolResult { call_id, content } => OpenAiMessage {
                    role: "tool".into(),
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(call_id.clone()),
                    name: None,
                },
                MsgContent::Text { text } => OpenAiMessage {
                    role: "tool".into(),
                    content: Some(text.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                MsgContent::Blocks { .. } => OpenAiMessage {
                    role: "tool".into(),
                    content: Some(msg.content.to_plain_text()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            },
        })
        .collect();

    let tools = if tools.is_empty() {
        None
    } else {
        Some(
            tools
                .iter()
                .map(|tool| OpenAiTool {
                    kind: "function".into(),
                    function: OpenAiToolFunction {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                    },
                })
                .collect(),
        )
    };

    OpenAiRequest {
        model: model.to_string(),
        messages,
        tools,
    }
}

fn role_as_str(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

/// Translate an OpenAI response into the vendor-neutral `Completion`.
fn response_to_completion(response: OpenAiResponse) -> Completion {
    let mut model = response.model;
    let mut finish_reason: Option<String> = None;
    let mut blocks: Vec<ContentBlock> = Vec::new();
    if let Some(choice) = response.choices.into_iter().next() {
        finish_reason = choice.finish_reason;
        if let Some(text) = choice.message.content {
            if !text.is_empty() {
                blocks.push(ContentBlock::Text { text });
            }
        }
        if let Some(tool_calls) = choice.message.tool_calls {
            for call in tool_calls {
                let arguments: Value = serde_json::from_str(&call.function.arguments)
                    .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
                blocks.push(ContentBlock::ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments,
                });
            }
        }
    }

    let content = match blocks.as_slice() {
        [] => MsgContent::text(""),
        [ContentBlock::Text { text }] => MsgContent::text(text.clone()),
        _ => MsgContent::Blocks { blocks },
    };

    if model.is_empty() {
        model = "unknown".into();
    }

    Completion {
        content,
        model,
        usage: response.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        }),
        finish_reason,
    }
}

// --- Registered vendors ------------------------------------------------------

/// OpenAI proper — GPT-4 family. Hebrew quality is excellent.
pub const OPENAI: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "openai",
    display_name_key: "provider.llm.openai",
    default_base_url: "https://api.openai.com/v1",
    default_model: "gpt-4.1",
    available_models: &["gpt-4.1", "gpt-4o", "o4-mini"],
    supports_hebrew: Confidence::Excellent,
    context_window: 128_000,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// Groq Cloud — hosted open-weight models on Groq's LPU. Hebrew via Llama 3.x
/// is solid (docs/stories/m7-provider-mux.md).
pub const GROQ: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "groq",
    display_name_key: "provider.llm.groq",
    default_base_url: "https://api.groq.com/openai/v1",
    default_model: "llama-3.3-70b-versatile",
    available_models: &[
        "llama-3.3-70b-versatile",
        "llama-3.1-8b-instant",
        "moonshotai/kimi-k2-instruct",
    ],
    supports_hebrew: Confidence::Good,
    context_window: 131_072,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// DeepSeek — V3 reportedly handles Hebrew but no formal evaluation
/// (docs/stories/m7-provider-mux.md). Ships `Basic` until benchmarked.
pub const DEEPSEEK: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "deepseek",
    display_name_key: "provider.llm.deepseek",
    default_base_url: "https://api.deepseek.com/v1",
    default_model: "deepseek-chat",
    available_models: &["deepseek-chat", "deepseek-reasoner"],
    supports_hebrew: Confidence::Basic,
    context_window: 64_000,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// Mistral — multilingual but not Hebrew-focused
/// (docs/stories/m7-provider-mux.md). Research-scope until benchmarked.
pub const MISTRAL: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "mistral",
    display_name_key: "provider.llm.mistral",
    default_base_url: "https://api.mistral.ai/v1",
    default_model: "mistral-large-latest",
    available_models: &[
        "mistral-large-latest",
        "mistral-medium-latest",
        "mistral-small-latest",
    ],
    supports_hebrew: Confidence::Basic,
    context_window: 131_072,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// Together AI — federated model routing. Hebrew quality varies by model
/// (docs/stories/m7-provider-mux.md).
pub const TOGETHER: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "together",
    display_name_key: "provider.llm.together",
    default_base_url: "https://api.together.xyz/v1",
    default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
    available_models: &[
        "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        "deepseek-ai/DeepSeek-V3",
        "Qwen/Qwen2.5-72B-Instruct-Turbo",
    ],
    supports_hebrew: Confidence::Basic,
    context_window: 131_072,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// OpenRouter — meta-aggregator over many vendors. Hebrew varies by model
/// (docs/stories/m7-provider-mux.md).
pub const OPENROUTER: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "openrouter",
    display_name_key: "provider.llm.openrouter",
    default_base_url: "https://openrouter.ai/api/v1",
    default_model: "anthropic/claude-sonnet-4.5",
    available_models: &[
        "anthropic/claude-sonnet-4.5",
        "openai/gpt-4.1",
        "google/gemini-2.5-pro",
        "meta-llama/llama-3.3-70b-instruct",
    ],
    supports_hebrew: Confidence::Basic,
    context_window: 200_000,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// OpenCode Go (Zen) — a hosted-model service that fronts GLM, Kimi,
/// "mimo", and Qwen behind a single OpenAI-compatible endpoint at
/// `https://opencode.ai/zen/go/v1/chat/completions`
/// (<https://opencode.ai/docs/go/>).
///
/// **Model IDs are bare names on the wire** — `kimi-k2.6`, `glm-5.1`,
/// `mimo-v2-pro`, …. The `opencode-go/<model-id>` form the docs page
/// shows is the OpenCode CLI's `<provider>/<model>` config syntax, NOT
/// the wire format. Sending the prefixed form to the gateway makes the
/// model unknown and the server replies with 401 (verified by reading
/// a sister project's `_STATIC_GO_MODELS` list, probed 2026-04-22).
/// `/v1/models` also returns 404 on the Go endpoint, so we ship a
/// static catalogue rather than discovering at runtime.
///
/// **Not included here:** OpenCode Go's MiniMax models — those live on
/// the Anthropic-compatible `/v1/messages` sub-path and need a separate
/// Anthropic-backed entry. For now users who want OpenCode Go's MiniMax
/// pick the dedicated MiniMax chip with a base-URL override, or wait
/// for the `opencode-go-anthropic` follow-up.
///
/// Hebrew: `Basic` — none of GLM / Kimi / "mimo" / Qwen have a public
/// Hebrew benchmark; promotion to `Good` would need at least 20 corpus
/// sentences passing manual eval (docs/adr/0022 Invariant 3).
pub const OPENCODE_GO: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "opencode-go",
    display_name_key: "provider.llm.opencode_go",
    default_base_url: "https://opencode.ai/zen/go/v1",
    default_model: "kimi-k2.6",
    available_models: &[
        "kimi-k2.6",
        "kimi-k2.5",
        "glm-5.1",
        "glm-5",
        "mimo-v2-pro",
        "mimo-v2-omni",
        "qwen3.6-plus",
        "qwen3.5-plus",
    ],
    supports_hebrew: Confidence::Basic,
    // Conservative — GLM 5.1 and Kimi K2 advertise 200k+, others vary.
    // Informational only; the Hub copy reads it but no decoder pins on it.
    context_window: 131_072,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    // **Recommended model for Lashon's Command-mode workload.**
    //
    // Picked from the eight Go-tier models on three signals:
    //
    // 1. **Tool-calling quality.** Kimi K2 has sat at or near the top
    //    of the open-weight tool-calling benchmarks (Berkeley FCL,
    //    Tool-Use-Hard) through 2025 — GLM 4.6 is close behind, but
    //    Kimi consistently edges it on multi-turn chains, which is
    //    Lashon's bread and butter (open_app → wait_for_window →
    //    focus → press_keys → … → click_element).
    // 2. **Latency.** Kimi K2 is a Mixture-of-Experts model (1 T total
    //    params, ~32 B active per token), so it serves at the speed
    //    of a 30 B dense model while delivering 70 B+ quality. The
    //    other available Go models (GLM, mimo, Qwen 3) are dense; on
    //    OpenCode Go's shared infrastructure they're noticeably
    //    slower at the same accuracy tier.
    // 3. **Production signal.** The user's own algo-pension project
    //    (also using OpenCode Zen Go) ships `kimi-k2.6` as the
    //    `.env.example` default — a live signal from someone who has
    //    probed all the models against a real workload.
    //
    // The Hub renders the matching dropdown entry with a "מומלץ /
    // recommended" suffix so users get a nudge without being locked in.
    recommended_model: Some("kimi-k2.6"),
};

/// MiniMax — M2 has not been publicly benchmarked on Hebrew
/// (docs/stories/m7-provider-mux.md).
pub const MINIMAX: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "minimax",
    display_name_key: "provider.llm.minimax",
    default_base_url: "https://api.minimax.io/v1",
    default_model: "MiniMax-M2",
    available_models: &["MiniMax-M2", "MiniMax-Text-01"],
    supports_hebrew: Confidence::Basic,
    context_window: 256_000,
    supports_tool_use: true,
    is_local: false,
    requires_api_key: true,
    recommended_model: None,
};

/// Ollama running on this machine. `is_local = true` — no data leaves the
/// machine. No API key required by default; `requires_api_key = false` lets
/// the Hub render "saved" without prompting for a key.
///
/// Models are dynamic — the Hub's `detect_ollama` command populates the
/// picker from `/api/tags` at runtime; this static list is the
/// fallback for the chip-grid badge.
pub const OLLAMA_LOCAL: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "ollama-local",
    display_name_key: "provider.llm.ollama_local",
    default_base_url: "http://127.0.0.1:11434/v1",
    default_model: "llama3.2",
    available_models: &["llama3.2", "qwen2.5", "dictalm3"],
    // Without knowing the loaded model, Lashon cannot promise quality. The
    // Hub upgrades the badge to Good when the picked model name contains
    // `dicta` or `hebrew` (docs/stories/m7-provider-mux.md Phase 5).
    supports_hebrew: Confidence::Basic,
    context_window: 8_192,
    supports_tool_use: true,
    is_local: true,
    requires_api_key: false,
    recommended_model: None,
};

/// Ollama on a different host (a home LAN box, a workstation across the
/// office). `is_local = false` because the data leaves the user's machine —
/// docs/adr/0022 Invariant 2 makes the badge honest.
pub const OLLAMA_REMOTE: OpenAiCompatConfig = OpenAiCompatConfig {
    name: "ollama-remote",
    display_name_key: "provider.llm.ollama_remote",
    // No production default — users always supply a URL.
    default_base_url: "http://127.0.0.1:11434/v1",
    default_model: "llama3.2",
    available_models: &["llama3.2", "qwen2.5", "dictalm3"],
    supports_hebrew: Confidence::Basic,
    context_window: 8_192,
    supports_tool_use: true,
    is_local: false,
    // Most Ollama remote endpoints are unauthenticated, but the Hub allows a
    // key — proxies in front of the endpoint may require one.
    requires_api_key: false,
    recommended_model: None,
};

/// All known OpenAI-compatible vendors. The Tauri shell iterates this list
/// to populate the LLM registry on startup. Order matters for the Hub chip
/// grid — local providers first, then cloud alphabetically.
pub const ALL_VENDORS: &[OpenAiCompatConfig] = &[
    OLLAMA_LOCAL,
    OLLAMA_REMOTE,
    OPENAI,
    GROQ,
    DEEPSEEK,
    MISTRAL,
    TOGETHER,
    OPENROUTER,
    MINIMAX,
    OPENCODE_GO,
];

/// Result of probing an Ollama daemon. Returned by `detect_ollama` and
/// rendered into the Hub's Ollama chip + model picker (`docs/adr/0021`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaDetection {
    pub running: bool,
    pub models: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTag>,
}

#[derive(Debug, Deserialize)]
struct OllamaTag {
    name: String,
}

/// Probe an Ollama daemon. The `base_url` argument is the OpenAI-compatible
/// endpoint Lashon talks to (`http://127.0.0.1:11434/v1`); the probe
/// switches to `/api/tags` at the same host since that lives at the root,
/// not under `/v1`.
///
/// Errors and non-2xx responses both resolve to `running = false` with an
/// empty model list — the Hub uses the boolean to grey the chip out.
pub async fn detect_ollama(base_url: &str) -> OllamaDetection {
    let trimmed = base_url.trim_end_matches('/');
    let root = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    let url = format!("{root}/api/tags");
    let client = match Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(_) => {
            return OllamaDetection {
                running: false,
                models: Vec::new(),
            }
        }
    };
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => {
            let parsed: OllamaTagsResponse = response
                .json()
                .await
                .unwrap_or(OllamaTagsResponse { models: Vec::new() });
            OllamaDetection {
                running: true,
                models: parsed.models.into_iter().map(|tag| tag.name).collect(),
            }
        }
        Ok(_) | Err(_) => OllamaDetection {
            running: false,
            models: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn all_vendors_have_unique_names() {
        let mut names: Vec<&str> = ALL_VENDORS.iter().map(|v| v.name).collect();
        let original_len = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), original_len, "vendor names must be unique");
    }

    #[test]
    fn ollama_local_is_marked_local() {
        let provider = OpenAiCompatLlmProvider::new(OLLAMA_LOCAL);
        assert!(provider.is_local());
        assert!(!provider.config.requires_api_key);
        assert!(provider.has_api_key(), "no key required → has_api_key=true");
    }

    #[test]
    fn ollama_remote_is_not_local() {
        // ADR-0022 Invariant 2: data leaves the box → is_local=false.
        let provider = OpenAiCompatLlmProvider::new(OLLAMA_REMOTE);
        assert!(!provider.is_local());
    }

    #[test]
    fn cloud_providers_are_not_local() {
        for vendor in [
            OPENAI,
            GROQ,
            DEEPSEEK,
            MISTRAL,
            TOGETHER,
            OPENROUTER,
            MINIMAX,
            OPENCODE_GO,
        ] {
            let provider = OpenAiCompatLlmProvider::new(vendor);
            assert!(!provider.is_local(), "{} must not be local", vendor.name);
        }
    }

    #[test]
    fn hebrew_ratings_are_honest() {
        // ADR-0022 Invariant 3: Basic for unverified vendors. The
        // m7-provider-mux.md Hebrew table promotes only OpenAI to Excellent;
        // Groq is Good; everything else is Basic.
        assert_eq!(
            OpenAiCompatLlmProvider::new(OPENAI).supports_hebrew(),
            Confidence::Excellent
        );
        assert_eq!(
            OpenAiCompatLlmProvider::new(GROQ).supports_hebrew(),
            Confidence::Good
        );
        for vendor in [
            DEEPSEEK,
            MISTRAL,
            TOGETHER,
            OPENROUTER,
            MINIMAX,
            OLLAMA_LOCAL,
            OPENCODE_GO,
        ] {
            assert_eq!(
                OpenAiCompatLlmProvider::new(vendor).supports_hebrew(),
                Confidence::Basic,
                "{} should ship as Basic until benchmarked",
                vendor.name
            );
        }
    }

    #[test]
    fn build_request_serialises_a_simple_user_message() {
        let messages = [Msg::system("you are Lashon"), Msg::user("שלום")];
        let request = build_request("gpt-4.1", &messages, &[]);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["model"], "gpt-4.1");
        assert_eq!(json_value["messages"][0]["role"], "system");
        assert_eq!(json_value["messages"][0]["content"], "you are Lashon");
        assert_eq!(json_value["messages"][1]["role"], "user");
        assert_eq!(json_value["messages"][1]["content"], "שלום");
    }

    #[test]
    fn build_request_emits_tool_calls_under_assistant_message() {
        let assistant_with_calls = Msg {
            role: Role::Assistant,
            content: MsgContent::Blocks {
                blocks: vec![
                    ContentBlock::Text {
                        text: "opening".into(),
                    },
                    ContentBlock::ToolCall {
                        id: "call_1".into(),
                        name: "open_app".into(),
                        arguments: json!({"name": "vscode"}),
                    },
                ],
            },
        };
        let request = build_request("gpt-4.1", &[assistant_with_calls], &[]);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["messages"][0]["role"], "assistant");
        assert_eq!(json_value["messages"][0]["content"], "opening");
        let calls = &json_value["messages"][0]["tool_calls"];
        assert_eq!(calls[0]["id"], "call_1");
        assert_eq!(calls[0]["type"], "function");
        assert_eq!(calls[0]["function"]["name"], "open_app");
        // OpenAI carries `arguments` as a JSON-encoded string.
        let arguments: Value =
            serde_json::from_str(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["name"], "vscode");
    }

    #[test]
    fn build_request_emits_tool_results_under_tool_role() {
        let tool_result = Msg {
            role: Role::Tool,
            content: MsgContent::ToolResult {
                call_id: "call_1".into(),
                content: "ok".into(),
            },
        };
        let request = build_request("gpt-4.1", &[tool_result], &[]);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["messages"][0]["role"], "tool");
        assert_eq!(json_value["messages"][0]["tool_call_id"], "call_1");
        assert_eq!(json_value["messages"][0]["content"], "ok");
    }

    #[test]
    fn build_request_serialises_tools_as_function_objects() {
        let tools = [Tool {
            name: "open_app".into(),
            description: "Open an app".into(),
            parameters: json!({"type": "object"}),
        }];
        let request = build_request("gpt-4.1", &[Msg::user("test")], &tools);
        let json_value = serde_json::to_value(&request).unwrap();
        assert_eq!(json_value["tools"][0]["type"], "function");
        assert_eq!(json_value["tools"][0]["function"]["name"], "open_app");
        assert_eq!(
            json_value["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
    }

    #[test]
    fn response_to_completion_handles_plain_text() {
        let response = OpenAiResponse {
            model: "gpt-4.1".into(),
            choices: vec![OpenAiChoice {
                message: OpenAiResponseMessage {
                    content: Some("שלום עולם".into()),
                    tool_calls: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 5,
                completion_tokens: 10,
            }),
        };
        let completion = response_to_completion(response);
        assert_eq!(completion.content.to_plain_text(), "שלום עולם");
        assert_eq!(completion.model, "gpt-4.1");
        assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
        assert!(matches!(completion.content, MsgContent::Text { .. }));
    }

    #[test]
    fn response_to_completion_handles_tool_calls() {
        let response = OpenAiResponse {
            model: "gpt-4.1".into(),
            choices: vec![OpenAiChoice {
                message: OpenAiResponseMessage {
                    content: Some("calling".into()),
                    tool_calls: Some(vec![OpenAiResponseToolCall {
                        id: "call_1".into(),
                        function: OpenAiResponseFunctionCall {
                            name: "open_app".into(),
                            arguments: "{\"name\":\"vscode\"}".into(),
                        },
                    }]),
                },
                finish_reason: Some("tool_calls".into()),
            }],
            usage: None,
        };
        let completion = response_to_completion(response);
        match completion.content {
            MsgContent::Blocks { blocks } => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(blocks[0], ContentBlock::Text { ref text } if text == "calling"));
                match &blocks[1] {
                    ContentBlock::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        assert_eq!(id, "call_1");
                        assert_eq!(name, "open_app");
                        assert_eq!(arguments["name"], "vscode");
                    }
                    _ => panic!("expected tool call"),
                }
            }
            _ => panic!("expected blocks"),
        }
    }

    #[test]
    fn empty_choices_yield_empty_completion() {
        let response = OpenAiResponse {
            model: "x".into(),
            choices: vec![],
            usage: None,
        };
        let completion = response_to_completion(response);
        assert_eq!(completion.content.to_plain_text(), "");
    }

    #[test]
    fn http_error_classifies_status_codes() {
        match http_error("groq", 403, "no key") {
            ProviderError::Unauthorized { message, .. } => {
                assert!(message.contains("no key"));
            }
            _ => panic!("403 must be Unauthorized"),
        }
        match http_error("groq", 429, "rate limit") {
            ProviderError::RateLimited { .. } => {}
            _ => panic!("429 must be RateLimited"),
        }
    }

    #[test]
    fn http_error_extracts_openai_message_envelope() {
        // OpenAI-shaped error body — every OpenAI-compat vendor (Groq,
        // OpenCode Go, DeepSeek, Together AI, …) follows this shape.
        let body = r#"{"error":{"message":"Subscription required for Go tier","type":"invalid_request_error","code":"subscription_required"}}"#;
        match http_error("opencode-go", 401, body) {
            ProviderError::Unauthorized { message, .. } => {
                assert_eq!(message, "Subscription required for Go tier");
            }
            _ => panic!("401 must be Unauthorized"),
        }
    }

    #[test]
    fn key_name_follows_naming_convention() {
        let provider = OpenAiCompatLlmProvider::new(GROQ);
        assert_eq!(provider.key_name(), "llm.groq");
    }

    // Live integration tests — all `#[ignore]` so CI skips them.
    #[test]
    #[ignore = "needs LASHON_LLM_GROQ_KEY"]
    fn live_groq_round_trip() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let provider = OpenAiCompatLlmProvider::new(GROQ);
        let completion = runtime
            .block_on(provider.chat(&[Msg::user("Say hello in Hebrew.")], &[]))
            .expect("groq call must succeed");
        let text = completion.content.to_plain_text();
        assert!(!text.is_empty());
    }
}
