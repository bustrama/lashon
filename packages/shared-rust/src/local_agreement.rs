//! LocalAgreement-2 commit policy for streaming transcription.
//!
//! faster-whisper is not a streaming model. The dictation worker fakes live
//! transcription by re-decoding the audio buffer roughly twice a second and
//! folding each result into the utterance-so-far (see
//! `apps/desktop/src-tauri/src/dictation.rs`; since `docs/adr/0037` the re-decode
//! covers only the uncommitted tail, reassembled with the committed prefix
//! before it reaches this committer). Successive hypotheses revise their tail as
//! the model hears more audio. Shown verbatim, that tail flickers — words appear,
//! change, and vanish — which reads as broken.
//!
//! LocalAgreement-n (the CUNI-KIT IWSLT-2022 policy) removes the flicker: a word
//! is *committed* only once it has appeared, in the same position, in `n`
//! consecutive hypotheses. We use n = 2 — commit the longest common prefix of
//! the two most recent hypotheses. Committed words are final and never change
//! again; only the short provisional tail past them can still move.
//!
//! This committer is deliberately pure: it knows nothing about gRPC, audio, or
//! Whisper. It folds a stream of full-utterance hypotheses into a stable
//! `(committed, provisional)` split, which keeps it unit-testable. It is
//! Hebrew/RTL-safe by construction: it never reorders or rewrites tokens, only
//! splits on and rejoins with whitespace, so right-to-left ordering and
//! combining marks (niqqud) pass through untouched.

/// The current view of a streamed utterance: a stable prefix the user can trust
/// and a provisional tail that may still change as more audio arrives.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Preview {
    /// Words confirmed by two consecutive hypotheses — final, render solid.
    pub committed: String,
    /// The unconfirmed remainder of the latest hypothesis — render muted.
    pub provisional: String,
}

/// Accumulates full-utterance hypotheses under the LocalAgreement-2 policy.
///
/// Feed each re-decode to [`observe`](Self::observe) during a take, then
/// [`finalize`](Self::finalize) once with the closing decode.
#[derive(Debug, Clone, Default)]
pub struct LocalAgreement {
    /// Words committed so far. Grows monotonically; never shrinks or rewrites.
    committed: Vec<String>,
    /// The previous hypothesis, tokenised — one half of the agreement check.
    previous: Vec<String>,
}

impl LocalAgreement {
    /// A fresh committer for one take.
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe a non-final hypothesis (the full utterance decoded so far) and
    /// return the updated preview. Any word now agreed by the two most recent
    /// hypotheses, beyond what is already committed, becomes committed.
    pub fn observe(&mut self, hypothesis: &str) -> Preview {
        let current = tokenize(hypothesis);
        let agreed = common_prefix_len(&self.previous, &current);
        // Monotonic: only ever extend the committed prefix, never retract it.
        // A committed word is final even if a later decode would revise it —
        // that is the contract that lets the UI render it as settled.
        if agreed > self.committed.len() {
            self.committed = current[..agreed].to_vec();
        }
        let preview = self.preview_against(&current);
        self.previous = current;
        preview
    }

    /// Observe the closing hypothesis. The utterance is over, so the final
    /// decode — the model's best and most complete view — is trusted in full
    /// and everything is committed. Returns the committed display text.
    ///
    /// Callers that inject text should use the raw final transcript, not this
    /// return value: it is normalised for display (collapsed whitespace) and is
    /// meant for the live preview surface.
    pub fn finalize(&mut self, hypothesis: &str) -> String {
        self.committed = tokenize(hypothesis);
        self.previous = self.committed.clone();
        self.committed.join(" ")
    }

    /// The committed text as it currently stands.
    pub fn committed_text(&self) -> String {
        self.committed.join(" ")
    }

    /// Split a hypothesis into the settled prefix and the provisional tail,
    /// taking the committed words as authoritative.
    fn preview_against(&self, current: &[String]) -> Preview {
        let committed = self.committed.join(" ");
        let provisional = if current.len() > self.committed.len() {
            current[self.committed.len()..].join(" ")
        } else {
            String::new()
        };
        Preview {
            committed,
            provisional,
        }
    }
}

