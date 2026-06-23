//! Language-model provider abstraction (docs/architecture.md §4, docs/adr/0019).
//!
//! `LLMProvider` is the seam every Command-mode (M8), Chat-mode (M8), and
//! future LLM-driven affordance dispatches through. M7 ships the trait, two
//! impls — the dedicated Anthropic Messages-API client and the
//! parameterised OpenAI-compatible client that serves Groq, OpenAI,
//! DeepSeek, Mistral, Together AI, OpenRouter, MiniMax, and both flavours of
//! Ollama (local and remote) — and the lazy `ProviderRegistry`.
//!
//! Callers never see vendor-specific structs: `Msg`, `Tool`, `Completion`,
//! and `Token` are the vendor-neutral surface, and each impl translates
//! to its wire format internally.

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

pub use crate::provider::{Confidence, ProviderError, ProviderMeta};

pub mod anthropic;
pub mod local;
pub mod openai_compat;

/// A boxed, `Send` future of a trait-bound result. Returned from every
/// async `LLMProvider` method so `Box<dyn LLMProvider>` is object-safe on
/// stable Rust 1.95 (docs/adr/0019).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A boxed, `Send` stream of streaming-token items.
pub type TokenStream<'a> = Pin<Box<dyn Stream<Item = Result<Token>> + Send + 'a>>;

/// The role of a message in a chat-style conversation. Anthropic and OpenAI
/// both use the same four roles, with subtle differences in how they encode
/// tool-results (OpenAI: a separate `Tool` role; Anthropic: a `user` message
/// whose content carries a `tool_result` block). Each provider impl handles
/// the difference internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A single content block in an assistant turn. Anthropic emits these
/// natively; OpenAI emits string content + a separate `tool_calls` array,
/// which the OpenAI impl translates to `Blocks` for callers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text — assistant prose.
    Text { text: String },
    /// A tool/function invocation the assistant wants to make. `id`
    /// correlates with the matching `MsgContent::ToolResult` reply.
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
}

/// The vendor-neutral content of a message. The three shapes cover:
///
/// - `Text` — plain user / system / assistant text (the OpenAI common case).
/// - `ToolResult` — the user's reply to a previous `ToolCall`; carries the
///   tool's stringified output.
/// - `Blocks` — multi-block assistant content (Anthropic's native shape and
///   OpenAI's function-call response — text + one or more `ToolCall` blocks).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MsgContent {
    Text { text: String },
    ToolResult { call_id: String, content: String },
    Blocks { blocks: Vec<ContentBlock> },
}

impl MsgContent {
    /// Shorthand for `MsgContent::Text { text: … }`.
    pub fn text(s: impl Into<String>) -> Self {
        MsgContent::Text { text: s.into() }
    }

    /// Project the message down to the assistant's concatenated plain text.
    /// Tool calls are skipped — callers that care about them inspect the
    /// `Blocks` variant directly.
    pub fn to_plain_text(&self) -> String {
        match self {
            MsgContent::Text { text } => text.clone(),
            MsgContent::ToolResult { content, .. } => content.clone(),
            MsgContent::Blocks { blocks } => blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::ToolCall { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

/// A vendor-neutral chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Msg {
    pub role: Role,
    pub content: MsgContent,
}

impl Msg {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MsgContent::text(text),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MsgContent::text(text),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MsgContent::text(text),
        }
    }
}

/// A tool definition the LLM may invoke. M8's tool registry serialises its
/// `LashonTool` trait's schema into one of these; the provider impl
/// translates it to its vendor wire format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: String,
    /// A JSON Schema describing the tool's parameters. Both Anthropic
    /// (`input_schema`) and OpenAI (`function.parameters`) accept JSON Schema
    /// here, so callers pass the same `serde_json::Value`.
    pub parameters: serde_json::Value,
}

