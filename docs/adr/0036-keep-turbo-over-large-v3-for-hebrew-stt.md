# 36. Keep turbo (`large-v3-turbo`) over non-turbo `large-v3` for Hebrew STT

## Status

Accepted — 2026-06-28. Resolves the deferred **"Model / quality note"** in
[`docs/adr/0035`](0035-streaming-dictation-via-repeated-unary.md) and
[`docs/stories/streaming-dictation.md`](../stories/streaming-dictation.md), and
supersedes the non-turbo recommendation carried over from the closed
[#1](https://github.com/bustrama/lashon/issues/1). Tracking:
[#9](https://github.com/bustrama/lashon/issues/9).

**Decision: keep `ivrit-ai/whisper-large-v3-turbo-ct2` (turbo).** No model swap.

## Context

The shipped transcription model is **`ivrit-ai/whisper-large-v3-turbo-ct2`**
(turbo). Issue #1 argued turbo is "English-leaning and silently loses Hebrew
accuracy" and recommended the non-turbo **`large-v3`**. That swap was
deliberately deferred so it would not block live streaming dictation (#8,
commit `ad48311`); the streaming driver is model-agnostic, so the swap is a
clean, separable decision — taken now.

ivrit-ai publishes a ready CTranslate2 build, **`ivrit-ai/whisper-large-v3-ct2`**
(Apache-2.0, `model.bin` present), so no `ct2-transformers-converter` step was
needed — it drops into the existing faster-whisper engine unchanged.

Both models were benchmarked on **this machine** (RTX 4080, 16 GB,
`int8_float16` on CUDA) over [`tests/hebrew-corpus/`](../../tests/hebrew-corpus),
through the real engine code path (`beam_size=5`, the companion language
detector, `postprocess.sanitize`). Model selection was threaded through
`load_engine(model_id=…)` as an explicit, optional parameter (the shipped hot
path passes nothing → turbo); the WER and stream benchmarks expose it via
`$LASHON_STT_MODEL_ID` / `--model`. Production behaviour is unchanged.

## Measurements

### Accuracy — `scripts/wer-bench.py`

| Tier (decode language) | turbo | large-v3 | Δ | gate |
|---|---|---|---|---|
| **`read`** — FLEURS, forced `he` (gating) | **23.9 %** | **22.2 %** | **−1.7 pp** (~7 % rel.) | ≤ 27 %, both pass |
| `code-switch` — detected language (informational) | 16.1 % | **17.2 %** | **+1.1 pp (worse)** | ≤ 30 %, both pass |

- `read` is 25 clips. The improvement is real but **modest** and well within
  what 25 encyclopedic clips can move; it is not the decisive margin #1
  anticipated.
- `code-switch` is **slightly worse** on large-v3, including a script-mixing
  artifact (`refactor` → `refקטור` mid-word) the turbo model did not produce.
- The `studio` tier (clean dictation — the actual product domain, target
  ≤ 12 %) is **empty**, so the most representative domain is unmeasured. `read`
  (encyclopedic text with transliterated foreign names) is a deliberately
  *harder* proxy than real dictation.

### Latency — `scripts/stream-test.py` (GPU, 0.5 s hop, re-decode of growing buffer)

| Clip | turbo (avg / max · ×RT) | large-v3 (avg / max · ×RT) | slowdown |
|---|---|---|---|
| `read-001` (7.4 s) | 167 / 375 ms · 0.32× | 775 / 1156 ms · 1.46× | ~4.6× |
| `read-017` (29.9 s) | ~343 / ~500 ms · ~0.7× | **2141 / 3688 ms · 4.23×** | ~6× |

One-shot final decode (the inject-on-stop cost): `read-001` turbo **188 ms** vs
large-v3 **1156 ms**; a ~30 s take finalises in ~0.5 s on turbo vs **~2.8 s** on
large-v3.

large-v3's re-decode cost **grows with buffer length** (32 decoder layers vs
turbo's 4 over the same 30 s mel window); turbo stays roughly flat and
sub-real-time. Consequences for streaming (per ADR-0035):

- large-v3 exceeds the **500 ms hop** even on a short clip (775 ms) and reaches
  **2–3.7 s** on longer takes — partials would lag **> 1 s** behind speech,
  violating the Tier-A "< 1 s lag" target.
- At 2.1 s avg / 3.7 s max it crosses ADR-0035's **2.5 s self-disable budget**,
  so streaming would **switch itself off mid-take** on longer utterances and
  fall back to one-shot. Turbo's speed is what makes live streaming viable; the
  streaming story already flagged this.

### Size

`model.bin`: turbo **1.62 GB** → large-v3 **3.09 GB** (**1.91×**, +1.47 GB) added
to every installer download and to first-run disk.

## Decision

**Keep turbo.** Against the release caveat — `v1.0.0-beta.1` is tagged, and a
mid-beta model swap demands a *clear, measured Hebrew-WER win* to justify the
risk, the larger download, and the slower decode — the evidence does the
opposite:

- the Hebrew-WER win is **marginal** (−1.7 pp on a harder-than-dictation proxy,
  with the product-domain `studio` tier unmeasured), and **negative** on
  code-switching;
- the cost is **severe**: 4.6–6× slower decode that **breaks the streaming
  feature shipped in this same beta**, plus a near-doubled download.

This is not the clear win the bar requires. Turbo stays the default.

## Consequences

- **No change to the shipped model, manifests, CUDA pinning, or installer.**
  The `faster-whisper` / `ctranslate2` / cuDNN set and the SHA-256 integrity
  check are untouched. `cargo test --workspace` stays green (the change set is
  Python-only).
- **The model-selection seam is kept.** `load_engine(model_id=…)` /
  `FasterWhisperEngine(model_id=…)` and the `$LASHON_STT_MODEL_ID` / `--model`
  knobs on the two benchmark scripts remain as reusable A/B-eval infrastructure
  — this is what made the comparison reproducible. The shipped hot path never
  passes a non-default id.
- **The `large-v3-ct2` manifest entry is reverted**, not committed: a
  rejected, installer-bundled 3 GB model would needlessly bloat the download.
  Re-running the eval is a documented three-step reproduction (below).
- The large-v3 conclusion is **streaming-coupled**, not absolute. Revisit if any
  of these change: (a) the `studio` clean-dictation tier is recorded and shows a
  materially larger gap; (b) streaming is dropped or made optional, removing the
  latency constraint; (c) a Hebrew fine-tune lands that closes the gap at
  turbo-class speed.

### Reproducing the benchmark

```sh
# 1. Fetch the ready CT2 build (Apache-2.0; ~3 GB, gitignored):
services/stt-sidecar/.venv/Scripts/python -c \
  "from huggingface_hub import snapshot_download as d; \
   d('ivrit-ai/whisper-large-v3-ct2', local_dir='models/stt/whisper-large-v3-ct2')"

# 2. Add a matching entry to models/manifests/stt.json
#    (id ivrit-ai-whisper-large-v3-ct2, local_dir models/stt/whisper-large-v3-ct2);
#    `python scripts/verify-models.py --record` fills the SHA-256 set.

# 3. A/B (PYTHONPATH=src; sidecar venv with the `bench` extra):
PYTHONPATH=services/stt-sidecar/src python scripts/wer-bench.py            # turbo
LASHON_STT_MODEL_ID=ivrit-ai-whisper-large-v3-ct2 \
  PYTHONPATH=services/stt-sidecar/src python scripts/wer-bench.py          # large-v3
python scripts/stream-test.py tests/hebrew-corpus/read/read-017.wav --model ivrit-ai-whisper-large-v3-ct2
```
