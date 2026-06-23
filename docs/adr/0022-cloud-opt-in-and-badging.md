# 22. Cloud opt-in rules and provider badging

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** Milestone M7
  ([`../stories/m7-provider-mux.md`](../stories/m7-provider-mux.md));
  `.claude/rules/security.md`; `docs/soul.md`; `docs/architecture.md §4`.

## Context

Lashon's first principle is local-first and privacy-respecting by construction.
`docs/soul.md` states: "The user owns their data. You operate locally by
default." `.claude/rules/security.md` states: "Never make a stage default to a
cloud provider. Cloud is always opt-in and always badged."

M7 introduces the first cloud providers. Without a formal policy, future
contributors could inadvertently:

- Default new cloud providers to active.
- Add cloud providers without a visible badge.
- Route audio or transcript data to the cloud silently (e.g. a helper
  function that "just sends a quick check to the API").
- Present Hebrew-quality claims that are not backed by any evidence.

This ADR codifies the invariants that make cloud an opt-in, transparent
choice, and defines what "badged" means in every surface of the UI.

## Decision

### Invariant 1: No cloud default, ever

The default value for every `<stage>.provider` settings key must resolve to a
local provider or to `"none"`. A fresh install of Lashon at any milestone must
never route audio, transcript, or text to a cloud provider without the user
having explicitly selected that provider.

This is enforced in the `ProviderRegistry` default-selection policy
(ADR-0019): when no provider has been explicitly set, the registry selects the
first `is_local() == true` provider with the highest `supports_hebrew()`
confidence. There is no code path that selects a cloud provider by default.

The `settings.json` initial values are:

```
stt.provider      = "local-faster-whisper"
llm.*.provider    = "none"
tts.provider      = "local-piper"   (M10)
```

Any PR that changes these defaults to a cloud provider is a breaking security
invariant and must be rejected.

### Invariant 2: `is_local()` must be honest

`is_local()` returning `true` must mean that inference runs on this machine
and no audio, PCM, or transcript leaves the machine boundary. It is not a
statement about whether the model weights were downloaded from the internet
(they were); it is a statement about runtime data flow.

**`is_local() == false`** implies: the caller's audio PCM or transcript text
is transmitted over the network to a third-party server. This is a
privacy-relevant fact that must be surfaced to the user.

`OllamaRemoteLlmProvider` (a user's home LAN Ollama) has `is_local() == false`
even though the data stays in the user's home network — because the data
leaves the machine boundary and the privacy posture is different from
purely local inference.

### Invariant 3: `supports_hebrew()` must be honest

`supports_hebrew()` must reflect evidence, not aspirations or vendor marketing:

| Confidence | Meaning | How to earn it |
|---|---|---|
| `None` | Provider does not accept Hebrew audio / text | Verified rejection or no Hebrew language support claimed |
| `Basic` | Provider accepts Hebrew; quality unverified by Lashon | Vendor docs claim Hebrew support; no independent benchmark |
| `Good` | Usable Hebrew quality | Either: the WER benchmark via `scripts/wer-bench.py --provider <x>` shows WER ≤ 25%; or: a manual test of 20 corpus sentences shows plausible output with no systematic failures |
| `Excellent` | Benchmarked Hebrew quality | WER ≤ 15% on `tests/hebrew-corpus/`, or independently benchmarked and documented in `docs/providers.md` |

A provider whose `supports_hebrew()` is set higher than the evidence supports
is a trust violation — the badge misleads the user into choosing a provider
whose Hebrew quality they cannot rely on.

**Research-scope providers** (MiniMax, DeepSeek, Mistral, Together AI,
OpenRouter, Deepgram, ElevenLabs Scribe) ship at `Confidence::Basic` in M7.
Promotion to `Good` requires evidence documented in `docs/providers.md`.

### Invariant 4: Every active cloud provider is badged

A cloud provider (`is_local() == false`) must display a `☁` badge in every
surface where it is named:

1. **Hub provider chip** — `☁` in `--text-muted` next to the provider name.
2. **Tongue widget** — a small `☁ <provider name>` overlay during transcription
   or LLM inference (see ADR-0021).
3. **Conversation panel header** — the provider chip includes `☁` and the
   provider name.

The badge must be present whenever a cloud provider is *active*, not merely
configured. A user who has an API key stored but has not selected the provider
sees no badge.

### Invariant 5: Key confirmation before first cloud use

When a cloud provider is selected for the first time and the user has not yet
stored an API key, the provider is set as the chosen provider in settings but
is not actually used until a key is entered. The dictation FSM returns a
`ProviderError::KeyNotFound` at call time, which surfaces as a toast:

"Cloud provider {name} needs an API key — configure it in Settings → STT" (or
LLM). This is the M7 UX; a future milestone may add a more guided flow.

The provider is never attempted without a key. An empty or missing key never
causes the provider to silently fail over to the local default — it surfaces
as an explicit, actionable error.

### Invariant 6: Audio is never transmitted to cloud in dictation mode without consent

If the active STT provider is cloud, the Tongue's listening state (the
waveform animation) carries the `☁` badge continuously while the mic is open.
The user is never left wondering whether their audio is going to a cloud
server during a dictation take.

