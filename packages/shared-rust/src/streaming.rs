//! Pure orchestration for live streaming dictation.
//!
//! The dictation worker fakes a live transcript by re-decoding the uncommitted
//! tail of the audio buffer roughly twice a second and folding each hypothesis
//! through [`LocalAgreement`](crate::local_agreement::LocalAgreement). Three
//! small decisions in that loop are pure policy, so they live here — unit-tested,
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
//! - [`WindowAnchor`] — *how much* audio to re-decode. It tracks how far the
//!   committed prefix reaches into the buffer and re-decodes only the audio past
//!   it, so the cost stays bounded by the uncommitted tail however long the take
//!   runs — lifting the old 30 s ceiling the whole-buffer re-decode forced
//!   (`docs/adr/0035`, `docs/adr/0037`).
//!
//! The worker that drives these lives in
//! `apps/desktop/src-tauri/src/dictation.rs`; the benchmark that set the cadence
//! is `scripts/stream-test.py` (see `docs/adr/0035`).

use std::time::Duration;

use crate::stt::Segment;

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

/// Tracks the streaming re-decode's *window* so each re-decode covers only the
/// audio whose transcript has not yet committed — never the whole growing buffer.
///
/// faster-whisper is not a streaming model: the worker fakes live partials by
/// re-decoding the buffer and folding each hypothesis through
/// [`LocalAgreement`](crate::local_agreement::LocalAgreement). Re-decoding the
/// *entire* buffer every tick costs more as a take grows and, past Whisper's
/// 30 s mel window, stops being constant-time — which is why the original design
/// capped a take at 30 s (`docs/adr/0035`).
///
/// `WindowAnchor` lifts that cap. It remembers how far the committed prefix
/// reaches into the audio ([`offset`](Self::offset)) and the committed words
/// before it. Each re-decode runs only on `samples_since(offset)`, primed with
/// the committed tail as Whisper context ([`prompt`](Self::prompt)); the worker
/// reassembles the full hypothesis as [`global`](Self::global) (`prefix` +
/// window text) before folding it through the same committer, so the committer's
/// view still spans the whole take. As whole leading segments of the window
/// commit, [`advance`](Self::advance) moves the anchor past them — so the window
/// tracks the uncommitted tail and re-decode cost stays bounded by it, however
/// long the take runs (`docs/adr/0037`).
///
/// It only ever advances over *whole* committed segments, which guarantees it
/// never drops audio whose text is still provisional. Streaming is preview-only:
/// the authoritative transcript is the final full-buffer decode, so a rare
/// imperfection at a window seam is cosmetic and self-corrects.
///
/// Hebrew/RTL-safe by construction: it only splits on and rejoins whitespace and
/// compares whole tokens, never reordering or rewriting them.
#[derive(Debug, Clone)]
pub struct WindowAnchor {
    /// Window start: how many leading buffer samples the re-decode skips.
    offset: usize,
    /// Committed words for the audio before `offset` — the immutable head of the
    /// reassembled hypothesis and the source of the decoder-context prompt.
    prefix: Vec<String>,
    /// Samples per second, to map a segment's end time to a sample offset.
    rate: f32,
}

impl WindowAnchor {
    /// A fresh anchor at the start of the buffer. `rate` is the audio sample
    /// rate (16 kHz for the STT pipeline).
    pub fn new(rate: u32) -> Self {
        Self {
            offset: 0,
            prefix: Vec::new(),
            rate: rate as f32,
        }
    }

    /// Ready the anchor for a new take: window back to the buffer start, no
    /// committed prefix.
    pub fn reset(&mut self) {
        self.offset = 0;
        self.prefix.clear();
    }

    /// The sample the re-decode window starts at — pass to `samples_since`.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The decoder-context prompt for the next windowed decode: up to `max_words`
    /// of the committed tail. Empty while the window is still at the buffer start
    /// (no audio precedes it, so there is no context to restore).
    pub fn prompt(&self, max_words: usize) -> String {
        let start = self.prefix.len().saturating_sub(max_words);
        self.prefix[start..].join(" ")
    }

    /// Reassemble the full-utterance hypothesis from this window's decoded text:
    /// the committed prefix, then the window text. This is what the committer
    /// sees, so its view spans the whole take though the decode saw only the
    /// window.
    pub fn global(&self, window_text: &str) -> String {
        let window_text = window_text.trim();
        if self.prefix.is_empty() {
            return window_text.to_string();
        }
        let head = self.prefix.join(" ");
        if window_text.is_empty() {
            head
        } else {
            format!("{head} {window_text}")
        }
    }

