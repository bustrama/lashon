# 1. Application stack: Tauri 2 + SvelteKit 5 + Rust core, Python ML sidecars

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Context source:** `docs/architecture.md`, `docs/tech-stack.md`

## Context

Lashon is a local-first desktop voice assistant for Windows (primary), macOS,
and Linux from a single codebase. The build has demanding, partly conflicting
requirements:

- An always-on-top, transparent, borderless overlay widget (the Tongue).
- Global hotkeys, a tray icon, autostart, and signed auto-update.
- Latency-critical audio capture, voice-activity detection, wake-word
  detection, and system-wide text injection — sub-800 ms end-to-end.
- GPU-accelerated Hebrew ML inference (STT, optionally TTS).
- A small installer and a modest memory footprint.
- Code signing and notarisation across three operating systems.

A stack must be chosen before any code is written, because it determines the
language of the latency-critical core, the CI matrix, and the contributor
toolchain.

## Decision

- **Application shell:** Tauri 2.
- **Frontend:** SvelteKit 5 with TypeScript.
- **Core logic:** Rust (audio, VAD, hotkeys, injection, the FSM, the provider
  mux, the tool runner).
- **ML inference:** Python sidecars, spawned as child processes and reached
  over gRPC.

## Rationale

### Tauri 2 over Electron

- **Installer and memory.** Tauri uses the OS webview (WebView2 / WKWebView /
  WebKitGTK) instead of bundling Chromium — single-digit-MB installers and a
  much smaller resident footprint, both of which matter for an always-running
  background app.
- **One language for the hot path.** The core is Rust — the same language as
  our latency-critical audio, VAD, and injection code. There is no JavaScript
  ↔ native boundary in the critical loop.
- **Batteries included.** First-class plugins for global shortcuts, tray,
  updater, and autostart — exactly the surface Lashon needs.
- **Trade-off.** Webview rendering differs slightly per OS. Mitigated by the
  RTL / injection regression matrix in `docs/testing.md`.

### SvelteKit 5 over React

- **Compiler, not virtual DOM.** Svelte compiles to small, direct DOM updates —
  this matters for a 60 fps audio waveform animating inside a tiny always-on-top
  window.
- **Runes.** Svelte 5's fine-grained reactivity suits streaming UIs (live
  partial transcripts, streamed LLM tokens).
- **RTL ergonomics.** `dir="auto"` and logical CSS give a clean right-to-left
  story with less ceremony than React for an app of this size.
- **Trade-off.** A smaller ecosystem than React — acceptable, since Lashon's
  component surface (Tongue, waveform, panels) is bespoke regardless.

### Python ML sidecars over pure-Rust inference

- **The Hebrew STT stack is Python-native.** faster-whisper / ctranslate2, the
  ivrit-ai models, silero-vad, and HuggingFace `transformers` (for MMS/XTTS)
  are best supported in Python. Reimplementing them in Rust would be a research
  project and would lag upstream Hebrew model releases.
- **Crash isolation.** A sidecar crash cannot take down the UI; the ML process
  can be killed and reloaded independently.
- **A replaceable seam.** The sidecar boundary is a gRPC contract
  (`packages/proto`). A future pure-Rust STT provider can replace it behind the
  same `STTProvider` trait without touching callers.
- **Trade-off.** The release ships a Python runtime (frozen with PyInstaller):
  a larger installer and exposure to CUDA/cuDNN/ctranslate2 version drift.
  Mitigated by pinning exact DLLs and documenting required versions
  (the risks table in `docs/architecture.md`).

## Alternatives considered

- **Electron + React + Node core.** Rejected: large installer, and the
  latency-critical code would land in Node/native addons rather than a single
  fast core language.
- **Pure-Rust including ML inference (e.g. `candle`).** Rejected for v1: too
  much research risk, and it would forfeit access to Hebrew-specific Python
  models. Left open as a future provider behind the trait seam.
- **A separate native overlay process.** Rejected: Tauri already provides a
  transparent always-on-top window; a second process is unwarranted complexity.

## Consequences

- Contributors need three toolchains: Rust, Node, and Python.
- CI runs a three-OS matrix (`windows-2022`, `macos-14`, `ubuntu-24.04`) from
  Milestone M0 onward.
- The provider-trait seam (`docs/architecture.md`) is mandatory: no caller may bind
  directly to faster-whisper or to any single vendor.
- The Rust ↔ Python boundary needs a transport — decided separately in
  [ADR-0002](0002-grpc-loopback-tcp-transport.md).