/// A completed (non-streaming) LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    /// The assistant's content — `Text` for plain replies, `Blocks` when the
    /// response includes tool calls or mixed content.
    pub content: MsgContent,
    /// The model that produced the response (for the Hub's "answered by"
    /// note, and for usage logging — never recorded with the user's text).
    pub model: String,
    /// Token usage where the provider reports it (Anthropic and OpenAI both
    /// do; some OpenAI-compat clones omit it for non-standard models).
    pub usage: Option<Usage>,
    /// The reason the model stopped generating. Vendor-specific in detail
    /// (`"end_turn"`, `"stop"`, `"tool_use"`, `"tool_calls"`, `"max_tokens"`)
    /// — kept as a string for callers that need to branch on it.
    pub finish_reason: Option<String>,
}

/// Token usage as reported by the provider. Tracked at `tracing::debug!`
/// level only — never persisted alongside the user's prompt or response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A single streaming token / event from `LLMProvider::stream`. Both
/// Anthropic SSE and OpenAI SSE emit roughly the same shape — text chunks
/// interleaved with tool-call lifecycle events; each provider impl
/// normalises into this enum.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A plain text delta — append to the assistant's reply.
    Text(String),
    /// The assistant has begun emitting a tool call.
    ToolCallStart { id: String, name: String },
    /// A partial JSON delta for the in-progress tool call's `arguments`.
    /// Concatenated chunks parse to the same JSON the non-streaming
    /// `Completion` would have carried.
    ToolCallArguments {
        id: String,
        partial_arguments: String,
    },
    /// The tool-call has finished emitting.
    ToolCallEnd { id: String },
    /// The generation has finished — `reason` is vendor-specific.
    Finish {
        reason: Option<String>,
        usage: Option<Usage>,
    },
}

/// Maximum-tokens hint passed to providers that require it (Anthropic does;
/// OpenAI defaults). Kept conservative — M8's caller can raise it once
/// per-mode budgets are tuned.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// A language-model provider. Each engine implements this trait; callers
/// route through it and never bind to a concrete vendor
/// (docs/architecture.md §4, docs/adr/0019).
pub trait LLMProvider: Send + Sync {
    /// One-shot chat completion. `messages` carries the full conversation
    /// (system + turns); `tools` lists the callable tools — pass `&[]` to
    /// opt out of tool use.
    fn chat<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<Completion>>;

    /// Streaming chat — the returned stream yields `Token`s as the model
    /// generates. Providers that do not natively stream may buffer; M7
    /// callers (the Hub's "test prompt" button) always use `chat`.
    fn stream<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<TokenStream<'a>>>;

    /// A stable, unique identifier — `"anthropic"`, `"groq"`, …. Used as the
    /// `id` in `settings.json` and as the keychain key suffix.
    fn name(&self) -> &str;

    /// i18n key for the display name — `"provider.llm.anthropic"`, …
    fn display_name_key(&self) -> &str;

    /// Whether the provider can be invoked with `tools`. Anthropic, OpenAI,
    /// Groq, and most modern providers return `true`; some legacy
    /// OpenAI-compat servers return `false`.
    fn supports_tool_use(&self) -> bool;

    /// How well this provider handles Hebrew (docs/adr/0022).
    fn supports_hebrew(&self) -> Confidence;

    /// Context window in tokens — informational, for the Hub copy.
    fn context_window(&self) -> usize;

    /// True when inference runs locally — no data leaves the machine.
    /// `OllamaRemoteLlmProvider` is **not** local (data leaves the box),
    /// even when the remote is on the same LAN (docs/adr/0022 Invariant 2).
    fn is_local(&self) -> bool;

    /// The default model id sent on the wire when the user has not
    /// selected an explicit model.
    fn default_model(&self) -> &str;

    /// The model picker's options. Each entry is a model id accepted by
    /// the provider's API. Dynamic providers (Ollama) populate this from
    /// `/api/tags` at construction; static providers return a fixed list.
    fn available_models(&self) -> Vec<String>;

    /// Whether this provider has an API key (or doesn't need one — Ollama).
    /// Read by `Provider::Meta::has_api_key` for the Hub's "saved" pill.
    fn has_api_key(&self) -> bool;

