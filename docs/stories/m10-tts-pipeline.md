# M10 — TTS pipeline

> **Status: Planning.** This story is the entry point for Phase 3 (voice
> response). The roadmap entry is "Piper local default; streaming
> sentence pipeline; audio ducking; voice picker." Phase 3's broader
> Definition of Done (cloud providers, voice-quality bar) carries over
> to M11.

## Why

End of M9 leaves Lashon mute. Command mode lands a transcript bubble
on the tongue, the dispatcher reports `command:result`, and the user
reads it. For the four common chat-app recipes the visual receipt is
fine — the recipe executed, the user can see "נשלח". But the value
proposition the product promises is **voice in, voice out**: Lashon
acknowledges, confirms, asks for clarification. That's what closes
the loop for hands-free use.

The smallest version of Phase 3 — a Hebrew voice that speaks the
already-emitted `command:result.text` — unblocks every downstream
flow (chat mode, error speech, confirmation prompts, recipe-success
chimes upgraded to spoken text). M11 then adds the cloud providers
that the user can upgrade to; M10 makes the **default-on, local-only,
zero-config path** work.

## Scope (this milestone)

### In scope

- **`TTSProvider` trait** in `lashon-core::tts` mirroring `LLMProvider`
  (M7) — `synthesize`, `stream`, `voices`, `supports_hebrew`,
  `is_local`. The architecture doc's illustrative signature
  (`docs/architecture.md`) is the starting point; the authoritative
  trait lives in code.
- **One built-in local provider — Piper.** Default Hebrew voice
  bundled in the installer (~30–50 MB, MIT-licensed). The
  installation pattern matches the existing wake-word default model
  (`models/wake/wakewords/hey_lashon.onnx`): tracked in the source
  tree, shipped via Tauri resources, never downloaded.
- **Audio output path.** A `cpal` output stream the Rust core owns,
  fed by a PCM queue. The chime path stays on Web Audio in the
  frontend (it's UI-locality, not provider output).
- **Streaming sentence pipeline.** A small `SentenceSplitter` that
  takes a token stream (or a one-shot string), splits on
  `.!?،؛؟\n` plus Hebrew sentence-end heuristics, and hands each
  ready sentence to the active provider. v1 implementation only
  needs the one-shot path; the token-stream interface is reserved
  for the chat-mode shape so M12+ can plug in.
- **`is_speaking` gate.** A new `Arc<AtomicBool>` in `Gates`
  alongside the existing `is_capturing`. The wake-word worker
  already gates on capture; it gets the same gate on speaking so
  the wake never self-triggers on Lashon's own audio. Set true
  before the first PCM chunk hits the output stream, false on
  end-of-stream.
- **Audio ducking via Silero VAD.** A second `SileroVad` instance
  runs while `is_speaking == true`, fed from the existing
  capture pipeline's PCM (suspending the dictation worker is wrong
  — the mic stream stays open). On voice detection: pause the
  output stream within 150 ms (the DoD budget) and either resume
  from the next sentence boundary or cancel the whole queue.
- **Tongue `speak` state.** New branch in `tongueState`, violet
  halo (`--indigo`/`--accent-violet`), gentle pulse animation. The
  existing design-system row for Speaking documents the intent;
  M10 makes it real.
- **Hub TTS section.** Mirror the LLM section's chip grid +
  per-mode picker. v1 ships **one chip (Piper local) and one
  voice**, but the UI shape is the M11-ready scaffold: per-mode
  picker (Command / Chat), masked API-key slot disabled-but-present,
  sample-play button, settings persistence under `tts.command.*` /
  `tts.chat.*`.
- **Spoken command-mode confirmations.** Wire the
  `command:result.text` payload through the new TTS path. The
  existing tool-summary fallback is the line that gets spoken when
  the LLM returns no prose. Per the Phase-3 DoD ≤ 400 ms first-byte
  latency on Tier A — Piper hits this on CPU.
- **Per-mode default.** Command uses the fastest (Piper); chat-mode
  default placeholder is "same as command" until M11 adds quality
  options.
- **Settings keys.** `tts.command.provider`, `tts.command.voice`,
  `tts.chat.provider`, `tts.chat.voice`, `tts.enabled` (master
  on/off), `tts.ducking.enabled`. All persisted via the existing
  `tauri-plugin-store` settings.json.

