---
description: Privacy and security invariants — secrets, telemetry, the local-first posture
globs: ["**/*"]
---

# Security & privacy

Lashon is local-first and privacy-respecting by construction. These are hard
invariants — never trade them for convenience.

## Never

- Never commit API keys, tokens, or any secret. Keys live only in the OS
  keychain (`keyring`); `.env` files are git-ignored.
- Never log transcript content, audio, or PII — not even at debug level.
  - **Documented exception:** the Command-mode dispatcher honours
    `LASHON_DEBUG_TOOL_ARGS=1` to log tool arg values + result content
    for debugging "the model said it worked but nothing happened"
    failures (see `lashon_core::command_mode::debug_tool_args_enabled`).
    The flag is off by default and must stay off in shipped builds. A
    new tool that wants similar opt-in verbosity should reuse this flag
    rather than inventing its own — one knob, one risk surface.
- Never make a stage default to a cloud provider. Cloud is always opt-in and
  always badged.
- Never bundle CC-BY-NC or CPML models in the installer — those are opt-in
  downloads with a non-commercial badge.
- Never ship a release installer without code signing. (The unsigned `v0.1.0`
  preview was an explicit, recorded exception —
  [ADR-0006](../../docs/adr/0006-release-packaging-and-signing.md).)
- No telemetry by default.

## Licensing

- Lashon's own code is **GPL-3.0-only** (the open-core relicense — see
  [ADR-0032](../../docs/adr/0032-ship-as-open-core-product.md) and
  [`NOTICE`](../../NOTICE)). The paid binary is a signed build of this same
  GPLv3 source; the value is signing + notarization + auto-update + support,
  not closed code.
- The CI license scan (`cargo deny`, `pip-licenses`) must stay green. The hard
  bars are now **AGPL** (its network copyleft exceeds our terms) and
  **CC-BY-NC** (non-commercial — it would forbid selling the binary).
  Dependencies should stay GPLv3-compatible; prefer permissive (MIT / BSD /
  Apache) crates — which is what the `deny.toml` allow-list still encodes.
- Keep GPL / AGPL **build-only** tools (e.g. PyInstaller) out of the shipped /
  base dependency set — they may run in CI or local builds but must never be
  redistributed in the installer or linked into the app.
- Models: **CC-BY-NC** and CPML model weights are never bundled — opt-in,
  badged downloads only (unchanged).