    /// Advance the window past every whole leading segment whose words have now
    /// committed. `committed_global` is the committer's committed text (which
    /// always begins with the anchor's `prefix`); `segments` are the window
    /// decode's segments, with times relative to the current [`offset`](Self::offset).
    ///
    /// A segment advances the anchor only when *all* its words fall inside the
    /// committed region — a half-committed segment keeps its audio in the window,
    /// so nothing still provisional is ever dropped. A text-less segment (e.g. a
    /// span sanitised down to nothing) never drives an advance on its own; its
    /// audio stays in the window until a later, committed segment carries the
    /// anchor past it.
    pub fn advance(&mut self, committed_global: &str, segments: &[Segment]) {
        let committed = tokenize(committed_global);
        // The committer only ever extends a prefix it shares with us. If it
        // hasn't committed past `prefix`, or somehow doesn't start with it,
        // there is nothing whole to absorb.
        if committed.len() <= self.prefix.len() || !starts_with(&committed, &self.prefix) {
            return;
        }
        let tail = &committed[self.prefix.len()..];

        let mut consumed = 0usize; // tail words covered by whole committed segments
        let mut end_secs = 0.0f32; // end time of the last such segment
        let mut committed_any = false;
        for segment in segments {
            let words = tokenize(&segment.text);
            if words.is_empty() {
                // Carries no committed words; leave its audio in the window
                // rather than let it alone move the anchor.
                continue;
            }
            let next = consumed + words.len();
            if next <= tail.len() && tail[consumed..next] == words[..] {
                consumed = next;
                end_secs = segment.end;
                committed_any = true;
            } else {
                // First segment not yet fully committed — it and everything
                // after it stay in the window.
                break;
            }
        }

        if committed_any {
            self.offset += (end_secs * self.rate).round() as usize;
            self.prefix.extend_from_slice(&tail[..consumed]);
        }
    }
}

