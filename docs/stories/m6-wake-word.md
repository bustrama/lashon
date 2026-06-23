# Wake word and the audio overhaul

Milestone **M6**. Branch `m6-wake-word`.

> **Status: ready for PR.** Silero VAD endpointing and the full wake-word
> subsystem — engine, capture, worker, settings, and Hub UI — have landed.
> A trained "Hey Lashon" classifier was produced (see
> [`wake-word-training.md`](../wake-word-training.md)) and live-tested.
> CI on the three runners is the remaining gate.

## Why

Hands-free dictation shipped in M2 on a deliberately crude **energy-RMS VAD**
with a flat 5 s silence timeout — a stepping stone, recorded as such in
[ADR-0005](../adr/0005-hands-free-dictation-and-energy-vad.md), which defers the
"audio overhaul" to M6: real Silero VAD, a rolling capture buffer, and
wake-word capture.

M6 is that overhaul, and it has two coupled halves:

- **End-of-utterance detection.** A 5 s timeout is a coarse "you are done"
  signal, not phrase endpointing — every hands-free take waits five silent
  seconds before it transcribes. Silero VAD with 500 ms endpointing
  (docs/roadmap.md §1.1) replaces it: dictation stops when the speaker stops.
- **Wake word.** "Hey Lashon" hands-free activation. This is *why* the
  endpointing is a prerequisite, not a nice-to-have: a wake-triggered take has
  no hotkey-release edge to end it, so only the VAD endpoint can.

## How

Decisions: [ADR-0015](../adr/0015-silero-vad-and-utterance-endpointing.md)
(Silero VAD) and [ADR-0016](../adr/0016-wake-word-engine.md) (the wake-word
engine and the "Hey Lashon" model strategy).

### Silero VAD and endpointing — `lashon-core::vad`

- `Endpointer` — pure, deterministic end-of-utterance logic over a stream of
  per-frame speech probabilities. The rule: end on 500 ms of clean silence, or
  1500 ms after the last real speech when faint mid-word energy keeps the
  silence from being clean. Fully unit-tested; the worker call site is a thin
  shell.
- `SileroVad` — the Silero VAD v5 ONNX model run via the `ort` crate (ONNX
  Runtime, CPU), scoring each 512-sample / 32 ms frame with a speech
  probability.
- The energy `SpeechDetector` of ADR-0005 is removed; `rms()` stays — it is the
  tongue's loudness meter, not a speech classifier.

### The rolling buffer — `lashon-core::audio`

- A 30 s ring buffer (`ringbuf`) fed by an always-on capture stream, so
  wake-word detection has continuous audio with no dictation session open. The
  bounded push-to-talk take reads a window from it.

### The wake-word engine — `lashon-core::wake`

- The openWakeWord ONNX pipeline (melspectrogram -> shared embedding ->
  per-wake-word classifier) via `ort`.
- A 2-consecutive-frame threshold suppresses false activations; a sensitivity
  setting tunes it.
- `is_capturing` / `is_speaking` gates suspend detection while Lashon is
  capturing a take or (from M10) speaking, so it never self-triggers.
- On a detection the engine opens a dictation take — the same path a hotkey
  press takes — which the Silero endpoint then closes.

### Settings and UI

- `wakeword.enabled` (default **off**), `wakeword.sensitivity`, and
  `wakeword.model` (default `hey_lashon`).
- A "Wake word" section in the Settings Hub — a toggle, a sensitivity slider,
  and a picker over the installed classifiers — with he+en localization. The
  picker renders friendly names (`hey_lashon` → "Hey Lashon"; user-trained
  files get a title-cased fallback).
- A collapsible **"More wake words"** sub-section offers one-click installs
  of the openWakeWord pretrained classifiers — `Hey Jarvis`, `Alexa`,
  `Hey Mycroft`, `Hey Rhasspy` — each behind a "Non-commercial" badge and a
  licence-confirmation dialog. The download path is SHA-256-verified end to
  end against `models/manifests/wake-classifiers.json`; the files are never
  bundled. A link out to `openwakeword.com/library` rounds out the section
  for users looking for less common phrases.
