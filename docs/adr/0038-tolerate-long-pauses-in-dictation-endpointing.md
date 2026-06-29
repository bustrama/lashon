# 38. Tolerate multi-second pauses in dictation endpointing

## Status

Accepted — 2026-06-29. Re-tunes the endpoint thresholds set in
[ADR-0015](0015-silero-vad-and-utterance-endpointing.md); companion to
[ADR-0037](0037-tail-only-windowed-redecode.md) (which lifted the 30 s capture
cap). Mechanism unchanged — only the constants in
`EndpointConfig::default()` (`packages/shared-rust/src/vad.rs`).

## Context

ADR-0037 removed the 30 s take cap, but a user testing long-form dictation still
saw takes "cap around 30 s." A stop-reason diagnostic in the dictation worker
showed the truth: takes were ending on **`vad-endpoint`**, not the cap — one
real take ran **44.7 s** and ended because the speaker paused. The 30 s the user
saw earlier was the same thing landing at a different pause.

So the real limit on long-form dictation was never the cap — it was the
end-of-utterance detector ending the take at the *first* real pause. ADR-0015
deliberately set that aggressively — **500 ms of clean silence**, or **1500 ms**
after the last speech when faint "mid-word" energy keeps the silence from being
clean — because at the time dictation was short-utterance and the same detector
also serves wake-word turns, where a snappy stop matters. For long-form
dictation (now supported, takes allowed up to the 5-minute backstop), those
thresholds cut the speaker off the moment they pause to think or breathe.

## Decision

Raise the dictation endpoint thresholds so a multi-second pause is tolerated:

- `silence`: 500 ms → **5 s** — clean silence ends the take after 5 s.
- `hold`: 1500 ms → **6 s** — a pause carrying faint mid-word energy ends the
  take 6 s after the last real speech.

`no_speech_timeout` (the "triggered but never spoke" guard) stays 6 s, and
`min_speech` / the thresholds are unchanged. The two-tier mechanism from
ADR-0015 is untouched — only the constants move.

`hold` must stay **≥ `silence`**: for a clean pause `quiet` (time since last
energy) and `trailing` (time since last speech) grow together, so a smaller
`hold` would end the take first and the longer `silence` would never take
effect. 5 s / 6 s preserves the "clean silence ends sooner than a mumbled tail"
ordering while giving both a multi-second budget.

## Consequences

- **Long-form dictation works:** you can pause to think mid-take without it
  ending. With ADR-0037's unbounded capture, a take now runs until you stop for
  ~5 s, press the hotkey again, or hit the 5-minute backstop.
- **Trade-off — a ~5 s tail before finalisation.** After you truly finish, the
  take waits ~5 s before transcribing and injecting. This is the deliberate cost
  the user chose over eager cut-offs. The extra trailing silence is **not**
  transcribed: `finish_take` still trims to the last speech + `TAIL_MARGIN`
  (500 ms), so accuracy and the final decode are unaffected — only the
  wait-to-finalise grows.
- **Partial walk-back of ADR-0015's responsiveness**, scoped to dictation.
  ADR-0015 optimised for snappy short turns; the product now also serves
  long-form, which reverses the priority. Wake-word *triggering* is unaffected
  (separate path); only the length of a wake-triggered dictation take changes,
  consistent with hotkey takes.
- **Tests split mechanism from policy.** The threshold-specific tests now build
  an explicit fast `EndpointConfig` (500 ms / 1500 ms) so they exercise the
  *mechanism* regardless of the shipped default, and one test pins the default
  policy (5 s / 6 s). Future re-tuning is a one-line change that can't silently
  break the mechanism tests.

## Alternatives considered

- **A shorter tolerance (~2.5 s).** Offered; the user chose ~5 s for genuine
  long-form pauses. 2.5 s remains a reasonable alternative if 5 s feels long.
- **Manual stop only (no silence auto-end).** Rejected by the user — they want
  the take to auto-end when they're done, just not eagerly.
- **A user-configurable silence tolerance (settings).** Deferred: ship a
  sensible default now; expose the knob when the settings panel lands (it would
  also let wake-word turns keep a snappier value than long-form dictation if
  that split ever proves necessary).
