# 15. Silero VAD and per-utterance endpointing

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` workstream 1.1; milestone M6. Supersedes
  the VAD half of [ADR-0005](0005-hands-free-dictation-and-energy-vad.md).

> **Amended 2026-06-29 by [ADR-0038](0038-tolerate-long-pauses-in-dictation-endpointing.md):**
> the two-tier endpoint **mechanism** below is unchanged, but the dictation
> **thresholds** were re-tuned for long-form dictation — clean silence
> 500 ms → **5 s**, hold 1500 ms → **6 s**. Read the specific "500 ms / 1500 ms"
> values below as "the snappy values this ADR shipped"; the current defaults are
> in ADR-0038.

## Context

[ADR-0005](0005-hands-free-dictation-and-energy-vad.md) shipped hands-free
dictation on a lightweight energy-RMS detector with a flat 5 s silence timeout,
explicitly a stepping stone — it deferred Silero VAD, "alongside the
workstream-1.1 lock-free ring buffer and wake-word capture", to "the M6 audio
overhaul".

That overhaul is M6. The energy detector has two limits the roadmap (§1.1)
always meant Silero to fix:

- It is energy-only — a sustained loud environment blurs the speech/silence
  line.
- The 5 s timeout is a coarse "you are done" signal, not phrase endpointing:
  every hands-free take waits five silent seconds before it transcribes.

And wake word — the rest of M6 — cannot work without real endpointing: a
wake-triggered take has no hotkey-release edge, so only a voice-activity
endpoint can end it.

## Decision

Adopt **Silero VAD v5** via the **`ort`** ONNX-runtime crate, and replace the
timeout with a real endpoint detector.

- **`ort` `=2.0.0-rc.12`**, CPU-only — default features, no GPU execution
  provider. `ort`'s `download-binaries` statically links a prebuilt ONNX
  Runtime, so nothing extra ships at runtime. The roadmap also prescribes `ort`
  for wake word, so this is one shared ONNX dependency
  ([ADR-0016](0016-wake-word-engine.md) builds on it).
- **`lashon-core::vad::SileroVad`** runs `silero_vad.onnx` (Silero v5, MIT) —
  512-sample / 32 ms frames at 16 kHz, with the model's 64-sample carried
  context and recurrent state threaded inside the wrapper.
- **`lashon-core::vad::Endpointer`** — pure, deterministic logic over the
  per-frame speech probabilities. An utterance ends on **500 ms of clean
  silence** (no frame even reaching an energy floor), or — when faint mid-word
  energy keeps the silence from being clean — **1500 ms after the last real
  speech**. A no-speech timeout ends a take in which the user never spoke; a
  min-speech gate discards a stray blip. Being pure, it is exercised entirely
  by unit tests and the worker call site stays a thin shell.
- The energy `SpeechDetector` is **removed**. `rms()` stays — it is the
  tongue's waveform loudness meter, not a classifier.
- Capture **resamples to 16 kHz as it records** (a streaming resampler in
  `audio.rs`) — Silero requires exactly 16 kHz, and resampling only at `stop()`
  no longer suffices for live framing.
- Models are downloaded, never committed (`models/manifests/m6-audio.json`),
  and SHA-256-verified by `lashon-core::model` before `ort` loads them — a
  tampered ONNX graph is native code
  ([ADR-0010](0010-harden-the-stt-sidecar-trust-boundary.md)).

## Consequences

- Endpointing is phrase-accurate: dictation stops ~500 ms after the speaker
  stops, not after a flat 5 s. ADR-0005's deliberate gap between a coarse
  timeout and real endpointing is closed.
- A new runtime dependency, `ort`, plus the ONNX Runtime it statically links.
  **CI on all three runners fetches the prebuilt runtime at build time**
  (`download-binaries`); there is no GPU coupling and no library to package.
- `ort`'s recoverable errors carry a non-`Send` payload, so they are reduced to
  their message at the `anyhow` boundary (`vad::ort_err`).
- The Silero model (~2.3 MB) is resolved and verified at runtime. If it is
  absent — a fresh checkout, or a packaged build before its first-run
  download — the worker logs it and hands-free **degrades to a second press or
  the hard-cap backstop**, rather than failing. (That backstop was raised from
  30 s to 5 minutes so it no longer fires mid-utterance —
  [ADR-0037](0037-tail-only-windowed-redecode.md).) Wiring the packaged-build
  first-run download is a tracked follow-up.
- ADR-0005 predicted the worker's VAD call site would be unchanged; in practice
  the frame cadence moved from 200 ms to 32 ms and the `Endpointer` was added —
  a refinement of that prediction, not a contradiction of the seam.
