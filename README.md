# Ottid · אוטיד

**Speak Hebrew, see it typed — anywhere. Speak a command, watch your PC do it.
All on your own machine.**

Ottid (אוטיד) is a local-first, **Hebrew-first** voice
assistant for the desktop. Most dictation tools treat Hebrew as an afterthought
bolted onto an English product. Ottid is built the other way around.

- **Hebrew is the product, not a setting.** A Hebrew-specialized speech model
  and an RTL-native interface — right-to-left ordering, combining marks, and
  mixed Hebrew/English (code-switching) handled correctly, everywhere you type.
- **Fully local. Private by construction.** Speech recognition, language
  models, and speech synthesis all run **on-device** by default. Your audio
  never leaves your machine — and because Ottid is open source under the GPL,
  you can audit exactly what it does. No telemetry. Cloud providers exist only
  as opt-in adapters, each marked with a clear "cloud" badge.
- **More than dictation — it operates your PC.** Beyond typing what you say,
  Ottid understands spoken commands and acts on them: a voice-driven command
  mode plus scriptable recipes that drive your foreground app, all hands-free.

Windows-first, open source, and yours to inspect.

> **Status:** solo-maintained. Issues and bug reports are welcome; external pull
> requests are not accepted — see [Contributing](#contributing) below.

---

## What it does

Ottid turns speech into text and action on your own machine, in three modes:

- **Dictation** — hold a hotkey (or go hands-free with VAD endpointing and an
  optional "Hey Lashon" wake word), speak Hebrew, and the text appears in the
  focused app with correct right-to-left ordering.
- **Command** — speak a natural-language command; Ottid operates your PC,
  short-circuiting common intents through fast, deterministic recipes.
- **Chat** — ask a question; Ottid answers, by voice.

Speech recognition, language models, and speech synthesis all run **locally**
by default. Cloud providers exist only as opt-in adapters, each marked with a
clear "cloud" badge. No transcripts, audio, or telemetry leave the machine
without explicit consent.

## Install

Download the installer from the
[latest release](https://github.com/bustrama/ottid/releases/latest): the file
ending in `_x64-setup.exe`. If you'd rather not install, the release also has
a portable `.zip`. Releases up to v1.1 carry Ottid's old name, Lashon
(`Lashon_1.1.0_x64-setup.exe`).

- **Windows only, for v1.x.** There are no macOS or Linux builds yet.
- **Code signing.** Releases up to v1.1 aren't code-signed, so Windows
  SmartScreen stops them with "Windows protected your PC". Click **More
  info**, then **Run anyway**. Later releases are signed
  ([ADR-0043](docs/adr/0043-sign-windows-releases-with-azure-artifact-signing.md)).
  You can always read the source, or build it yourself (below).
- **The free dictation edition.** The published build does dictation only.
  Command mode and recipes are left out of it; a build from source includes
  them.
- **Upgrading from Lashon?** The first release under the Ottid name installs
  over it: its installer removes the old app and keeps your settings, history
  and downloaded models.

On **first run** Ottid downloads the ~1.6 GB Hebrew speech model; on an NVIDIA
GPU it also fetches the CUDA runtime for faster transcription. After that it
works offline. Press **Ctrl+Space**, speak Hebrew, then pause — the text is
pasted into the focused app.

## Run from source

**Prerequisites:** Rust 1.95, Node 20+, Python 3.11–3.12, and a WebView2
runtime (Windows; bundled by the OS on Windows 11).

```sh
# desktop app (Tauri 2 + SvelteKit 5)
cd apps/desktop
npm install
npm run tauri dev
```

To build the installers yourself, see
[`docs/packaging-windows.md`](docs/packaging-windows.md).

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the development workflow and
[`docs/architecture.md`](docs/architecture.md) for the system design.

## Roadmap

Ottid is built in three phases:

1. **Dictation** — Hebrew speech-to-text with system-wide injection. *(current)*
2. **PC operation** — voice-driven command mode, plus delegation to external
   coding agents.
3. **Voice response** — Hebrew-perfect text-to-speech for confirmations and chat.

Dictation and command mode are built and working, and the dictation edition
ships as a Windows installer. The current focus is code-signing it.

The full roadmap — scope, milestones, and per-phase workstreams — lives in
[`docs/roadmap.md`](docs/roadmap.md). Active work is tracked as stories in
[`docs/stories/`](docs/stories/).

## Diagnostic logs

If Ottid errors or crashes, its diagnostic log is the most useful thing to
attach to a bug report. Open the tray menu (or right-click Ottid) and
choose **יומני אבחון · Open logs folder** — on Windows the logs live under
`%LOCALAPPDATA%\app.ottid.desktop\logs`. They record only structural
diagnostic events — start-up, hardware tier, errors, timings — and, by design,
**never** contain your transcribed text, audio, or prompts.

## Contributing

Ottid is a **solo-maintained** project. **External pull requests are not
accepted and will not be reviewed** — please don't spend effort on a PR, as it
won't be merged. **Bug reports and issues are very welcome**, though: if
something is broken or behaves wrong (especially around Hebrew), please
[open an issue](https://github.com/bustrama/ottid/issues). The source is
GPL-3.0-only, so you're also free to fork and modify it for your own use.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full policy and the internal
development workflow.

## License

[GPL-3.0-only](LICENSE) © 2026 Ottid contributors.

Ottid is free software: you may redistribute and/or modify it under the terms
of **version 3 of the GNU General Public License** as published by the Free
Software Foundation. It is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; see [`LICENSE`](LICENSE) for the full terms.

Bundled and optional third-party components retain their own licenses; see
[`NOTICE`](NOTICE). Only MIT/Apache-licensed models ship in the installer;
non-commercially-licensed models are surfaced as clearly-badged opt-in
downloads, never bundled.
