# Provider mux foundation

Milestone **M7**. Branches `feat/m7-llm-providers` (LLM half), and a
follow-up STT branch for cloud STT + streaming.

> **Status: in progress.** The LLM half is landing on
> `feat/m7-llm-providers` — Phases 1 (LLM trait + types + registry), 4
> (cloud LLM providers + Hub LLM section), and 5 (Ollama local). The
> ADRs that govern the work moved from Draft to Accepted:
> `docs/adr/0019-provider-mux-traits.md`,
> `docs/adr/0020-keychain-integration.md`,
> `docs/adr/0021-hub-provider-switching-ux.md`, and
> `docs/adr/0022-cloud-opt-in-and-badging.md`. Phase 2 (cloud STT +
> Deepgram / AssemblyAI streaming) and Phase 3 (Hub STT section) remain
> planned as a separate follow-up branch.

## Why

Every milestone since M0 has spoken to a single, fixed provider at every stage:

- **STT** — `FasterWhisperProvider`, always the gRPC sidecar.
- **LLM** — nothing wired yet; the dictation FSM has no LLM in its loop at all.
- **TTS** — nothing wired yet.

`docs/architecture.md §4` sketches the trait abstraction that *should* govern
every stage — `SttProvider`, `LLMProvider`, `TTSProvider`, `AgentProvider` —
but these are illustrative sketches, not load-bearing code. The `SttProvider`
trait in `lashon-core::stt` exists and is used, but there is no registry, no
runtime dispatch, and no way for the user to switch.

M7 makes the provider abstraction **load-bearing**:

1. Formalise the traits in `lashon-core` with the exact signatures the
   architecture doc intends.
2. Wrap the existing sidecar client as the first `SttProvider` impl — no
   regression, just a new seam.
3. Plumb cloud STT providers (Groq, OpenAI Whisper, ElevenLabs Scribe,
   Deepgram, AssemblyAI) as additional impls behind a runtime registry.
4. Introduce an `LLMProvider` trait and plumb cloud LLM providers (Anthropic,
   OpenAI, Groq, MiniMax, DeepSeek, Mistral, Together AI, OpenRouter, Ollama
   remote, **and Ollama local**). The trait's wire contract is the
   **OpenAI-compatible Chat Completions API** — Ollama, LM Studio,
   llama.cpp's `llama-server`, Jan, vLLM, mistral.rs, Groq, Together AI,
   OpenRouter, DeepSeek, and Mistral all expose `/v1/chat/completions`, so
   a single OpenAI-compat provider impl handles all of them, parameterised
   by base URL and optional API key. Anthropic is the lone vendor with its
   own wire format and gets its own dedicated impl.
5. Store API keys in the OS keychain via the `keyring` crate.
6. Surface provider switching, API-key entry, and cloud badging in the
   Settings Hub (STT and LLM sections, per
   `docs/design-system.md` Hub layout items 4–5).

TTS is deferred to M10/M11 — Piper is not yet wired, so there is no incumbent
to wrap, and the streaming playback pipeline (cpal output + ducking) is its
own work. **Local LLM via Ollama is in M7 as Phase 5** — it reuses the
OpenAI-compat code path, so the additional risk is small. The `LLMProvider`
trait is also the seam Lashon's own future features consume (M8 Command mode,
M8 Chat mode, any future LLM-driven affordance) — not just user-driven Hub
test prompts.

## Scope

### In scope

- `SttProvider` trait finalisation (aligned with architecture.md §4 sketches),
  including a `transcribe_stream` method with a default impl that buffers the
  input PCM stream and delegates to `transcribe` — non-streaming providers
  stay conforming with zero per-provider code.
- `FasterWhisperProvider` wrapped behind the finalised trait — zero behaviour
  change, just the registry seam.
- Cloud STT providers: Groq Whisper, OpenAI Whisper API, ElevenLabs Scribe,
  Deepgram Nova-3, AssemblyAI Universal-2.
- **Real streaming STT** for Deepgram (WebSocket) and AssemblyAI
  (WebSocket) — `transcribe_stream` overridden with native WS impls. The
  local sidecar's streaming wrapper is a follow-up; dictation does not
  regress on the local path.
- `LLMProvider` trait: `chat`, `stream`, `supports_tool_use`,
  `supports_hebrew`, `context_window`, `is_local`.
- Cloud LLM providers: Anthropic (its own Messages-API impl), plus a single
  **OpenAI-compatible impl** parameterised by base URL — used for OpenAI,
  Groq, MiniMax, DeepSeek, Mistral, Together AI, OpenRouter, **Ollama
  local (`http://127.0.0.1:11434/v1`)**, and Ollama remote.
- `ProviderRegistry` — a runtime registry in `lashon-core` that stores the
  active provider per stage and vends the boxed trait object.
