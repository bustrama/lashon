//! Voice-activity detection and end-of-utterance detection.
//!
//! `SileroVad` runs the Silero VAD v5 ONNX model, scoring each 512-sample
//! (32 ms) frame of 16 kHz audio with a speech probability. `Endpointer` is
//! pure logic over that stream of probabilities: it decides when an utterance
//! has ended — the signal that stops a hands-free dictation take.
//!
//! This supersedes the energy-RMS detector of docs/adr/0005; the move to
//! Silero VAD and per-utterance endpointing is recorded in docs/adr/0015.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

/// Samples in one Silero VAD frame at 16 kHz — the model's fixed window.
pub const FRAME_SAMPLES: usize = 512;

/// Wall-clock span of one [`FRAME_SAMPLES`]-wide frame at 16 kHz: 32 ms exactly
/// (512 / 16000 s).
pub const FRAME_DURATION: Duration = Duration::from_millis(32);

/// Root-mean-square amplitude of a PCM chunk. `0.0` for an empty slice.
///
/// This is a loudness measure for the tongue's waveform meter — not a speech
/// classifier. Voice activity is [`SileroVad`]'s job.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (sum_squares / samples.len() as f64).sqrt() as f32
}

/// Samples of the previous window Silero v5 carries forward as context.
const CONTEXT_SAMPLES: usize = 64;

/// Width of the model's `input` tensor: the 64-sample context followed by one
/// [`FRAME_SAMPLES`]-wide frame.
const INPUT_SAMPLES: usize = CONTEXT_SAMPLES + FRAME_SAMPLES;

/// Length of the recurrent state tensor — shape `[2, 1, 128]`.
const STATE_LEN: usize = 2 * 128;

/// The id of the Silero VAD entry in `models/manifests/m6-audio.json`.
pub const SILERO_MODEL_ID: &str = "silero-vad-v5";

/// Silero VAD v5 — a speech probability for each 32 ms frame.
///
/// The v5 ONNX model's `input` tensor is 576 wide: a 64-sample context from
/// the previous window prepended to the current 512-sample frame. The model
/// also threads a recurrent `state`. Both are kept inside this struct, so a
/// caller just feeds successive [`FRAME_SAMPLES`]-wide frames and reads back a
/// probability. Build one per process — loading the graph is not free — and
/// [`reset`](Self::reset) it between takes.
pub struct SileroVad {
    session: Session,
    /// The last `CONTEXT_SAMPLES` samples of the previous window.
    context: [f32; CONTEXT_SAMPLES],
    /// The recurrent LSTM state, threaded output-to-input across calls.
    state: Vec<f32>,
}

/// Reduce an `ort` error to its message. `ort`'s errors carry a non-`Send`
/// recovery payload — a session builder, raw ONNX handles — so they cannot be
/// lifted into `anyhow::Error` by `?` directly.
fn ort_err<R>(err: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("ONNX Runtime: {err}")
}

impl SileroVad {
    /// Load the Silero VAD model named by [`SILERO_MODEL_ID`], verifying it
    /// against the manifest first.
    pub fn load() -> Result<Self> {
        let dir = crate::model::verified_dir(SILERO_MODEL_ID)?;
        Self::load_from(&dir.join("silero_vad.onnx"))
    }

