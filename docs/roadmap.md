# Roadmap

Lashon is built in three phases, milestone by milestone. This document is the
forward-looking plan: scope, the milestone list, and the per-phase workstream
detail. Active, picked-up work lives as a story in [`stories/`](stories/);
architecture and design specs live in the sibling docs
([`architecture.md`](architecture.md),
[`providers.md`](providers.md), [`design-system.md`](design-system.md),
[`tech-stack.md`](tech-stack.md), [`testing.md`](testing.md)).

> **Pivot note (2026-06): ship as an open-core product.** The next release is
> **`v1.0.0`** — a packaging + release push, not a feature milestone (see
> [ADR-0032](adr/0032-ship-as-open-core-product.md)). **v1.0 is
> Windows-only** ([ADR-0033](adr/0033-focus-on-windows-for-v1.md)); macOS/Linux
> shipping is paused. Code **signing is back on the critical path** (supersedes
> the ADR-0023 deferral). The milestone table below (esp. M10–M13) predates this
> pivot — treat ADR-0032 / 0033 as authoritative for the v1.0 plan.

## Scope

### In scope (v1.0)

- **Phase 1** — Hebrew-perfect dictation engine with system-wide text injection.
- **Phase 2** — Voice-driven PC operation (apps, files, windows, browser, code
  editor) via a tool-use LLM, with delegation to external coding agents
  (OpenCode, Codex, Claude Code, Aider) for heavy tasks.
- **Phase 3** — Hebrew-perfect TTS for command confirmations and chat-mode
  replies, local-first with cloud providers as opt-in.
- Three activation methods: global hotkey, push-to-talk, wake word — all
  toggleable.
- An installable, signed app for Windows/macOS/Linux with auto-update.
- A distinct, beautiful, RTL-native UI/UX.

### Out of scope (v1.0)

- Mobile (iOS/Android).
- Meeting transcription / diarization / live captions.
- Multi-user / team sync.
- Voice-cloning ethics workflows (consent recording, watermarking) — defer to v2.
- Custom-trained Hebrew models from scratch — we use ivrit-ai / Dicta / Piper as-is.
- Computer-vision-based screen understanding — defer to v2 (the obvious Phase 4+).

### Explicit anti-goals

- Cloud-by-default routing.
- Telemetry on transcript content.
- Signing up users / accounts / sync servers.
- A Wispr Flow visual clone — Lashon is aesthetically distinct.

## Milestones

Each milestone is one feature branch and one PR; it merges only when its
Definition of Done is met and CI is green on all three runners (see
[`../CONTRIBUTING.md`](../CONTRIBUTING.md)).