- OS keychain integration for cloud API keys (`keyring` crate, Windows
  Credential Manager / macOS Keychain / Linux Secret Service).
- Settings Hub: STT section (provider picker, model picker, API-key entry
  for cloud, test transcription button), LLM section (provider picker per
  mode — cleanup/command/chat, model picker, API-key entry, test prompt).
- Cloud-provider badge in the Tongue and Conversation panel header while a
  cloud provider is active.
- Trait conformance tests and mocked-provider integration tests in
  `lashon-core`; no real API calls in CI.
- He+en localization for every new Hub string.
- `docs/adr/` entries for each architectural decision made.

### Explicitly deferred

- **TTS mux** — M10 (Piper local) and M11 (cloud TTS). There is no incumbent
  TTS provider to wrap.
- **Agent provider trait** — M9 (external agent delegation).
- **LLM-driven dictation cleanup** — permanently cut (M5 product decision).
  M7's LLM infrastructure does not re-introduce it; no `cleanup` mode entry
  appears in the Hub's LLM section.
- **Local sidecar streaming STT** — `transcribe_stream` for
  `FasterWhisperProvider` is a follow-up. The default trait impl (buffer +
  delegate) carries the local path through M7.
- **Per-provider spend-cap and usage card** — **outside Lashon's scope.**
  Users set spend limits in each provider's own console (every cloud
  provider exposes monthly caps in their dashboards); duplicating that
  in-app is not worth the implementation cost. If a future user-facing
  need emerges it is its own dedicated milestone.
- **Rate-limit surfacing beyond a simple error toast** — deferred to a
  polish pass.

## Phased breakdown

M7 is split into five phases, each landable on `main` as an independent PR
without breaking the app. Phase 1 is the smallest meaningful slice; phases 2–5
add providers in order of risk, ending with local LLM as an opt-in.

---

### Phase 1 — Trait finalisation + sidecar wrapped (1–2 days)

**Goal:** the existing STT sidecar continues to work exactly as before, but is
now dispatched through a formal `SttProvider` trait and a `ProviderRegistry`.
No cloud provider, no UI change. This PR locks the trait shape.

**Deliverables:**

- `lashon-core::stt` — `SttProvider` trait finalised:
  `transcribe`, `transcribe_stream` (default impl buffers and delegates to
  `transcribe`), `supports_hebrew`, `is_local`. The existing
  `FasterWhisperProvider` already implements `transcribe`; a `name()`
  method (returns `"local-faster-whisper"`), a `display_name_key()` method
  (returns `"provider.stt.local_faster_whisper"`), and the
  `transcribe_stream` default (no override, native streaming for the local
  sidecar is a follow-up) are added.
- `lashon-core::llm` — new module: `LLMProvider` trait skeleton with `chat`,
  `stream`, `supports_tool_use`, `supports_hebrew`, `context_window`,
  `is_local`, `name`, `display_name`. No impls yet; just the type definitions
  (`Msg`, `Tool`, `Completion`, `Token` structs), the trait, and the `Confidence`
  enum (shared with `stt`).
- `lashon-core::provider_registry` — a `ProviderRegistry<T: ProviderKind>`
  struct: stores `active_provider_id: String` plus a static map of
  `id → Box<dyn T>`. The active provider is looked up on each call. In this
  phase it is constructed with only the local sidecar provider.
- The Tauri shell's dictation worker is updated to route STT through the
  registry rather than holding a `FasterWhisperProvider` directly — a one-line
  change in call site, no behaviour change.
- Unit tests: `FasterWhisperProvider` trait conformance, registry dispatch,
  `Confidence` ordering.

**Files touched:**

- `packages/shared-rust/src/stt.rs` — add `name`, `display_name` to trait +
  impl; introduce `Confidence` as a shared type.
- `packages/shared-rust/src/llm.rs` — new file (trait + types only).
- `packages/shared-rust/src/provider_registry.rs` — new file.
- `packages/shared-rust/src/lib.rs` — expose new modules.
- `apps/desktop/src-tauri/src/dictation.rs` — route through registry.
- `docs/adr/0019-provider-mux-traits.md`.

**Risks:**

- The `#[allow(async_fn_in_trait)]` the existing stt.rs uses is appropriate for
  single-threaded callers; confirm it holds with the registry's `Box<dyn …>`
  pattern. If not, switch to `async_trait` macro or RPIT (use `pin_project`
  or `BoxFuture` on trait returns). This is the key design question of Phase 1.
- `ProviderRegistry` must be `Send + Sync` for the Tauri state manager — ensure
  the trait bounds are propagated correctly.

---

### Phase 2 — Keychain + cloud STT providers (2 days)

