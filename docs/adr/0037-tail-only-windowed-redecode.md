# 37. Tail-only windowed re-decode for unbounded streaming dictation

## Status

Accepted — 2026-06-29. Builds on
[ADR-0035](0035-streaming-dictation-via-repeated-unary.md); lifts the 30 s take
ceiling that ADR carried. Touches the STT gRPC contract
([`packages/proto/stt.proto`](../../packages/proto/stt.proto)),
`lashon-core::streaming`, and the dictation worker.

## Context

ADR-0035 fakes live partials by re-decoding the **whole growing buffer** every
~500 ms and folding each hypothesis through client-side LocalAgreement-2. That
design quietly assumed a short take. Its own cadence benchmark says the re-decode
cost is "**~constant in buffer length**" — but only because "faster-whisper
processes a fixed 30 s mel window." That constant-cost claim holds **only while
the buffer is ≤ 30 s**. Past 30 s the model windows the audio sequentially, so a
re-decode of the whole buffer costs proportionally more the longer the take runs.

So the take was capped at `MAX_TAKE = 30 s`
([`docs/stories/streaming-dictation.md`](../stories/streaming-dictation.md) — "cap
at MAX_TAKE = 30 s"). The comment in `dictation.rs` framed that constant as a
backstop for a wedged detector, but at 30 s it fired **mid-sentence during
ordinary long-form dictation**. The user-visible symptom was a hard limit on
*both* how long you could speak and how much came back — the capture stopped at
30 s, so only 30 s of audio ever reached the decoder.

The decoder is **not** the limit: faster-whisper's `transcribe()` (no
`chunk_length` override, `vad_filter=False`) sequentially windows through the
whole clip and transcribes audio far past 30 s correctly. The only limit was the
capture cap.

Raising the cap alone is not enough. With the whole-buffer re-decode, a 4-minute
take would re-decode 4 minutes of audio every hop; each re-decode soon overruns
the `DecodeScheduler`'s 2.5 s budget and streaming **self-disables**, killing
live partials for the rest of a long take — the opposite of what "live partials"
promises. To raise the cap *and* keep partials live, the re-decode cost must be
bounded by something other than the total take length.

## Decision

### 1. `MAX_TAKE` becomes a real safety backstop (5 minutes)

`MAX_TAKE` is raised from 30 s to **5 minutes** and re-documented as what it
always should have been: a net for a capture that would otherwise never end (a
wedged VAD detector, or VAD unavailable so only a second press stops the take) —
**not** a normal cap. The normal stops are unchanged: the Silero VAD endpoint, or
a second hotkey press. Five minutes sits far above any real continuous utterance
while still bounding a forgotten session — at 16 kHz mono f32 the buffer grows
~62 KB/s, so the cap bounds it to ~19 MB. A cap must still exist (the buffer is
in-memory and unbounded otherwise); it just must not fire in normal use.

### 2. Re-decode only the uncommitted tail, via a `WindowAnchor`

Instead of `samples_since(0)`, each re-decode runs on `samples_since(offset)`,
where `offset` is how far the **committed** transcript already reaches into the
audio. As LocalAgreement-2 commits words, the window slides forward past them, so
each decode covers only the still-provisional tail — typically the last sentence
or two, a few seconds of audio — regardless of how long the take has run.

The bookkeeping is pure policy, so it lives in `lashon-core::streaming` as
`WindowAnchor` (11 tests, Hebrew + mixed-script): it owns the window `offset` and
the committed-word `prefix`, derives the decoder-context prompt (§5), reassembles
the full hypothesis (§4), and advances itself (§3). No audio, gRPC, or Tauri — it
unit-tests like `DecodeScheduler` and `LanguageLatch` beside it. The cadence
(`DecodeScheduler`) is unchanged and still paces on **total** new audio, not the
window length.

### 3. Advance only over whole committed segments — using segment timings

