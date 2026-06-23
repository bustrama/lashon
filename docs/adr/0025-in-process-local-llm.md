# 25. Built-in local LLM (bundled llama-server subprocess, Vulkan, Qwen3-1.7B)

- **Status:** Accepted (revised 2026-05-25 — see "Reversal" below)
- **Date:** 2026-05-24, revised 2026-05-25
- **Deciders:** Lashon contributors
- **Context source:** branch `claude/local-llm-windows-os-WPjre`;
  follow-up to [ADR-0019](0019-provider-mux-traits.md) and the M7 story
  ([`../stories/m7-provider-mux.md`](../stories/m7-provider-mux.md)).

## TL;DR (revised)

Lashon ships a Vulkan-enabled prebuilt **`llama-server`** (~80 MB,
from upstream `ggml.llamacpp`) as a Tauri-bundled resource. The
`LocalLlmProvider` is a thin OpenAI-compatible HTTP shim over the
loopback port the server binds to; the Tauri shell spawns the server
on first chat, kills it via a Win32 Job Object on exit, and recycles
it when the user switches model. Inference latency on an RTX 4080 with
Qwen3-1.7B-Q8_0: cold ~6.5 s (one-time per session, includes prompt
eval of system + tool definitions), warm **~500 ms** for a full
Command-mode tool call.

The original decision (in-process via `mistralrs`) is preserved
verbatim below; the reasons it was reversed are documented in the
"Reversal — 2026-05-25" section near the bottom.

## Context

M7 plumbed local LLM via the `OllamaLocalLlmProvider` chip — the
OpenAI-compatible client pointed at `http://127.0.0.1:11434/v1`. That
works, but it offloads the "running an LLM" problem onto the user: they
must install Ollama, start the daemon (or set it to autostart), pull a
model with `ollama pull …`, and keep it healthy. For a user who picked
Lashon precisely because they want a local-first voice assistant, "go
install a server" is a steep cliff.

The architecture and security rules
([`.claude/rules/security.md`](../../.claude/rules/security.md),
[`.claude/rules/architecture.md`](../../.claude/rules/architecture.md))
already favour local providers as the default at every stage. The Hub's
"Local (Ollama)" chip greys itself out when the daemon is absent — a
working-by-default local LLM closes that gap.

The product also already has an in-process inference pattern: ONNX
Runtime via the `ort` crate runs Silero VAD and the wake-word
classifiers ([ADR-0015](0015-silero-vad-and-utterance-endpointing.md),
[ADR-0016](0016-wake-word-engine.md)) with no sidecar and no server.
The STT engine is a Python sidecar only because the ivrit-ai fine-tune
is a CT2 model with a Python-first ecosystem ([ADR-0006](0006-release-packaging-and-signing.md)).
Nothing about LLM inference forces a sidecar — modern Rust crates speak
GGUF directly.

For Command-mode workloads (the M8 dispatcher, single-turn or
2–3-step chained tool calls), a small instruction-tuned model is
sufficient. Research compiled on this branch concluded that a 1.7 B
to 4 B Q4_K_M model reaches 95–98 % of frontier-LLM effective
performance on OS-operation tasks, at sub-second end-to-end latency on
a modern CPU.

## Decision

### 1. In-process, no server

Local LLM inference runs inside the Tauri process — same posture as
ONNX VAD and wake word. No subprocess, no loopback HTTP, no daemon
the user has to start. The model is loaded on first use and kept warm
for the session.

The `OllamaLocalLlmProvider` and `OllamaRemoteLlmProvider` chips stay
in the Hub for users who already run Ollama; the new `LocalLlmProvider`
sits alongside them as the **default local LLM**.

### 2. Runtime: `mistralrs` (pure Rust, no C/C++ toolchain)