**Goal:** a user can enter an API key for a cloud STT provider in the Hub,
Lashon stores it in the OS keychain, and transcription routes to that provider
when it is active.

**Deliverables:**

- `lashon-core::keychain` — a thin, cross-platform wrapper over the `keyring`
  crate: `store_key(service, key_name, secret)`, `get_key(service, key_name)`,
  `delete_key(service, key_name)`. Service name: `"lashon"`. Key names:
  `"stt.groq"`, `"stt.openai"`, `"stt.elevenlabs"`, `"stt.deepgram"`,
  `"stt.assemblyai"`. Keys are never written to disk, logged, or included in
  crash reports.
- Cloud STT impls in `lashon-core::stt`:
  - `GroqSttProvider` — multipart HTTP POST to `api.groq.com/openai/v1/audio/transcriptions`,
    model `whisper-large-v3`, content-type `multipart/form-data`. Language
    forced to `he` or the detected language (passed from the caller). The PCM
    float32 buffer is encoded to 16-bit WAV in memory before sending.
    `supports_hebrew() → Confidence::Good`. `is_local() → false`.
  - `OpenAiSttProvider` — same multipart contract, `api.openai.com` or a
    configurable base URL (for proxy / Azure OpenAI). Model: `whisper-1` by
    default, `gpt-4o-transcribe` as the optional upgrade.
    `supports_hebrew() → Confidence::Good`.
  - `ElevenLabsScribeSttProvider` — POST to
    `api.elevenlabs.io/v1/speech-to-text`. `supports_hebrew()` is marked
    `Confidence::Basic` (ElevenLabs claims support but no public Hebrew WER
    benchmark is available — research scope, see Hebrew section below).
  - `DeepgramSttProvider` — WebSocket streaming + REST batch to
    `api.deepgram.com`, model `nova-3-general`, language `he`.
    `transcribe_stream` is overridden with a native WS implementation
    that emits `TranscriptDelta` items as Deepgram returns them.
    `supports_hebrew() → Confidence::Basic` (Nova-3 Hebrew is beta;
    not independently benchmarked).
  - `AssemblyAiSttProvider` — WebSocket streaming + two-step REST (upload
    audio, poll transcript) for batch. `transcribe_stream` is overridden
    with a native WS implementation. `supports_hebrew() → Confidence::Good`
    (Universal-2 has published Hebrew numbers).
- Each cloud provider is constructed with the API key from the keychain and a
  configurable base URL (empty = default).
- The `ProviderRegistry` for STT is populated with all five cloud providers
  plus the local one; the active provider defaults to the local sidecar.
- Tauri command `set_stt_provider(id: String)` — validates the id is known,
  stores `stt.active_provider` in the settings store, updates the registry.
- Tauri command `save_api_key(stage: String, provider: String, secret: String)`
  — routes to `lashon-core::keychain::store_key`; the Tauri shell never sees
  the raw secret in return values or events.
- `reqwest` (already in Cargo.toml) used for all HTTP cloud calls; `tokio-tungstenite`
  for Deepgram WS path.

**Files touched:**

- `packages/shared-rust/src/keychain.rs` — new.
- `packages/shared-rust/src/stt.rs` — five cloud impls added.
- `packages/shared-rust/src/provider_registry.rs` — population logic.
- `packages/shared-rust/src/lib.rs` — expose keychain module.
- `apps/desktop/src-tauri/src/lib.rs` — `set_stt_provider`, `save_api_key`
  commands.
- `packages/shared-rust/Cargo.toml` — confirm `reqwest` features cover
  `multipart`; add nothing new if already present.
- `docs/adr/0020-keychain-integration.md`.

**Risks:**

- WAV-encoding the PCM float32 in memory before sending (no temp file, no
  disk write) — use `hound` (already in Cargo.toml); confirm the encoding
  round-trip produces valid audio for the provider.
- The `keyring` crate on Linux requires `libsecret` or `kwallet`. CI runners
  do not have a running keyring daemon; cloud STT tests must be feature-gated
  or skipped in CI. Document the Linux runtime requirement.
- Deepgram and AssemblyAI WebSocket paths add latency-management complexity
  (reconnect, partial-result reconciliation, backpressure). Both ship in M7
  as the streaming-STT impls; the risk is accepted because cloud streaming
  is a headline M7 capability.

---

### Phase 3 — Hub STT section (1–2 days)

**Goal:** the Hub's STT section is live. The user can pick any of the six STT
providers, enter an API key for cloud providers, and test transcription.

**Deliverables:**