### Rate-limit and cost surfacing (M7 baseline)

In M7, cost and rate-limit surfacing is minimal but not absent:

- The Hub does **not** show an app-side spend-cap field. Every cloud
  provider exposes monthly spend caps in their own console (Anthropic,
  OpenAI, Groq, etc.); duplicating that in Lashon is not worth the
  implementation cost. If a user-facing need emerges later, a spend-cap
  is its own dedicated milestone.
- When a cloud LLM provider returns a rate-limit error (HTTP 429), the error
  toast includes the provider name and the rate-limit message. The dictation
  FSM does not retry automatically (it surfaces the error to the user).
- Token usage from LLM responses is logged at `tracing::debug!` level
  (never at info or above, never to disk) in M7. A usage card in Settings
  is a post-M8 polish item.

### Licensing gate: CC-BY-NC and CPML models

This is an existing rule, not new to M7, but applied consistently:

- Any cloud provider that bundles or requires a CC-BY-NC or CPML model
  (rather than a model the provider licenses to the user under a commercial
  terms) must be surfaced with a "non-commercial" badge alongside the `☁`
  badge. In practice, cloud API providers (Groq, OpenAI, Anthropic, etc.)
  provide access to their models under their own commercial API terms; no
  non-commercial badge is required for them.
- This rule applies to any future cloud provider that routes through a
  model the user must separately agree to a non-commercial license for.

## Alternatives considered

- **Opt-out rather than opt-in** — rejected categorically. "Local by default"
  is a product promise, not a preference. A user who never opens Settings
  should never have their audio silently routed to a cloud API.
- **Show the cloud badge only in Settings** — rejected. The badge must be
  visible at the point of use (the Tongue, the Conversation panel) so the
  user is reminded during every interaction, not only in Settings.
- **A one-time consent banner for cloud use** — insufficient. Users forget
  consents they gave months ago. The per-operation badge is a continuous,
  low-friction reminder.
- **Trust provider self-reporting for `supports_hebrew()`** — rejected. The
  `Confidence::Basic` default for unverified providers, with a clear badge,
  is more honest than trusting a vendor's marketing claim.

## Consequences

- Every new provider added in any future milestone must implement `is_local()`
  and `supports_hebrew()` honestly or the PR review must reject it.
- A CI lint rule could check that no `stt.provider`, `llm.*.provider`, or
  `tts.provider` default value in any `settings.json` or settings init code
  references a provider with `is_local() == false`. This is not enforced in
  M7 (no automated check exists) but is a candidate for M8 or a tooling
  milestone.
- `docs/providers.md` becomes the authoritative record of evidence for each
  provider's `supports_hebrew()` rating. When a provider is promoted from
  `Basic` to `Good`, the promotion must be accompanied by a `docs/providers.md`
  update documenting the test methodology and result. A PR that promotes the
  enum value without updating `docs/providers.md` is incomplete.
- The `☁` badge in the Tongue is a new UI element. Its presence must be
  covered by the `npm run check` and `svelte-check` gates (type safety of the
  conditional rendering), and the accessibility rule: ARIA-live region must
  announce the active provider when it changes, including the cloud/local
  status, so a screen-reader user knows what is processing their audio.
