# 19. Formal provider-mux trait abstraction

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** Milestone M7 — provider mux foundation
  ([`../stories/m7-provider-mux.md`](../stories/m7-provider-mux.md));
  formalises the sketches in
  [`../architecture.md §4`](../architecture.md).

## Context

`docs/architecture.md §4` sketches `SttProvider`, `LLMProvider`, and
`TTSProvider` traits as the central design principle. The sketch shows intent
— "every stage exposes a common trait" — but does not pin:

- The exact method signatures (notably `transcribe_stream`,
  `warmup`, `context_window`, `first_token_latency_ms`).
- The `Confidence` type (shared between STT and LLM, or per-stage?).
- How `is_local()` and `supports_hebrew()` interact with the registry's
  default-selection logic.
- The async execution model: `#[allow(async_fn_in_trait)]` works for a
  concrete receiver but breaks with `Box<dyn SttProvider>` because the
  implicit `impl Future` return type is not object-safe. Either every trait
  method that is async must return a concrete `BoxFuture`, or the `async-trait`
  macro is used, or Rust 1.75+ associated `impl Trait` in trait is used with an
  explicit `Send` bound.
- The lifecycle: when is a provider constructed? Who owns it? How are
  construction errors surfaced?
- The registry: how many providers can be active simultaneously? Is it
  per-stage, per-mode-within-stage, or flat?

M7 needs answers to all of the above before the first line of provider
code is written, because the first PR (`docs/stories/m7-provider-mux.md`
Phase 1) locks the trait shape that phases 2–5 build against.

## Decision

### The `Confidence` enum

Defined once in `lashon-core::provider` (a new module) and re-exported from
stage-specific modules:

```rust
/// How well a provider handles Hebrew.
/// Used by both SttProvider and LLMProvider for the `supports_hebrew` signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Confidence {
    None,
    Basic,    // accepts Hebrew, quality unverified
    Good,     // usable; verified by informal testing or provider docs
    Excellent, // benchmarked against the Hebrew corpus (WER / manual eval)
}
```

`PartialOrd + Ord` so the registry can steer towards the highest-confidence
Hebrew provider when a Hebrew-first default is computed.

### The async execution model: `BoxFuture`

`Box<dyn SttProvider + Send + Sync>` requires trait object safety. Rust 1.75
`async fn in trait` produces an opaque `impl Future` return type; this is not
object-safe unless the concrete type is known. The options are:

1. `async-trait` macro — adds a proc-macro dependency, allocates a `Box` per
   call, well-understood pattern.
2. `BoxFuture` return type declared explicitly — no macro, same allocation, but
   more verbose.
3. Associated `impl Trait` with `+ Send` — Rust nightly / RPIT in traits;
   not stable in Rust 1.95.

**Decision: explicit `BoxFuture` on async trait methods.** No new dependency;
the pattern is compatible with stable Rust 1.95. The existing
`#[allow(async_fn_in_trait)]` in `stt.rs` is removed from the trait definition
and the `BoxFuture` pattern applied. The `FasterWhisperProvider` implementation
wraps its existing `async fn` body in `Box::pin(async move { … })`.

```rust
use std::pin::Pin;
use std::future::Future;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

### `SttProvider` (finalised)

```rust
pub trait SttProvider: Send + Sync {
    /// Transcribe a complete 16 kHz mono float32 PCM buffer.
    fn transcribe<'a>(
        &'a self,
        pcm_f32: &'a [f32],
        language: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<Transcript>>;