    /// Load the model from an explicit `.onnx` path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let session = Session::builder()
            .map_err(ort_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err)?
            .with_intra_threads(1)
            .map_err(ort_err)?
            .commit_from_file(path)
            .map_err(ort_err)
            .with_context(|| format!("loading Silero VAD from {}", path.display()))?;
        Ok(Self {
            session,
            context: [0.0; CONTEXT_SAMPLES],
            state: vec![0.0; STATE_LEN],
        })
    }

    /// Clear the context and recurrent state — call between dictation takes so
    /// one take's tail cannot bleed into the next.
    pub fn reset(&mut self) {
        self.context = [0.0; CONTEXT_SAMPLES];
        self.state.iter_mut().for_each(|s| *s = 0.0);
    }

    /// Score one 512-sample (32 ms) frame: its probability of carrying speech,
    /// in `[0, 1]`.
    pub fn observe(&mut self, frame: &[f32; FRAME_SAMPLES]) -> Result<f32> {
        // The model input is the carried context followed by this frame.
        let mut input = Vec::with_capacity(INPUT_SAMPLES);
        input.extend_from_slice(&self.context);
        input.extend_from_slice(frame);
        // This window's last CONTEXT_SAMPLES become the next call's context.
        let mut next_context = [0.0_f32; CONTEXT_SAMPLES];
        next_context.copy_from_slice(&input[FRAME_SAMPLES..INPUT_SAMPLES]);

        let input_t = Tensor::from_array(([1_i64, INPUT_SAMPLES as i64], input.into_boxed_slice()))
            .map_err(ort_err)?;
        let state_t = Tensor::from_array(([2_i64, 1, 128], self.state.clone().into_boxed_slice()))
            .map_err(ort_err)?;
        let sr_t =
            Tensor::from_array(([1_i64], vec![16_000_i64].into_boxed_slice())).map_err(ort_err)?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input" => input_t,
                "state" => state_t,
                "sr" => sr_t,
            ])
            .map_err(ort_err)?;

        // Thread the recurrent state forward for the next call.
        let (_, next_state) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        self.state.clear();
        self.state.extend_from_slice(next_state);
        self.context = next_context;

        let (_, probability) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        probability
            .first()
            .copied()
            .context("Silero VAD returned an empty output tensor")
    }
}

/// Tuning for [`Endpointer`] — the end-of-utterance thresholds of
/// docs/roadmap.md §1.1.
#[derive(Debug, Clone, Copy)]
pub struct EndpointConfig {
    /// A frame is *speech* when its Silero probability is at least this.
    pub speech_threshold: f32,
    /// A frame carries *energy* — sub-speech sound, the "mid-word energy" of a
    /// thinking pause — when its probability is at least this. Always below
    /// `speech_threshold`.
    pub energy_threshold: f32,
    /// End the utterance after this much uninterrupted true silence — a stretch
    /// in which no frame even reaches `energy_threshold`.
    pub silence: Duration,
    /// The cap: end this long after the last real speech frame regardless of
    /// faint energy still poking through, so a long mumbled tail still stops.
    pub hold: Duration,
    /// A take whose total speech is shorter than this is discarded as a stray
    /// noise rather than transcribed.
    pub min_speech: Duration,
    /// End a take in which no speech is ever heard after this long — dictation
    /// was triggered but the user never spoke.
    pub no_speech_timeout: Duration,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            // 0.4/0.25 (rather than the textbook 0.5/0.35) is permissive enough
            // to catch quieter speech and non-Hebrew accents whose Silero scores
            // run lower than confident Hebrew talking.
            speech_threshold: 0.4,
            energy_threshold: 0.25,
            silence: Duration::from_millis(500),
            hold: Duration::from_millis(1500),
            min_speech: Duration::from_millis(250),
            no_speech_timeout: Duration::from_secs(6),
        }
    }
}

/// Whether a dictation take should keep capturing or stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointSignal {
    /// The utterance is still going — keep capturing.
    Continue,
    /// The utterance has ended — stop the take.
    Ended,
}

/// End-of-utterance detector.
///
/// Fed one Silero speech probability per 32 ms frame, it tracks trailing
/// silence and reports when the utterance is over. The rule (docs/roadmap.md
/// §1.1): end on 500 ms of clean silence, or — when faint mid-word energy keeps
/// the silence from being clean — 1500 ms after the last real speech. It is
/// pure and deterministic, so the call site in the dictation worker stays a
/// thin shell and the logic is exercised entirely by unit tests.
#[derive(Debug, Clone)]
pub struct Endpointer {
    config: EndpointConfig,
    /// Frames observed so far. `frames * FRAME_DURATION` is the take's length.
    frames: u32,
    /// Frames whose probability reached `speech_threshold`.
    speech_frames: u32,
    /// Index of the most recent speech frame, once any speech has been heard.
    last_speech: Option<u32>,
    /// Index of the most recent frame reaching `energy_threshold`.
    last_energy: Option<u32>,
    /// Latched once the take has ended, so the verdict never flips back.
    ended: bool,
}

