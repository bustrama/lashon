# Tech stack & hardware tiers

This document describes *what* Lashon is built from and *where* it runs. Exact
versions are pinned in the manifests (`Cargo.toml`s, `package.json`,
`pyproject.toml`) and summarised in [`../CLAUDE.md`](../CLAUDE.md) — those are
authoritative for versions; this document is authoritative for composition.

## Application shell

- **Tauri 2** — Rust core plus the system webview.
- **SvelteKit 5 / Svelte 5** frontend, **TypeScript**.
- **Vite** build tooling.

## Rust crates

The `lashon-core` crate (`packages/shared-rust/`) holds GUI-independent logic;
the Tauri crate (`apps/desktop/src-tauri/`) is a thin shell. Key dependencies
and their roles:

**Tauri & plugins**

- `tauri` (with `macos-private-api`) — app shell
- `tauri-plugin-global-shortcut` — the three-chord hotkey manager
- `tauri-plugin-autostart`, `tauri-plugin-updater`, `tauri-plugin-store`,
  `tauri-plugin-shell`, `tauri-plugin-tray-icon`, `tauri-plugin-fs`,
  `tauri-plugin-os`

**Audio & ML**

- `cpal` — audio I/O (capture and playback)
- `ringbuf` — lock-free rolling ring buffer
- `ort` (features `cuda`, `directml`, `load-dynamic`) — ONNX runtime for Silero
  VAD and openWakeWord
- `ndarray` — tensor math

**Input & platform**

- `enigo` — synthetic keyboard/mouse input
- `arboard` — clipboard with multi-format support
- `windows` — Win32 accessibility, keyboard/mouse input, data exchange
- `core-foundation` — macOS platform glue
- `nvml-wrapper` — NVIDIA GPU detection
- `sysinfo` — RAM/CPU detection
- `ash` — Vulkan capability probe

**gRPC & async**

- `tonic` / `prost` — gRPC client to the Python sidecar
- `tokio` — async runtime

**External agents & networking**

- `portable-pty` — PTY sessions for delegated external agents
- `async-openai` — OpenAI-compatible HTTP (cloud providers, Ollama, llama-server, LM Studio)
- `reqwest` — HTTP client
- `eventsource-stream` — SSE streaming
- `tokio-tungstenite` — WebSocket (ElevenLabs)

**TTS & audio output**

- `piper-rs` — local TTS
- `rubato` — resampling
- `hound` — WAV I/O

**Storage & errors**

- `sqlx` (SQLite, `runtime-tokio`) — interaction history and long-term memory
- `keyring` — OS keychain for cloud API keys
- `serde` / `serde_json` — serialisation
- `tracing` / `tracing-subscriber` — structured logging
- `anyhow` / `thiserror` — error handling

## Python STT sidecar

The `lashon_stt` package (`services/stt-sidecar/`) is a Python gRPC service.
Key dependencies:

- `faster-whisper`, `ctranslate2` — the default Hebrew STT engine
- `pywhispercpp` — CPU/Vulkan fallback engine
- `silero-vad` — voice activity detection
- `huggingface-hub` — model download on first run
- `grpcio`, `grpcio-tools`, `protobuf` — the gRPC transport
- `soundfile`, `numpy`, `regex` — audio I/O and post-processing
- `transformers`, `torch` — loaded only when the optional MMS/XTTS TTS engines
  are active

## Frontend dependencies

- `@tauri-apps/api` and the matching plugin packages (`global-shortcut`,
  `store`, `updater`, `shell`)
- `svelte`, `@sveltejs/kit`
- `lucide-svelte` — icons
- `motion` — animation
- `@xterm/xterm` + `@xterm/addon-fit` — the Agent panel terminal
- `highlight.js` — code-block highlighting

## Hardware tiers

Lashon detects the host's capability at onboarding and picks default models per
tier. VRAM accounting includes warm-loaded models plus headroom. The user can
override; Lashon never silently downgrades.

| Tier | Hardware | STT (warm) | LLM (warm) | TTS | Net VRAM | E2E latency target |
|---|---|---|---|---|---|---|
| **A — Studio** | RTX 4070+ / 4080 / 4090, 12+ GB VRAM, 32 GB RAM | Whisper-large-v3-turbo CT2 fp16 (4 GB warm via int8_float16) | DictaLM-3.0-Nemotron-12B Q4 (7 GB) | Piper CPU (free) | ~12 GB used, 4 GB headroom on 16 GB | Dict: <800 ms · Cmd: <1.8 s · Chat first-byte: <900 ms |
| **B — Workstation** | RTX 3060 / 4060, 6–8 GB VRAM, 16 GB RAM | Whisper-large-v3-turbo CT2 int8_float16 (3 GB warm-on-demand) | DictaLM-3.0-1.7B Q8 (2 GB) | Piper CPU | ~5 GB | Dict: <1.5 s · Cmd: <2.5 s · Chat first-byte: <1.5 s |
| **C — Office** | CPU-only (i5+/Ryzen 5+) or AMD GPU with Vulkan, 8+ GB RAM | whisper.cpp + ivrit turbo Q5 GGUF (RAM, Vulkan if avail) | rule-based cleanup only; cloud LLM recommended for cmd/chat | Piper CPU | 0 VRAM | Dict: <3 s · Cmd/Chat: cloud-dependent |
| **D — Minimal** | Old laptop, ≤ 4 GB free RAM | whisper.cpp small Hebrew Q4 | cloud only | Piper CPU | 0 | best-effort |

The default-model map per tier is encoded in
`apps/desktop/src-tauri/tiers.json`.

**Auto-detection logic:**

```rust
fn detect_tier() -> Tier {
    let vram = nvml::total_vram_gb().unwrap_or(0.0);
    let ram = sysinfo::total_ram_gb();
    let cuda = nvml::has_cuda();
    let vulkan = ash::has_vulkan_gpu();
    match (cuda, vram, ram) {
        (true,  v, r) if v >= 12.0 && r >= 24.0 => Tier::A,
        (true,  v, r) if v >=  6.0 && r >= 12.0 => Tier::B,
        _ if vulkan && ram >= 8.0               => Tier::C,
        _                                        => Tier::D,
    }
}
```

Tier detection runs at onboarding (tiers are tested A, then B, then C, then D);
the user may override it in onboarding and in Settings → Models.

**Memory swap policy:**

- The LLM stays warm permanently — it is the most expensive to reload.
- STT is warm by default on Tier A; loaded on-hotkey on Tier B and below.
- TTS streams from disk (Piper) or from the warm Python sidecar (MMS/XTTS).
