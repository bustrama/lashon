//! Pure orchestration for live streaming dictation.
//!
//! The dictation worker fakes a live transcript by re-decoding the growing
//! audio buffer roughly twice a second and folding each hypothesis through
//! [`LocalAgreement`](crate::local_agreement::LocalAgreement). Two small
//! decisions in that loop are pure policy, so they live here — unit-tested,
//! free of audio, gRPC, and Tauri:
//!
//! - [`DecodeScheduler`] — *when* to fire the next re-decode. It enforces the
//!   min-sample gate (don't decode a fraction of a second of audio), the
//!   re-decode cadence (one decode per hop of new audio), and single-flight
//!   (never two decodes at once). It also self-disables on a machine that
//!   cannot sustain the cadence — measured from real decode latency, not a
//!   hardcoded device check — so the same code path degrades to today's
//!   one-shot-on-stop behaviour on a slow CPU instead of stalling.
//! - [`LanguageLatch`] — *which* language to ask for. The first decode autodetects
//!   (companion-model language ID, `docs/adr/0009`); the detected code is latched
//!   and forced on every later decode and the final, so the detector never reruns
//!   mid-utterance and the language can't flip on a noisy chunk.
//!
//! The worker that drives these lives in
//! `apps/desktop/src-tauri/src/dictation.rs`; the benchmark that set the cadence
//! is `scripts/stream-test.py` (see `docs/adr/0035`).

use std::time::Duration;

/// Decides when the streaming worker should fire its next re-decode.
///
/// The worker calls [`should_decode`](Self::should_decode) every capture tick
/// with the current buffer length and whether a decode is already running. When
/// it does fire one, it calls [`mark_decoded`](Self::mark_decoded) with the
/// snapshot length, and feeds each finished decode's latency back through
/// [`observe_latency`](Self::observe_latency).
#[derive(Debug, Clone)]
pub struct DecodeScheduler {
    /// Don't decode until at least this many samples are buffered. Sub-second
    /// decodes are mostly noise and waste a decode slot.
    min_samples: usize,
    /// Fire a re-decode once this many new samples have arrived since the last.
    hop_samples: usize,
    /// A decode slower than this means the machine can't sustain live partials;
    /// streaming self-disables and the worker keeps only the final decode.
    max_latency: Duration,
    /// Buffer length at the last fired decode. `None` until the first fires.
    last_decode_at: Option<usize>,
    /// Latched off once a decode proves too slow — stays off for the session.
    disabled: bool,
}

impl DecodeScheduler {
    /// Build a scheduler. `min_samples` is the gate before the first decode,
    /// `hop_samples` the new-audio cadence between decodes, and `max_latency`
    /// the per-decode budget past which streaming self-disables.
    pub fn new(min_samples: usize, hop_samples: usize, max_latency: Duration) -> Self {
        Self {
            min_samples,
            hop_samples,
            max_latency,
            last_decode_at: None,
            disabled: false,
        }
    }

    /// Whether to fire a re-decode now, given the buffered sample count and
    /// whether a decode is already in flight.
    ///
    /// Single-flight (`in_flight`) wins over everything: only one decode runs at
    /// a time, so a slow decode simply lowers the partial rate rather than
    /// piling up work behind the capture thread.
    pub fn should_decode(&self, buffered: usize, in_flight: bool) -> bool {
        if self.disabled || in_flight || buffered < self.min_samples {
            return false;
        }
        match self.last_decode_at {
            None => true, // first decode, the moment the gate opens
            Some(prev) => buffered.saturating_sub(prev) >= self.hop_samples,
        }
    }

    /// Record that a decode was just fired against a buffer of this length.
    pub fn mark_decoded(&mut self, buffered: usize) {
        self.last_decode_at = Some(buffered);
    }

    /// Feed back a finished decode's wall-clock latency. A decode that overran
    /// the budget shows this machine can't keep up; streaming disables for the
    /// rest of the session (the next [`reset`](Self::reset) keeps it off).
    pub fn observe_latency(&mut self, latency: Duration) {
        if latency > self.max_latency {
            self.disabled = true;
        }
    }

    /// True once streaming has self-disabled on this machine.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Ready the scheduler for a new take. Clears the per-take cadence cursor
    /// but deliberately preserves the session-wide disable: a machine that was
    /// too slow last take is still too slow this take.
    pub fn reset(&mut self) {
        self.last_decode_at = None;
    }
}

/// Latches the language detected on the first decode and forces it thereafter.
///
/// The first re-decode runs with the empty string — the sidecar autodetects via
/// the companion model (`docs/adr/0009`). The detected code is then latched and
/// returned for every later decode and the final, so detection runs once per
/// take and a noisy chunk can't flip the language mid-utterance.
#[derive(Debug, Clone, Default)]
pub struct LanguageLatch {
    latched: Option<String>,
}

impl LanguageLatch {
    /// A fresh, unlatched latch.
    pub fn new() -> Self {
        Self::default()
    }

    /// The language to request now: the latched code, or `""` (autodetect)
    /// until the first decode reports one.
    pub fn query(&self) -> &str {
        self.latched.as_deref().unwrap_or("")
    }