| Milestone | Scope | Status |
|---|---|---|
| **M0** — Bootstrap | Repo scaffold, CI green on three OSes, "Hello Lashon" Tauri window with a Hebrew greeting | ✓ Done |
| **M1** — Hebrew STT pipeline | Hebrew sample WAV → transcript on disk, WER ≤ 12% on the test corpus | ✓ Done |
| **M2** — Hotkey + injection | From any app, push-to-talk Hebrew → text at the cursor with correct RTL ordering; clipboard preserved | ✓ Done |
| **M3** — Tongue UI minimum | Always-on-top tongue with idle / listening / transcribing / error states at 60 fps; drag-to-snap | ✓ Done |
| **M4** — Onboarding + settings | Fresh install → 4-min onboarding → first successful Hebrew dictation; settings persist; i18n he+en | ✓ Done |
| **M5** — STT device by tier | The hardware tier sets the STT device mode — tiers A/B probe the GPU, C/D run on the CPU; a user tier override is now honoured. (Redefined — the original LLM-cleanup M5 was cut.) | ✓ Done |
| **M6** — Wake word | Default "Hey Lashon" at ≤ 1 false activation/hour; in-app Hebrew wake-word trainer wizard | ✓ Done |
| **M7** — Provider mux foundation | STT + LLM + TTS trait abstractions; cloud providers plumbed; keychain key storage; Settings UI for switching | ✓ Done |
| **M8** — Tool registry + command mode | Native tool set usable via LLM tool-calls; 20 Hebrew test commands pass; confirmation policy enforced | ✓ Done |
| **M9** — Recipes | `recipe.yaml` schema + parser + validator + 10 starters; runtime executor (Windows-first); intent cascade (regex tier) wired into the Command-mode dispatcher; Hub Recipes tab + Steps panel; `lashon-mcp` stdio server so Claude Desktop / Cursor / any MCP host can author and read recipes; STT word-aliases. **Phases 1a–1d + 1g shipped; tier 2/3 cascade + Hub Creator UI deferred.** (Redefined — the original "External agent delegation" M9 is re-scoped for a later milestone.) | ✓ Done on `main` (PRs #71/72/74/75/77/78/79/81; ADRs 0027/0028) |
| **M10** — TTS pipeline | Piper local default; streaming sentence pipeline; audio ducking; voice picker. **Phase 3 minimum.** | Planned |
| **M11** — Cloud TTS + advanced local | ElevenLabs, Azure, Cartesia plumbed; optional MMS/XTTS download flow. **Phase 3 DoD met.** | Planned |
| **M12** — Memory + history | Long-term memory (`remember` tool); History tab with audio replay; Memory editor | Planned |
| **M13** — Hardening, signing, installers | Code-signed installers Win/Mac/Linux; auto-update; crash reporting opt-in; comprehensive QA pass. **Phase 4 / v1.0 DoD met.** | Partially done |

**On milestone order.** After M2, an early `v0.1.0` pre-release was cut — the
dictation preview packaged as an *unsigned* Windows installer (the packaging
half of M13; see [`adr/0006-release-packaging-and-signing.md`](adr/0006-release-packaging-and-signing.md)
and [`releasing.md`](releasing.md)). Code signing and the rest of M13's
hardening are deferred; a security review has, however, already hardened the
STT sidecar trust boundary — a per-process gRPC auth token and boot-time
model-integrity verification ([`adr/0010-harden-the-stt-sidecar-trust-boundary.md`](adr/0010-harden-the-stt-sidecar-trust-boundary.md)).
M3 has since shipped, and the next feature work is M4 onward. The **interactive
first-run tutorial** — the "learn how to use Lashon" slice of M4 — also shipped
early, in `v0.2.0` (issue #9; see [`stories/m4-interactive-tutorial.md`](stories/m4-interactive-tutorial.md)
and [`adr/0008-first-run-tutorial-window.md`](adr/0008-first-run-tutorial-window.md)),
and has since gained a first-run warm-up display with byte-level model-download
progress. The **Settings Hub** followed as the second M4 slice — an in-house he+en
localization, a persistent Hub window, and a rebindable dictation hotkey
([`stories/m4-settings-hub.md`](stories/m4-settings-hub.md),
[`adr/0011-localization-architecture.md`](adr/0011-localization-architecture.md)).
A third unsigned pre-release, `v0.3.0`, was then cut — it packages the Settings
Hub together with everything else landed since `v0.2.0`: the M3 tongue UI,
companion-model language detection, and the STT trust-boundary hardening.
The onboarding-hardware slice — mic permission and hardware-tier detection
([`stories/m4-onboarding-hardware.md`](stories/m4-onboarding-hardware.md),
[`adr/0013-onboarding-hardware-detection.md`](adr/0013-onboarding-hardware-detection.md))
— then completed M4. A fourth unsigned pre-release, `v0.4.0`, packages M4's
completion, the portable / all-users distribution, and M5 — the hardware tier
driving STT device selection ([`adr/0014`](adr/0014-stt-device-by-hardware-tier.md)).
A fifth unsigned pre-release, `v0.5.0`, packages M6 — the wake word and audio
overhaul ([`adr/0015`](adr/0015-silero-vad-and-utterance-endpointing.md),
[`adr/0016`](adr/0016-wake-word-engine.md)). Five unsigned pre-releases on,
the **signing** half of M13 is now indefinitely deferred —
see [`adr/0023-defer-code-signing-for-now.md`](adr/0023-defer-code-signing-for-now.md)
for the OSS-economics rationale and the SignPath-reputation / paid-cert
triggers that would unblock it. The other halves of M13 track separately:
in-app auto-update ([`adr/0017`](adr/0017-auto-update-via-tauri-plugin-updater.md))
and cross-OS installers ([`adr/0018`](adr/0018-cross-os-installer-matrix.md))
land in `v0.6.0`.

**Total estimate:** ~60–80 dev-days of focused work for v1.0. UI, Rust core, and
the Python sidecars can advance in parallel after M5.

## Phase 1 — Hebrew dictation engine (MVP foundation)

**Objective:** Hold the hotkey, speak Hebrew, release, and see correctly
inserted Hebrew text in any focused app. Sub-800 ms hotkey-release-to-paste on
Tier A hardware.

**Definition of Done:**

1. Hebrew sample corpus WER ≤ 12% on `tests/hebrew-corpus/`.
2. Mixed Hebrew+English code-switch sentences inserted with correct RTL ordering
   and no combining-mark corruption.
3. Works in: Notepad, Word, Chrome (Gmail compose), Slack, VS Code, Cursor,
   Discord, Telegram Desktop, WhatsApp Desktop.
4. The clipboard is preserved through dictation (any pre-existing text/image
   clipboard survives).
5. Onboarding completes in < 4 min on a clean Win11 (excluding model download).
6. All three activation methods (push-to-talk, toggle, wake word) work and are
   user-configurable.

### 1.1 Audio capture & VAD (Rust core)

- `cpal` 16 kHz mono Float32 input.
- A 30-second rolling ring buffer (`ringbuf` crate).
- **Silero VAD v5** via the `ort` crate (ONNX), 32 ms frames.
- Endpoint logic: clean silence → end-of-utterance; an extended hold if there is
  mid-word energy. Thresholds re-tuned for long-form dictation — 5 s silence /
  6 s hold ([adr/0038](adr/0038-tolerate-long-pauses-in-dictation-endpointing.md);
  originally 500 ms / 1500 ms in [adr/0015](adr/0015-silero-vad-and-utterance-endpointing.md)).
- Suspend the wake-word detector during active capture using a
  `Mutex<bool> is_capturing` gate.

### 1.2 STT provider (Python sidecar)

- The sidecar exposes the gRPC service
  `Stt.{TranscribeBytes, TranscribeStream, HealthCheck}` on a loopback
  transport.
- Engine: `faster-whisper` with `ivrit-ai/whisper-large-v3-turbo-ct2`. The
  device follows the hardware tier (M5,
  [`adr/0014`](adr/0014-stt-device-by-hardware-tier.md)): tiers A/B probe the
  GPU and fall back to CPU; tiers C/D run on the CPU, skipping the CUDA
  runtime. The Tauri shell passes the choice in `LASHON_STT_DEVICE`.
- Model warm-up on sidecar boot; warm inference ≤ 250 ms for 3 s of audio on a
  4080.
- Sanitizer pass: regex-strip `<\|.*?\|>`, `<ctrl\d+>`, `[\x00-\x08\x0b-\x1f]`.
- PyInstaller-aware path helper:
  ```python
  def base_dir():
      return Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent
  ```

### 1.3 Optional cleanup LLM — cut

An optional LLM pass to strip filler words and fix punctuation in transcripts
was prototyped (milestone M5) and then cut. The ivrit-ai Hebrew STT output is
already clean enough that the pass — which needs a multi-gigabyte model
download and a managed local LLM runtime — did not justify its cost, and an
LLM editor tended to paraphrase rather than lightly correct. Dictation injects
the STT transcript directly.

### 1.4 Global hotkey manager

- `tauri-plugin-global-shortcut` with press/release events.
- Three default chords (configurable):
  - Dictation: `Ctrl+Space` (Win) / `Cmd+Option+Space` (Mac) / `Super+Space` (Linux)
  - Command: `Ctrl+Win+.`
  - Chat: `Ctrl+Win+/`
- Double-tap within 300 ms = toggle mode for the same chord.
- Conflict validator — rejects `Win+L`, `Ctrl+Alt+Del`, and OS-reserved chords.

### 1.5 Wake word

- `openWakeWord` ONNX via `ort` in Rust.
- Bundled default model: `hey_lashon_v1.onnx`, trained on Piper-synthesized
  "Hey Lashon".
- In-app trainer wizard (later milestone) to train a Hebrew "היי לשון".
- A CPU thread, ≤ 25% of one core, with a throttle-on-battery option.
- A 2-consecutive-frame threshold to suppress false positives.

### 1.6 Text injection (Rust)

- Hebrew detection (codepoints U+0590–U+05FF, U+FB1D–U+FB4F) forces the
  **clipboard path** unconditionally.
- Per-injection attempt order:
  1. UIA `TextPattern.SetValue` probe (5 ms timeout) — skipped if Hebrew is detected.
  2. Clipboard snapshot (`EnumClipboardFormats` + per-format save) →
     `SetClipboardData(CF_UNICODETEXT, utf16)` → `SendInput` synthetic `Ctrl+V`
     → restore the clipboard 250 ms later.
  3. Last resort: `KEYEVENTF_UNICODE` per codepoint — never for Hebrew.
- Cross-platform via `enigo` + `arboard`; platform-specific overrides in
  `inject/win.rs`, `inject/mac.rs`, `inject/linux.rs`.
- Per-app injection-profile overrides in settings (some apps need a 50 ms delay
  after `Ctrl+V`).

### 1.7 Tongue UI (states for Phase 1)

- Idle / Listening / Transcribing / Error states (Polishing/Tool/Speaking
  reserved for later phases).
- An always-on-top transparent Tauri window, drag-to-snap edges.
- A 60 fps audio waveform during Listening.
- Full visual spec in [`design-system.md`](design-system.md).

### 1.8 Onboarding

- Welcome → mic permission → hardware-tier detection → model download (ivrit-ai
  turbo ≈ 1.5 GB) → hotkey rebind → live test → done.
- The **mic-permission** and **hardware-tier** steps have shipped as the M4
  onboarding-hardware slice ([`stories/m4-onboarding-hardware.md`](stories/m4-onboarding-hardware.md),
  [`adr/0013-onboarding-hardware-detection.md`](adr/0013-onboarding-hardware-detection.md)).
  They extend the tutorial window: the mic step opens a capture stream to probe
  access (and raise the macOS prompt); the hardware step detects the host's
  tier (`lashon-core::hardware`) and lets the user override it. The detected
  tier persists as `hardware.tier` — wiring it to per-tier model selection is
  M5-and-later work.
- The **interactive tutorial** — a skippable, first-run walkthrough that teaches
  the user how to dictate, ending in a live practice step — has shipped as an
  early slice (issue #9; [`stories/m4-interactive-tutorial.md`](stories/m4-interactive-tutorial.md),
  [`adr/0008-first-run-tutorial-window.md`](adr/0008-first-run-tutorial-window.md)).
  It runs in a dedicated `tutorial` window, is re-openable from the tray, and
  surfaces first-run warm-up with byte-level model-download progress while the
  STT model is fetched and loaded.

## Phase 2 — PC operation (Command Mode + Agent Delegation)

**Objective:** Speak a natural-language command in Hebrew or English and have it
executed on the user's PC. Two paths: native tools (fast, deterministic) and
external-agent delegation (powerful, slower).

**Definition of Done:**

1. 20 representative voice commands (`tests/commands.he.yaml` +
   `commands.en.yaml`) execute correctly with no hallucinated actions.
2. Tool-execution failures speak a clear Hebrew/English error (via Phase 3 TTS
   once available; before that, a tongue error banner).
3. External-agent delegation: speak
   "פתח את claude code ובקש לתקן את הבאג ב-main.rs" → Claude Code spawns in the
   Agent panel with the transcribed prompt.
4. Confirmation-required actions (delete, send, shutdown) **always** prompt
   before execution.
5. Privacy: no transcript or tool call leaves the machine when the active LLM
   provider is local.
6. Action telemetry (`history.db`) captures timestamp, mode, transcript, tool
   name, args, and result — never auto-uploaded.

### 2.1 Command-mode routing

- After STT, classify intent:
  - Wake-prefix detection: `^(לשון|lashon)[,،:\s]` — strip the prefix → Command/Chat.
  - Verb-lexicon match (`open|run|find|send|create|delete|בצע|פתח|מצא|שלח|צור|מחק|…`) → Command.
  - Question pattern (`?|מה|איך|למה|why|how|what`) → Chat.
  - Otherwise → Dictation (the default fallback).
- Override: an explicit hotkey forces the mode regardless of content.

### 2.2 Native tool registry (Rust)

Each tool implements `trait LashonTool` with `name`, `description`, `parameters`
(JSON schema), and `execute(args) -> ToolResult`. Tools register at startup;
schemas are serialized to whatever format the chosen LLM provider needs
(Anthropic, OpenAI, Gemini all differ slightly — an adapter pattern).

Tool list for v1 (schema-only here; full implementation per milestone):

| Tool | Purpose | Notes |
|---|---|---|
| `open_app` | Launch any app by name | Resolves via Windows App Paths registry, Start Menu shortcuts, `which`/`mdfind` on Mac, `.desktop` files on Linux |
| `focus_window` | Bring a window to front by title substring | Win32 EnumWindows + SetForegroundWindow; AXUIElement Mac; wmctrl Linux |
| `close_window` | Close the active or a named window | |
| `type_text` | Type text into the focused field | Uses the Phase 1 injector — supports Hebrew |
| `press_keys` | Hotkey combo | enigo |
| `click_at` / `move_to` | Mouse | enigo |
| `screenshot` | Save a screenshot | Native APIs, returns a file path |
| `volume` / `brightness` / `wifi_toggle` / `bluetooth_toggle` | System controls | Win32 IAudioEndpointVolume, WMI, etc. |
| `lock_screen` / `sleep` / `restart` / `shutdown` | Power | **Always requires confirmation** |
| `clipboard_get` / `clipboard_set` | Clipboard | arboard |
| `file_list` / `file_create` / `file_read` / `file_write` / `file_move` / `file_copy` / `file_delete` | File ops | **delete requires confirmation**; supports path shortcuts: `desktop`, `downloads`, `documents`, `home`, plus Hebrew aliases (`שולחן העבודה`, `הורדות`) |
| `file_search` | Find by name/ext under a root | |
| `open_url` | Default browser | |
| `browser_action` | Pluggable browser controller | Default impl: `playwright-rust` for Chromium-based browsers; a headed instance attached to the user's profile via `--user-data-dir` |
| `web_search` | Search via self-hosted SearXNG (default) or Brave/Tavily/Google APIs (opt-in) | |
| `set_reminder` | Schedule a notification | Win Task Scheduler / launchd / systemd-user |
| `send_message` | WhatsApp/Telegram/Slack | Each platform's protocol/API where possible; UI-automation fallback. **Always confirms the recipient before sending.** |
| `delegate_agent` | Hand off to an external agent (see 2.3) | |
| `read_screen` | OCR the active window | Tesseract (Hebrew) or PaddleOCR for v1; reserved for Phase 4 vision |
| `remember` | Save a fact to long-term memory | See 2.5 |
| `lashon_settings` | Adjust Lashon itself | "use claude opus", "switch to piper", "increase volume" |

### 2.3 External agent delegation

- `delegate_agent({agent: "claude_code" | "opencode" | "codex" | "aider" | "goose", prompt: string, cwd?: string})`.
- Spawns the agent in a `portable-pty` session; attaches stdin/stdout to the
  Agent panel (xterm.js + Svelte).
- Lashon stays voice-active: the user can speak follow-ups → Lashon types into
  the agent's stdin.
- The agent process lives until the user closes the tab or speaks
  "stop the agent" / "סגור את הסוכן".
- The Agent panel supports multiple concurrent agent tabs.
- The tool injects a standard prompt per agent ("you are running inside Lashon,
  the user spoke this: …").

### 2.4 Command-mode LLM loop

- System prompt: identity, available tools, the current OS, a focused-app hint,
  language preference, today's date+time, and recent memory snippets.
- Conversation: user transcript → tool calls → tool results → optional follow-up
  tool calls → a final spoken summary.
- Hard cap: 8 tool calls per command (user-configurable).
- **Confirmation tools:** any tool flagged `requires_confirmation: true` triggers
  a modal in the tongue's expanded view plus a spoken "האם לאשר? כן או לא".
- Tool results stream back into the LLM context until the model returns no more
  tool calls or hits the cap.
- On any tool error: the LLM gets one repair attempt; on a second failure the
  user gets an error toast plus a spoken brief.

### 2.5 Memory (long-term, local)

- A SQLite table `memory(category, key, value, confidence, last_used, source)`.
- The LLM has an implicit `remember(category, key, value)` tool to silently
  persist facts (name, preferences, project names, contact mappings).
- Categories: `identity | preferences | projects | relationships | wishes | aliases | notes`.
- Loaded into command/chat system prompts as
  `[KNOWN FACTS]\n- name: Ofir\n- city: Tel Aviv\n- editor: Cursor\n…`.
- A user-facing memory editor in Hub → Memory tab; can delete individual facts
  and export a full dump.
- **All values are stored in English** for LLM-language portability; the UI
  translates labels for display.

### 2.6 Permissions & confirmation policy

- Confirmation defaults:
  - **Always confirm:** `file_delete`, `send_message`, `shutdown`, `restart`,
    any `lashon_settings` change that switches the active LLM/STT/TTS provider to
    a cloud one, any `file_write` to a system directory.
  - **Never confirm:** `type_text`, `clipboard_*`, `volume`, `web_search`,
    `file_read`, `file_list`.
  - **Configurable:** all others.
- An "auto-confirm for the next 5 minutes" toast option for power users.
- Confirmations are spoken in Hebrew when the UI language is Hebrew, English
  otherwise.

### 2.7 Conversation/agent panel UI

- Slides out from the right edge of the screen, 420 px wide, dismissible.
- Tabs: `Conversation | Agent: claude-code | Agent: opencode | Memory | History`.
- Conversation tab: streaming Hebrew text bubbles, code blocks with a copy
  button, tool-call cards with collapsed JSON, an "audio" play button to replay
  the TTS for any reply.
- Agent tabs: a full-fidelity xterm.js terminal, agent process status, a
  "send transcript" button to push the last STT result as stdin.

## Phase 3 — Voice response (TTS)

**Objective:** Lashon speaks back in natural Hebrew (or other supported
languages) for command confirmations and chat replies. Local-first, cloud
opt-in. Streaming TTS where possible.

**Definition of Done:**

1. Command confirmations spoken with ≤ 400 ms first-byte latency on Tier A (Piper).
2. Chat replies stream — the first audio chunk plays before the LLM has finished
   generating.
3. Audio ducking: if the user starts speaking (VAD detects voice) while Lashon
   is speaking, Lashon pauses within 150 ms and resumes from a sentence boundary
   or cancels.
4. All cloud providers expose a single API-key field in Settings; switching is
   one click.
5. Hebrew voice-quality test: 20 native-Hebrew sentences scored ≥ 4/5 by 3
   native-speaker reviewers (Piper baseline) and ≥ 4.5/5 (Azure/ElevenLabs).

### 3.1 TTS provider mux

- A common `TTSProvider` trait (see [`architecture.md`](architecture.md)).
- Default chain: Piper local → fall back to the next configured provider if
  synthesis fails.
- Per-mode default: command mode uses the fastest (Piper); chat mode uses the
  best (user-chosen).
- A voice picker in Settings with a sample-play button.

### 3.2 Local TTS engines

- **Piper:** bundle the `piper-rs` crate, ship `he_IL-default-medium.onnx`
  (~50 MB), CPU-only.
- **MMS-TTS-heb:** offered as an optional download (~600 MB), runs via the
  Python sidecar (HuggingFace Transformers `VitsModel`), GPU optional.
- **Coqui XTTS-v2:** optional download (~1.8 GB), GPU required for real-time;
  voice-clone mode behind an explicit "I confirm I have permission for this
  voice" gate.

### 3.3 Cloud TTS engines

- All implemented as thin HTTP clients with streaming where supported
  (ElevenLabs WS, Azure SSE, Cartesia HTTP/2, OpenAI streaming).
- API keys stored in the OS keychain (Windows Credential Manager / macOS
  Keychain / Linux Secret Service via the `keyring` crate).
- A per-provider voice list cached in settings, with a refresh button.

### 3.4 Audio playback & ducking

- A `cpal` output stream, 24 kHz typical (matches Piper/MMS); resample if needed
  via `rubato`.
- Ducking: a shared `Mutex<bool> is_speaking`; the VAD callback checks it; if the
  user starts speaking, send `pause` to the current TTS stream, wait 500 ms for
  the user to confirm the interruption, then either resume or cancel.
- Sentence-boundary buffering: stream TTS into a sentence-aware queue that can be
  cancelled cleanly between sentences.

### 3.5 Streaming pipeline

- Chat mode: LLM tokens → a sentence buffer (split on `.!?،؛؟\n` and Hebrew
  sentence-end heuristics) → as soon as a full sentence is ready, send it to TTS
  → audio chunks → playback queue.
- Result: the first audio plays within ~600 ms of the user finishing speaking on
  Tier A.

### 3.6 Voice identity ("Lashon's voice")

- Default Hebrew voice: the most natural Piper `he_IL` voice as Lashon's "house
  voice".
- Branding-relevant: a single named voice the user comes to recognise as
  Lashon's.
- The user can switch; the default is sticky.

## Phase 4 — Polish, installer, signing, distribution

**Definition of Done:**

1. A fresh Win11/macOS 14/Ubuntu 24.04 VM installs Lashon, completes onboarding,
   and runs all three phases end-to-end with no external internet beyond the
   initial model downloads.
2. Code-signed installers (Win cert, Mac notarized, Linux GPG-signed AppImage).
3. Auto-update from GitHub Releases via `tauri-plugin-updater`.
4. Crash reporting (opt-in) ships sanitized logs only.
5. No transcript/audio data leaves the device by default.

**Windows** packaging and the release runbook are documented separately —
see [`packaging-windows.md`](packaging-windows.md) and
[`releasing.md`](releasing.md).

**macOS** — toolchain Xcode 16, a Rust darwin-universal target, an Apple
Developer ID. `tauri build --target universal-apple-darwin --bundles dmg`,
notarized via `xcrun notarytool`. Entitlements: `device.audio-input`,
`automation.apple-events`, `NSAccessibilityUsageDescription` (he + en).

**Linux** — `tauri build --bundles appimage,deb`. Document
`sudo usermod -aG input $USER` if the user opts into the ydotool fallback.

**Auto-update** — `tauri-plugin-updater` with a signed manifest hosted on GitHub
Releases; release channels `stable` and `beta`.

**Crash reporting** — opt-in, ships only logs plus system info, never
transcripts.
