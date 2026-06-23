# Lashon — Architecture

The system design of Lashon — its posture, interaction modes, topology, and the
provider abstraction that shapes every change. This document is authoritative
for architecture and is kept current with the code. For the build roadmap see
[`roadmap.md`](roadmap.md); for decision records see [`adr/`](adr/).

## 1. Posture

Lashon is a local-first desktop voice assistant spanning three stages:
speech-to-text (STT), PC operation, and text-to-speech (TTS). Every stage runs
locally by default. Cloud providers exist only as opt-in adapters, each
surfaced honestly with a "cloud" badge. The user owns their data — nothing
leaves the machine without explicit consent.

## 2. Three interaction modes

Lashon has one capture pipeline feeding three modes, distinguished by trigger
and by where the result goes:

| Mode | Trigger | Output | Provider stack |
|---|---|---|---|
| **Dictation** | Push-to-talk hotkey | The focused text field (clipboard paste) | STT → optional cleanup LLM → text injector. No agent loop. |
| **Command** | Wake word, command hotkey, or a command-verb prefix | PC tool execution + spoken confirmation | STT → **word-aliases** (M9 — post-STT substitution from `stt.word_aliases`) → **recipe cascade** (M9 — regex tier; deterministic short-circuit on match) → fall through to: cleanup → tool-use LLM → tool runner → TTS |
| **Chat** | Chat hotkey or a "Lashon, question" prefix | Conversation panel + streamed TTS | STT → cleanup → chat LLM (streaming) → TTS (streaming) |

**Dictation is the hot path.** It carries no LLM in the critical loop and is
optimised for latency — sub-800 ms hotkey-release-to-paste on Tier A hardware.
Command and chat modes trade latency for capability.

## 3. System topology

The app is a single Tauri 2 process. Its layers, top to bottom:

- **Shell** — the SvelteKit frontend (the Tongue widget, the Hub, the
  Conversation and Agent panels), a hotkey manager for three configurable
  chords, plus tray, autostart, and updater.
- **Dictation FSM** — the Rust core state machine on a `tokio` runtime:
  `Idle → Listening → Buffering → Transcribing → Routing → {Inject | CommandLoop
  | ChatLoop} → Speak → Idle`.
- **Capability layer** — audio capture (`cpal` + ring buffer + Silero VAD +
  wake word), the native tool runner, and the TTS provider mux with audio
  playback and ducking.
- **Provider mux** — the abstraction seam (see §4). Behind it sit a Python STT
  sidecar reached over gRPC, a local LLM server (llama.cpp / Ollama) or a cloud
  API, and TTS engines local or cloud.
- **Persistence** — SQLite for interaction history and long-term memory, plus
  settings and logs.

ML inference lives in **Python sidecars** — STT always, TTS for the optional
MMS/XTTS engines — spawned as child processes and reached over gRPC. This keeps
the Hebrew-native Python ML ecosystem available, isolates inference crashes
from the UI, and — because the boundary is a proto contract — leaves room for a
pure-Rust provider later without changing callers. The sidecar boundary is a
trust boundary: every gRPC call carries a per-process auth token, so the
loopback bind constrains locality while the token authenticates the caller. See
[ADR-0001](adr/0001-tauri-sveltekit-rust-stack.md),
[ADR-0002](adr/0002-grpc-loopback-tcp-transport.md), and
[ADR-0010](adr/0010-harden-the-stt-sidecar-trust-boundary.md).

## 4. The provider abstraction

The single most important architectural decision: **every stage exposes a
common trait, and each provider implements it.** The user picks a provider per
stage. No code path is hardcoded to a vendor, and no path defaults to cloud.

```rust
trait STTProvider {
    async fn transcribe(&self, pcm_f32: &[f32], language: &str) -> Result<Transcript>;
    async fn transcribe_stream(&self, pcm_rx: Receiver<AudioChunk>) -> Stream<Partial>;
    fn supports_hebrew(&self) -> Confidence;   // None / Basic / Good / Excellent
    fn is_local(&self) -> bool;
    fn warmup(&self) -> Result<()>;
}

trait LLMProvider {
    async fn chat(&self, messages: Vec<Msg>, tools: Vec<Tool>) -> Result<Completion>;
    async fn stream(&self, messages: Vec<Msg>, tools: Vec<Tool>) -> Stream<Token>;
    fn supports_tool_use(&self) -> bool;
    fn supports_hebrew(&self) -> Confidence;
    fn context_window(&self) -> usize;
    fn is_local(&self) -> bool;
}

trait TTSProvider {
    async fn synthesize(&self, text: &str, voice: &str) -> Result<Vec<i16>>;
    async fn stream(&self, text: &str, voice: &str) -> Stream<AudioChunk>;
    fn voices(&self) -> Vec<Voice>;
    fn supports_hebrew(&self) -> Confidence;
    fn first_token_latency_ms(&self) -> u32;
    fn is_local(&self) -> bool;
}

trait AgentProvider {            // external coding agents, PC-operation only
    async fn spawn(&self, prompt: &str, cwd: &Path) -> Result<AgentSession>;
}
```

`is_local()` and `supports_hebrew()` are not decoration — the UI uses them to
badge cloud providers and to steer Hebrew-capable defaults. These traits are
the contract every milestone builds against; the signatures above are
illustrative — the trait definitions in `lashon-core`
(`packages/shared-rust/src/`) are authoritative.

## 5. Why this shapes every change

- A new STT / LLM / TTS engine is a new trait implementation plus a settings
  entry — never an edit to the FSM or to callers.
- Hebrew support is a first-class, testable property of each provider.
- The local/cloud boundary is explicit and visible, by construction.

## 6. Risks & mitigations

The standing engineering risks that shape Lashon's design and review priorities.

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| ivrit-ai license/availability changes | Low | High | Pin the commit SHA; mirror to our HF org |
| CUDA/cuDNN version drift breaks ctranslate2 | Med | High | Bundle exact DLLs; document required versions; auto-detect + prompt |
| Hebrew RTL paste glitches in specific apps | Med | Med | Per-app injection-profile overrides; explicit RTL marks (U+202B/U+202C) fallback |
| Clipboard race with managers (Ditto, ClipboardFusion) | Med | Med | Detect, warn, offer a "skip restore" flag |
| Windows SmartScreen blocks an unsigned exe | High if unsigned | High | Code-signing certificate; sign all binaries including sidecars |
| Model downloads blocked on some networks | Med | Med | Custom mirror-URL setting; manual import path; `HF_ENDPOINT` env var |
| LLM cleanup hallucinates content | Med | High | Conservative prompt; max-tokens 1.5× input; n-gram Jaccard guard ≥ 0.5; user toggle |
| Wake-word false activations | Med | Med | 2-frame threshold; sensitivity slider; battery-aware throttle |
| Tool-execution accidents (deletes, sends) | Med | High | Confirmation-policy whitelist; spoken Hebrew/English confirmation; atomic undo log |
| External agent CLI breaks its API | Med | Med | Pin tested agent versions; show a compatibility matrix; degrade gracefully |
| Cloud provider key exfiltration | Low | High | Keys in the OS keychain only; never logged; redacted from crash reports; ZDR opt-in where supported |
| GPL/CC-NC contamination | Low | Med | `cargo-deny` + `pip-licenses` in CI; CC-NC TTS models surfaced as optional downloads, never bundled |
| User confusion: local vs cloud routing | Med | Med | A cloud badge on every cloud provider chip; the provider name shown in the tongue during use |
| Token-cost runaway in cloud mode | Med | Med | Per-provider spend-cap setting; a daily-usage card in Settings; warn at 80% |
