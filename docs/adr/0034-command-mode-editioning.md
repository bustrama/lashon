# 34. Command-mode editioning — a free dictation build, command mode compiled out

## Status

Accepted — 2026-06-16. Builds on the open-core direction
([ADR-0032](0032-ship-as-open-core-product.md)): defines two build editions — a
lean dictation-only build and a full build that adds command mode.

## Context

The open-core direction calls for two distinct editions: a **dictation-only
build** (Hebrew STT + injection, nothing more) and a **full build** that adds
**command mode** (LLM dispatch, the tool set, recipes, the local LLM, MCP). The
dictation-only edition needs to be a genuinely smaller, self-contained artifact.
Two requirements:

1. Command mode must be **genuinely absent from the dictation-only binary** —
   not merely hidden behind a setting — so the edition is a real reduction in
   surface and footprint, not a runtime toggle.
2. The two editions must come from **one build configuration axis**, so a single
   codebase produces both without divergent source trees.

## Decision

Gate command mode behind a **`command-mode` Cargo feature** (the umbrella over
the existing `local-llm` + `mcp-server` features), with
`default = ["command-mode"]`.

- **Full build** = default features (current behaviour, unchanged).
- **Dictation-only build** = `cargo build --no-default-features`. The
  `#[cfg(feature = "command-mode")]` code is **never compiled in** — the
  dictation-only `.exe` contains no LLM providers, dispatcher, tools, recipes, or
  llama-server. It is a strictly smaller artifact.
- **Frontend** = a build-time edition flag (`VITE_LASHON_EDITION=free|full`)
  tree-shakes the command-mode UI — **including its Hub sections** — out of the
  dictation-only bundle, so the surface is gone too; and the Tauri commands those
  panels call don't exist in the dictation-only binary anyway (belt and
  suspenders).
- Two build profiles → two installers. The dictation-only one is materially
  lighter (no ~80 MB llama-server, no 2.5 GB Qwen download, fewer deps).

**The source stays fully open (GPL-3.0-only).** Both editions build from the
same public repository; the editions differ only in which Cargo features are
enabled at build time. Reproducing the full build requires a Tauri + Rust + CUDA
+ llama-server toolchain, so the dictation-only installer is the practical
default for users who do not build from source.

## What is gated vs kept

| Layer | Kept (dictation-only) | Gated behind `command-mode` |
|---|---|---|
| **lashon-core** | audio, vad, sidecar, stt, inject, wake, hotkey, hardware, model, transcript, keychain | command_mode, llm (+ providers), llama_server, tool, tools/, recipes/, mcp/, the 2 bins |
| **Tauri shell** | dictation (inject path), wakeword (dictation slot), core commands | command_mode.rs, llm.rs, recipes.rs; command-hotkey handlers; command wake-slot; ~22 `invoke_handler` entries; the 2 `.manage()` states |
| **Frontend / Hub** | tongue (dictation), Hardware, Language, dictation hotkey, wake **dictation** slot, Voice-corrections | **Hub:** LLM section, Recipes tab, MCP tab, command/chat hotkeys, wake **command** slot; tongue command + chat modes |

## Consequences

- Command mode is genuinely absent from the dictation-only binary — there is no
  command-mode code in it to enable or patch.
- The dictation-only installer is materially leaner (dictation only), giving a
  real "lightweight Hebrew dictation" build for users who want nothing more.
- The two editions stay cleanly separated by a single feature flag.
- Building the full edition from source requires the full toolchain; the
  dictation-only installer remains the practical default for non-builders.

## Alternatives considered

- **Hide command mode behind a runtime setting** — rejected: the code still
  ships in the dictation-only binary and could be force-enabled, so the edition
  would not be a genuine reduction in surface or footprint.
- **Full source closure** (command mode in a private closed repo; revert the
  core to a permissive licence so a proprietary module can link it; split the
  codebase) — rejected: it unwinds the GPLv3 relicense, makes the most
  privacy-sensitive feature (screen-reading / app-control) unauditable, and is a
  significant codebase split — whereas a build-time feature flag achieves the
  edition split with the source staying fully open and auditable.
