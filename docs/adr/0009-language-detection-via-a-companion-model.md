# 9. Spoken-language detection via a companion model

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** Lashon contributors
- **Context source:** v0.2.0 release testing; `docs/providers.md` (STT catalog);
  `.claude/rules/hebrew.md` (mixed Hebrew/English is first-class)

## Context

Dictating in English produced Hebrew characters — English speech transcribed
into Hebrew letters. The dictation path sends an empty language string so
Whisper auto-detects, and the symptom looked like the model "translating"
English.

Direct measurement against `ivrit-ai/whisper-large-v3-turbo-ct2`, the default
STT model, found the cause. The model is a Hebrew fine-tune, and its
fine-tuning **collapsed the language-detection head**: `detect_language`
returns `he` at probability `1.000` for unambiguously English audio, with `en`
at `0.000`. Auto-detect (`language=None`) therefore *always* resolves to
Hebrew, and the decode is forced to `<|he|>`.

Two further measurements shaped the decision:

- **`avg_logprob` does not discriminate language.** Decoding the same clip
  forced to `he` and forced to `en` yields near-identical confidence (gaps of
  ±0.01, sign unstable across clips). So "decode both ways, keep the more
  confident result" is not viable.
- **The forced token only bites on hard audio.** On clean speech the model
  transcribes the right script regardless of the forced language; on noisier
  real-microphone audio the forced token decides — which is why clean test
  clips passed while real English dictation failed.

The model cannot be asked what language it heard, and its transcription
quality is exactly why it is the default (Hebrew WER). Hebrew **and** mixed
Hebrew/English are first-class for Lashon — neither auto-detect nor a hard-pinned
language is acceptable.

## Decision

Add a **small companion model used only for language identification**. A
vanilla `Systran/faster-whisper-tiny` (~78 MB, MIT) — its detector is intact —
identifies the spoken language; the ivrit-ai model then transcribes with that
language **forced**.

- `FasterWhisperEngine` loads two `WhisperModel`s: the ivrit-ai transcription
  model and the tiny detector, both on the same device/compute type.
- `transcribe(pcm, language=None)` runs `detector.detect_language()` and forces
  the result for the decode. An **explicit** `language` argument still bypasses
  detection — the WER benchmark and the integration tests pass `"he"` directly.
- The detector is a second entry in `models/manifests/stt.json`
  (`id: faster-whisper-tiny`, `role: language-detector`), downloaded and
  SHA-256-verified on first run by the same `ensure_model` path as the main
  model. `ensure_model` gained a `label` argument so the first-run progress
  line reads "downloading the language detector".

### Alternatives rejected

- **Whisper's own auto-detection** — the collapsed detector is the bug itself.
- **Decode twice, pick by confidence** — `avg_logprob` does not separate the
  languages (measured above).
- **Use vanilla `large-v3-turbo` for everything** — its detector works, but it
  forfeits the ivrit-ai fine-tune's Hebrew accuracy, and Hebrew is the product.
- **A user-selected dictation language** — reliable, but manual, and it gives
  up automatic per-utterance switching between Hebrew and English.

## Consequences

- First run downloads ~78 MB more, and the sidecar holds a second model in
  memory (tiny — tens of MB). Language ID adds a small per-utterance overhead;
  measured dictation stays within the `docs/testing.md` STT budget.
- Language is detected **per utterance**, not per segment — code-switching
  *within* a single utterance still resolves to one language. Acceptable for
  dictation; a future engine may detect per segment.
- The detector is MIT-licensed — no change for the CI license scan, and it is a
  first-run download, never bundled.
- This is an STT-engine-internal change. The provider seam, the gRPC contract,
  and the Rust core are untouched: the core still sends an empty language and
  receives a `Transcript`.
- The bug predated this ADR — it was present in `v0.1.0`. The auto-detect
  switch in commit `8a72f60` did not fix English; it only replaced a hard-pinned
  `he` with an auto-detect that always resolves to `he`.