    /// Streaming transcription — emits TranscriptDelta items as the model
    /// refines partial hypotheses. The default impl buffers the input
    /// stream and delegates to `transcribe`, emitting one final delta.
    /// Providers with native streaming (Deepgram, AssemblyAI) override.
    fn transcribe_stream<'a>(
        &'a self,
        pcm: BoxStream<'a, Vec<f32>>,
        language: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<BoxStream<'a, anyhow::Result<TranscriptDelta>>>> {
        // Default: buffer + delegate. Defined as a `default fn` body in
        // the trait so non-streaming providers inherit it free.
        default_buffer_and_delegate(self, pcm, language)
    }

    /// Whether `transcribe_stream` has a native streaming impl (vs the
    /// buffer-and-delegate default). Drives the Hub's "live captions"
    /// affordance — only providers that natively stream show it.
    fn supports_streaming(&self) -> bool { false }

    /// Human-readable unique identifier ("local-faster-whisper", "groq", …).
    fn name(&self) -> &str;

    /// i18n key for the display name ("provider.stt.local_faster_whisper", …).
    fn display_name_key(&self) -> &str;

    /// How well this provider handles Hebrew (Confidence::None to Excellent).
    fn supports_hebrew(&self) -> Confidence;

    /// True when transcription runs on this machine — no audio leaves it.
    fn is_local(&self) -> bool;
}

/// A partial or final transcript emitted by a streaming provider.
pub struct TranscriptDelta {
    /// The cumulative transcript text up to this point. The Hub renders
    /// the latest delta as a replacement of the live transcript area,
    /// not as an append — providers refine prior hypotheses freely.
    pub text: String,
    /// `true` when this delta is the final transcript for the utterance
    /// (the stream may yield one or more interim deltas first).
    pub is_final: bool,
    /// Per-delta confidence in [0.0, 1.0]; `None` when the provider does
    /// not surface it.
    pub confidence: Option<f32>,
}

pub type BoxStream<'a, T> = Pin<Box<dyn futures::Stream<Item = T> + Send + 'a>>;
```

`transcribe_stream` is in M7's scope from Phase 1. The default impl
(`default_buffer_and_delegate`) collects the input PCM stream into a
`Vec<f32>`, calls `transcribe`, and yields one `TranscriptDelta` with
`is_final = true`. This keeps non-streaming providers conforming with
zero per-provider code. Phase 2 of M7 ships native streaming impls for
Deepgram (WS to `api.deepgram.com/v1/listen`) and AssemblyAI (WS to
`api.assemblyai.com/v2/realtime/ws`); the local sidecar's streaming
wrapper is a follow-up, not part of M7.

`warmup()` is not on the public trait; the `FasterWhisperProvider`'s warm-up
is internal to its construction / the sidecar lifecycle.

### `LLMProvider` (finalised)

```rust
pub trait LLMProvider: Send + Sync {
    fn chat<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, anyhow::Result<Completion>>;

    fn stream<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, anyhow::Result<Box<dyn Stream<Item = anyhow::Result<Token>> + Send + Unpin>>>;

