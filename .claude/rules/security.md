---
description: Privacy and security invariants — secrets, telemetry, the local-first posture
globs: ["**/*"]
---

# Security & privacy

Ottid is local-first and privacy-respecting by construction. These are hard
invariants — never trade them for convenience.

## Never

- Never commit API keys, tokens, or any secret. Keys live only in the OS
  keychain (`keyring`); `.env` files are git-ignored.
- Never log transcript content, audio, or PII — not even at debug level.
  - **Documented exception:** the Command-mode dispatcher honours
    `OTTID_DEBUG_TOOL_ARGS=1` to log tool arg values + result content
    for debugging "the model said it worked but nothing happened"
    failures (see `ottid_core::command_mode::debug_tool_args_enabled`).
    The flag is off by default and must stay off in shipped builds. A
    new tool that wants similar opt-in verbosity should reuse this flag
    rather than inventing its own — one knob, one risk surface.
- Never make a stage default to a cloud provider. Cloud is always opt-in and
  always badged.
- Never bundle CC-BY-NC or CPML models in the installer — those are opt-in
  downloads with a non-commercial badge.
- Never ship a release installer without code signing. `release.yml` enforces
  it: a release run fails unless both the Authenticode and the updater
  signature are configured, and an unsigned build is possible only as an
  explicitly marked pre-release
  ([ADR-0043](../../docs/adr/0043-sign-windows-releases-with-azure-artifact-signing.md)).
  v0.1.0, v1.0.0 and v1.1.0 shipped unsigned as recorded exceptions
  ([ADR-0006](../../docs/adr/0006-release-packaging-and-signing.md),
  [ADR-0039](../../docs/adr/0039-ship-v1-unsigned-windows-only.md)).
- No telemetry by default.

## Inbound listeners

The Claude Code hooks bridge is the only thing that connects *to* Ottid
([ADR-0050](../../docs/adr/0050-claude-code-hooks-bridge.md)). It, and any
listener after it, keeps these invariants.

- **Never a TCP port.** Only a pipe or socket the OS restricts to the user:
  - Windows: a named pipe with an explicit DACL for the user's SID only,
    `FILE_FLAG_FIRST_PIPE_INSTANCE` and `PIPE_REJECT_REMOTE_CLIENTS`.
  - Unix: a 0600 socket in a 0700 directory the user owns.
- **A per-process token,** kept only in memory and in a user-only file
  (Windows DACL / Unix 0600), deleted when the listener stops.
  - Never log it, and never put it in an environment variable, a URL, a
    command line, or another program's settings.
  - It never crosses the wire: both sides prove they hold it with an HMAC
    over fresh nonces, checked in constant time.
- **Caps on everything:** each frame's size (checked before the body is
  read), every read before the user's turn, and open connections.
- **Never log a request's content.** Log only an id, the tool's name, the
  decision and the timing. Never the tool's input, its command line or its
  folder.
- **Fail toward the caller's own prompt.** The hook client never allows on
  its own, and never denies because something broke. It prints nothing, and
  Claude Code asks as it always did. It gives up before Claude Code's hook
  timeout.
- **Listen only when the user opted in.** The bridge runs only while Ottid's
  hook is installed.
- **Edit another program's settings only on consent.** The user first sees
  the exact change, and it is applied only to the file as previewed. Back the
  file up first, and keep every unrelated key byte for byte.

## Licensing

- Ottid's own code is **GPL-3.0-only** (the open-core relicense — see
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
