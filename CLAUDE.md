# Lashon — context for Claude Code

Local-first, Hebrew-first desktop voice assistant: STT → PC operation → TTS,
shipped as a signed cross-platform app.

## Authority

There is no monolithic plan. Each concern has a focused, living doc, kept
current with the code:

- `docs/architecture.md` — system design, the provider abstraction, risks
- `docs/providers.md` — the STT / LLM / TTS / agent catalog
- `docs/tech-stack.md` — stack composition and hardware tiers
- `docs/design-system.md` — UI / UX
- `docs/testing.md` — testing strategy and performance budgets
- `docs/soul.md` — Lashon's identity
- `docs/roadmap.md` — phases, the fourteen milestones (M0–M13), workstreams
- `docs/stories/` — active and upcoming work units
- `docs/adr/` — architecture decision records

Each doc is authoritative for its own area. `.claude/rules/` holds the working
rules — conventions, pitfalls, and the security invariants — as short,
glob-scoped files; consult the ones matching the files you are editing. Keep
this file short; it is a pointer, not a copy.

## Current state

Lashon is **feature-complete through M9** (Hebrew dictation, command mode, and
recipes) and is now being packaged to ship as an **open-core product**: free
**GPL-3.0-only** source plus a paid one-time *signed* binary of that same
source, **Windows-first** for v1.0, never a subscription.

- **Why open-core / Windows-first:** [ADR-0032](docs/adr/0032-ship-as-open-core-product.md),
  [ADR-0033](docs/adr/0033-focus-on-windows-for-v1.md) - signing is back on the
  critical path (supersedes ADR-0023).
- **Forward plan:** [`docs/roadmap.md`](docs/roadmap.md) - **decisions:** [`docs/adr/`](docs/adr/)

Shipped: M0-M9, plus unsigned pre-releases v0.1.0-v0.6.0 and the open-core
**v1.0.0** — the free, dictation-only, Windows, unsigned edition (in-app
auto-update + cross-OS installers landed in v0.6.0). The next release is
**v1.1.0** (Windows): single-instance enforcement and persistent diagnostic
logging that harden the shipped build. Code-signing is still the next major
step. The full milestone-by-milestone dev narrative is kept off-repo.

## Repository layout

- `apps/desktop/` — Tauri 2 + SvelteKit 5 app. `src-tauri/` is the Rust GUI
  shell; `src/` is the SvelteKit frontend.
- `packages/shared-rust/` — the `lashon-core` crate: GUI-independent logic
  (provider clients, sidecar lifecycle), fully unit-tested. New non-GUI logic
  goes here, not in the Tauri crate.
- `packages/proto/` — shared `.proto` contracts (`stt.proto`, `tts.proto`).
- `services/stt-sidecar/` — Python gRPC sidecar (`lashon_stt`).
- `docs/` — developer documentation (`architecture.md`, `providers.md`,
  `tech-stack.md`, `design-system.md`, `testing.md`, `soul.md`, `roadmap.md`),
  plus `stories/` (work units) and `adr/` (decision records). **Not
  published to the website** — the public site lives on the `gh-pages`
  branch (see `.claude/rules/pages.md`).

A Cargo workspace at the repo root ties the two Rust crates together.

## Tech stack — versions pinned exactly (no `^`, `~`, `*`)

- Toolchains: Rust 1.95.0 (`rust-toolchain.toml`), Node 20 LTS (`.nvmrc`),
  Python 3.11–3.12.
- Shell: Tauri 2.11.2. Frontend: SvelteKit 2.60.1 / Svelte 5.55.7 /
  TypeScript 5.9.3 / Vite 8.0.13.
- gRPC: tonic 0.12.3 / prost 0.13.5 (Rust); grpcio 1.68.1 (Python).
- STT: faster-whisper 1.1.1 / ctranslate2 4.5.0 on CUDA (cuDNN 9.1.0.70); the
  model is ivrit-ai/whisper-large-v3-turbo-ct2 (downloaded, never committed).
- Exact versions live in the `Cargo.toml`s, `package.json`, and
  `pyproject.toml`; lockfiles are committed.

## Run commands

```sh
# desktop app — run from apps/desktop
npm install
npm run tauri dev          # launches the tongue window
npm run check              # svelte-check (type-check)
npm run build              # frontend production build

# Rust — run from the repo root
cargo check --workspace --all-targets
cargo test --workspace     # lashon-core unit tests

# end-to-end sidecar smoke test (needs the Python env)
cargo test -p lashon-core --test healthcheck -- --ignored

# STT sidecar standalone — run from services/stt-sidecar
python -m lashon_stt.server   # needs PYTHONPATH=src, or `pip install .`

# STT model + WER benchmark — run from the repo root
python scripts/verify-models.py --download   # fetch the Hebrew STT model
python scripts/wer-bench.py                  # transcribe the corpus, score WER
```

## Before you start, read

1. `docs/architecture.md` — system design and the provider abstraction.
2. `docs/roadmap.md` — phases, milestones, and per-phase workstreams.
3. `docs/adr/` — every architectural decision and its rationale.
4. `docs/soul.md` — Lashon's identity.
5. `CONTRIBUTING.md` — branch model, milestone DoD, commit conventions.
