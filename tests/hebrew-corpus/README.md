# Hebrew test corpus

Audio fixtures and ground-truth transcripts for the STT word-error-rate (WER)
benchmark — see `docs/testing.md`. The benchmark gates CI: a regression past a
gating tier's WER target fails the build.

Audio files are tracked with **Git LFS** (`.gitattributes` covers
`tests/**/*.wav`). Run `git lfs install` once before adding clips.

## Tiers

| Tier | Source | WER target |
|---|---|---|
| `read/`   | CC-licensed Hebrew read speech (Google FLEURS `he_il`) | ≤ 27% |
| `studio/` | Clean Hebrew dictation recorded in a quiet room | ≤ 12% |
| `code-switch` | Hebrew/English code-switching clips, recorded locally in `local/` | ≤ 30%, non-gating |

`read/` is the active corpus; its target is a regression baseline — FLEURS
encyclopedic read speech is a harder domain than dictation, so the absolute WER
is higher than the clean-dictation goal. `studio/` clips are recorded by hand
and gate the benchmark once present. `read/` clips are fetched by
`scripts/fetch-corpus.py`.

The `code-switch` tier never gates CI — it is an informational baseline. Its
clips live in `local/`, which is git-ignored: the recordings stay off GitHub
and only the transcripts in `manifest.json` are committed, so the benchmark
skips the tier wherever the clips are absent. It transcribes with language
detection on instead of forcing Hebrew, exercising the companion detector (see
`docs/adr/0009`).

## Recording the `studio/` tier

For each sentence in `manifest.json` under the `studio` category:

- Record in a quiet room, at a natural pace.
- Mono **WAV** — 16 kHz / 16-bit is ideal; any sample rate is resampled by the
  benchmark.
- Name the files `studio-01.wav` … `studio-12.wav` and place them in `studio/`.

Ten or more clips is enough to run the gate; the full twelve is ideal. The
sentences deliberately mix plain Hebrew, Hebrew/English code-switching, numbers,
questions, and imperative commands.