- Hub `stt` section in `+page.svelte` (replacing the current placeholder that
  does not exist yet):
  - A provider picker: a chip-grid listing local and cloud providers. Each chip
    shows the provider display name, a `☁` badge for cloud, and a
    `✓ Hebrew` or `~ Hebrew (unverified)` badge (driven by the provider's
    `supports_hebrew()` return).
  - When a cloud provider is selected: an API key input field (password-type,
    masked). On save it calls `save_api_key`; a test-transcription button
    sends a short canned WAV and shows the result inline.
  - A model picker (where applicable — e.g. OpenAI whisper-1 vs gpt-4o-transcribe).
  - A base-URL override field (collapsed by default, for proxy / self-hosted
    configs).
  - The active provider is highlighted; the local provider's chip shows the
    hardware tier it runs on.
- Tongue and Conversation panel header show a `☁ <provider name>` chip whenever
  a cloud provider is active for the current operation.
- He+en localization for all new strings.
- Tauri command `get_stt_providers() → Vec<ProviderMeta>` where `ProviderMeta`
  carries `{id, display_key, is_local, supports_hebrew, has_key_stored}`.

**Files touched:**

- `apps/desktop/src/routes/hub/+page.svelte` — STT section.
- `apps/desktop/src/lib/i18n/locales/he.json`, `en.json` — all new keys.
- `apps/desktop/src-tauri/src/lib.rs` — `get_stt_providers` command.
- `apps/desktop/src/lib/components/Tongue.svelte` — cloud provider chip.
- `docs/adr/0021-hub-provider-switching-ux.md`,
  `docs/adr/0022-cloud-opt-in-and-badging.md`.

**Risks:**

- The masked API-key field must never surface the raw key value to the
  frontend JS layer. The Hub calls `save_api_key`; it should never call a
  "get key" command. The `has_key_stored` flag (a boolean from `keychain`)
  is sufficient to render the "key saved" state.

---

### Phase 4 — Cloud LLM providers (2 days)

**Goal:** `LLMProvider` is plumbed with all cloud providers. The Hub's LLM
section is live. The dictation FSM can route LLM calls through the registry
when an LLM is needed (this is preliminary — Command mode is M8; Chat mode
is also M8; but the provider infrastructure and Hub UI must exist before M8
starts).

**Deliverables:**

- Cloud LLM impls in `lashon-core::llm`:
  - `AnthropicLlmProvider` — Anthropic Messages API; `claude-sonnet-4-6` default.
    `supports_hebrew() → Confidence::Excellent`. `supports_tool_use() → true`.
  - `OpenAiLlmProvider` — OpenAI Chat Completions, OpenAI-compatible base URL.
    Models: `gpt-4.1`, `o4-mini`. `supports_hebrew() → Confidence::Excellent`.
  - `GroqLlmProvider` — same Chat Completions contract, base `api.groq.com`.
    Models: `llama-3.3-70b-versatile`, `llama-4-maverick`. `supports_hebrew() → Confidence::Good`.
  - `OpenRouterLlmProvider` — OpenAI-compat at `openrouter.ai/api/v1`.
    `supports_hebrew()` is `Confidence::Basic` at registry level (varies by
    routed model; exposed per-model in the picker).
  - `OllamaRemoteLlmProvider` — OpenAI-compat at a user-supplied base URL
    (`http://192.168.x.y:11434/v1` etc). `is_local()` returns `false`
    (traffic leaves the machine). `supports_hebrew()` is `Confidence::Basic`
    at registry level (depends on the hosted model).
  - `MiniMaxLlmProvider`, `DeepSeekLlmProvider`, `MistralLlmProvider`,
    `TogetherAiLlmProvider` — all thin wrappers over `async-openai` with
    their respective base URLs and default models. `supports_hebrew()`:
    - MiniMax: `Confidence::Basic` (no public Hebrew benchmark).
    - DeepSeek V3: `Confidence::Basic` (anecdotally decent; unverified formally).
    - Mistral Large 2: `Confidence::Basic` (multilingual but not Hebrew-focused).
    - Together AI: `Confidence::Basic` (varies by model).
  - All use `async-openai` (already in Cargo.toml) for the Chat Completions path;
    Anthropic uses `reqwest` directly (the Messages API is not OpenAI-compatible).
- API key storage: same `lashon-core::keychain` pattern as Phase 2, key names
  `"llm.anthropic"`, `"llm.openai"`, `"llm.groq"`, etc.
- `ProviderRegistry` for LLM populated.
- Tauri commands: `set_llm_provider(mode: String, id: String)` (mode is
  `"command"` or `"chat"` — each mode can use a different provider),
  `save_api_key` extended for LLM stage.
- Hub LLM section in `+page.svelte`:
  - Two sub-pickers: one for command mode, one for chat mode (cleanup is
    a later concern — the LLM cleanup pass was cut in M5 and is not planned
    for M7).
  - Per-provider: display name, cloud badge, Hebrew badge, API-key field,
    model picker dropdown, base-URL override (for self-hosted/proxy).
  - Test prompt field — type a short Hebrew sentence, click "test" — dispatches
    a `chat` call and shows the response inline in the Hub.