impl Endpointer {
    /// A detector tuned by `config`. One is built per dictation take.
    pub fn new(config: EndpointConfig) -> Self {
        Self {
            config,
            frames: 0,
            speech_frames: 0,
            last_speech: None,
            last_energy: None,
            ended: false,
        }
    }

    /// Observe the next frame's speech probability and report whether the take
    /// should continue or stop. Sticky: once it returns `Ended`, it always
    /// does.
    pub fn observe(&mut self, speech_prob: f32) -> EndpointSignal {
        if self.ended {
            return EndpointSignal::Ended;
        }
        let idx = self.frames;
        self.frames += 1;

        if speech_prob >= self.config.energy_threshold {
            self.last_energy = Some(idx);
        }
        if speech_prob >= self.config.speech_threshold {
            self.speech_frames += 1;
            self.last_speech = Some(idx);
        }

        let signal = self.decide(idx);
        self.ended = signal == EndpointSignal::Ended;
        signal
    }

    /// Whether enough speech was heard for the take to be worth transcribing.
    pub fn heard_speech(&self) -> bool {
        self.speech_duration() >= self.config.min_speech
    }

    /// Total audio classified as speech.
    pub fn speech_duration(&self) -> Duration {
        FRAME_DURATION * self.speech_frames
    }

    /// When the last speech frame ended, measured from the take's start — the
    /// point past which only trailing silence remains, so the worker can trim
    /// the tail before transcription. `None` if no speech was ever heard.
    pub fn speech_end(&self) -> Option<Duration> {
        self.last_speech.map(|idx| FRAME_DURATION * (idx + 1))
    }

    fn decide(&self, idx: u32) -> EndpointSignal {
        match self.last_speech {
            // Nothing said yet — end only once the no-speech timeout elapses.
            None => {
                if FRAME_DURATION * self.frames >= self.config.no_speech_timeout {
                    EndpointSignal::Ended
                } else {
                    EndpointSignal::Continue
                }
            }
            // `last_energy` is always at or past `last_speech` — speech clears
            // the lower energy bar too — so `quiet` never exceeds `trailing`.
            Some(last_speech) => {
                let trailing = FRAME_DURATION * (idx - last_speech);
                let last_energy = self.last_energy.unwrap_or(last_speech);
                let quiet = FRAME_DURATION * (idx - last_energy);
                if quiet >= self.config.silence || trailing >= self.config.hold {
                    EndpointSignal::Ended
                } else {
                    EndpointSignal::Continue
                }
            }
        }
    }
}

