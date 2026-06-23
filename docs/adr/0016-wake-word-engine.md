# 16. The wake-word engine

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` §1.5; milestone M6. Builds on
  [ADR-0015](0015-silero-vad-and-utterance-endpointing.md) (the `ort` runtime).

## Context

M6's headline is the wake word — "Hey Lashon" hands-free activation
(`docs/roadmap.md` §1.5). The roadmap prescribes the openWakeWord ONNX models
run via `ort`, a two-frame detection threshold, and a bundled wake-word model.
[ADR-0015](0015-silero-vad-and-utterance-endpointing.md) already brought `ort`
in for Silero VAD; the wake word reuses it.

Two things shape the design: openWakeWord's pipeline structure, and a licensing
split in its models.

## Decision

### The engine — `lashon-core::wake`

- openWakeWord's three-stage ONNX pipeline: a melspectrogram model, a shared
  audio-embedding model, and a per-phrase classifier. `WakeWord::observe` runs
  them over a rolling ~2.5 s buffer and returns a wake-likelihood score.
- A **sliding-window** evaluation: each ~80 ms step re-runs the melspectrogram
  over the buffer, batches every 76-frame embedding window into one embedding
  inference, and classifies the most recent 16 embeddings — three ONNX runs per
  step. Simple and correct; openWakeWord's incremental streaming is an
  optimisation deferred unless profiling demands it.
- `Trigger` — pure, debounced detection: the wake word fires only after the
  score clears the threshold on two consecutive frames (§1.5), and re-arms only
  after a sub-threshold frame, so one utterance fires once.

### Capture and the workers

- An always-on capture (`audio::open_wake_stream`) delivers 16 kHz mono chunks
  over an `mpsc` channel. This stands in for the roadmap's literal "30 s
  lock-free ring buffer": the detector needs only ~2.5 s of context (kept
  inside `WakeWord`), and a channel models a producer/consumer audio stream
  more directly than a random-access ring buffer. The dictation take keeps its
  own bounded capture.
- A dedicated `wakeword` worker thread (the Tauri crate) owns the capture and
  the engine. On a detection it posts a take to the dictation channel — the
  wake word's equivalent of a hotkey press — so it reuses the whole dictation
  path and the Silero endpoint closes the take.
- `is_capturing` / `is_speaking` gates (`Gates`, shared `Arc<AtomicBool>`s):
  the wake worker is suspended while dictation is capturing — and, from M10,
  while TTS is speaking — so it never self-triggers on Lashon's own audio
  (`.claude/rules/architecture.md`). `is_speaking` is wired but unset until M10.

### The "Hey Lashon" model

openWakeWord's models split cleanly by licence:

- The melspectrogram and embedding models are **Apache-2.0** — shipped,
  manifest-managed (`models/manifests/m6-audio.json`), SHA-256-verified before
  load like every other model.
- openWakeWord's **pretrained classifiers are CC-BY-NC-SA-4.0** — exactly what
  `.claude/rules/security.md` forbids bundling. The classifier therefore cannot
  be a shipped openWakeWord model.
- Lashon's wake-word classifier (`hey_lashon.onnx`) is **trained by us** —
  openWakeWord's automated training synthesises the phrase and yields a
  classifier Lashon owns and may license freely (see
  [`wake-word-training.md`](../wake-word-training.md)). It is an offline GPU
  build, loaded by path, never a manifest model.
- The classifier ships in the installer as a Tauri bundle resource and is
  staged into `$LASHON_MODELS_ROOT/wakewords/` on first launch
  (`lashon_core::model::install_bundled_wake_classifiers`). The staging is
  idempotent and never overwrites a user's own replacement at the same path,
  so a user-trained classifier survives upgrades.
- Until that model exists, wake word ships **disabled by default**; the engine,
  worker, gates, settings, and Hub UI are all in place and dormant.

### Settings

`wakeword.enabled` (default off) and `wakeword.sensitivity`, with a Hub "Wake
word" section. The wake worker reads them at startup — a change applies on the
next launch, like the M5 STT-device tier ([ADR-0014](0014-stt-device-by-hardware-tier.md)).

## Consequences

- The wake-word subsystem is complete and compiles, but **cannot be verified
  end-to-end here**: there is no trained `hey_lashon.onnx`, and detection
  accuracy needs wake-phrase audio. Verified instead: the three-model pipeline
  loads and runs, scores silence low, and the `Trigger` debounce is
  unit-tested.
- Wake word is **off by default and dormant** until `hey_lashon.onnx` is
  trained and placed. The engine is model-agnostic — any openWakeWord-format
  classifier with a 16-embedding window works.
- No `ringbuf` dependency; the channel-based capture is the deliberate
  deviation from the roadmap §1.1 wording.
- The roadmap's battery-aware throttle and the in-app wake-word trainer wizard
  stay deferred — §1.5 already calls the trainer a "later milestone".
- Two `cpal` input streams coexist briefly while a wake-triggered take runs;
  the wake worker is gated off for its duration.