    fn name(&self) -> &str;
    fn display_name_key(&self) -> &str;
    fn supports_tool_use(&self) -> bool;
    fn supports_hebrew(&self) -> Confidence;
    fn context_window(&self) -> usize;  // in tokens
    fn is_local(&self) -> bool;
    fn default_model(&self) -> &str;    // e.g. "claude-sonnet-4-6"
    fn available_models(&self) -> &[&str];
}
```

The `Stream` trait here is `futures::Stream`; `tokio-stream` is the practical
impl. `tokio-stream` is already an indirect dependency via `tonic`; confirm
it is directly usable.

### Vendor-neutral `Msg` and `Tool` types

These cover both Anthropic-style multi-block content (`MsgContent::Blocks`
with inline `ContentBlock::ToolCall` blocks) and OpenAI-style string
content with separate `tool_calls` (`MsgContent::Text` + a sibling
assistant message carrying `ContentBlock::ToolCall`s), without leaking
either vendor's wire format. Each provider impl translates to its vendor
wire format internally; callers never see vendor-specific structs. M8's
tool registry locks against these types and will not be allowed to break
them once it ships:

```rust
/// A vendor-neutral chat message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Msg {
    pub role: Role,
    pub content: MsgContent,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Role { System, User, Assistant, Tool }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MsgContent {
    Text(String),
    /// A tool invocation result the assistant previously emitted.
    ToolResult { call_id: String, content: String },
    /// Blocks: used by Anthropic's multi-block content and by
    /// OpenAI's function-call response format.
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ContentBlock {
    Text(String),
    ToolCall { id: String, name: String, arguments: serde_json::Value },
}
```

Each provider impl translates `Vec<Msg>` and `Vec<Tool>` into its own wire
format. The translation lives entirely inside the provider impl; callers
(the dictation FSM, the test harness) never see vendor-specific types.

### `ProviderRegistry<T>`

A generic registry keyed by provider name:

```rust
pub struct ProviderRegistry<T: ?Sized> {
    providers: HashMap<String, Arc<Box<dyn T>>>,
    active_id: String,
}

impl<T: ?Sized + Send + Sync> ProviderRegistry<T> {
    pub fn active(&self) -> &Box<dyn T> { … }
    pub fn set_active(&mut self, id: &str) -> Result<()> { … }
    pub fn list(&self) -> Vec<ProviderMeta> { … }
}
```

`ProviderMeta` is a serialisable summary for the Hub frontend:

```rust
#[derive(serde::Serialize)]
pub struct ProviderMeta {
    pub id: String,
    pub display_name_key: String,
    pub is_local: bool,
    pub supports_hebrew: Confidence,
    pub has_api_key: bool,
}
```

Instantiation of cloud providers is deferred until the provider is first
selected (lazy construction) so that app startup does not require all keys
to be present. A provider whose key is absent and that is set as active
returns `Err(ProviderError::KeyNotFound)` at call time; the dictation FSM
surfaces this as an error toast and falls back to the local provider.

### Lifecycle and ownership

The registry lives in `tauri::Manager` app state — a `Mutex<ProviderRegistry<dyn SttProvider>>`.
The Tauri shell constructs it at startup with the local `FasterWhisperProvider`
as the default. A `set_stt_provider` Tauri command updates the active id; the
next `transcribe` call picks up the new provider.

`TtsProvider` registry is deferred to M10. The registry type is generic, so
the same `ProviderRegistry<T>` struct serves all stages.

### Default-selection policy

When no provider has been explicitly set (`stt.provider` absent from
`settings.json`) the registry defaults to the first `is_local() == true`
entry with the highest `supports_hebrew()` confidence. This is always
`FasterWhisperProvider` in M7. The policy is implemented once, in the
registry, and never revisited by callers.

## Alternatives considered

- **`async-trait` macro** — straightforward but adds `async-trait` as a
  dependency. `BoxFuture` achieves the same with zero new deps; chosen over the
  macro for that reason.
- **`Arc<dyn SttProvider>` directly on trait objects** — equivalent to the
  registry design; the registry adds the selection logic on top.
- **Per-stage enum dispatch (a `SttProviderEnum` with arms per vendor)** —
  avoids dynamic dispatch at the cost of an `enum` that every new provider
  must extend. Rejected: it couples all providers at the type level and
  violates the extension-without-modification property the architecture
  requires.
- **Single flat `ProviderRegistry` with a `stage` discriminant** — simpler
  to reason about than `ProviderRegistry<T>`, but loses type-safety at the
  Tauri command boundary (any provider for any stage could be set as active
  for a different stage). Generic is safer.

## Consequences

- The `#[allow(async_fn_in_trait)]` annotation in the existing `stt.rs` is
  removed from the trait definition (kept only in non-object-safe internal
  helpers if any remain).
- `FasterWhisperProvider::transcribe` is wrapped in `Box::pin(async move { … })`
  — a mechanical, testable change; no behaviour delta.
- New modules in `lashon-core`: `provider` (shared types), `llm`, `provider_registry`.
- `Msg`, `Tool`, `Completion`, `Token` are defined in `lashon-core::llm`; they
  are the stable API surface M8's tool registry will depend on. Define them
  carefully; changing them after M8 ships is a breaking internal change.
- The `ProviderRegistry` state in the Tauri shell is wrapped in a `Mutex`;
  this is the serialization point for concurrent dictation calls (two
  simultaneous dictations share a registry but each call acquires the
  active provider by cloning its `Arc`). A brief lock is taken only to read
  the active id and clone the `Arc`; actual transcription runs without the
  lock held.