`mistralrs` v0.8.1 (MIT, Rust 1.88+) — the Rust SDK for
[mistral.rs](https://github.com/EricLBuehler/mistral.rs). With **no
feature flags** it builds entirely in pure Rust (the project's README
explicitly calls this out). GGUF loading is supported via the
`GgufModelBuilder`; tool calling is supported via the `Tool`,
`ToolChoice`, `RequestBuilder` surface (matches the M7 vendor-neutral
`Msg`/`Tool` types one-for-one).

Alternatives considered:

| Runtime | Why not | Notes |
|---|---|---|
| `llama-cpp-2` | Needs CMake + a C++ toolchain on every CI runner | ~100 % of llama.cpp speed; viable if we ever need it |
| `candle-core` + `candle-transformers` | Lower-level than `mistralrs` (which is built on candle); reinventing the chat loop | `mistralrs` already does the work |
| `llama-server` sidecar over HTTP | Reintroduces the daemon problem the user asked us to avoid | Already covered by the Ollama chips |
| Python LLM sidecar (mirror the STT pattern) | Adds a ~500 MB PyInstaller frozen binary, slow first-boot, extra trust boundary | The STT sidecar is a license + ecosystem constraint — there is none here |

The CUDA / Metal / Vulkan acceleration paths are deferred — the
default ships **CPU-only** because Lashon's hardware tiers C/D run
on CPU anyway and tier A/B already have the Ollama escape hatch for
GPU inference at scale.

### 3. Default model: Qwen3-1.7B, Q8_0 GGUF

The original branch picked the **Q4_K_M** quant at 1.7 B (~1.1 GB
warm RAM, 50–80 tok/s CPU) per a research summary on the branch.
That quant is not published in the upstream `Qwen/Qwen3-1.7B-GGUF`
repo — only **Q8_0** is — so the manifest pins Q8_0 to keep the
vendor source pure (the smaller Q4_K_M lives in community mirrors
like `unsloth/Qwen3-1.7B-GGUF`; opting into a non-vendor publisher
is a separate decision):

| Metric | Value |
|---|---|
| Parameters | 1.7 B |
| Quantisation | Q8_0 (8-bit) |
| Disk footprint | 1.83 GB |
| RAM footprint | ~2 GB warm |
| License | Apache-2.0 |
| Source | `Qwen/Qwen3-1.7B-GGUF` on Hugging Face |

The 4 B variant (`qwen3-4b-q4_k_m`, 2.5 GB) is a follow-up — same
code path, extra manifest entry. It trades extra RAM and latency
for higher accuracy on chained tool calls; for users on tiers A/B
who want the quality, the Hub exposes it as an alternate download.

### 4. Bundled vs first-run download

The model is **downloaded on first use**, never bundled in the
installer — same posture as the Hebrew STT model
([ADR-0006](0006-release-packaging-and-signing.md)) and the openWakeWord
classifiers ([ADR-0016](0016-wake-word-engine.md)):

- Adding ~1 GB to every installer for a feature the user might not
  enable is the wrong default. The current Lashon installer is ~80 MB.
- The download is one click in the Hub's LLM section. Progress is
  surfaced as a percentage with byte counts.
- The model lives in the per-user app-data directory
  (`$LASHON_MODELS_ROOT/local-llm/`) — `models/local-llm/` from a
  source checkout — so an OS reinstall keeps it.

### 5. Integrity verification

GGUF tensors are fed straight to compute kernels — a tampered file is
native code, so it must be SHA-256-verified on every load, identical
posture to the ONNX models
([ADR-0010](0010-harden-the-stt-sidecar-trust-boundary.md)).

The manifest schema mirrors `wake-classifiers.json`:

```json
{
  "id": "qwen3-1.7b-q8_0",
  "display_name": "Qwen3 1.7B (Q8_0)",
  "license": "Apache-2.0",
  "source": "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF",
  "files": [{
    "path": "Qwen3-1.7B-Q8_0.gguf",
    "url": "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q8_0.gguf",
    "bytes": <upstream bytes>,
    "sha256": "<upstream sha256>"
  }]
}
```

The upstream `bytes` + `sha256` are filled in at manifest-authoring
time from the Hugging Face LFS pointer (the `oid sha256:…` line in
the `git-lfs` pointer file). When the upstream model is re-uploaded,
the manifest is updated in the same commit as the version bump — same
discipline as `models/manifests/m6-audio.json`.

A model whose SHA-256 does not match its manifest is refused at load
time with a clear error in the Hub; the user is prompted to delete +
re-download.

### 6. Cargo feature flag: `local-llm`, **ON by default**

The `mistralrs` dep is gated by `local-llm` in
`lashon-core/Cargo.toml`. The feature is **on by default** so every
CI run exercises it — the dep weight (~5 min compile, ~30 MB binary
contribution) is honest. Forks that want a lean build can opt out
with `--no-default-features`.

When the feature is off, `LocalLlmProvider::chat` returns a clear
`ProviderError::Configuration { detail: "local-llm feature disabled
at compile time" }` so the Hub's chip renders a "build did not
include local LLM support" message rather than a silent failure.

### 7. The `LocalLlmProvider` impl

A new `lashon-core::llm::local` module:

- Honest `LLMProvider` trait conformance: `is_local() → true`,
  `supports_hebrew() → Confidence::Basic` (Qwen3's Hebrew is
  competent but not benchmarked against the Hebrew corpus),
  `supports_tool_use() → true`, `context_window() → 32_768`
  (Qwen3's native window; capped at 4096 in the request builder for
  Command-mode latency).
- Lazy model load: the `Model` handle is constructed on the first
  `chat()` call, then held in an `OnceCell` (or `tokio::OnceCell`) for
  the session. Loading a 1 GB GGUF takes 1–3 seconds the first time;
  subsequent calls are warm.
- Message translation: `Msg::user/assistant/system/tool` map to
  mistralrs's `TextMessageRole`; `MsgContent::Blocks` carrying
  `ContentBlock::ToolCall` blocks map to mistralrs's
  `add_message_with_tool_call`. The translation is in the impl,
  invisible to callers.
- Tool translation: M8's tool registry already serialises its
  `LashonTool` schemas to `Tool { name, description, parameters }` —
  the impl wraps each into a mistralrs `Tool { tp:
  ToolType::Function, function: Function { … } }`.

### 8. Per-mode sampling defaults

Per the research summary's Command-mode tuning:

- `temperature = 0.1` (Command mode needs deterministic tool selection)
- `top_p = 0.9`
- `max_tokens = 100` (Command mode emits short tool calls, not prose)

The Hub does not expose per-call sampling controls in this PR — they
live in `settings.json` under `llm.local-llm.command.*` for
power users and are surfaced in a follow-up if the need emerges.

## Alternatives considered

- **Ship Ollama as the only local path** — what the M7 chip already
  does. Rejected because it makes a server install part of the
  setup. The Lashon ethos is "works out of the box".
- **Bundle the model in the installer** — adds ~1 GB to every
  download. Rejected per the existing first-run-download pattern.
- **Default to the 4 B model** — better accuracy, double the
  download, slower latency on weak hardware. The branch task said
  "pick the fastest and lightest" — the 1.7 B variant wins.
  4 B is a one-line manifest extension.
- **Skip the `local-llm` Cargo feature gate** — simpler but couples
  every CI matrix runner to the mistralrs build. Keeping it lets
  forks opt out without a fork.

## Consequences

- New crate dep: `mistralrs = "=0.8.1"` (under `local-llm` feature).
  Compile time on CI rises by ~5 minutes per cold runner.
- New manifest: `models/manifests/local-llm.json`. Tracks the
  bundled model catalog (one entry at PR landing; the 4 B variant is
  the obvious second entry).
- New per-user directory: `$LASHON_MODELS_ROOT/local-llm/`.
- New Tauri commands: `local_llm_status`, `install_local_llm_model`,
  `delete_local_llm_model`.
- The Hub's LLM section gains a "Local (built-in)" chip — the M7
  `OllamaLocalLlmProvider` chip stays as "Local (Ollama)".
- M8 Command mode picks up an out-of-the-box local LLM the moment
  the user clicks Download — no server install required.

## Open follow-ups

- The 4 B Qwen3 variant in the manifest, surfaced with a "best
  balance" badge.
- Per-mode sampling controls in the Hub for power users.
- JSON-schema-constrained decoding for tool calls — `llama-server`
  supports this natively via `--grammar`, but M8's tool dispatcher
  currently relies on the LLM's own `<tool_call>` format; tightening
  this is its own ADR.

## Reversal — 2026-05-25

The original decision above (in-process inference via `mistralrs`) was
reversed during the first end-to-end perf test on the branch. The
revised architecture: ship a **bundled `llama-server` subprocess** that
the Tauri shell manages with the same posture as the STT sidecar
(spawn-on-demand, health-checked, Win32 Job Object kill-on-parent-exit).
`LocalLlmProvider` is a thin OpenAI-compatible HTTP shim — a reuse of
the existing M7 `OpenAiCompatLlmProvider` wire format, pointed at
`http://127.0.0.1:<port>/v1`.

### What the reversal solves

1. **Catastrophic CPU perf in mistralrs.** End-to-end on a modern
   CPU, the in-process path took **779 s** for `LocalLlmProvider`'s
   one-token "blue" test; the same prompt through `llama-server` on
   the same hardware returns in **376 ms**. The root cause is
   mistralrs's pure-Rust matmul without SIMD; its only x86 CPU
   acceleration feature is `mkl` (heavy Intel runtime dep). llama.cpp
   ships its own hand-tuned AVX2/AVX-512/NEON kernels and
   auto-dispatches them at runtime via the bundled
   `ggml-cpu-*.dll` set.
2. **Windows + CUDA + VS 2025 toolchain wall.** mistralrs's CUDA
   path goes through `candle-cuda`, which pulls in MSVC stdlib
   headers from inside its `nvcc` invocations. MSVC 14.51 (the
   stdlib shipped with Visual Studio 2025) requires CUDA 13.2 or
   newer; the user's host had CUDA 12.1 and 12.3. The error
   surfaces as `STL1002 expected CUDA 13.2 or newer` and cannot be
   bypassed with `-allow-unsupported-compiler` — the gate is in the
   stdlib headers, not in `host_config.h`. The only fix was to
   install Visual Studio 2022 C++ Build Tools alongside VS 2025 (a
   ~3 GB install).
3. **`llama-cpp-2` (Rust bindings) had its own wall.** It builds
   llama.cpp from source via CMake, which needs `cmake`, `ninja`,
   and `libclang` on the build host. None ship with VS 2025 by
   default; another ~500 MB of installs.
4. **Cross-GPU support, free.** The prebuilt `llama-server` we
   ship is the Vulkan variant — it runs on **any** modern GPU
   (NVIDIA, AMD, Intel) and falls back to CPU when none is
   present. mistralrs only had CUDA on Windows; AMD and Intel
   users would have stayed CPU-bound.
5. **Architectural fit.** The subprocess pattern already exists
   in Lashon for the STT sidecar (Python via PyInstaller). The
   `attach_to_kill_on_close_job` helper, the per-process
   `Mutex<Option<Arc<…>>>` state, the `tauri::State<…>` wiring —
   all of it carries over verbatim. The "in-process" ADR
   invariant is relaxed to "managed-subprocess" for the LLM
   stage; the user-facing claim ("local-first, no external
   daemon") is unchanged because Lashon owns the subprocess
   lifecycle end-to-end.

### Trade-offs accepted

- **Installer +~80 MB.** The Vulkan-enabled `llama-server` + its
  minimum DLL set (`ggml*.dll`, `llama*.dll`, `mtmd.dll`,
  `libomp140.x86_64.dll`) totals ~80 MB uncompressed, ~30 MB in
  NSIS. Bundled rather than first-run-downloaded for the same
  reason the STT sidecar binaries bundle: code is "small enough",
  data (the 1.83 GB GGUF) is "downloads on demand". Matches the
  existing pattern.
- **Loopback HTTP overhead.** A measured ~1–3 ms per chat call
  (negligible — < 1 % of the warm response time).
- **Process startup cost.** ~5–10 s for the model load on first
  chat of a session. Mitigated by reusing the existing
  `ready_llama_server` pattern (idempotent) and by spawning the
  server only when the user actually invokes the local provider.
- **Bundled-binary provenance.** We mirror upstream `ggml.llamacpp`
  releases. The README in `apps/desktop/src-tauri/binaries/llama-server/`
  documents how to refresh; the upstream is MIT, so redistribution
  is fine.

### What stays the same

- `LocalLlmProvider`'s public Rust surface (`name`, `display_name_key`,
  `is_local`, `supports_hebrew`, `available_models`, `has_api_key`,
  `default_model`) is unchanged — only the runtime swapped.
- The Hub's "Local (built-in)" chip — same chip, same download
  button, same model picker. The chip's behaviour is now: pick
  model → download GGUF (already worked) → first chat spawns
  `llama-server` (new) → subsequent chats reuse it (new).
- The manifest (`models/manifests/local-llm.json`) is unchanged.
- The `local-llm-cuda` Cargo feature is **removed** — GPU
  acceleration is a runtime choice made by `llama-server` based on
  the host's available compute libraries, not a compile-time flag.
- ADR-0010's SHA-256 verification continues to apply to the GGUF on
  every load (the manifest's verified hashes are checked at
  install time and re-checked on disk).