- He+en localization.

**Files touched:**

- `packages/shared-rust/src/llm.rs` — all cloud impls.
- `packages/shared-rust/src/lib.rs` — expose.
- `apps/desktop/src-tauri/src/lib.rs` — new commands.
- `apps/desktop/src/routes/hub/+page.svelte` — LLM section.
- `apps/desktop/src/lib/i18n/locales/{he,en}.json`.

**Risks:**

- Anthropic's Messages API format differs from OpenAI Chat Completions
  (different field names for tool definitions, different streaming format).
  The `LLMProvider::chat` method's input types (`Msg`, `Tool`) must be
  vendor-neutral; each impl is responsible for translating them. Define these
  types carefully — M8's tool registry will pass real tool schemas through here.
- `async-openai` is in the tech-stack doc and `Cargo.toml` (verify); if it is
  not yet in `lashon-core`'s deps, this is the phase where it lands.
- The LLM section does not wire up to anything that *uses* the LLM yet (Command
  and Chat modes are M8). The Hub test-prompt function is the only user-facing
  exercise in M7. Ensure the test prompt path uses the dictation FSM's error
  reporting so failures are visible.

---

### Phase 5 — Local LLM via Ollama (opt-in, 1–2 days, in M7 scope)

**Goal:** Ollama running locally (the user installs Ollama separately) is
offered as a local LLM option. This enables Tier A/B users to run
DictaLM-3.0-Nemotron-12B or another Hebrew-capable model without a cloud key.

**Deliverables:**

- `OllamaLocalLlmProvider` — OpenAI-compat endpoint probed at
  `http://127.0.0.1:11434/v1`. `is_local() → true`. On connection, the
  provider queries `/api/tags` to list installed models and populates the
  model picker. `supports_hebrew()` is derived from the model name: if the
  model name contains `dicta` or `hebrew` it reports `Confidence::Good`;
  otherwise `Confidence::Basic`.
- The Hub's LLM section shows a "Local (Ollama)" entry; if Ollama is not
  running, the entry is greyed out with a tooltip
  (`hub.llm.ollamaNotRunning` i18n key). A "connect" button probes the
  endpoint.
- `detect_ollama()` Tauri command — tests the `/api/tags` endpoint; returns
  `{running: bool, models: Vec<String>}`.
- He+en localization.
- **No model download in M7** — the user is expected to have pulled the
  model via `ollama pull dictalm3` themselves.

**Files touched:**

- `packages/shared-rust/src/llm.rs` — `OllamaLocalLlmProvider`.
- `apps/desktop/src-tauri/src/lib.rs` — `detect_ollama` command.
- `apps/desktop/src/routes/hub/+page.svelte` — Ollama entry in LLM section.
- `apps/desktop/src/lib/i18n/locales/{he,en}.json`.

**Assessment:** Phase 5 is low-risk because `OllamaLocalLlmProvider` is the
shared OpenAI-compat impl with `is_local() → true` and a configurable base
URL defaulting to `http://127.0.0.1:11434/v1`. The main risk is
endpoint-discovery variance (users run Ollama on non-default ports), which
the base-URL override field already handles. If Phase 4 spills, Phase 5's
small surface area lets it land in the same PR.

---

## Hebrew handling per provider

### STT

| Provider | `supports_hebrew()` | Notes | Test plan |
|---|---|---|---|
| `FasterWhisperProvider` | `Excellent` | ivrit-ai fine-tune on 295 h Hebrew | Production WER benchmark gates CI |
| `GroqSttProvider` | `Good` | Groq Whisper Large v3 — standard Whisper, not Hebrew-fine-tuned | Run `scripts/wer-bench.py` with `--provider groq` behind a `LASHON_GROQ_KEY` env var (CI-excluded) |
| `OpenAiSttProvider` | `Good` | `whisper-1` has reasonable Hebrew; `gpt-4o-transcribe` is untested | Same WER harness, `--provider openai` |
| `ElevenLabsScribeSttProvider` | `Basic` | ElevenLabs claims Hebrew Scribe support; no public WER available | Research scope: manual test with 20 corpus sentences; promote to `Good` if WER ≤ 20% |
| `DeepgramSttProvider` | `Basic` | Nova-3 Hebrew is beta per Deepgram docs | Research scope: manual test; publish findings in `docs/providers.md` |
| `AssemblyAiSttProvider` | `Good` | Universal-2 Hebrew cited in AssemblyAI docs | WER harness `--provider assemblyai` |

