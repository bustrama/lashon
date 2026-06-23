# Lashon · לָשׁוֹן

**Speak Hebrew, see it typed — anywhere. Speak a command, watch your PC do it.
All on your own machine.**

Lashon (לָשׁוֹן — "tongue / language") is a local-first, **Hebrew-first** voice
assistant for the desktop. Most dictation tools treat Hebrew as an afterthought
bolted onto an English product. Lashon is built the other way around.

- **Hebrew is the product, not a setting.** A Hebrew-specialized speech model
  and an RTL-native interface — right-to-left ordering, combining marks, and
  mixed Hebrew/English (code-switching) handled correctly, everywhere you type.
- **Fully local. Private by construction.** Speech recognition, language
  models, and speech synthesis all run **on-device** by default. Your audio
  never leaves your machine — and because Lashon is open source under the GPL,
  you can audit exactly what it does. No telemetry. Cloud providers exist only
  as opt-in adapters, each marked with a clear "cloud" badge.
- **More than dictation — it operates your PC.** Beyond typing what you say,
  Lashon understands spoken commands and acts on them: a voice-driven command
  mode plus scriptable recipes that drive your foreground app, all hands-free.

Windows-first, open source, and yours to inspect.

> **Status:** solo-maintained. Issues and bug reports are welcome; external pull
> requests are not accepted — see [Contributing](#contributing) below.

---

## What it does

Lashon turns speech into text and action on your own machine, in three modes:

- **Dictation** — hold a hotkey (or go hands-free with VAD endpointing and an
  optional "Hey Lashon" wake word), speak Hebrew, and the text appears in the
  focused app with correct right-to-left ordering.
- **Command** — speak a natural-language command; Lashon operates your PC,
  short-circuiting common intents through fast, deterministic recipes.
- **Chat** — ask a question; Lashon answers, by voice.

Speech recognition, language models, and speech synthesis all run **locally**
by default. Cloud providers exist only as opt-in adapters, each marked with a
clear "cloud" badge. No transcripts, audio, or telemetry leave the machine
without explicit consent.

## Install

No installer is published yet — for now Lashon is built from source (see
below); a signed Windows installer is on the way. Watch the
[Releases page](https://github.com/bustrama/lashon/releases) for the first build.

On **first run** Lashon downloads the ~1.6 GB Hebrew speech model; on an NVIDIA
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

Lashon is built in three phases:

1. **Dictation** — Hebrew speech-to-text with system-wide injection. *(current)*
2. **PC operation** — voice-driven command mode, plus delegation to external
   coding agents.
3. **Voice response** — Hebrew-perfect text-to-speech for confirmations and chat.

Dictation and command mode are built and working; the current focus is
packaging and a signed installer for the first release.

The full roadmap — scope, milestones, and per-phase workstreams — lives in
[`docs/roadmap.md`](docs/roadmap.md). Active work is tracked as stories in
[`docs/stories/`](docs/stories/).

## Contributing

Lashon is a **solo-maintained** project. **External pull requests are not
accepted and will not be reviewed** — please don't spend effort on a PR, as it
won't be merged. **Bug reports and issues are very welcome**, though: if
something is broken or behaves wrong (especially around Hebrew), please
[open an issue](https://github.com/bustrama/lashon/issues). The source is
GPL-3.0-only, so you're also free to fork and modify it for your own use.

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full policy and the internal
development workflow.

## License

[GPL-3.0-only](LICENSE) © 2026 Lashon contributors.

Lashon is free software: you may redistribute and/or modify it under the terms
of **version 3 of the GNU General Public License** as published by the Free
Software Foundation. It is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; see [`LICENSE`](LICENSE) for the full terms.

Bundled and optional third-party components retain their own licenses; see
[`NOTICE`](NOTICE). Only MIT/Apache-licensed models ship in the installer;
non-commercially-licensed models are surfaced as clearly-badged opt-in
downloads, never bundled.
