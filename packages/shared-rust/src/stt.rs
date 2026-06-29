//! Speech-to-text provider abstraction (docs/architecture.md §4) and the
//! local faster-whisper provider.

use anyhow::{Context, Result};

use crate::sidecar::{healthcheck, ready_sidecar, HealthReport, SidecarState};
use crate::stt_proto::stt;

// `Confidence` is the same shape for STT and LLM (docs/adr/0019). It lives in
// `lashon-core::provider` and is re-exported here so callers can keep saying
// `use lashon_core::stt::Confidence`.
pub use crate::provider::Confidence;

/// One decoded segment and where it sits in the submitted audio. Timestamps are
/// seconds from the start of *that request's* buffer — for a windowed streaming
/// re-decode that is the window, not the whole utterance. The streaming
/// committer advances its window anchor past already-committed segments with
/// these (docs/adr/0037).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Segment {
    pub text: String,
    pub start: f32,
    pub end: f32,
}

/// A completed transcription.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Transcript {
    pub text: String,
    pub language: String,
    pub confidence: f32,
    pub inference_ms: u32,
    /// Per-segment timings, in order. Empty unless the decode reported them; the
    /// one-shot final decode ignores them.
    pub segments: Vec<Segment>,
}

/// Per-call knobs for [`SttProvider::transcribe`]. A struct rather than bare
/// arguments so new options (e.g. a windowed-decode prompt) don't churn every
/// call site each time the seam grows.
#[derive(Debug, Clone, Copy, Default)]
pub struct TranscribeOptions<'a> {
    /// BCP-47-style language hint, e.g. "he". Empty autodetects via the
    /// companion detector (docs/adr/0009).
    pub language: &'a str,
    /// Recent committed text, passed to Whisper as decoding context for a
    /// *windowed* streaming re-decode whose buffer no longer starts at the
    /// utterance's beginning (docs/adr/0037). Empty for the one-shot final
    /// decode and any decode anchored at sample 0.
    pub initial_prompt: &'a str,
}

impl<'a> TranscribeOptions<'a> {
    /// Options that just force a language, no decoding-context prompt — the
    /// shape every non-streaming caller wants.
    pub fn language(language: &'a str) -> Self {
        Self {
            language,
            initial_prompt: "",
        }
    }
}

/// A speech-to-text provider. Each engine implements this trait; callers route
/// through it and never bind to a concrete vendor (docs/architecture.md §4).
#[allow(async_fn_in_trait)]
pub trait SttProvider {
    /// Transcribe 16 kHz mono float32 PCM (samples in [-1.0, 1.0]).
    async fn transcribe(&self, pcm_f32: &[f32], opts: TranscribeOptions<'_>) -> Result<Transcript>;

    /// How well this provider handles Hebrew.
    fn supports_hebrew(&self) -> Confidence;

    /// True when transcription runs on this machine — no audio leaves it.
    fn is_local(&self) -> bool;
}

/// Pack 32-bit float PCM samples into little-endian bytes for the gRPC wire.
fn pcm_to_le_bytes(pcm_f32: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pcm_f32.len() * 4);
    for sample in pcm_f32 {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// Speech-to-text via the local faster-whisper Python sidecar — the default
/// provider on hardware tiers A–C.
#[derive(Default)]
pub struct FasterWhisperProvider {
    sidecar: SidecarState,
}

impl FasterWhisperProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Probe the STT sidecar's health, spawning it on the first call.
    ///
    /// Used to wait out the first-run model download before dictation is
    /// attempted — see the dictation worker.
    pub async fn health(&self) -> HealthReport {
        healthcheck(&self.sidecar).await
    }
}

impl SttProvider for FasterWhisperProvider {
    async fn transcribe(&self, pcm_f32: &[f32], opts: TranscribeOptions<'_>) -> Result<Transcript> {
        let sidecar = ready_sidecar(&self.sidecar).await?;
        let mut client = sidecar.client().await?;
        let response = client
            .transcribe_bytes(stt::TranscribeBytesRequest {
                pcm_f32: pcm_to_le_bytes(pcm_f32),
                language: opts.language.to_string(),
                initial_prompt: opts.initial_prompt.to_string(),
            })
            .await
            .context("STT sidecar TranscribeBytes RPC")?
            .into_inner();
        Ok(Transcript {
            text: response.text,
            language: response.language,
            confidence: response.confidence,
            inference_ms: response.inference_ms,
            segments: response
                .segments
                .into_iter()
                .map(|s| Segment {
                    text: s.text,
                    start: s.start,
                    end: s.end,
                })
                .collect(),
        })
    }

    fn supports_hebrew(&self) -> Confidence {
        // ivrit-ai/whisper-large-v3-turbo-ct2 is fine-tuned on Hebrew.
        Confidence::Excellent
    }

    fn is_local(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_packs_to_little_endian_bytes() {
        let bytes = pcm_to_le_bytes(&[0.0, 1.0, -1.0]);
        assert_eq!(bytes.len(), 12);
        assert_eq!(&bytes[0..4], &0.0f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &1.0f32.to_le_bytes());
    }

    #[test]
    fn faster_whisper_provider_is_local_and_hebrew_capable() {
        let provider = FasterWhisperProvider::new();
        assert!(provider.is_local());
        assert_eq!(provider.supports_hebrew(), Confidence::Excellent);
    }

    #[test]
    fn transcript_carries_hebrew_text() {
        let transcript = Transcript {
            text: "שלום עולם".to_string(),
            language: "he".to_string(),
            confidence: 0.97,
            inference_ms: 240,
            segments: vec![Segment {
                text: "שלום עולם".to_string(),
                start: 0.0,
                end: 1.2,
            }],
        };
        assert_eq!(transcript.text, "שלום עולם");
        assert_eq!(transcript.language, "he");
        assert_eq!(transcript.segments[0].end, 1.2);
    }

    #[test]
    fn transcribe_options_default_to_autodetect_no_prompt() {
        let opts = TranscribeOptions::default();
        assert_eq!(opts.language, "");
        assert_eq!(opts.initial_prompt, "");
        // The `language` constructor sets the hint and leaves the prompt empty —
        // the shape every non-streaming caller uses.
        let he = TranscribeOptions::language("he");
        assert_eq!(he.language, "he");
        assert_eq!(he.initial_prompt, "");
    }
}