A provider's `supports_hebrew()` value must be honest. `Basic` means "it
accepts Hebrew audio and returns plausible text, but accuracy is unverified
against a benchmark." The UI distinguishes `Good`/`Excellent` with a
`✓ Hebrew` badge and `Basic`/`None` with `~ Hebrew (unverified)`.

### LLM

| Provider | `supports_hebrew()` | Notes | Test plan |
|---|---|---|---|
| `AnthropicLlmProvider` | `Excellent` | Claude handles Hebrew at near-native quality | Unit test: Hebrew prompt → Hebrew response (contains Hebrew codepoints) |
| `OpenAiLlmProvider` | `Excellent` | GPT-4-class models are strong in Hebrew | Same |
| `GroqLlmProvider` | `Good` | Llama 3.3 70B has solid Hebrew; Llama 4 Maverick is newer — check | Hebrew roundtrip test behind `LASHON_GROQ_KEY` |
| `MiniMaxLlmProvider` | `Basic` | MiniMax M2 has not been publicly benchmarked on Hebrew | **Research scope** — manual test with 5 Hebrew prompts before promoting |
| `DeepSeekLlmProvider` | `Basic` | DeepSeek V3 reportedly handles Hebrew but no formal evaluation | **Research scope** |
| `MistralLlmProvider` | `Basic` | Mistral Large 2 is multilingual; Hebrew quality anecdotally decent | **Research scope** |
| `TogetherAiLlmProvider` | `Basic` | Varies by routed model | Expose per-model in picker |
| `OpenRouterLlmProvider` | `Basic` | Varies by routed model | Expose per-model in picker |
| `OllamaRemoteLlmProvider` | `Basic` | Depends on what the user is hosting | |
| `OllamaLocalLlmProvider` | `Basic`/`Good` | `Good` if model name contains `dicta`/`hebrew` | |

Research-scope providers are those where the quality claim is unverified.
They ship with a `Basic` badge and a tooltip in the Hub noting that Hebrew
quality has not been benchmarked. The implementer should not promote them
to `Good` without running at least 20 Hebrew prompts covering code-switching
and verifying the response language is correctly Hebrew.

---

## Keychain integration

**`keyring` crate** is the chosen approach (already listed in
`docs/tech-stack.md`). It wraps:

- **Windows** — Windows Credential Manager (`wincred`). Keys appear under
  `lashon/<key_name>` in Credential Manager. No extra dependencies; the
  `windows` crate already in the workspace provides the underlying Win32
  bindings.
- **macOS** — macOS Keychain Services. The app's bundle ID scopes the items
  (`com.bustrama.lashon/<key_name>`). Requires the `keychain-access-groups`
  entitlement in the macOS build.
- **Linux** — `libsecret` (GNOME Keyring / KDE Wallet via the Secret Service
  D-Bus API). The `keyring` crate's `SecretService` backend covers both.
  **Caveat:** Linux headless environments (CI runners, SSH sessions, server
  installs without a keyring daemon) will fail to store or retrieve keys.
  Document this; offer a `LASHON_<STAGE>_<PROVIDER>_KEY` env-var fallback
  for headless use. Cloud provider tests in CI use these env vars; the
  keychain path is tested via a platform-specific integration test tagged
  `#[ignore]`.

**Key naming convention:**

```
service  = "lashon"
key_name = "<stage>.<provider>"   e.g. "stt.groq", "llm.anthropic"
```

**Tauri command surface:**

- `save_api_key(stage, provider, secret)` — writes to keychain, never echoes
  the secret back.
- `has_api_key(stage, provider) → bool` — the only read path exposed to the
  frontend; returns a boolean, never the raw key.
- The Rust command handlers call `lashon-core::keychain`; the keychain module
  loads the key for provider construction at registry init time (or on demand
  if the user switches providers mid-session).

**Key never leaves Rust:** the frontend can call `save_api_key` to store a
key and `has_api_key` to check presence; it can never retrieve the raw value.
Crash reports are explicitly scrubbed of any keychain data (the existing ADR-0010
pattern of "token never touches a log line" extended to all keys).

---

## Settings Hub UX for provider switching

See `docs/adr/0021-hub-provider-switching-ux.md` for the full decision record.
The summary:

**STT section (item 4 in `design-system.md`):**
- A chip-grid of available providers, one chip per provider. The active
  provider's chip is outlined in `--accent-aqua`.
- Local providers: no badge. Cloud providers: `☁` badge in `--text-muted`.
- Hebrew support: `✓` (Good/Excellent) or `~` (Basic/None) badge.
- Selecting a cloud chip reveals an API-key input below the grid (not a
  separate screen — inline, slide-down). The key is masked. A `✓ saved` pill
  replaces the input once a key is stored.
- "Test transcription" button: speaks a canned Hebrew sentence and shows the
  result.

