//! End-to-end: transcribe audio through the faster-whisper provider.
//!
//! Marked `#[ignore]` — it needs the Python STT sidecar environment and the
//! downloaded STT model. Run it explicitly:
//!
//! ```text
//! cargo test -p lashon-core --test transcribe -- --ignored
//! ```

use lashon_core::stt::{FasterWhisperProvider, SttProvider, TranscribeOptions};

#[tokio::test]
#[ignore = "requires the Python STT sidecar environment and the STT model"]
async fn transcribes_pcm_without_error() {
    let provider = FasterWhisperProvider::new();
    // Three seconds of silence — proves the PCM → gRPC → transcript path runs
    // end to end. Transcription accuracy is covered by the WER benchmark.
    let pcm = vec![0.0f32; 16_000 * 3];
    let transcript = provider
        .transcribe(&pcm, TranscribeOptions::language("he"))
        .await
        .expect("transcription should succeed");
    assert_eq!(transcript.language, "he");
}

#[tokio::test]
#[ignore = "requires the Python STT sidecar environment and the STT model"]
async fn transcribe_reports_segment_timings() {
    let provider = FasterWhisperProvider::new();
    // A short speech-shaped tone over silence so the decoder emits at least one
    // segment — exercises the segment-timing path the streaming window anchor
    // relies on (docs/adr/0037). The values aren't asserted, only the shape:
    // every reported segment must carry a non-negative, ordered [start, end].
    let mut pcm = vec![0.0f32; 16_000];
    for (i, s) in pcm.iter_mut().enumerate() {
        *s = 0.1 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin();
    }
    let transcript = provider
        .transcribe(&pcm, TranscribeOptions::language("he"))
        .await
        .expect("transcription should succeed");
    for segment in &transcript.segments {
        assert!(segment.start >= 0.0, "segment start is non-negative");
        assert!(segment.end >= segment.start, "segment end follows start");
    }
}
