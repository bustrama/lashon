# Providers

Every stage — speech-to-text, language model, text-to-speech — sits behind a
common trait, so the user picks a provider per stage. No code path is hardcoded
to a vendor, and no path defaults to cloud. See
[`architecture.md`](architecture.md) for the trait contracts; this document is
the catalog of providers Lashon ships or intends to support.

Local providers are the default at every stage. Cloud providers are opt-in
adapters, each surfaced with a clear "cloud" badge.

## STT providers

| Provider | Local | Hebrew | Latency | Notes |
|---|---|---|---|---|
| **`ivrit-ai/whisper-large-v3-turbo-ct2`** via faster-whisper | ✓ | Excellent | ~0.10× RT on 4080 | **Default — all tiers** (GPU on A/B, CPU on C/D — see M5, `adr/0014`). Apache-2.0. Fine-tuned on 295h crowd + 93h professional Hebrew |
| `ivrit-ai/whisper-large-v3-ct2` | ✓ | Excellent+ | ~0.27× RT | "Max accuracy" toggle |
| `Systran/faster-whisper-large-v3-turbo` (vanilla) | ✓ | Good | same as above | Fallback if ivrit-ai unavailable |
| Groq Whisper Large v3 | ✗ | Good | ~50 ms first-byte | Cloud opt-in, ~$0.04/hr |
| OpenAI Whisper API (`whisper-1` / `gpt-4o-transcribe`) | ✗ | Good | ~200 ms | Cloud opt-in |
| ElevenLabs Scribe | ✗ | Claimed | ~150 ms | Cloud opt-in |
| Deepgram Nova-3 | ✗ | Beta | ~80 ms | Cloud opt-in |
| AssemblyAI Universal-2 | ✗ | Good | ~250 ms | Cloud opt-in |

**Language detection.** The ivrit-ai fine-tunes cannot identify the spoken
language — fine-tuning collapsed their detector, which reports Hebrew for any
audio. A small companion model, `Systran/faster-whisper-tiny` (MIT, ~78 MB,
first-run download), does language ID only; the chosen STT model then
transcribes with that language forced. See
[ADR-0009](adr/0009-language-detection-via-a-companion-model.md).

## LLM providers (cleanup, command, chat)

**Local — built-in via bundled `llama-server` ([ADR-0025](adr/0025-in-process-local-llm.md)):**

| Model | Size on disk | Hebrew | Tool-use | Role |
|---|---|---|---|---|
| **Qwen3-1.7B** Q8_0 GGUF | 1.83 GB | Basic (unbenchmarked) | Yes (native) | **Default built-in pick** — smallest variant the upstream `Qwen/Qwen3-1.7B-GGUF` repo publishes |
| Qwen3-4B Q4_K_M GGUF | 2.5 GB | Basic (unbenchmarked) | Yes (native) | "Best balance" toggle — higher accuracy on 2–3 chained tool calls |

The user downloads one at first use from the Hub; both are Apache-2.0 and ship in the per-user `local-llm/` directory. Inference runs through a Lashon-managed `llama-server` subprocess (the prebuilt ggml release, ~80 MB bundled in the installer, Vulkan-enabled so it runs on any modern GPU and falls back to CPU when none is present). The Tauri shell spawns the server on first chat and kills it on app exit via a Win32 Job Object — same posture as the STT sidecar. See [ADR-0025](adr/0025-in-process-local-llm.md) for the architectural choice and verification scheme.

**Local — via a separately-installed Ollama (the legacy local path):**

| Model | Size | Quant on 16 GB | Hebrew | Tool-use | Role |
|---|---|---|---|---|---|
| **DictaLM-3.0-Nemotron-12B-Instruct** | 12B | Q4_K_M ≈ 7 GB | SOTA | Yes (Nemotron-style) | **Default Tier A** for cleanup + command + chat |
| DictaLM-3.0-24B-Instruct | 24B | W4A16 ≈ 13 GB | Best | Yes (Thinking variant) | "Max quality" toggle |
| DictaLM-3.0-1.7B-Instruct | 1.7B | Q8 ≈ 2 GB | Good for cleanup | Limited | Tier B/C cleanup-only |
| DictaLM-2.0-Instruct (7B Mistral) | 7B | Q4 ≈ 4 GB | Strong | Yes | Legacy fallback |
| Hebrew-Gemma-11B-Instruct | 11B | Q4 ≈ 7 GB | Strong | Yes | Alternative |
| Qwen2.5-14B-Instruct | 14B | Q4 ≈ 8.5 GB | Decent | Excellent | Cross-language fallback, strong tool-use |
| Llama 3.1 8B Instruct | 8B | Q4 ≈ 5 GB | Mediocre | Good | English-only fallback |

**Cloud (OpenAI-compatible API, all opt-in via API key):**