**LLM section (item 5 in `design-system.md`):**
- Two sub-pickers: **Command mode** and **Chat mode**. Same chip-grid pattern.
- Below the active provider: model dropdown, API-key field (cloud only),
  base-URL override (collapsed, for self-hosted / proxy / Ollama remote).
- "Test prompt" field: a short Hebrew sentence, click send, result inline.
- **No spend-cap field.** Users set spend limits in each provider's own
  console.

**Persistence schema** (`tauri-plugin-store` / `settings.json`):
```json
{
  "stt.provider": "local-faster-whisper",
  "llm.command.provider": "anthropic",
  "llm.command.model": "claude-sonnet-4-6",
  "llm.chat.provider": "openai",
  "llm.chat.model": "gpt-4.1",
  "llm.anthropic.base_url": "",
  "llm.openai.base_url": "",
  "llm.ollama.base_url": "http://127.0.0.1:11434",
  "stt.openai.model": "whisper-1"
}
```

Keys follow the existing `<stage>.<sub>` convention; no top-level
restructuring. API keys are **not** in `settings.json` — they live in the
keychain only.

**Defaults:**

- STT defaults to `local-faster-whisper` (no regression for existing users).
- LLM command and chat mode default to `none` (no LLM, same as M0–M6 today).
  The LLM is not used until the user explicitly selects a provider — cloud is
  never the silent default.
- No provider change is ever forced silently; a cloud provider is only active
  if the user has explicitly selected it and stored a key.

---

## Test strategy

### Trait conformance tests (in `lashon-core`, no API calls)

A `MockSttProvider` and `MockLlmProvider` implement the respective traits with
fixed, predictable return values. Every test that exercises registry dispatch,
provider selection, or the Hub's Tauri command handlers uses mocks. Located in
`packages/shared-rust/src/stt.rs` and `llm.rs` test modules.

```rust
// Example:
struct MockSttProvider { response: &'static str }
impl SttProvider for MockSttProvider {
    async fn transcribe(&self, _pcm: &[f32], _lang: &str) -> Result<Transcript> {
        Ok(Transcript { text: self.response.to_string(), language: "he".to_string(),
                        confidence: 0.99, inference_ms: 1 })
    }
    fn supports_hebrew(&self) -> Confidence { Confidence::Excellent }
    fn is_local(&self) -> bool { true }
    fn name(&self) -> &str { "mock" }
    fn display_name(&self) -> &str { "Mock Provider" }
}
```

### Integration tests gated by env vars (not run in CI)

Each cloud provider has an integration test file tagged `#[ignore]` that
reads its key from an environment variable and makes a single real API call
with a known Hebrew audio clip / prompt, asserting the response contains
Hebrew codepoints. These run in local dev with `cargo test -- --ignored`
or in a manual pre-release gate:

```
LASHON_GROQ_KEY=...     cargo test -p lashon-core stt::groq -- --ignored
LASHON_OPENAI_KEY=...   cargo test -p lashon-core stt::openai -- --ignored
LASHON_ANTHROPIC_KEY=... cargo test -p lashon-core llm::anthropic -- --ignored
```

### CI: zero real API calls

CI runs `cargo test --workspace` which skips all `#[ignore]` tests.
No cloud credential is ever in the CI environment. The license scanner
(`cargo deny`) and the existing WER benchmark are unaffected.

### Hebrew regression test

For STT providers where Hebrew quality is claimed, the `wer-bench.py`
script gains a `--provider` flag that routes through the appropriate HTTP
client. This is a manual/pre-release gate, not CI-automated (costs API
credits).

