# 5. Hands-free dictation with an energy-based VAD

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` workstreams 1.1 (audio capture & VAD), 1.4 (hotkeys)

## Context

M2 shipped push-to-talk dictation — hold `Ctrl+Space` to capture, release to
transcribe. The product direction is for **hands-free** to be the *default*:
press the hotkey once, speak, and have capture end on its own. That needs
toggle activation plus automatic end-of-session detection.

The roadmap (`docs/roadmap.md`, workstream 1.1) prescribes **Silero VAD v5**
(ONNX via the `ort` crate, 32 ms frames) with 500 ms-of-silence end-of-utterance
detection. Adopting Silero now would add a Rust dependency, an ONNX runtime, a
bundled model file, and a real-time frame pipeline — and `audio.rs` already
defers the related workstream-1.1 ring-buffer and wake-word capture work to
milestone M6.

## Decision

Drive the hands-free silence auto-stop with a **lightweight energy (RMS)
detector** — the new `lashon-core::vad` module — not Silero, for now:

- Press `Ctrl+Space` once to start; capture ends after a **5 s** speech-free
  window, or a second press. The window is a constant for now.
- The dictation worker polls the live take every 200 ms
  (`AudioCapture::samples_since`), classifies each chunk by RMS via
  `vad::is_speech`, and trims trailing silence before transcription.
- Push-to-talk "hold mode" is retained as a `DictationMode` variant behind a
  constant; a settings panel will later expose both the mode and the timeout.

Silero VAD stays the roadmap's choice and is **deferred to the M6 audio
overhaul**, alongside the workstream-1.1 lock-free ring buffer and wake-word
capture.

## Consequences

- Hands-free ships now with **no new dependency** and no model to bundle.
- The detector **calibrates to the room** — it tracks the noise floor and
  counts speech as audio that rises well above it, adapting to microphone
  gain rather than trusting a fixed threshold. It is still energy-only, so a
  sustained loud environment can blur the speech/silence line. Mitigations:
  the 5 s window — long enough to ride out natural pauses, short enough to keep
  dictation responsive — and a second key-press that always stops.
- The 5 s **session timeout** intentionally differs from workstream 1.1's
  500 ms per-utterance endpointing — it is a coarse "you are done" signal, not
  phrase segmentation.
- Activation mode and timeout are compile-time constants until a settings
  panel exposes them.
- When M6 introduces Silero VAD, `lashon-core::vad` is the seam to replace; the
  worker's `is_speech` call site stays unchanged.
