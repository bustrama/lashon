---
description: The Python STT sidecar — gRPC contract, frozen-binary paths, CUDA pinning
globs: ["services/stt-sidecar/**"]
---

# STT sidecar

`lashon_stt` is a Python gRPC service the Rust core spawns. See
[`docs/architecture.md`](../../docs/architecture.md) and ADRs 0002, 0004, 0006,
0010.

## Transport

- The sidecar binds gRPC to `127.0.0.1` on an ephemeral port and prints a
  two-line stdout handshake — `LASHON_STT_TOKEN=<hex>` then
  `LASHON_STT_PORT=<port>`; the Rust core parses both. Those lines are a
  cross-language contract — changing either is a breaking change to both
  `server.py` and `lashon-core::sidecar`, together.
- The token (minted per process with `secrets.token_hex`) authenticates the
  caller: every RPC must carry it as `x-lashon-auth` metadata, and the sidecar
  rejects calls without it with `UNAUTHENTICATED`. Never log the token or write
  it to disk — it travels only on the stdout pipe
  ([ADR-0010](../../docs/adr/0010-harden-the-stt-sidecar-trust-boundary.md)).

## Frozen-binary awareness

- The sidecar runs both from Python source (`tauri dev`) and frozen with
  PyInstaller (release). Path code must handle both: use the `base_dir()` /
  `sys._MEIPASS` pattern, and honour `LASHON_MODELS_ROOT` / `LASHON_CUDA_ROOT`
  when set. Never hardcode a path that only works from a source checkout.

## Model integrity

- `ensure_model` SHA-256-verifies every present model file against its manifest
  on every boot, not only after a download — a tampered same-size `model.bin`
  is native code inside `ctranslate2`. Never weaken this back to a size-only
  check ([ADR-0010](../../docs/adr/0010-harden-the-stt-sidecar-trust-boundary.md)).

## CUDA pinning

- `faster-whisper`, `ctranslate2`, and `nvidia-cudnn-cu12` are a CUDA-matched
  set. cuDNN must match the dispatch DLL inside the pinned `ctranslate2`
  ([ADR-0004](../../docs/adr/0004-gpu-cuda-runtime-pinning.md)). Bumping any one
  is a coupled, tested change — also refresh `models/manifests/cuda.json`.
- The CPU path must keep working — the `cuda` extra is opt-in.

## Language detection

- The ivrit-ai STT model's own language detector is collapsed — it reports
  Hebrew for any audio. The engine loads a separate tiny model
  (`DETECTOR_MODEL_ID`) for language ID and forces the result on the decode;
  never reintroduce `language=None` auto-detect on the transcription model
  ([ADR-0009](../../docs/adr/0009-language-detection-via-a-companion-model.md)).
- An explicit `language` passed to `transcribe` bypasses detection — keep that
  path; the WER benchmark and tests rely on it.

## Conventions

- PEP 8; type annotations on function signatures; `pytest` for tests.
- Pin every dependency exactly in `pyproject.toml`. Keep GPL build tools
  (PyInstaller) in an extra, never the base dependencies — the CI license scan
  installs only the base set.