| Provider | Models | Hebrew | Tool-use |
|---|---|---|---|
| **Anthropic** | Claude Opus 4.7, Sonnet 4.6, Haiku 4.5 | Excellent | Excellent |
| **OpenAI** | GPT-5, GPT-4.1, o-series | Excellent | Excellent |
| **Groq** | Llama 3.3 70B, Llama 4 Maverick/Scout | Good | Good (fast) |
| **MiniMax** | abab-7-preview, M2 | Decent | Yes |
| **DeepSeek** | V3.1, R1 | Decent | Yes |
| **Mistral** | Large 2, Codestral | Decent | Yes |
| **Together AI** | hosted open models | varies | varies |
| **OpenRouter** | gateway | varies | varies |
| **Ollama remote** (e.g. home LAN) | any | varies | varies |

## External agent providers (PC operation, delegated)

These are spawned as subprocesses in a PTY. Lashon hands them a transcribed
prompt and renders their TUI output in the Agent panel. Used for heavy
coding/research tasks where the user's spoken intent maps to a session, not a
one-shot.

| Agent | Strength | Provider config |
|---|---|---|
| **Claude Code** | Best coding agent; the user's primary | Reads `ANTHROPIC_API_KEY` from system env |
| **OpenCode** (sst/opencode, Go) | Multi-provider, local-friendly TUI, MCP support | Has its own provider config |
| **Codex CLI** (OpenAI) | OpenAI's coding agent | Reads `OPENAI_API_KEY` |
| **Aider** | Mature git-aware code editor | Multi-provider |
| **Goose** (Block) | Multi-provider agent, MCP-first | Local + cloud |

Lashon ships no agent of its own at this level — it orchestrates. Each agent
runs in its own PTY; users switch via a tab strip.

Agents are installed by the user separately and detected on `PATH` at runtime;
Lashon only exposes the ones it finds:

- `claude-code` (Anthropic) — `npm i -g @anthropic-ai/claude-code`
- `opencode` (Go binary) — `brew/scoop install sst/tap/opencode` or direct download
- `codex` (OpenAI) — `npm i -g @openai/codex`
- `aider` — `pip install aider-chat`
- `goose` (Block) — direct download

## TTS providers (Phase 3)

| Provider | Local | Hebrew | First-byte | Quality | Notes |
|---|---|---|---|---|---|
| **Piper TTS** (`he_IL-*`) | ✓ | OK | ~50 ms CPU | Robotic but clear | **Default Tier C.** MIT, ONNX, ~50 MB |
| **Meta MMS-TTS-heb** | ✓ | Good | ~200 ms CPU / 50 ms GPU | Natural prosody | CC-BY-NC-4.0 (verify before commercial bundle; offer as opt-in download) |
| **Coqui XTTS-v2** | ✓ | Good (multilingual) | ~400 ms GPU | Natural, voice-clone | CPML-1.0 (non-commercial — opt-in download with clear notice) |
| **F5-TTS** | ✓ | Limited official; community Hebrew fine-tunes appearing | ~300 ms GPU | Natural | CC-BY-NC-4.0 |
| **OpenVoice v2** | ✓ | Multilingual | ~500 ms GPU | Voice clone | MIT |
| **Saspeech / SOSE** (Israeli academic) | ✓ | Native Hebrew | varies | Native | Check current license; community models exist |
| **ElevenLabs** | ✗ | Excellent (v3 multilingual) | ~250 ms streaming | Best | Cloud opt-in. Hebrew supported. |
| **Azure Speech** | ✗ | Native Hebrew voices (`he-IL-AvriNeural`, `he-IL-HilaNeural`) | ~200 ms | Excellent | Cloud opt-in |
| **Google Cloud TTS** | ✗ | Multiple Hebrew voices (he-IL Standard + WaveNet + Neural2 + Studio) | ~300 ms | Excellent | Cloud opt-in |
| **OpenAI TTS** (`tts-1` / `gpt-4o-mini-tts`) | ✗ | Hebrew works | ~400 ms | Good | Cloud opt-in |
| **Cartesia Sonic** | ✗ | Multilingual | ~90 ms (lowest) | Excellent | Cloud opt-in |
| **PlayHT** | ✗ | Hebrew | ~300 ms | Good | Cloud opt-in |

**Default ladder per tier:**

- **Tier A** — Piper (instant) for command confirmations; Coqui XTTS for
  chat-mode replies if the user opts in; ElevenLabs/Azure cloud offered as an
  upgrade.
- **Tier B** — Piper local for everything; cloud upgrade prominent.
- **Tier C** — Piper local only; cloud upgrade prominent.

## Bundle policy

Only MIT/Apache-licensed models ship in the installer. CC-BY-NC and CPML models
(MMS-TTS, Coqui XTTS, F5-TTS) are surfaced as **optional downloads** with a
clear "non-commercial license" badge — the user opts in once and the app
downloads the model. No CC-BY-NC / CPML weights are ever bundled.
