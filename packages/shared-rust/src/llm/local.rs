//! In-process local LLM provider (`docs/adr/0025`).
//!
//! `LocalLlmProvider` is a thin OpenAI-compatible HTTP shim over a
//! Lashon-managed `llama-server` subprocess (the prebuilt ggml binary,
//! bundled as a Tauri resource). The Tauri shell owns the subprocess
//! lifecycle — spawn on first use, health-check, Win32 Job Object
//! kill-on-parent-exit — and pokes the live loopback URL into the
//! provider via `with_base_url`. Inference itself goes over loopback
//! HTTP, same shape as M7's `OllamaLocalLlmProvider`.
//!
//! The subprocess design replaces an in-process attempt: `mistralrs`
//! delivered >100× too slow on CPU (no SIMD without Intel MKL) and the
//! `llama-cpp-2` Rust bindings hit a build wall (cmake + libclang +
//! the Windows CUDA stdlib mismatch on VS 2025). The prebuilt
//! `llama-server` runs the same llama.cpp kernels with no
//! build-tooling cost and gives us Vulkan compute on any GPU (NVIDIA,
//! AMD, Intel) out of the box.
//!
//! Default model: `qwen3-4b-q4_k_m` (Apache-2.0, ~2.5 GB on disk,
//! ~3.5 GB warm RAM with the 16 K context the M8.2 prompt needs).
//! Picked as the smallest Qwen3 variant that can reliably handle a
//! multi-step interactive tool chain — the earlier 1.7B Q8_0 default
//! consistently dropped messaging-app chains mid-flow (Discord: typed
//! into the wrong field, never pressed the final Enter to send) and
//! kept saying "done" after only partial work. The 1.7B variant
//! stays in the picker for users on very weak hardware who prefer
//! speed over accuracy.

use anyhow::Result;

use super::openai_compat::{OpenAiCompatConfig, OpenAiCompatLlmProvider};
use super::{BoxFuture, Completion, LLMProvider, Msg, TokenStream, Tool};
use crate::provider::Confidence;

/// Stable id under which the local-LLM provider lives in `settings.json`,
/// the keychain (n/a — no key needed), and the Hub chip grid.
pub const PROVIDER_ID: &str = "local-llm";

/// i18n key for the Hub display name.
pub const DISPLAY_NAME_KEY: &str = "provider.llm.local_llm";

/// The model id the Hub picks by default — overridable in `settings.json`
/// under `llm.<mode>.model`. Mirrors the first entry in
/// `models/manifests/local-llm.json`.
pub const DEFAULT_MODEL: &str = "qwen3-4b-q4_k_m";

/// The model picker's options — drawn from the manifest at startup.
/// Hard-coded here as the registry's static fallback; the live list
/// (with install status) comes from `crate::model::available_local_llm_models`.
/// Ordered with the default first so the Hub renders it first.
pub const AVAILABLE_MODELS: &[&str] = &["qwen3-4b-q4_k_m", "qwen3-1.7b-q8_0"];

/// Qwen3's effective context window for Lashon: the model is trained
/// to 40,960 tokens but the dispatcher caps each request at 4096 for
/// Command-mode latency (docs/adr/0025 §8). `LocalLlmProvider` reports
/// the trained context here so the Hub copy is honest; the actual
/// llama-server context is sized at spawn time.
pub const CONTEXT_WINDOW: usize = 40_960;

/// Always `true` — the runtime is a separate process this build can
/// spawn unconditionally. Kept as a constant so the Tauri shell's
/// `local_llm_status` command can render the same "ready / not ready"
/// chrome that the in-process variant exposed (`docs/adr/0025` §6).
pub const RUNTIME_AVAILABLE: bool = true;

/// Synthetic `OpenAiCompatConfig` that gives the inner client the
/// shape it needs (no auth, OpenAI-compatible wire format). The
/// `default_base_url` field is intentionally empty — every caller
/// must populate it via `with_base_url` after the Tauri shell spawns
/// `llama-server` and learns the actual loopback port.
const VENDOR_CONFIG: OpenAiCompatConfig = OpenAiCompatConfig {
    name: PROVIDER_ID,
    display_name_key: DISPLAY_NAME_KEY,
    default_base_url: "",
    default_model: DEFAULT_MODEL,
    available_models: AVAILABLE_MODELS,
    supports_hebrew: Confidence::Basic,
    context_window: CONTEXT_WINDOW,
    supports_tool_use: true,
    is_local: true,
    requires_api_key: false,
    recommended_model: Some(DEFAULT_MODEL),
};

/// `LLMProvider` impl that forwards chat requests to a Lashon-managed
/// `llama-server` over loopback HTTP. Construction is cheap; the
/// inner `reqwest::Client` is recreated per provider instance to stay
/// `Send`/`Sync` without locking.
///
/// The Tauri shell builds this on every chat call (lifecycle of the
/// underlying subprocess is owned by the shell, not the provider).
pub struct LocalLlmProvider {
    model_id: String,
    base_url: String,
    inner: OpenAiCompatLlmProvider,
}

impl LocalLlmProvider {
    /// Construct a provider for `model_id` that has no base URL set
    /// yet. **Calling `chat` in this state errors** — the Tauri shell
    /// must call `with_base_url` after `llama-server` reports ready.
    pub fn new(model_id: impl Into<String>) -> Self {
        let model_id = model_id.into();
        let inner = OpenAiCompatLlmProvider::new(VENDOR_CONFIG).with_model(&model_id);
        Self {
            model_id,
            base_url: String::new(),
            inner,
        }
    }