/// Split on Unicode whitespace into words. `split_whitespace` trims and
/// collapses runs, so the token stream is independent of how the model spaced
/// the text — what matters for agreement is the word sequence, not the gaps.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// Length of the longest shared prefix of two word sequences.
fn common_prefix_len(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words commit one tick after they first appear, once a second hypothesis
    /// confirms them — the defining behaviour of LocalAgreement-2.
    #[test]
    fn commits_the_stable_prefix_one_tick_late() {
        let mut la = LocalAgreement::new();

        let p = la.observe("שלום");
        assert_eq!(p.committed, ""); // first sighting — nothing confirmed yet
        assert_eq!(p.provisional, "שלום");

        let p = la.observe("שלום עולם");
        assert_eq!(p.committed, "שלום"); // confirmed by the second hypothesis
        assert_eq!(p.provisional, "עולם");

        let p = la.observe("שלום עולם טוב");
        assert_eq!(p.committed, "שלום עולם");
        assert_eq!(p.provisional, "טוב");
    }

    /// A revised tail must never corrupt the committed prefix: "sat"/"sap"
    /// disagree, so neither commits until the audio settles.
    #[test]
    fn a_flickering_tail_never_pollutes_committed() {
        let mut la = LocalAgreement::new();
        la.observe("the cat");
        let p = la.observe("the cat sat");
        assert_eq!(p.committed, "the cat");
        assert_eq!(p.provisional, "sat");

        // Next decode revises the tail word — it stays provisional.
        let p = la.observe("the cat sap");
        assert_eq!(p.committed, "the cat");
        assert_eq!(p.provisional, "sap");

        // Only once two decodes agree on the tail does it commit.
        la.observe("the cat sat down");
        let p = la.observe("the cat sat down now");
        assert_eq!(p.committed, "the cat sat down");
        assert_eq!(p.provisional, "now");
    }

    /// Code-switching Hebrew/English is common in real use (.claude/rules/hebrew.md).
    #[test]
    fn handles_mixed_hebrew_english() {
        let mut la = LocalAgreement::new();
        la.observe("תפתח את");
        let p = la.observe("תפתח את chrome");
        assert_eq!(p.committed, "תפתח את");
        assert_eq!(p.provisional, "chrome");

        let p = la.observe("תפתח את chrome עכשיו");
        assert_eq!(p.committed, "תפתח את chrome");
        assert_eq!(p.provisional, "עכשיו");
    }

    /// Niqqud and other combining marks must survive the split/rejoin intact.
    #[test]
    fn preserves_combining_marks() {
        let mut la = LocalAgreement::new();
        la.observe("שָׁלוֹם");
        let p = la.observe("שָׁלוֹם עוֹלָם");
        assert_eq!(p.committed, "שָׁלוֹם");
        assert_eq!(p.provisional, "עוֹלָם");
    }

    /// finalize trusts the closing decode fully and commits all of it.
    #[test]
    fn finalize_commits_everything() {
        let mut la = LocalAgreement::new();
        la.observe("שלום");
        let final_text = la.finalize("שלום עולם טוב");
        assert_eq!(final_text, "שלום עולם טוב");
        assert_eq!(la.committed_text(), "שלום עולם טוב");
    }

    /// Whitespace in the model output (double spaces, stray newlines) must not
    /// affect the word sequence or the commit decision.
    #[test]
    fn whitespace_is_normalised() {
        let mut la = LocalAgreement::new();
        la.observe("שלום  עולם");
        let p = la.observe("שלום עולם\nטוב");
        assert_eq!(p.committed, "שלום עולם");
        assert_eq!(p.provisional, "טוב");
    }

    /// An empty or whitespace-only hypothesis yields an empty preview and never
    /// panics — silence at the start of a take is normal.
    #[test]
    fn empty_hypothesis_is_inert() {
        let mut la = LocalAgreement::new();
        let p = la.observe("");
        assert_eq!(p, Preview::default());
        let p = la.observe("   ");
        assert_eq!(p.committed, "");
        assert_eq!(p.provisional, "");
    }
}
