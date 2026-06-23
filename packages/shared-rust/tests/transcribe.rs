//! End-to-end: transcribe audio through the faster-whisper provider.
//!
//! Marked `#[ignore]` — it needs the Python STT sidecar environment and the
//! downloaded STT model. Run it explicitly:
//!
//! ```text
//! cargo test -p lashon-core --test transcribe -- --ignored
//! ```

use lashon_core::stt::{FasterWhisperProvider, SttProvider};

#[tokio::test]
#[ignore = "requires the Python STT sidecar environment and the STT model"]
async fn transcribes_pcm_without_error() {
    let provider = FasterWhisperProvider::new();
    // Three seconds of silence — proves the PCM → gRPC → transcript path runs
    // end to end. Transcription accuracy is covered by the WER benchmark.
    let pcm = vec![0.0f32; 16_000 * 3];
    let transcript = provider
        .transcribe(&pcm, "he")
        .await
        .expect("transcription should succeed");
    assert_eq!(transcript.language, "he");
}
