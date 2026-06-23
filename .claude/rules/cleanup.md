# Cleanup routine

The repo accumulates large ignored output — Cargo `target/` (tens of GB),
the PyInstaller frozen sidecar, the npm `node_modules`, the Python `.venv`,
and downloaded model weights. Nothing of this is tracked, but it dominates
the on-disk footprint and slows backups, search indexers, and the occasional
`du -sh`.

The routine lives in [`scripts/clean.ps1`](../../scripts/clean.ps1) (Windows)
and [`scripts/clean.sh`](../../scripts/clean.sh) (macOS / Linux). Both expose
the same tiers and the same `--dry-run` switch.

## Tiers

| Tier | What it removes | Recovery cost |
|---|---|---|
| `light` (default) | `target/` via `cargo clean`; empty orphan dirs under `.claude/worktrees/` that aren't in `git worktree list`. | One cold `cargo build` / `cargo test`. |
| `medium` | `light` + `services/stt-sidecar/.venv`, `services/stt-sidecar/{build,dist}`, `apps/desktop/node_modules`, `apps/desktop/.svelte-kit`, `apps/desktop/build`, and the regenerable contents of `apps/desktop/src-tauri/binaries/{lashon-stt,llama-server}` (the `.gitkeep` and llama-server `README.md` stay). | `npm install`, recreate the venv (`python -m venv .venv` + `pip install -e .[cuda]`), re-freeze the sidecar with PyInstaller, re-mirror the llama-server release artefacts per `docs/adr/0025`. |
| `aggressive` | `medium` + `models/stt/` + `models/local-llm/`. | Multi-GB re-downloads on next run (Whisper-large-v3-turbo-ct2 from ivrit-ai, Qwen3-4B GGUF from the official `Qwen/Qwen3-4B-GGUF` repo). |

## What never gets touched

- Anything tracked by git (`git ls-files`). The script never reaches into
  committed sources.
- Committed model files: `models/wake/wakewords/hey_lashon.onnx`,
  `models/wake/openwakeword/*`, `models/vad/silero-vad-v5/*`. These are
  Apache-2.0 / MIT and ship in the installer per
  [`docs/adr/0015`](../../docs/adr/0015-vad-end-of-utterance-detection.md)
  and [`docs/adr/0016`](../../docs/adr/0016-wake-word-engine.md).
- Registered git worktrees — only zero-byte orphan directories under
  `.claude/worktrees/` are removed. `git worktree list` is the source of
  truth; if a directory is listed there it stays. The `gh-pages` worktree
  used for the marketing site
  ([`.claude/rules/pages.md`](pages.md)) is therefore safe.
- `.env*` files. They are gitignored and the cleanup script never reaches
  for them.

## When to run it

- **Light** — whenever `target/` crosses ~5–10 GB, or any time the repo
  feels heavy. Safe to run in the middle of any session; the next
  `cargo check`/`cargo test` will repopulate the cache.
- **Medium** — after a long branch swap, before a fresh-clone-style
  sanity test of the install steps, or when reclaiming space matters more
  than the next dev loop being instant.
- **Aggressive** — only when you genuinely want the models gone (low disk,
  testing the first-run download path end-to-end, or freeing the drive for
  another workload). Re-download is slow.

## Adding new ignored output to the cleanup paths

If a new build step creates a new ignored directory worth reclaiming
(another sidecar's `dist/`, a new bundled binary set, a third-party tool's
local cache), add it to **both** scripts under the matching tier — keep
the PowerShell and bash variants in lockstep. Update the table above with
the recovery cost in the same PR so the routine stays self-documenting.

Do **not** bake a hardcoded list of files into the script — each tier should
remove a category of artefact (build cache, language environment, model
weight), and a new path joins because it fits a category, not because we
saw it on disk one day.
