//! Wake-word detection — the openWakeWord ONNX pipeline.
//!
//! Three chained ONNX models (docs/adr/0016): a melspectrogram model and a
//! shared audio-embedding model — both openWakeWord's, Apache-2.0 — followed by
//! a small per-phrase classifier. [`WakeWord`] runs them over a rolling audio
//! buffer and scores how likely the wake phrase was just spoken; [`Trigger`]
//! turns that score stream into a debounced fire.
//!
//! The melspectrogram and embedding models ship via `models/manifests/`. The
//! classifier is an offline-trained artifact (see docs/adr/0016) loaded by
//! path — it is deliberately not in the manifest.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

/// openWakeWord feeds audio to the pipeline in 80 ms chunks at 16 kHz.
pub const CHUNK_SAMPLES: usize = 1_280;

/// Mel bins per frame — the melspectrogram model's output width.
const MEL_BINS: usize = 32;
/// Mel frames in one embedding window.
const EMBED_FRAMES: usize = 76;
/// Mel-frame hop between successive embedding windows.
const EMBED_STRIDE: usize = 8;
/// Audio-embedding dimensionality.
const EMBED_DIM: usize = 96;
/// Embeddings the classifier consumes — openWakeWord's v0.1 window. A custom
/// classifier trained with a different window would need this changed.
const CLASSIFIER_WINDOW: usize = 16;
/// Rolling audio kept for inference — enough for `CLASSIFIER_WINDOW`
/// embeddings with margin (~2.5 s at 16 kHz).
const AUDIO_KEEP: usize = 40_000;

/// The id of the shared openWakeWord models in `models/manifests/m6-audio.json`.
pub const SHARED_MODEL_ID: &str = "openwakeword-shared";

/// Reduce an `ort` error to its message — `ort`'s errors carry a non-`Send`
/// recovery payload, so they cannot be lifted into `anyhow` by `?` directly.
fn ort_err<R>(err: ort::Error<R>) -> anyhow::Error {
    anyhow!("ONNX Runtime: {err}")
}

/// Load an ONNX model into a single-threaded CPU session.
fn load_session(path: &Path) -> Result<Session> {
    Session::builder()
        .map_err(ort_err)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(ort_err)?
        .with_intra_threads(1)
        .map_err(ort_err)?
        .commit_from_file(path)
        .map_err(ort_err)
        .with_context(|| format!("loading the ONNX model at {}", path.display()))
}

/// The name of a model's sole input tensor.
fn first_input_name(session: &Session) -> Result<String> {
    session
        .inputs()
        .first()
        .map(|input| input.name().to_string())
        .ok_or_else(|| anyhow!("the ONNX model exposes no inputs"))
}

/// The openWakeWord pipeline for one wake phrase.
///
/// Build one per process — loading three ONNX graphs is not free — and feed it
/// successive chunks of 16 kHz mono audio with [`observe`](Self::observe).
pub struct WakeWord {
    melspec: Session,
    embedding: Session,
    classifier: Session,
    /// Each model's sole input-tensor name, queried at load — TensorFlow-
    /// converted ONNX graphs do not share a naming convention.
    melspec_input: String,
    embedding_input: String,
    classifier_input: String,
    /// Rolling raw audio, 16 kHz mono, newest at the end.
    audio: Vec<f32>,
}

impl WakeWord {
    /// Load the shared openWakeWord models — verified against the manifest —
    /// and a wake-phrase classifier from `classifier_path`.
    pub fn load(classifier_path: &Path) -> Result<Self> {
        let shared = crate::model::verified_dir(SHARED_MODEL_ID)?;
        let melspec = load_session(&shared.join("melspectrogram.onnx"))?;
        let embedding = load_session(&shared.join("embedding_model.onnx"))?;
        let classifier = load_session(classifier_path)?;
        Ok(Self {
            melspec_input: first_input_name(&melspec)?,
            embedding_input: first_input_name(&embedding)?,
            classifier_input: first_input_name(&classifier)?,
            melspec,
            embedding,
            classifier,
            audio: Vec::with_capacity(AUDIO_KEEP + CHUNK_SAMPLES),
        })
    }

    /// Clear the rolling buffer — call when detection resumes after a pause so
    /// stale audio cannot trigger a phantom hit.
    pub fn reset(&mut self) {
        self.audio.clear();
    }

    /// Feed a chunk of 16 kHz mono audio and score how likely the wake phrase
    /// was just spoken, in `[0, 1]`. Returns `0.0` while the rolling buffer is
    /// still filling.
    pub fn observe(&mut self, chunk: &[f32]) -> Result<f32> {
        self.audio.extend_from_slice(chunk);
        if self.audio.len() > AUDIO_KEEP {
            let excess = self.audio.len() - AUDIO_KEEP;
            self.audio.drain(..excess);
        }

        let mel = self.melspectrogram()?;
        let frames = mel.len() / MEL_BINS;
        // The classifier needs CLASSIFIER_WINDOW embeddings; the last one spans
        // mel frames `(WINDOW-1)*STRIDE .. (WINDOW-1)*STRIDE + EMBED_FRAMES`.
        if frames < EMBED_FRAMES + (CLASSIFIER_WINDOW - 1) * EMBED_STRIDE {
            return Ok(0.0); // still filling
        }
        let embeddings = self.embeddings(&mel, frames)?;
        self.classify(&embeddings)
    }