- No new tongue state: a wake-triggered take reuses `capturing`.

### Models

- ONNX weights are downloaded and SHA-256-verified, never committed — the
  pattern of `models/manifests/`. M6 adds `models/manifests/m6-audio.json`
  (Silero VAD and the openWakeWord models) and a `lashon-core` resolver that
  verifies every file before `ort` loads it: a tampered ONNX is native code.

## The "Hey Lashon" model — a deliberate gap

M6's roadmap DoD names a *default "Hey Lashon"* wake word. The wake-word
**engine** is in scope and built here; the **model** is not — a custom
openWakeWord classifier is produced by an offline GPU training run (synthesize
the phrase with Piper TTS, then train on it). So:

- The engine ships **model-agnostic**, loading any openWakeWord classifier.
- A documented training procedure ships at
  [`wake-word-training.md`](../wake-word-training.md).
- Wake word stays **disabled by default** until the trained model lands —
  enabling an unverified wake word by default would be the wrong posture for
  both correctness and the false-activation budget.
- The in-app trainer wizard stays deferred — roadmap §1.5 already calls it a
  "later milestone".

## Acceptance Criteria

- [x] `lashon-core::vad::Endpointer` — 500 ms / 1500 ms endpoint logic, pure and
      unit-tested.
- [x] `SileroVad` runs the v5 ONNX model via `ort`; the dictation worker feeds
      it 32 ms frames and ends the take on the `Endpointer` verdict.
- [x] Always-on wake-word capture (`audio::open_wake_stream`) feeds the engine
      over a channel.
- [x] `lashon-core::wake` — the openWakeWord pipeline, the 2-frame `Trigger`,
      the `is_capturing` / `is_speaking` gates; a detection opens a take.
- [x] `wakeword.enabled` / `wakeword.sensitivity` settings and a Hub section,
      he+en.
- [x] `cargo test --workspace` green; `cargo clippy --workspace` and
      `npm run check` clean.
- [ ] CI green on `windows-2022`, `macos-14`, `ubuntu-24.04`.
- [x] Manual check: "Hey Lashon" opens a take and the Silero endpoint closes
      it (verified live by the user with the trained classifier installed).

## Files

- `packages/shared-rust/src/vad.rs` — `Endpointer` and `SileroVad`.
- `packages/shared-rust/src/wake.rs` — the wake-word engine and `Trigger`.
- `packages/shared-rust/src/model.rs` — ONNX model resolution and SHA-256
  verification.
- `packages/shared-rust/src/audio.rs` — resample-on-capture and the always-on
  `open_wake_stream`.
- `packages/shared-rust/Cargo.toml` — `ort`, `sha2`.
- `apps/desktop/src-tauri/src/dictation.rs` — the worker feeds Silero frames to
  the `Endpointer`; `DictationChannel::trigger` opens a wake-triggered take.
- `apps/desktop/src-tauri/src/wakeword.rs` — the wake-word worker.
- `apps/desktop/src-tauri/src/lib.rs` — the `Gates` and worker spawning.
- `apps/desktop/src/routes/hub/+page.svelte` — the Wake-word section.
- `apps/desktop/src/lib/settings.ts`,
  `apps/desktop/src/lib/i18n/locales/{he,en}.json` — the settings and copy.
- `models/manifests/m6-audio.json`, `scripts/verify-models.py` — the Silero VAD
  and openWakeWord model registry and its downloader.
- `docs/adr/0015-silero-vad-and-utterance-endpointing.md`,
  `docs/adr/0016-wake-word-engine.md`, `docs/wake-word-training.md` — the
  decision records and the training procedure.

## Dependencies

Builds on the M2 hands-free worker and ADR-0005's energy VAD (which this
supersedes). New runtime dependencies: `ort` (ONNX Runtime, CPU-only — no CUDA
coupling) and `ringbuf`. The "Hey Lashon" classifier model is an external,
offline-trained artifact, tracked as a follow-up.