/// Split on Unicode whitespace into words — the same tokenisation
/// [`LocalAgreement`](crate::local_agreement::LocalAgreement) uses, so the
/// anchor's segment words line up with the committer's committed words.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// Whether `seq` begins with every word of `prefix`, in order.
fn starts_with(seq: &[String], prefix: &[String]) -> bool {
    seq.len() >= prefix.len() && seq[..prefix.len()] == prefix[..]
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

    // --- WindowAnchor -----------------------------------------------------

    // 16 kHz: 1 s = 16 000 samples, 0.5 s = 8 000 — the worker's real rate.
    const RATE: u32 = 16_000;

    fn seg(text: &str, start: f32, end: f32) -> Segment {
        Segment {
            text: text.to_string(),
            start,
            end,
        }
    }

    /// A fresh anchor sits at the buffer start: no skipped audio, no prompt, and
    /// the global hypothesis is just the window text.
    #[test]
    fn fresh_anchor_is_at_the_buffer_start() {
        let anchor = WindowAnchor::new(RATE);
        assert_eq!(anchor.offset(), 0);
        assert_eq!(anchor.prompt(10), "");
        assert_eq!(anchor.global("שלום עולם"), "שלום עולם");
    }

    /// A whole committed segment moves the anchor to that segment's end and folds
    /// its words into the prefix (prompt + global), so the next window starts past
    /// it. Hebrew is the common case.
    #[test]
    fn advances_over_a_whole_committed_segment() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance("שלום עולם", &[seg("שלום עולם", 0.0, 1.0)]);
        assert_eq!(anchor.offset(), 16_000); // 1.0 s
        assert_eq!(anchor.prompt(10), "שלום עולם");
        assert_eq!(anchor.global("טוב"), "שלום עולם טוב");
    }

    /// A segment only half-committed by LocalAgreement keeps its audio in the
    /// window — the anchor must not advance into provisional text.
    #[test]
    fn does_not_advance_a_half_committed_segment() {
        let mut anchor = WindowAnchor::new(RATE);
        // The decoder put three words in one segment; only two have committed.
        anchor.advance("שלום עולם", &[seg("שלום עולם טוב", 0.0, 1.5)]);
        assert_eq!(anchor.offset(), 0);
        assert_eq!(anchor.prompt(10), "");
    }

    /// With several segments, the anchor absorbs the committed leading run and
    /// stops at the first not-yet-committed one, leaving its audio in the window.
    #[test]
    fn stops_at_the_first_uncommitted_segment() {
        let mut anchor = WindowAnchor::new(RATE);
        let segments = [
            seg("שלום", 0.0, 0.5),
            seg("עולם", 0.5, 1.0),
            seg("טוב ונעים", 1.0, 2.0),
        ];
        // First two segments committed; the third has not.
        anchor.advance("שלום עולם", &segments);
        assert_eq!(anchor.offset(), 16_000); // through "עולם" at 1.0 s
        assert_eq!(anchor.prompt(10), "שלום עולם");
        assert_eq!(anchor.global("טוב"), "שלום עולם טוב");
    }

    /// Advances accumulate across calls, each set of segment times being relative
    /// to the *current* window start — so the anchor walks the buffer take-long.
    #[test]
    fn advances_accumulate_across_calls() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance("אחת", &[seg("אחת", 0.0, 1.0), seg("שתיים שלוש", 1.0, 2.0)]);
        assert_eq!(anchor.offset(), 16_000);
        assert_eq!(anchor.prompt(10), "אחת");

        // Next window starts at 16_000; its segment times are relative to that.
        anchor.advance(
            "אחת שתיים שלוש",
            &[seg("שתיים שלוש", 0.0, 1.0), seg("ארבע", 1.0, 1.5)],
        );
        assert_eq!(anchor.offset(), 32_000); // +1.0 s of the new window
        assert_eq!(anchor.prompt(10), "אחת שתיים שלוש");
        assert_eq!(anchor.global("ארבע"), "אחת שתיים שלוש ארבע");
    }

    /// The prompt is capped to the most recent committed words — Whisper's
    /// initial_prompt has a bounded context budget.
    #[test]
    fn prompt_caps_to_the_recent_committed_tail() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance(
            "one two three four five",
            &[
                seg("one", 0.0, 0.5),
                seg("two", 0.5, 1.0),
                seg("three", 1.0, 1.5),
                seg("four", 1.5, 2.0),
                seg("five", 2.0, 2.5),
            ],
        );
        assert_eq!(anchor.prompt(2), "four five");
        assert_eq!(anchor.prompt(0), "");
    }

    /// Nothing newly committed (or no segments) leaves the anchor put.
    #[test]
    fn no_new_commit_does_not_advance() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance("", &[seg("שלום", 0.0, 1.0)]); // committed empty
        assert_eq!(anchor.offset(), 0);
        anchor.advance("שלום", &[]); // committed, but no segments to map
        assert_eq!(anchor.offset(), 0);
    }

    /// A text-less segment never drives an advance on its own: between a
    /// committed and an uncommitted segment it must not carry the anchor past the
    /// committed one's end.
    #[test]
    fn a_textless_segment_does_not_overadvance() {
        let mut anchor = WindowAnchor::new(RATE);
        let segments = [
            seg("שלום", 0.0, 0.5),
            seg("", 0.5, 0.7), // sanitised to nothing
            seg("עולם", 0.7, 1.2),
        ];
        // Only the first segment committed.
        anchor.advance("שלום", &segments);
        assert_eq!(anchor.offset(), 8_000); // through "שלום" at 0.5 s, not 0.7
    }

    /// Mixed Hebrew/English (code-switching) commits and advances like any other
    /// token run (.claude/rules/hebrew.md).
    #[test]
    fn advances_over_mixed_script_segments() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance(
            "תפתח את chrome",
            &[seg("תפתח את", 0.0, 0.8), seg("chrome", 0.8, 1.4)],
        );
        // Both segments committed; the anchor sits at the last one's end (1.4 s).
        assert_eq!(anchor.offset(), (1.4 * RATE as f32).round() as usize);
        assert_eq!(anchor.prompt(10), "תפתח את chrome");
    }

    /// reset readies a fresh take: back to the buffer start, prefix cleared.
    #[test]
    fn reset_returns_to_the_buffer_start() {
        let mut anchor = WindowAnchor::new(RATE);
        anchor.advance("שלום", &[seg("שלום", 0.0, 1.0)]);
        assert_ne!(anchor.offset(), 0);
        anchor.reset();
        assert_eq!(anchor.offset(), 0);
        assert_eq!(anchor.prompt(10), "");
        assert_eq!(anchor.global("חדש"), "חדש");
    }
}