    /// Run the melspectrogram model over the whole rolling buffer; returns the
    /// mel frames row-major (`frame * MEL_BINS + bin`), openWakeWord-scaled.
    fn melspectrogram(&mut self) -> Result<Vec<f32>> {
        // openWakeWord's melspectrogram expects PCM in int16 range, not [-1, 1].
        let pcm: Vec<f32> = self.audio.iter().map(|s| s * 32_768.0).collect();
        let input = Tensor::from_array(([1_i64, pcm.len() as i64], pcm.into_boxed_slice()))
            .map_err(ort_err)?;
        let outputs = self
            .melspec
            .run(ort::inputs![self.melspec_input.as_str() => input])
            .map_err(ort_err)?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
        // openWakeWord scales the raw mel output before the embedding model.
        Ok(data.iter().map(|m| m / 10.0 + 2.0).collect())
    }

    /// Run the embedding model over every 76-frame mel window, batched into a
    /// single inference; returns the embeddings row-major, oldest window first.
    fn embeddings(&mut self, mel: &[f32], frames: usize) -> Result<Vec<f32>> {
        let starts: Vec<usize> = (0..=frames - EMBED_FRAMES).step_by(EMBED_STRIDE).collect();
        let mut batch = Vec::with_capacity(starts.len() * EMBED_FRAMES * MEL_BINS);
        for &start in &starts {
            let from = start * MEL_BINS;
            let to = (start + EMBED_FRAMES) * MEL_BINS;
            batch.extend_from_slice(&mel[from..to]);
        }
        let input = Tensor::from_array((
            [starts.len() as i64, EMBED_FRAMES as i64, MEL_BINS as i64, 1],
            batch.into_boxed_slice(),
        ))
        .map_err(ort_err)?;
        let outputs = self
            .embedding
            .run(ort::inputs![self.embedding_input.as_str() => input])
            .map_err(ort_err)?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
        Ok(data.to_vec())
    }

    /// Score the most recent `CLASSIFIER_WINDOW` embeddings.
    fn classify(&mut self, embeddings: &[f32]) -> Result<f32> {
        let tail = embeddings.len() - CLASSIFIER_WINDOW * EMBED_DIM;
        let window = embeddings[tail..].to_vec();
        let input = Tensor::from_array((
            [1_i64, CLASSIFIER_WINDOW as i64, EMBED_DIM as i64],
            window.into_boxed_slice(),
        ))
        .map_err(ort_err)?;
        let outputs = self
            .classifier
            .run(ort::inputs![self.classifier_input.as_str() => input])
            .map_err(ort_err)?;
        let (_, score) = outputs[0].try_extract_tensor::<f32>().map_err(ort_err)?;
        score
            .first()
            .copied()
            .context("the wake-word classifier returned an empty output")
    }
}

/// Debounces the wake-word score stream.
///
/// The wake word fires only once the score clears the threshold on **two
/// consecutive** frames (docs/roadmap.md §1.5), which suppresses a one-frame
/// blip. After a fire it re-arms only once a sub-threshold frame has passed,
/// so one sustained utterance triggers exactly once.
#[derive(Debug, Clone, Default)]
pub struct Trigger {
    /// Consecutive frames seen at or above the threshold.
    over: u32,
    /// Whether the current run of over-threshold frames has already fired.
    fired: bool,
}

impl Trigger {
    /// Feed the next score against `threshold`. Returns `true` on the frame the
    /// wake word fires.
    pub fn observe(&mut self, score: f32, threshold: f32) -> bool {
        if score >= threshold {
            self.over += 1;
        } else {
            self.over = 0;
            self.fired = false;
        }
        if self.over >= 2 && !self.fired {
            self.fired = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_frame_over_threshold_does_not_fire() {
        let mut trigger = Trigger::default();
        assert!(!trigger.observe(0.9, 0.5));
    }

    #[test]
    fn two_consecutive_frames_fire() {
        let mut trigger = Trigger::default();
        assert!(!trigger.observe(0.9, 0.5));
        assert!(trigger.observe(0.9, 0.5));
    }

    #[test]
    fn a_sub_threshold_frame_resets_the_run() {
        let mut trigger = Trigger::default();
        trigger.observe(0.9, 0.5); // over (1)
        assert!(!trigger.observe(0.1, 0.5)); // resets
        assert!(!trigger.observe(0.9, 0.5)); // over (1) again — not yet
        assert!(trigger.observe(0.9, 0.5)); // over (2) — fires
    }

    #[test]
    fn one_utterance_fires_exactly_once() {
        let mut trigger = Trigger::default();
        trigger.observe(0.9, 0.5);
        assert!(trigger.observe(0.9, 0.5)); // fires
                                            // A sustained high score must not re-fire without a quiet frame first.
        assert!(!trigger.observe(0.9, 0.5));
        assert!(!trigger.observe(0.9, 0.5));
        // After a quiet frame it re-arms.
        trigger.observe(0.1, 0.5);
        trigger.observe(0.9, 0.5);
        assert!(trigger.observe(0.9, 0.5));
    }

    #[test]
    #[ignore = "needs the openWakeWord models on disk; run with --ignored"]
    fn wake_word_loads_and_scores_silence_low() {
        let classifier = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../models/wake/_devtest/hey_jarvis_v0.1.onnx");
        let mut wake = WakeWord::load(&classifier).expect("the wake-word models load");
        let silence = [0.0_f32; CHUNK_SAMPLES];
        let mut score = 0.0;
        // ~3.2 s of silence — long enough to fill the rolling buffer.
        for _ in 0..40 {
            score = wake.observe(&silence).expect("inference runs");
            assert!((0.0..=1.0).contains(&score), "score {score} out of range");
        }
        assert!(score < 0.5, "silence scored as the wake word: {score}");
    }
}
