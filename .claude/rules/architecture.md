---
description: The provider-abstraction seam, the lashon-core / Tauri-shell boundary, the STT sidecar
globs: ["packages/shared-rust/src/**/*.rs", "apps/desktop/src-tauri/src/**/*.rs"]
---

# Architecture

Full design in [`docs/architecture.md`](../../docs/architecture.md). The rules
that constrain changes:

## The provider seam

- Every stage — STT, LLM, TTS, external agents — sits behind a trait. A new
  engine is a new trait implementation plus a settings entry, **never** an edit
  to the dictation FSM or to callers.
- `is_local()` and `supports_hebrew()` are load-bearing, not decoration: the UI
  badges cloud providers and steers Hebrew-capable defaults from them.
  Implement them honestly.
- No code path defaults to cloud, and none binds directly to a vendor
  (`faster-whisper`, a specific API). Bind to the trait.

## The lashon-core / Tauri boundary

- GUI-independent logic lives in `lashon-core` (`packages/shared-rust/`):
  provider clients, the sidecar lifecycle, the dictation FSM. It depends on the
  runtime/networking stack, never on `tauri`.
- `apps/desktop/src-tauri/` is a thin GUI shell. New non-GUI logic goes in
  `lashon-core` — see [ADR-0003](../../docs/adr/0003-core-logic-in-a-tauri-independent-crate.md).

## The STT sidecar

- The Rust core reaches the Python STT sidecar over gRPC on loopback TCP. On
  startup the sidecar prints two fixed-format stdout lines —
  `LASHON_STT_TOKEN=<hex>` then `LASHON_STT_PORT=<port>` — and the core parses
  both ([ADR-0002](../../docs/adr/0002-grpc-loopback-tcp-transport.md),
  [ADR-0010](../../docs/adr/0010-harden-the-stt-sidecar-trust-boundary.md)).
  Those lines are a cross-language contract — change them in `server.py` and
  `lashon-core::sidecar` together or not at all.
- Every gRPC call carries the per-process token as `x-lashon-auth` metadata;
  the sidecar rejects calls without it. The token authenticates the caller —
  the loopback bind only constrains locality.

## Wake word

- Suspend the wake-word detector while Lashon is capturing or speaking —
  otherwise it self-triggers on its own microphone audio and TTS output. The
  `is_capturing` and `is_speaking` gates exist for exactly this.
