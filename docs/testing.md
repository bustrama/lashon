# Testing strategy

Hebrew is exercised explicitly at every layer — never English-only, and mixed
Hebrew/English too. Each milestone's tests cover every layer that milestone
touches (see [`roadmap.md`](roadmap.md) for milestone scope).

## Phase 1 — STT

- `tests/hebrew-corpus/` — Hebrew clips with ground-truth transcripts, in three
  tiers (`manifest.json`): `read` — 25 CC-licensed FLEURS clips, the active CI
  gate (WER ≤ 27%, a regression baseline; encyclopedic read speech is a harder
  domain than dictation); `studio` — 12 clean-dictation clips recorded by hand,
  the M1 gate (WER ≤ 12%), scored once the recordings land; `code-switch` — 10
  Hebrew/English clips, a local-only non-gating baseline run with language
  detection on (exercises the companion detector — see
  [ADR-0009](adr/0009-language-detection-via-a-companion-model.md)).
- CI bench job: `scripts/wer-bench.py` transcribes every clip present on disk,
  scores WER per tier, and fails CI when a gating tier is over target; tiers
  with no clips on disk are skipped.
- Latency bench: a hotkey-press→paste end-to-end timer, with P50/P95/P99
  enforced.

## Phase 2 — Commands & tools

- `tests/commands.he.yaml` — 20 voice commands with the expected tool sequence
  (asserted on exact tool name + argument matching).
- Tool unit tests in Rust with fixtures.
- Integration: spawn a sandbox test app, voice-command Lashon to manipulate it,
  assert the outcome.
- Confirmation-policy tests: assert every `requires_confirmation` tool blocks
  until approval.

## Phase 3 — TTS

- 20 Hebrew test sentences in `tests/tts-quality.yaml`.
- Manual: 3 native-Hebrew reviewers score each provider/voice (MOS 1–5).
- Automated: alignment WER (the TTS output, Whisper-transcribed, against the
  source text) per provider — a sanity check for no semantic shift.
- Latency bench: first-byte and total synthesis time per provider.

## Cross-cutting

- Hebrew RTL injection regression matrix: 12 target apps × 5 representative
  phrases, a manual checklist per release.
- Cross-tier auto-detection CI: a matrix of {Win11+4080, Win11+1660,
  Ubuntu+CPU-only, macOS+M1} runners asserts the correct tier is chosen.
- Memory-leak: a 1-hour continuous push-to-talk loop; RSS drift under 50 MB
  passes.

## Performance budgets (enforced in CI)

| Metric | Tier A | Tier B | Tier C |
|---|---|---|---|
| Hotkey press → audio capture start | 20 ms | 30 ms | 50 ms |
| VAD frame processing | 35 ms | 50 ms | 80 ms |
| STT inference (3 s audio) | 250 ms | 600 ms | 2 s |
| LLM cleanup (≤ 60 tok out) | 350 ms | 800 ms | cloud-dep |
| Clipboard inject | 80 ms | 80 ms | 120 ms |
| **Dictation E2E (3 s utterance)** | **≤ 800 ms** | **≤ 1.5 s** | **≤ 3 s** |
| Command E2E (single-tool) | ≤ 1.8 s | ≤ 2.5 s | cloud-dep |
| Chat first-byte (LLM) | 600 ms | 1.2 s | cloud-dep |
| Chat first-byte (TTS) | 900 ms | 1.5 s | cloud-dep |
