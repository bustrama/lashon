//! Shared provider types used by both the STT and the LLM stages
//! (docs/adr/0019). `Confidence` rates a provider's Hebrew quality on a
//! four-point scale; `ProviderMeta` is the serialisable summary the Hub
//! renders into its chip grid; `ProviderError` is the typed error every
//! provider surfaces back to the dictation FSM and the Hub.

use serde::{Deserialize, Serialize};

/// How well a provider handles Hebrew. The four ranks are deliberately ordered
/// so the registry can prefer the highest-confidence Hebrew provider when no
/// explicit selection has been made (docs/adr/0019, docs/adr/0022).
///
/// The four ranks are honest, not aspirational — `.claude/rules/security.md`
/// requires every cloud provider to surface a faithful Hebrew rating. Promotion
/// to `Good` or `Excellent` is gated on documented benchmark or manual-eval
/// evidence in [`docs/providers.md`](../../../docs/providers.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Confidence {
    /// The provider rejects Hebrew or has no Hebrew language support.
    None,
    /// Hebrew is accepted but the quality has not been verified by Lashon.
    /// Drives the `~ Hebrew (unverified)` Hub badge.
    Basic,
    /// Hebrew is usable — verified by informal testing or vendor documentation
    /// (docs/adr/0022, WER ≤ 25% on the test corpus or 20-sentence manual eval).
    Good,
    /// Hebrew is benchmarked — WER ≤ 15% on `tests/hebrew-corpus/` or
    /// independently documented in `docs/providers.md`.
    Excellent,
}

/// A serialisable summary of a provider, returned by the Tauri command surface
/// (`get_llm_providers`, `get_stt_providers`) and rendered into the Hub's
/// chip grid (docs/adr/0021). The chip carries the cloud badge, the Hebrew
/// badge, and a `has_api_key` indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMeta {
    /// Stable id used in `settings.json` (`"anthropic"`, `"groq"`, …).
    pub id: String,
    /// The i18n key for the display name (`"provider.llm.anthropic"`, …).
    pub display_name_key: String,
    /// True when inference runs on the user's own machine.
    pub is_local: bool,
    /// Honest Hebrew-quality rating (docs/adr/0022).
    pub supports_hebrew: Confidence,
    /// True when this provider already has a key stored in the OS keychain,
    /// or doesn't require one (Ollama local). The Hub uses this to render
    /// "●●●●●● ✓ saved" vs the "Enter API key" input.
    pub has_api_key: bool,
    /// Default model — e.g. `"claude-sonnet-4-6"`, `"gpt-4.1"`, `"llama-3.3-70b-versatile"`.
    pub default_model: String,
    /// The model list shown in the picker dropdown.
    pub available_models: Vec<String>,
    /// Context-window size in tokens — drives the Hub copy ("200 K tokens").
    pub context_window: usize,
    /// Whether the provider supports vendor tool/function calling.
    pub supports_tool_use: bool,
    /// Optional pointer into `available_models` marking one as the
    /// fastest-yet-accurate pick for Lashon's Command-mode workload. The
    /// Hub renders the matching dropdown entry with a "מומלץ /
    /// recommended" suffix. `None` means the provider has no opinion.
    pub recommended_model: Option<String>,
}

/// Typed provider errors. Each variant maps to a user-visible toast in the
/// Hub or the dictation FSM (docs/adr/0019, docs/adr/0022).
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The active provider needs an API key but none is stored. A fresh
    /// install surfaces this as: "Cloud provider {name} needs an API key —
    /// configure it in Settings → LLM" (docs/adr/0022 Invariant 5).
    #[error("API key not found for provider {provider}")]
    KeyNotFound { provider: String },

    /// The active provider returned a 401/403 — likely a wrong key, a
    /// revoked key, or (for OpenCode Go and similar tiered services) a
    /// subscription-tier mismatch. The vendor's response body is included
    /// in the message so the user can read what the server actually said
    /// — `"subscription required"`, `"invalid api key"`, etc.
    #[error("provider {provider} rejected the request ({status}): {message}")]
    Unauthorized {
        provider: String,
        status: u16,
        message: String,
    },

    /// The active provider returned a 429 — surfaced verbatim in the toast so
    /// the user sees the vendor's retry guidance.
    #[error("provider {provider} is rate-limited: {message}")]
    RateLimited { provider: String, message: String },

    /// Anything else the provider returned an HTTP error for — the body is
    /// best-effort-truncated and surfaced for debugging.
    #[error("provider {provider} returned HTTP {status}: {body}")]
    Http {
        provider: String,
        status: u16,
        body: String,
    },

    /// The provider's response did not match the expected wire format.
    #[error("provider {provider} returned an unexpected response: {detail}")]
    BadResponse { provider: String, detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_orders_lowest_to_highest() {
        assert!(Confidence::None < Confidence::Basic);
        assert!(Confidence::Basic < Confidence::Good);
        assert!(Confidence::Good < Confidence::Excellent);
        let mut ranks = vec![
            Confidence::Good,
            Confidence::None,
            Confidence::Excellent,
            Confidence::Basic,
        ];
        ranks.sort();
        assert_eq!(
            ranks,
            vec![
                Confidence::None,
                Confidence::Basic,
                Confidence::Good,
                Confidence::Excellent,
            ]
        );
    }

    #[test]
    fn confidence_serialises_to_a_variant_string() {
        let json = serde_json::to_string(&Confidence::Excellent).unwrap();
        assert_eq!(json, "\"Excellent\"");
        let round_trip: Confidence = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, Confidence::Excellent);
    }

    #[test]
    fn provider_meta_round_trips_through_json() {
        let meta = ProviderMeta {
            id: "anthropic".into(),
            display_name_key: "provider.llm.anthropic".into(),
            is_local: false,
            supports_hebrew: Confidence::Excellent,
            has_api_key: false,
            default_model: "claude-sonnet-4-6".into(),
            available_models: vec!["claude-sonnet-4-6".into()],
            context_window: 200_000,
            supports_tool_use: true,
            recommended_model: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let round_trip: ProviderMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.id, meta.id);
        assert_eq!(round_trip.supports_hebrew, Confidence::Excellent);
    }

    #[test]
    fn provider_error_renders_a_hebrew_message_path() {
        let err = ProviderError::KeyNotFound {
            provider: "anthropic".into(),
        };
        let rendered = format!("{err}");
        assert!(rendered.contains("anthropic"));
    }
}