    /// Latch the language a decode reported. The first non-empty code wins;
    /// later reports are ignored, so the language is stable for the whole take.
    pub fn observe(&mut self, language: &str) {
        if self.latched.is_none() {
            let language = language.trim();
            if !language.is_empty() {
                self.latched = Some(language.to_string());
            }
        }
    }

    /// True once a language has been latched.
    pub fn is_latched(&self) -> bool {
        self.latched.is_some()
    }

    /// Forget the latched language, ready for a new take.
    pub fn reset(&mut self) {
        self.latched = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 16 kHz mono: 1 s = 16 000 samples, 0.5 s = 8 000. The worker's real
    // values; the tests use them so the arithmetic matches production.
    const MIN: usize = 16_000;
    const HOP: usize = 8_000;
    const BUDGET: Duration = Duration::from_millis(2_500);

    fn scheduler() -> DecodeScheduler {
        DecodeScheduler::new(MIN, HOP, BUDGET)
    }

    /// Nothing decodes until the min-sample gate (~1 s) is reached.
    #[test]
    fn holds_until_the_min_sample_gate() {
        let sched = scheduler();
        assert!(!sched.should_decode(0, false));
        assert!(!sched.should_decode(MIN - 1, false));
        assert!(sched.should_decode(MIN, false));
    }

    /// After the first decode, the next waits a full hop of new audio.
    #[test]
    fn paces_by_one_hop_of_new_audio() {
        let mut sched = scheduler();
        assert!(sched.should_decode(MIN, false));
        sched.mark_decoded(MIN);
        // Less than a hop of new audio — not yet.
        assert!(!sched.should_decode(MIN + HOP - 1, false));
        // A full hop of new audio — fire.
        assert!(sched.should_decode(MIN + HOP, false));
    }

    /// Single-flight: never schedule a second decode while one is running, no
    /// matter how much audio has piled up.
    #[test]
    fn single_flight_blocks_while_a_decode_runs() {
        let mut sched = scheduler();
        sched.mark_decoded(MIN);
        assert!(!sched.should_decode(MIN + 10 * HOP, true));
        // Once it finishes, the backlog of audio fires immediately.
        assert!(sched.should_decode(MIN + 10 * HOP, false));
    }

    /// A decode slower than the budget disables streaming for the session, and
    /// a new take does not re-enable it.
    #[test]
    fn self_disables_on_a_decode_that_cant_keep_up() {
        let mut sched = scheduler();
        assert!(!sched.is_disabled());
        // A CPU-class 12 s decode — far over the 2.5 s budget.
        sched.observe_latency(Duration::from_millis(12_000));
        assert!(sched.is_disabled());
        assert!(!sched.should_decode(MIN, false));
        // A new take clears the cadence cursor but not the disable.
        sched.reset();
        assert!(sched.is_disabled());
        assert!(!sched.should_decode(MIN, false));
    }

    /// A GPU-class decode (well under budget, including the one-time ~1 s warm-up
    /// on the first decode of a session) keeps streaming enabled.
    #[test]
    fn fast_decodes_keep_streaming_enabled() {
        let mut sched = scheduler();
        sched.observe_latency(Duration::from_millis(969)); // first-decode warm-up
        sched.observe_latency(Duration::from_millis(140)); // steady state
        assert!(!sched.is_disabled());
        assert!(sched.should_decode(MIN, false));
    }

    /// reset readies a fresh take: the gate applies again from the start.
    #[test]
    fn reset_restarts_the_cadence() {
        let mut sched = scheduler();
        sched.mark_decoded(MIN);
        sched.reset();
        // last_decode_at cleared — the first decode of the new take fires at the gate.
        assert!(sched.should_decode(MIN, false));
    }

    /// The first decode autodetects (empty language); the detected code is then
    /// forced. Hebrew is the common case.
    #[test]
    fn latches_hebrew_after_autodetect() {
        let mut latch = LanguageLatch::new();
        assert_eq!(latch.query(), ""); // first decode autodetects
        assert!(!latch.is_latched());

        latch.observe("he");
        assert_eq!(latch.query(), "he");
        assert!(latch.is_latched());
    }

    /// Once latched, the language never flips — a later chunk that the model
    /// reads as English must not change a Hebrew take's forced language. This is
    /// the code-switching guard (.claude/rules/hebrew.md).
    #[test]
    fn the_first_language_wins_against_a_mixed_chunk() {
        let mut latch = LanguageLatch::new();
        latch.observe("he");
        latch.observe("en"); // a code-switched chunk read as English
        assert_eq!(latch.query(), "he");
    }

    /// An empty or whitespace report does not latch — autodetect stays in
    /// effect until a real language code arrives.
    #[test]
    fn empty_reports_do_not_latch() {
        let mut latch = LanguageLatch::new();
        latch.observe("");
        assert_eq!(latch.query(), "");
        assert!(!latch.is_latched());
        latch.observe("   ");
        assert_eq!(latch.query(), "");

        latch.observe("en");
        assert_eq!(latch.query(), "en");
    }

    /// reset forgets the latched language for the next take.
    #[test]
    fn reset_clears_the_latched_language() {
        let mut latch = LanguageLatch::new();
        latch.observe("he");
        latch.reset();
        assert_eq!(latch.query(), "");
        assert!(!latch.is_latched());
    }
}