To slide the window, the anchor must map "this committed word" to "this audio
sample," which needs **timestamps**. The STT response gains `repeated Segment
segments` (`{ text, start, end }`, seconds from the request buffer's start) —
faster-whisper computes segment times for free while iterating the decode, so
this costs nothing extra. The one-shot final decode simply ignores them.

The anchor advances past a window segment **only when every one of its words has
committed**. A half-committed segment keeps its audio in the window, which
guarantees the window never drops audio whose text is still provisional. Segment
boundaries are where Whisper itself chose to break, so they are the most stable
place to restart a window. Matching is at the whitespace-token level — the same
tokenisation LocalAgreement uses — and the sidecar sanitises per segment so the
two token streams line up.

### 4. Reassemble the global hypothesis; the committer is unchanged

The decode now sees only the window, but LocalAgreement must still reason over
the whole utterance. So the worker reassembles `global = prefix + window_text`
(`WindowAnchor::global`) and folds **that** through the same, untouched
`LocalAgreement` committer. Because the committed prefix is frozen and we only
ever append the decoder's own continuation, the global token stream stays
continuous and monotonic across window advances — the committer needs no reset
and cannot retract. The well-tested commit policy carries over verbatim.

### 5. Prime the windowed decode with the committed tail (`initial_prompt`)

A window that no longer starts at audio 0 loses the linguistic context Whisper
would have had. The request gains an `initial_prompt` field; the worker passes
the last `STREAM_PROMPT_WORDS` (40) of committed text as Whisper decoding context
(biases decoding only — never emitted). Empty for the one-shot final and any
window still anchored at 0.

### 6. Streaming stays preview-only; the final decode is authoritative

Unchanged from ADR-0035 §6, and load-bearing here: on stop the worker runs the
full final `transcribe` on the whole trimmed take and **injects its raw text**.
The windowed partials only feed the on-screen preview. This is what makes the
windowing safe — a rare imperfection at a window seam (a committed word the next
window decodes slightly differently) is cosmetic and corrected by the next decode
and, definitively, by the authoritative final. We trade a little preview-only
accuracy for unbounded, live, cheap partials.

## Cost profile and graceful degradation

For normal speech the window is the uncommitted tail — a few seconds — so each
re-decode stays inside a single 30 s mel window and the ADR-0035 constant-cost
regime holds **for takes of any length**. The one case the window can still grow
is a *single uninterrupted utterance with no segment break* longer than the mel
window: there the anchor cannot advance until that segment finally closes, the
re-decode grows, and — exactly as today — the `DecodeScheduler` self-disables
partials if it overruns the 2.5 s budget. The authoritative final still captures
everything. This is the same graceful degradation ADR-0035 already defined, now
reached only in a genuinely pathological case rather than at a flat 30 s. A hard
window-length guard is a possible future refinement; it is deliberately omitted
for v1 to avoid a guard that could desync the preview, since the final already
covers correctness.

## Consequences

- **Dictation is no longer capped at 30 s.** Speak as long as you like; the VAD
  endpoint (or a second press) ends the take, and the 5-minute backstop only
  catches a wedged capture.
- **Live partials stay live on long takes** (on a streaming-capable GPU):
  re-decode cost is bounded by the uncommitted tail, not the take length.
- **Final injected text is unchanged** — still the raw full-buffer final decode;
  the windowing layer cannot alter what gets typed.
- **Contract additions are backward-compatible** proto3 field adds (`segments`,
  `initial_prompt`); Rust regenerates via `build.rs`, Python via `codegen` /
  the CI proto smoke. No generated files are committed.
- **The sidecar decode lock (ADR-0035 §4) still applies**, and windowed decodes
  are smaller, so streaming/final contention is lower than before.
- **One new pure module** (`WindowAnchor`) and a small provider-seam change
  (`TranscribeOptions`, `Transcript.segments`) — the two `transcribe` call sites
  and the ignored integration test move with it.

## Alternatives considered

- **Just raise `MAX_TAKE`, keep the whole-buffer re-decode.** Rejected: each
  re-decode then grows with the take and self-disables partials on long takes —
  it lifts the cap but breaks the live-partial promise it exists to serve.
- **Fixed-size sliding window (e.g. always the last 30 s).** Rejected: bounds
  cost to a *constant* but never below one whole mel window, and needs
  overlap-trimming to avoid double-emitting. Shrinking to the commit boundary is
  cheaper and aligns the window to natural segment breaks.
- **Word-level timestamps for a tighter anchor.** Rejected for v1:
  `word_timestamps=True` adds a cross-attention alignment pass to *every*
  streaming decode — latency this change is trying to reduce — whereas segment
  timings are already computed. Segment granularity is more than enough for a
  preview-only window.
- **Acoustic overlap (re-decode a little before the anchor) instead of a text
  prompt.** Deferred: it needs word timestamps to trim the re-emitted overlap.
  The `initial_prompt` text context is simpler and, for a preview, sufficient.