impl Default for Endpointer {
    fn default() -> Self {
        Self::new(EndpointConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 256]), 0.0);
    }

    #[test]
    fn rms_of_a_full_scale_square_wave_is_one() {
        assert!((rms(&[1.0, -1.0, 1.0, -1.0]) - 1.0).abs() < 1e-6);
    }

    /// Feed `n` frames at probability `prob`; return the last signal.
    fn feed(ep: &mut Endpointer, prob: f32, n: u32) -> EndpointSignal {
        let mut last = EndpointSignal::Continue;
        for _ in 0..n {
            last = ep.observe(prob);
        }
        last
    }

    #[test]
    fn continues_through_steady_speech() {
        let mut ep = Endpointer::default();
        // 6.4 s of confident speech — well past every timeout.
        assert_eq!(feed(&mut ep, 0.9, 200), EndpointSignal::Continue);
        assert!(ep.heard_speech());
    }

    #[test]
    fn ends_after_500ms_of_clean_silence() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.9, 30); // ~1 s of speech
                                // 500 ms / 32 ms ≈ 16 frames of true silence end the take.
        assert_eq!(feed(&mut ep, 0.0, 15), EndpointSignal::Continue);
        assert_eq!(ep.observe(0.0), EndpointSignal::Ended);
    }

    #[test]
    fn faint_energy_holds_past_the_500ms_silence_window() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.9, 30); // speech
                                // 0.3 sits above energy_threshold (0.25) but below speech_threshold
                                // (0.4): "mid-word energy". Poking it through every other frame keeps
                                // the silence from ever being clean, so the 500 ms rule never fires...
        let mut signal = EndpointSignal::Continue;
        let mut frames_after = 0u32;
        while signal == EndpointSignal::Continue && frames_after < 100 {
            let prob = if frames_after.is_multiple_of(2) {
                0.0
            } else {
                0.3
            };
            signal = ep.observe(prob);
            frames_after += 1;
        }
        assert_eq!(signal, EndpointSignal::Ended);
        // ...the take ends on the 1500 ms hold cap instead — well past the
        // ~16-frame (500 ms) clean-silence point, near the ~47-frame cap.
        assert!(
            frames_after > 16,
            "ended too early, at {frames_after} frames"
        );
        assert!(
            frames_after <= 48,
            "ended too late, at {frames_after} frames"
        );
    }

    #[test]
    fn ends_when_triggered_but_never_spoken() {
        let mut ep = Endpointer::default();
        // no_speech_timeout is 6 s ≈ 188 frames; 0.05 never reaches any bar.
        assert_eq!(feed(&mut ep, 0.05, 180), EndpointSignal::Continue);
        assert_eq!(feed(&mut ep, 0.05, 8), EndpointSignal::Ended);
        assert!(!ep.heard_speech());
    }

    #[test]
    fn a_stray_blip_is_not_enough_speech() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.9, 5); // ~160 ms — under min_speech (250 ms)
        feed(&mut ep, 0.0, 20);
        assert!(!ep.heard_speech());
    }

    #[test]
    fn speech_end_marks_the_last_speech_frame() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.9, 10);
        feed(&mut ep, 0.0, 5);
        // 10 speech frames — the last is index 9 — so speech ends at 10 frames.
        assert_eq!(ep.speech_end(), Some(FRAME_DURATION * 10));
    }

    #[test]
    fn no_speech_leaves_speech_end_unset() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.05, 30);
        assert_eq!(ep.speech_end(), None);
    }

    #[test]
    fn ended_is_sticky_even_against_later_speech() {
        let mut ep = Endpointer::default();
        feed(&mut ep, 0.9, 30);
        feed(&mut ep, 0.0, 16); // ends the take
        assert_eq!(ep.observe(0.0), EndpointSignal::Ended);
        // A frame of speech arriving after the verdict cannot revive the take.
        assert_eq!(ep.observe(0.9), EndpointSignal::Ended);
    }

    #[test]
    #[ignore = "needs the Silero VAD model on disk; run with --ignored"]
    fn silero_vad_loads_and_scores_silence_low() {
        let mut vad = SileroVad::load().expect("the Silero VAD model loads");
        let silence = [0.0_f32; FRAME_SAMPLES];
        let p1 = vad.observe(&silence).expect("inference runs");
        assert!((0.0..=1.0).contains(&p1), "probability {p1} out of range");
        assert!(p1 < 0.5, "silence scored as speech: {p1}");
        // The recurrent state threads — a second frame still scores in range.
        let p2 = vad.observe(&silence).expect("inference runs again");
        assert!((0.0..=1.0).contains(&p2), "probability {p2} out of range");
        // reset() leaves the detector usable.
        vad.reset();
        let p3 = vad.observe(&silence).expect("inference runs after reset");
        assert!((0.0..=1.0).contains(&p3), "probability {p3} out of range");
    }
}
