# 6. Release packaging: a frozen STT sidecar, runtime CUDA, an unsigned preview

- **Status:** Accepted (signing follow-up deferred — see [ADR-0023](0023-defer-code-signing-for-now.md))
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` (Phase 4 — build, packaging, signing),
  `docs/architecture.md` (CUDA/cuDNN drift); milestone M13

## Context

Post-M2 the app runs only from a development checkout: `lashon-core` spawns the
STT sidecar as `python -m lashon_stt.server` against the repository's `models/`
tree (`sidecar.rs`). A `tauri build` installer would launch the tongue on any
machine, but dictation would fail — a downloaded copy has no Python interpreter,
no sidecar, and no model.

Cutting a first release (`v0.1.0`, a dictation-only preview) needs four things
the roadmap groups under M13: a self-contained STT sidecar, a way to deliver
the ~1.6 GB Hebrew model, a Windows installer, and code signing.

## Decision

### Frozen sidecar, shipped as a Tauri resource

The STT sidecar is frozen with **PyInstaller** into a one-folder bundle
(`services/stt-sidecar/PyInstaller.spec`). One-folder, not one-file: a one-file
build re-extracts its whole payload to a temp directory on every launch.

The bundle ships as a Tauri **bundle resource**, not an `externalBin`:
`externalBin` is for a single executable named per target triple, whereas the
frozen sidecar is a directory (`lashon-stt.exe` + `_internal/`). The Tauri shell
resolves the bundled `lashon-stt.exe` from the resource directory and sets the
`LASHON_STT_SIDECAR` environment variable; `lashon-core::sidecar` already honours
it. When the resource is absent — `tauri dev` — the variable stays unset and the
sidecar runs from Python source, unchanged.

This keeps `lashon-core` independent of `tauri`
([ADR-0003](0003-core-logic-in-a-tauri-independent-crate.md)): the environment
variable is the seam, set by the GUI shell, read by the core.

### Model downloaded on first run

The model is **not** bundled. The Tauri shell points `LASHON_MODELS_ROOT` at a
per-user app-data directory; on first run the sidecar downloads the Apache-2.0
`ivrit-ai` model there, verifying every file against the SHA-256 sums already
recorded in `models/manifests/stt.json` (`model_download.py`). The tongue shows
a "preparing" state until the model is ready.

Bundling the model would add ~1.6 GB to every installer. Download-on-first-run
keeps the installer small at the cost of a one-time download and a working
network on first launch.

### One installer; the CUDA runtime fetched on first run

GPU acceleration needs the NVIDIA cuDNN/cuBLAS runtime — ~1.2 GB of wheels.
Bundling it would inflate the installer to ~900 MB and pin one cuDNN build onto
every machine (the CUDA/cuDNN drift risk in `docs/architecture.md`). Instead,
the release ships **one
~66 MB installer** and the CUDA runtime is fetched on first run, the same way as
the model:

- When an NVIDIA GPU is present (`nvidia-smi` on `PATH`), the sidecar downloads
  the cuDNN and cuBLAS wheels from PyPI — where `pip` fetches them anyway —
  verifies them against `models/manifests/cuda.json`, and extracts the DLLs into
  a per-user `LASHON_CUDA_ROOT` (`cuda_download.py`).
- With no NVIDIA GPU, nothing is downloaded and `load_engine()` runs on the CPU.
- A CUDA download failure is non-fatal — the engine falls back to the CPU.

The wheel versions in `cuda.json` match the `cuda` extra in `pyproject.toml`,
pinned to the cuDNN dispatch DLL inside `ctranslate2`
([ADR-0004](0004-gpu-cuda-runtime-pinning.md)). GitHub Releases therefore only
ever hosts the small installer.

### v0.1.0 ships unsigned, as a pre-release

`CLAUDE.md` forbids shipping an unsigned installer. A Windows code-signing
certificate (Certum, Azure Trusted Signing, or an OV/EV cert) costs money and
takes days-to-weeks of identity validation — it cannot be obtained in time for
this release.

v0.1.0 is therefore published as an **explicitly-marked unsigned GitHub
pre-release**: a deliberate, time-boxed exception taken with the maintainer's
informed consent. It is a preview for early testers, not the signed v1.0 the
roadmap's Phase 4 DoD requires. Windows SmartScreen warns on first run; the
release notes document the bypass. Obtaining a certificate and signing every
binary is the immediate follow-up for v0.1.x.

## Consequences

- The build is a documented two-step: freeze the sidecar, then `tauri build`
  (see `docs/packaging-windows.md`). A fresh checkout cannot `tauri build` until
  the sidecar is frozen into `apps/desktop/src-tauri/binaries/`.
- `pyinstaller` is GPL; it lives in a `build` extra in `pyproject.toml`, never
  the base dependencies, so the CI license scan — which installs only the base
  set — stays clean.
- `paths.py`, `model_registry.py`, and `faster_whisper_engine.py` each gained a
  frozen-aware branch (`sys._MEIPASS`, `LASHON_MODELS_ROOT`, `LASHON_CUDA_ROOT`).
  The run-from-source paths are unchanged, so `tauri dev` and the CI smoke test
  are unaffected.
- Bumping `ctranslate2` or the cuDNN pin stays a coupled change (ADR-0004), and
  now also means refreshing the wheel URLs and hashes in `cuda.json`.
- Auto-update (`tauri-plugin-updater`, `docs/roadmap.md` Phase 4) is **not** in v0.1.0:
  there is nothing to update from yet, and the updater needs its own signing
  key. It returns with a later release.
- The unsigned release is a known deviation from the Phase 4 / v1.0 DoD,
  recorded here so the follow-up — a signed v0.1.x — is not lost.

## Alternatives considered

- **One-file PyInstaller build.** Rejected: re-extracting ~1.5 GB on every
  launch. One-folder trades a larger install tree for instant start-up.
- **Bundle the model in the installer.** Rejected for v0.1.0: a ~1.8 GB
  installer for every user. Revisit when onboarding (M4) gives the download a
  real UI.
- **Bundling the CUDA runtime (one large installer, or a CPU/CUDA pair).**
  Rejected: it inflates the GitHub download to ~900 MB and pins a fixed cuDNN
  build onto every machine. Fetching from PyPI on first run keeps the installer
  small and lets the runtime track the pinned versions.
- **`externalBin`.** Rejected: it expects a single triple-named executable, not
  a one-folder bundle with an `_internal/` tree.
- **A self-signed certificate.** Rejected: it gives no SmartScreen benefit —
  users see the same warning. An unsigned pre-release is at least honest about
  what it is.