    /// Fetch the live model list from the provider's `/v1/models` endpoint
    /// (or vendor equivalent). Used by the Hub: when the user pastes an
    /// API key, the picker swaps from the hard-coded `available_models()`
    /// list to whatever the provider actually serves — so a brand-new
    /// release model (or a fine-tune private to the user's org) shows up
    /// without a Lashon update.
    ///
    /// The implementation is expected to **filter, sort, and cap** the
    /// raw response before returning so the Hub's dropdown stays usable
    /// (some providers — OpenRouter, Together AI — return hundreds of
    /// models, many of which are embedding / image / audio variants the
    /// Command-mode dispatcher cannot use). `total` carries the pre-cap
    /// count so the UI can show "30 of 78 models from provider".
    ///
    /// Default impl returns `available_models()` unmodified. Providers
    /// override when the vendor exposes a discovery endpoint
    /// (`AnthropicLlmProvider`, `OpenAiCompatLlmProvider`); providers
    /// without one (the bundled `LocalLlmProvider` — only one model runs
    /// per instance) keep the default.
    fn fetch_remote_models<'a>(&'a self) -> BoxFuture<'a, Result<RemoteModels>> {
        let models = self.available_models();
        let total = models.len();
        Box::pin(async move { Ok(RemoteModels { models, total }) })
    }
}

/// Output of [`LLMProvider::fetch_remote_models`]. The Hub renders
/// `models` directly into the dropdown; `total` is the pre-cap /
/// pre-filter count, used for the "showing N of M" hint so the user
/// knows the list was trimmed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModels {
    pub models: Vec<String>,
    pub total: usize,
}

/// Hard cap on the number of models surfaced to the Hub. OpenRouter +
/// Together AI return several hundred entries; even after filtering
/// out non-chat models the list is too long to scroll comfortably.
/// 30 is enough to show every recent flagship + a tail of older
/// snapshots; users who need an obscure version can type it into the
/// base-URL override or the `llm.<mode>.model` setting directly.
pub const REMOTE_MODELS_CAP: usize = 30;