---

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `Box<dyn SttProvider>` + `async_fn_in_trait` doesn't compose cleanly with Tauri state | Med | High | Evaluate in Phase 1; use `BoxFuture` / `async_trait` macro if needed; this is the first thing to validate |
| Cloud STT Hebrew accuracy worse than expected for providers rated `Basic` | Med | Med | Badge is honest; users see `~ Hebrew (unverified)`; they opt in knowingly |
| `keyring` on Linux requires a running Secret Service daemon (absent on CI / SSH) | High | Low | Document; provide env-var fallback; keychain integration test is `#[ignore]` |
| Anthropic Messages API serialisation vs OpenAI Chat Completions diverges at tool-call level | High (different formats) | Med | Define vendor-neutral `Msg`/`Tool` types in Phase 1; each impl translates — do not expose vendor-specific structs to callers |
| `async-openai` not yet in `lashon-core` Cargo.toml (it is in tech-stack doc but not confirmed in the crate) | Med | Low | Verify at Phase 4 start; add if missing |
| Migration from direct `FasterWhisperProvider` to registry breaks the dictation worker | Low | High | Phase 1 is solely this migration; no cloud providers involved; single PR, easy to revert |
| Deepgram + AssemblyAI WebSocket complexity (reconnect, partial-result reconciliation, backpressure) | Med | Med | Accept the complexity — cloud streaming is a headline M7 capability. Both impls land together so the same WS-handling shape is reused |
| User confusion: switching to a cloud LLM provider in the Hub but Command mode not yet wired (M8) | Med | Low | Hub test-prompt demonstrates the provider is working; make clear in Hub copy that Command mode uses the LLM from M8 onward |
| Secret leakage: API key entered in Hub could be logged or appear in crash reports | Low | High | Key never leaves the Rust layer; no "get_key" Tauri command; crash reporter scrubbed of keychain data (extending ADR-0010 precedent) |
| Token-cost runaway: user selects a cloud LLM, leaves Lashon idle, wake word triggers spurious takes | Low | Med | Cloud LLM not called in dictation mode (only STT routes to cloud); LLM is only called in Command/Chat, which require deliberate activation. Users set spend caps in the provider's own console — Lashon does not duplicate that. |
| MiniMax / DeepSeek Hebrew quality is worse than `Basic` implies | Med | Med | Badge is `Basic` = "untested"; promote only after manual evaluation; document in `docs/providers.md` |

---

## First PR scope

Phase 1 is the first PR. Its minimal shape:

1. `SttProvider` trait in `lashon-core::stt` gains `name()`,
   `display_name_key()`, and `transcribe_stream()` (default impl: buffer
   the PCM stream and delegate to `transcribe`); `FasterWhisperProvider`
   implements `name` + `display_name_key` and inherits the streaming
   default.
2. `lashon-core::llm` new module — trait + vendor-neutral type definitions
   (`Msg`, `Tool`, `Completion`, `Token`, `MsgContent`, `ContentBlock`)
   covering both Anthropic and OpenAI shapes. No impls yet.
3. `lashon-core::provider_registry` new module — generic registry with a
   single STT entry (the local sidecar); no dynamic dispatch yet beyond
   what already exists.
4. Dictation worker in `apps/desktop/src-tauri/src/dictation.rs` routes STT
   through the registry instead of holding a direct `FasterWhisperProvider`.
5. `docs/adr/0019-provider-mux-traits.md` — the trait ADR, ratifying the
   exact signatures, the `BoxFuture` call, and `TranscriptDelta`.

That is the smallest PR that: (a) proves the registry dispatches correctly,
(b) locks the trait shape that all later phases build against, and (c) carries
zero risk of regressing any existing user-facing behaviour.

---

## Resolved scope decisions

The five open questions in the planning draft are resolved as follows.

1. **Local LLM via Ollama — in M7 (Phase 5).** Ollama is a thin variant of
   the shared **OpenAI-compatible** LLM impl, pointed at
   `http://127.0.0.1:11434/v1` by default. The same impl serves LM Studio,
   `llama-server` (llama.cpp), Jan, mistral.rs, vLLM, and every hosted
   OpenAI-compat provider (Groq, Together AI, OpenRouter, DeepSeek,
   Mistral) — one provider impl, base URL swap. Anthropic is the only
   vendor with its own wire format and gets its own dedicated impl. The
   user installs Ollama separately; no model download in M7.

2. **`Msg` / `Tool` union covers both shapes.** The `MsgContent::Text /
   ToolResult / Blocks` enum in [ADR-0019](../adr/0019-provider-mux-traits.md)
   is locked: it expresses OpenAI-style string content + `tool_calls`,
   *and* Anthropic-style multi-block content with inline `ToolCall`
   blocks. Each provider impl translates to its vendor wire format
   internally; callers never see vendor-specific structs.

3. **Streaming STT in M7.** `transcribe_stream` is on the `SttProvider`
   trait from Phase 1 with a default impl that buffers the input PCM
   stream and delegates to `transcribe` — non-streaming providers stay
   conforming with zero per-provider code. Phase 2 ships **native
   WebSocket streaming** impls for Deepgram and AssemblyAI; the local
   sidecar's streaming wrapper is a follow-up (no regression on
   dictation).

4. **No app-side spend cap.** Users set spend limits in each provider's
   own console — every cloud provider exposes monthly caps in their
   dashboard. The Hub does not add a spend-cap field, and the
   `provider.spend_cap.*` key is dropped from the persistence schema.
   [ADR-0021](../adr/0021-hub-provider-switching-ux.md) and
   [ADR-0022](../adr/0022-cloud-opt-in-and-badging.md) reflect this.

5. **LLM dictation cleanup permanently cut.** M5's product decision
   stands. M7's LLM infrastructure does not re-introduce it; no `cleanup`
   mode entry appears in the Hub's LLM section.
