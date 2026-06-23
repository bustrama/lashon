# 4. GPU STT runtime: pinned CUDA libraries

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Context source:** `docs/architecture.md` (CUDA/cuDNN drift risk)

## Context

M1 runs faster-whisper on the GPU through `ctranslate2`. Two runtime failures
surfaced during bring-up on Windows — both are exactly the CUDA/cuDNN version
drift flagged as a key risk in `docs/architecture.md`.

1. **`pkg_resources` missing.** Python 3.12 virtual environments no longer
   include `setuptools`, but `ctranslate2` imports `pkg_resources` at import
   time. `pkg_resources` ships only with `setuptools`, and `setuptools` 81+
   removed it.

2. **cuDNN dispatch/sublib mismatch.** `ctranslate2 4.5.0` bundles the cuDNN
   dispatch DLL `cudnn64_9.dll` at version **9.1.0.70**. cuDNN 9 splits into a
   dispatch library plus version-matched sublibraries (`cudnn_ops64_9.dll`, …).
   Installing a newer `nvidia-cudnn-cu12` (9.22) supplied 9.22 sublibraries that
   the 9.1 dispatch could not load. Separately, `ctranslate2` loads cuDNN with
   the legacy Windows DLL search, which consults `PATH` but not
   `os.add_dll_directory` entries.

## Decision

Pinned in `services/stt-sidecar/pyproject.toml`:

- **`setuptools==80.10.2`** — a sidecar *runtime* dependency. It is the last
  series that still ships `pkg_resources`.
- **`nvidia-cudnn-cu12==9.1.0.70`** (in the `cuda` extra) — it must match the
  cuDNN dispatch DLL bundled inside `ctranslate2 4.5.0`.

And in `faster_whisper_engine.py`: `_register_cuda_dll_dirs()` adds the
pip-installed `nvidia/*/bin` directories to **both** `PATH` and
`os.add_dll_directory` before `ctranslate2` is imported.

## Consequences

- **Bumping `ctranslate2` is a coupled change.** A new ctranslate2 release may
  bundle a different cuDNN dispatch version; `nvidia-cudnn-cu12` must be
  re-pinned to match, and the bundled-DLL version re-checked.
- `setuptools` stays pinned `< 81` until `ctranslate2` drops its
  `pkg_resources` import.
- The CPU execution path needs none of this — the `cuda` extra is opt-in, and
  the engine falls back to CPU when GPU libraries are absent.