### Out of scope (deferred to M11+)

- **Cloud TTS providers** — ElevenLabs, Azure, Google, OpenAI,
  Cartesia, PlayHT. M11.
- **Optional local heavyweights** — MMS-TTS-heb, Coqui XTTS-v2,
  F5-TTS, OpenVoice v2. CC-BY-NC and CPML-licensed; surfaced as
  opt-in downloads with a non-commercial badge. M11.
- **Voice-clone consent workflow** — per scope rules in roadmap.md
  this is explicitly v2.
- **Chat mode itself** — chat hotkey, conversation panel, the
  Streamed-LLM-into-TTS loop. M10 ships the foundation
  (sentence-aware streaming, audio output, ducking) so chat mode
  can be built on top in a later milestone without re-doing
  plumbing.
- **TTS sidecar** (the existing `tts.proto`). v1 keeps Piper
  in-process; the sidecar only earns its keep when MMS/XTTS
  arrive, and the proto is already stable enough to wait.
- **History tab audio replay** — needs M12's history.db. Out of
  scope here.

## Open decisions (need to lock before Phase 1 lands)

The following design choices need ADRs in the same PR as the code
that implements them:

1. **Piper integration shape** (ADR-0033). Two options:
   - **(A) Sidecar binary.** Spawn the official `piper.exe` /
     `piper` binary, write text on stdin, read 16-bit PCM on
     stdout. Matches the existing pattern (STT sidecar via gRPC,
     llama-server via HTTP). Adds ~5 MB of bundled binary +
     ~30–50 MB voice ONNX.
   - **(B) Embedded via `piper-rs`.** Rust crate that wraps
     piper-cpp as a static library. Tighter integration, no IPC
     overhead, one less subprocess. Trade-off: piper-cpp build
     complexity, and `piper-rs` is a single-maintainer crate.
   - **Recommendation: (A) sidecar.** Consistent with STT and
     llama-server, isolates Piper's native code, doesn't add a
     C++ build dependency to the Rust toolchain, ~5 ms IPC
     overhead is well under the 400 ms first-byte budget.
2. **Audio output crate choice** (ADR-0034). `cpal` directly vs
   `rodio` on top.
   - `cpal` — already pinned for input; we know the version works
     on all three runners. Lower-level: we manage the ring buffer,
     format negotiation, sample-rate conversion (via `rubato`),
     pause/resume. ~150 lines of plumbing.
   - `rodio` — Sink abstraction with built-in queue, pause/resume,
     volume. ~30 lines of plumbing but adds a dep + we're not in
     control of the pause latency (relevant for the 150 ms
     ducking budget).
   - **Recommendation: `cpal` directly.** No new dep, deterministic
     pause latency, low-level control we'll need for ducking anyway.
3. **Default voice selection.** Which Piper Hebrew voice ships in
   the installer.
   - Piper publishes community voices at
     `huggingface.co/rhasspy/piper-voices/tree/main/he/he_IL/`. The
     `he_IL/UNKtest` voices are placeholder; the maintained voices
     vary in quality. We need to listen to candidates and pick
     one that passes the Phase-3 DoD voice-quality bar (≥ 4/5
     native speakers).
   - **Action:** I'll list the available `he_IL` voices, you pick
     the default before Phase 1 ships.
4. **Auto-speak policy for command mode.** Two questions:
   - Should the user opt in once (a Settings switch) or is TTS
     speech default-on for command results?
   - When the dispatcher returns prose (`text`) AND a tool summary
     (`tool_summaries`), what gets spoken? Just the prose? Both?
     Currently the tongue's flash already prefers prose and falls
     back to the last tool summary — the same rule probably applies
     to speech.
   - **Recommendation:** Default-on with a single "Speak responses"
     toggle in Hub (so a user who hates TTS can mute the whole
     loop). The flash's prose-or-summary rule maps cleanly to speech.

## Phased PR plan

Five PRs, each independently shippable and reviewable. The phasing
matches the M9 cadence (one PR per coherent slice, each ~300–800
lines of code plus tests).

### Phase 1 — Piper sidecar + audio output + first spoken command result

- `lashon-core::tts` module with the `TTSProvider` trait, a
  `ProviderRegistry<TTSProvider>`, and the Piper provider impl
  (sidecar binary spawn + stdin/stdout protocol).