/// Predicate for "this model id looks like a chat-completable model".
/// Conservative — only filters out hard-known-non-chat patterns
/// (embeddings, image gen, TTS, STT, moderation, legacy base
/// completion). Borderline cases (realtime variants, audio variants)
/// stay in the list since they're sometimes Command-mode-usable.
pub fn is_chat_capable_model(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    if lower.contains("embedding") {
        return false;
    }
    if lower.contains("moderation") {
        return false;
    }
    const NON_CHAT_PREFIXES: &[&str] = &[
        "dall-e",
        "tts-",
        "whisper-",
        "davinci-",
        "babbage-",
        "ada-",
        "gpt-image-",
        "text-embedding-",
        "text-moderation-",
        "omni-moderation-",
        "image-",
    ];
    if NON_CHAT_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return false;
    }
    true
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// A trait-conforming mock provider used by the registry tests and as a
    /// drop-in replacement when integration tests cannot reach a real cloud.
    pub struct MockLlmProvider {
        pub name: &'static str,
        pub display_name_key: &'static str,
        pub reply: String,
        pub hebrew: Confidence,
        pub local: bool,
        pub default_model: &'static str,
        pub models: Vec<String>,
    }

    impl MockLlmProvider {
        pub fn hebrew_excellent_local(reply: impl Into<String>) -> Self {
            Self {
                name: "mock",
                display_name_key: "provider.llm.mock",
                reply: reply.into(),
                hebrew: Confidence::Excellent,
                local: true,
                default_model: "mock-1",
                models: vec!["mock-1".into()],
            }
        }
    }

    impl LLMProvider for MockLlmProvider {
        fn chat<'a>(
            &'a self,
            _messages: &'a [Msg],
            _tools: &'a [Tool],
        ) -> BoxFuture<'a, Result<Completion>> {
            let reply = self.reply.clone();
            let model = self.default_model.to_string();
            Box::pin(async move {
                Ok(Completion {
                    content: MsgContent::text(reply),
                    model,
                    usage: Some(Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    }),
                    finish_reason: Some("stop".to_string()),
                })
            })
        }

        fn stream<'a>(
            &'a self,
            _messages: &'a [Msg],
            _tools: &'a [Tool],
        ) -> BoxFuture<'a, Result<TokenStream<'a>>> {
            let reply = self.reply.clone();
            Box::pin(async move {
                let stream = futures::stream::iter(vec![
                    Ok(Token::Text(reply)),
                    Ok(Token::Finish {
                        reason: Some("stop".into()),
                        usage: None,
                    }),
                ]);
                Ok(Box::pin(stream) as TokenStream<'a>)
            })
        }

        fn name(&self) -> &str {
            self.name
        }
        fn display_name_key(&self) -> &str {
            self.display_name_key
        }
        fn supports_tool_use(&self) -> bool {
            true
        }
        fn supports_hebrew(&self) -> Confidence {
            self.hebrew
        }
        fn context_window(&self) -> usize {
            8192
        }
        fn is_local(&self) -> bool {
            self.local
        }
        fn default_model(&self) -> &str {
            self.default_model
        }
        fn available_models(&self) -> Vec<String> {
            self.models.clone()
        }
        fn has_api_key(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::MockLlmProvider;
    use super::*;
    use futures::StreamExt;

    #[test]
    fn fetch_remote_models_default_returns_static_list() {
        // Providers that don't override fetch_remote_models inherit the
        // default impl, which mirrors available_models(). MockLlmProvider
        // doesn't override; the Hub's fallback path relies on this.
        let mock = MockLlmProvider::hebrew_excellent_local("hi");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let fetched = runtime.block_on(mock.fetch_remote_models()).unwrap();
        assert_eq!(fetched.models, mock.available_models());
        assert_eq!(fetched.total, fetched.models.len());
        assert!(!fetched.models.is_empty());
    }

    #[test]
    fn is_chat_capable_filters_embeddings_image_tts() {
        // Hard-known-non-chat: embeddings, image gen, TTS, STT,
        // moderation, legacy base completion.
        assert!(!is_chat_capable_model("text-embedding-3-small"));
        assert!(!is_chat_capable_model("text-embedding-ada-002"));
        assert!(!is_chat_capable_model("dall-e-3"));
        assert!(!is_chat_capable_model("tts-1"));
        assert!(!is_chat_capable_model("tts-1-hd"));
        assert!(!is_chat_capable_model("whisper-1"));
        assert!(!is_chat_capable_model("text-moderation-stable"));
        assert!(!is_chat_capable_model("omni-moderation-latest"));
        assert!(!is_chat_capable_model("davinci-002"));
        assert!(!is_chat_capable_model("babbage-002"));
        assert!(!is_chat_capable_model("gpt-image-1"));
        // Conservative: chat-shaped and unknown ids stay in.
        assert!(is_chat_capable_model("gpt-4o"));
        assert!(is_chat_capable_model("gpt-4o-mini"));
        assert!(is_chat_capable_model("o1"));
        assert!(is_chat_capable_model("o3-mini"));
        assert!(is_chat_capable_model("claude-sonnet-4-6"));
        assert!(is_chat_capable_model("llama-3.3-70b-versatile"));
        assert!(is_chat_capable_model("gpt-4o-realtime-preview"));
        // Borderline: audio variants stay (some are voice-chat capable).
        assert!(is_chat_capable_model("gpt-4o-audio-preview"));
    }

    #[test]
    fn role_serialises_lowercased() {
        let user = serde_json::to_string(&Role::User).unwrap();
        assert_eq!(user, "\"user\"");
        let tool = serde_json::to_string(&Role::Tool).unwrap();
        assert_eq!(tool, "\"tool\"");
    }

    #[test]
    fn msg_content_text_to_plain_text() {
        let content = MsgContent::text("שלום עולם");
        assert_eq!(content.to_plain_text(), "שלום עולם");
    }

    #[test]
    fn msg_content_blocks_concat_text_only() {
        let content = MsgContent::Blocks {
            blocks: vec![
                ContentBlock::Text {
                    text: "שלום".into(),
                },
                ContentBlock::ToolCall {
                    id: "call_1".into(),
                    name: "open_app".into(),
                    arguments: serde_json::json!({"name": "vscode"}),
                },
                ContentBlock::Text {
                    text: "עולם".into(),
                },
            ],
        };
        assert_eq!(content.to_plain_text(), "שלום\nעולם");
    }

    #[test]
    fn msg_round_trips_through_json() {
        let original = Msg::user("שלום, מה השעה?");
        let json = serde_json::to_string(&original).unwrap();
        let round_trip: Msg = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, original);
    }

    #[test]
    fn mock_provider_chat_returns_the_canned_reply() {
        let provider = MockLlmProvider::hebrew_excellent_local("שלום עולם");
        let messages = [Msg::user("שלום")];
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let completion = runtime
            .block_on(provider.chat(&messages, &[]))
            .expect("mock provider chat must succeed");
        assert_eq!(completion.content.to_plain_text(), "שלום עולם");
        assert_eq!(completion.model, "mock-1");
    }

    #[test]
    fn full_conversation_round_trips_through_json() {
        // The Msg/Tool union must carry every shape the LLM trait surface
        // needs (docs/adr/0019): system + user text + assistant text-blocks
        // + assistant tool-call + tool result. M8's tool registry will
        // build conversations exactly like this; the round-trip must lose
        // nothing.
        let conversation: Vec<Msg> = vec![
            Msg::system("את לשון."),
            Msg::user("פתח את VS Code"),
            Msg {
                role: Role::Assistant,
                content: MsgContent::Blocks {
                    blocks: vec![
                        ContentBlock::Text {
                            text: "פותחת את VS Code".into(),
                        },
                        ContentBlock::ToolCall {
                            id: "call_1".into(),
                            name: "open_app".into(),
                            arguments: serde_json::json!({"name": "vscode"}),
                        },
                    ],
                },
            },
            Msg {
                role: Role::Tool,
                content: MsgContent::ToolResult {
                    call_id: "call_1".into(),
                    content: "ok: vscode launched".into(),
                },
            },
            Msg::assistant("הכל בוצע."),
        ];
        let json = serde_json::to_string(&conversation).unwrap();
        let round_trip: Vec<Msg> = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, conversation);
        // The Hebrew text survives unchanged through the round-trip — every
        // Hebrew codepoint in the original is in the deserialised vec.
        let hebrew_chars: usize = round_trip
            .iter()
            .map(|msg| {
                msg.content
                    .to_plain_text()
                    .chars()
                    .filter(|c| ('\u{0590}'..='\u{05FF}').contains(c))
                    .count()
            })
            .sum();
        assert!(
            hebrew_chars > 0,
            "Hebrew chars must survive JSON round-trip"
        );
    }

    #[test]
    fn tool_definition_round_trips_through_json() {
        let tool = Tool {
            name: "open_app".into(),
            description: "Open an application by name".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"}
                },
                "required": ["name"]
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let round_trip: Tool = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, tool);
    }

    #[test]
    fn mock_provider_stream_yields_text_then_finish() {
        let provider = MockLlmProvider::hebrew_excellent_local("שלום");
        let messages = [Msg::user("שלום")];
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tokens = runtime.block_on(async {
            let mut stream = provider.stream(&messages, &[]).await.unwrap();
            let mut tokens = Vec::new();
            while let Some(token) = stream.next().await {
                tokens.push(token.unwrap());
            }
            tokens
        });
        assert!(matches!(tokens[0], Token::Text(ref t) if t == "שלום"));
        assert!(matches!(tokens[1], Token::Finish { .. }));
    }
}