    /// Override which model id this instance reports to the server.
    /// `llama-server` ignores the field in the wire payload (it serves
    /// whichever GGUF it was launched with) but Lashon's logs print
    /// it, so keeping it accurate aids diagnostics.
    pub fn with_model(mut self, model_id: impl Into<String>) -> Self {
        let new_id = model_id.into();
        if !new_id.is_empty() {
            self.model_id = new_id.clone();
            self.inner = self.inner.with_model(new_id);
        }
        self
    }

    /// Point this instance at a running `llama-server`'s loopback URL
    /// (`http://127.0.0.1:<port>/v1`, including the `/v1` suffix). The
    /// Tauri shell sets this once it has spawned the subprocess and
    /// the `/health` probe has come back OK.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        if !url.is_empty() {
            self.base_url = url.clone();
            self.inner = self.inner.with_base_url(url);
        }
        self
    }
}

impl Default for LocalLlmProvider {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL)
    }
}

impl LLMProvider for LocalLlmProvider {
    fn chat<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<Completion>> {
        if self.base_url.is_empty() {
            return Box::pin(async {
                Err(anyhow::anyhow!(
                    "local-llm: no llama-server base URL set — the Tauri shell \
                     must spawn the subprocess and call `with_base_url` first"
                ))
            });
        }
        // Delegation through the OpenAI-compat client gives us the
        // existing tool-call serialiser and the same Anthropic-shaped
        // `Completion` translation every other provider already returns.
        self.inner.chat(messages, tools)
    }

    fn stream<'a>(
        &'a self,
        messages: &'a [Msg],
        tools: &'a [Tool],
    ) -> BoxFuture<'a, Result<TokenStream<'a>>> {
        if self.base_url.is_empty() {
            return Box::pin(async {
                Err(anyhow::anyhow!(
                    "local-llm: no llama-server base URL set — the Tauri shell \
                     must spawn the subprocess and call `with_base_url` first"
                ))
            });
        }
        self.inner.stream(messages, tools)
    }

    fn name(&self) -> &str {
        PROVIDER_ID
    }

    fn display_name_key(&self) -> &str {
        DISPLAY_NAME_KEY
    }

    fn supports_tool_use(&self) -> bool {
        true
    }

    fn supports_hebrew(&self) -> Confidence {
        // Qwen3 is competent in Hebrew but has no published Hebrew WER
        // benchmark and is not in `tests/hebrew-corpus/`. Honest rating
        // per docs/adr/0022 Invariant 3: Basic until measured.
        Confidence::Basic
    }

    fn context_window(&self) -> usize {
        CONTEXT_WINDOW
    }

    fn is_local(&self) -> bool {
        // ADR-0022 Invariant 2: inference runs entirely on the user's
        // machine; no bytes leave the loopback interface.
        true
    }

    fn default_model(&self) -> &str {
        &self.model_id
    }

    fn available_models(&self) -> Vec<String> {
        AVAILABLE_MODELS.iter().map(|s| (*s).to_string()).collect()
    }

    fn has_api_key(&self) -> bool {
        // No key required — surface "saved" so the Hub never prompts.
        true
    }
}

/// Compose the canonical `http://127.0.0.1:<port>/v1` base URL the
/// `OpenAiCompatLlmProvider` expects. Used by the Tauri shell when it
/// spawns `llama-server` on a freshly chosen loopback port.
pub fn loopback_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_reports_local_and_keyless() {
        let provider = LocalLlmProvider::default();
        assert!(
            provider.is_local(),
            "in-process inference must report local"
        );
        assert!(
            provider.has_api_key(),
            "no key required → has_api_key=true so the Hub never prompts"
        );
        assert!(
            provider.supports_tool_use(),
            "Qwen3 family supports native tool calling"
        );
    }

    #[test]
    fn default_model_is_the_4b_qwen() {
        let provider = LocalLlmProvider::default();
        assert_eq!(provider.default_model(), DEFAULT_MODEL);
        assert_eq!(
            DEFAULT_MODEL, "qwen3-4b-q4_k_m",
            "the 4B Q4_K_M is the smallest Qwen3 variant that reliably \
             completes multi-step interactive tool chains; the 1.7B \
             stayed in the picker for very-weak-hardware users who \
             prefer speed over accuracy"
        );
    }

    #[test]
    fn hebrew_rating_is_honest_basic_until_benchmarked() {
        // ADR-0022 Invariant 3: every provider not benchmarked against
        // tests/hebrew-corpus/ ships as Basic. Promotion needs evidence.
        let provider = LocalLlmProvider::default();
        assert_eq!(provider.supports_hebrew(), Confidence::Basic);
    }

    #[test]
    fn available_models_lists_both_manifest_entries() {
        let provider = LocalLlmProvider::default();
        let models = provider.available_models();
        assert!(
            models.iter().any(|m| m == "qwen3-1.7b-q8_0"),
            "1.7B variant must be in the picker"
        );
        assert!(
            models.iter().any(|m| m == "qwen3-4b-q4_k_m"),
            "4B variant must be in the picker"
        );
    }

    #[test]
    fn with_model_overrides_the_default() {
        let provider = LocalLlmProvider::default().with_model("qwen3-4b-q4_k_m");
        assert_eq!(provider.default_model(), "qwen3-4b-q4_k_m");
    }

    #[test]
    fn with_model_empty_string_keeps_the_existing_pick() {
        let provider = LocalLlmProvider::default().with_model("");
        assert_eq!(
            provider.default_model(),
            DEFAULT_MODEL,
            "an empty override must not clobber the default"
        );
    }

    #[test]
    fn chat_without_base_url_errors_clearly() {
        let provider = LocalLlmProvider::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(provider.chat(&[Msg::user("hi")], &[]))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("base URL"),
            "error must name the missing base URL: {err}"
        );
    }

    #[test]
    fn loopback_base_url_renders_correctly() {
        assert_eq!(loopback_base_url(11435), "http://127.0.0.1:11435/v1");
    }
}