- `lashon-core::audio::output` — cpal output stream, PCM queue,
  pause/resume API.
- Bundle the chosen default Hebrew voice + the `piper` binary as
  Tauri resources; integrity-verify via `models/manifests/`.
- New Tauri command `tts_speak(text)` and a wire into
  `command_mode::run` so `command:result.text` is spoken.
- Add `is_speaking` to `Gates`; the wake-word worker reads it.
- Add `speak` state to `Tongue.svelte`, violet halo, ARIA-live
  region updates while speaking.
- Master "Speak responses" toggle in Hub (skeleton TTS section).
- ADR-0033 (sidecar choice) + ADR-0034 (cpal direct).
- DoD: speak a Hebrew command result end-to-end. Wake word does
  not self-trigger on TTS output. Tongue lights violet during
  speech.

### Phase 2 — Sentence-streaming pipeline

- `SentenceSplitter` that splits a string on `.!?،؛؟\n` + Hebrew
  end-of-sentence heuristics (verify against the corpus already in
  `tests/hebrew-corpus/`).
- Audio chunk queue that orders PCM by sentence index, plays
  sequentially.
- Refactor `TTSProvider::synthesize` to `stream` internally
  (sentence-at-a-time), keep the one-shot API on top.
- DoD: a 3-sentence Hebrew response begins playing within
  500 ms of the first sentence being ready; subsequent sentences
  queue without gaps.

### Phase 3 — Voice picker + per-mode Hub UI

- Hub TTS section v2: per-mode picker (Command / Chat), voice
  dropdown for each, sample-play button per voice.
- `get_tts_providers`, `set_tts_provider`, `get_tts_voices`,
  `test_tts_voice` Tauri commands (mirror M7's LLM commands).
- Settings persistence under `tts.command.*` / `tts.chat.*`.
- DoD: switching the active provider or voice from Hub takes
  effect on the next `tts_speak` call without an app restart.

### Phase 4 — Audio ducking via Silero VAD

- Second `SileroVad` instance constructed and held by the TTS
  audio output module; subscribed to the capture stream's PCM.
- On `is_speaking == true`, VAD runs every 32 ms frame; on a
  voice-detected event, pause the output queue within 150 ms;
  resume from next sentence boundary or cancel.
- Settings key `tts.ducking.enabled` (default true).
- DoD: speak a long Hebrew response, interrupt with the wake
  word — Lashon stops within 150 ms (measured with `tracing`),
  resumes on the next sentence or cancels per setting.

### Phase 5 — Hardening + WER-style voice-quality probe

- Hebrew voice-quality smoke test (the Phase-3 DoD "20 Hebrew
  sentences ≥ 4/5 by 3 native reviewers" is a manual gate, but
  we can add a "synthesize → re-transcribe → diff" automated
  smoke check that surfaces obvious regressions).
- Error path: synthesis failure speaks a tongue error banner
  (Phase 3 DoD #2) — until M11 cloud providers land, "synthesis
  failure" is rare, but the path is needed.
- Performance budget assertions: first-byte latency ≤ 400 ms on
  Tier A (CI runner with sample audio).

## What lands after M10

- **M11** adds the cloud TTS catalogue (ElevenLabs, Azure,
  Google, OpenAI, Cartesia, PlayHT) following the M7 pattern. The
  trait shape and Hub UI shipped here mean the cloud half is
  pure addition.
- **Chat mode** (currently unscheduled — likely M12+ or its own
  milestone) reuses the sentence-streaming pipeline from Phase 2
  and the audio output path from Phase 1.
- **Voice-clone consent workflow** is v2; Coqui XTTS / OpenVoice
  arrive in M11 with a "non-commercial license" badge gate.

## Open questions for the user

Before Phase 1 ships:

1. Which Piper Hebrew voice is the default? (I'll inventory the
   available `he_IL` voices on Hugging Face and propose two
   candidates; you pick.)
2. Default-on auto-speak for command results, or
   default-off-with-opt-in? My recommendation is default-on, but
   the inverse is a defensible "less surprising on first launch"
   choice.
3. Sidecar Piper vs embedded `piper-rs`? Recommendation in the
   ADR draft is sidecar; the inverse is fine if you'd rather
   avoid spawning yet another subprocess.

The Piper-sidecar and cpal-output ADRs were never written — M10 (TTS) was
deferred before Phase 1 began.
