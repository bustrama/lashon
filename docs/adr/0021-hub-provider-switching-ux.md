# 21. Hub UX for provider switching

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** Milestone M7, Phases 3–4
  ([`../stories/m7-provider-mux.md`](../stories/m7-provider-mux.md));
  see also [`../design-system.md`](../design-system.md) Hub layout items 4–5
  and [`../architecture.md §4`](../architecture.md).

## Context

The Settings Hub in `docs/design-system.md` already names STT (item 4) and
LLM (item 5) sections. Until M7 these sections are placeholders. M7 fills
them in with functional provider pickers.

Four design decisions are needed:

1. **Where** in the Hub do provider controls live (a single section, or
   split by stage and mode)?
2. **What** does each control expose (picker, key entry, model dropdown,
   test action)?
3. **How** are settings persisted (keys, schema, defaults)?
4. **How** is the cloud-vs-local state communicated outside the Hub (in the
   Tongue and the Conversation panel header)?

## Decision

### Hub section structure

The Hub has two new filled-in sections, replacing the placeholders:

**Section 4 — STT (Speech-to-Text)**

A single provider picker for STT — there is only one STT path (dictation).

```
┌─ STT ─────────────────────────────────────────────────────────┐
│  Provider                                                      │
│  [ Local (ivrit-ai) ✓he ] [ Groq ~ ☁ ] [ OpenAI ✓he ☁ ] …   │
│                                                                │
│  [if cloud selected]                                           │
│  API key  ●●●●●●●● ✓ saved          or  [ Enter API key ]     │
│  Model    ▼ whisper-large-v3                                   │
│  Base URL ▸ (custom endpoint, optional)                        │
│                                                                │
│  [ Test transcription ]  "שלום, זה בדיקה" → שלום זה בדיקה   │
└────────────────────────────────────────────────────────────────┘
```

**Section 5 — LLM (Language Model)**

Two sub-pickers: one for **Command mode** and one for **Chat mode**.
The cleanup-LLM mode (cut in M5) is not included.

```
┌─ LLM ─────────────────────────────────────────────────────────┐
│  Command mode                                                  │
│  [ None ] [ Anthropic ✓he ☁ ] [ OpenAI ✓he ☁ ] [ Groq ✓ ☁ ] │
│                                                                │
│  Chat mode                                                     │
│  [ None ] [ Anthropic ✓he ☁ ] [ OpenAI ✓he ☁ ] …             │
│                                                                │
│  [if cloud selected for either mode]                           │
│  API key  ●●●●●●●● ✓ saved                                     │
│  Model    ▼ claude-sonnet-4-6                                  │
│  Base URL ▸ (custom endpoint, optional)                        │
│                                                                │
│  [ Test prompt ]  "מה השעה?"  → "זה LLM לא יודע השעה…"       │
└────────────────────────────────────────────────────────────────┘
```

The "None" chip means no LLM is active for that mode — matching the M0–M6
default. Selecting "None" does not call any provider; Command mode in M8
will check the active LLM and warn if it is None.

### Provider chip anatomy

Each chip carries three visual signals:

```
[ <display name>  <hebrew-badge>  <cloud-badge> ]
```

- **Hebrew badge:** `✓` (Confidence::Good or Excellent), `~` (Basic), absent
  (None). Colour: `--state-success` for `✓`, `--text-muted` for `~`.
- **Cloud badge:** `☁` when `is_local() == false`. Colour: `--text-muted`.
- **Active state:** chip outline in `--accent-aqua` (matching the existing
  active-state pattern from the Hardware section's tier picker).

The chips are rendered from the `Vec<ProviderMeta>` returned by
`get_stt_providers()` / `get_llm_providers(mode)` Tauri commands. The
chip grid is RTL-native: in Hebrew mode chips flow right-to-left.

### API key inline reveal

When a cloud provider chip is selected and no key is stored, the section
below the chip grid reveals an API-key input (slide-down, not a modal).
The input is `type="password"` (masked). On blur / save:

1. The frontend calls `save_api_key(stage, provider, raw_value)`.
2. The input is replaced with `●●●●●● ✓ saved` (a mask + a `has_api_key`
   check confirming storage succeeded).

The raw key value never appears in any Svelte `$state` that gets serialised
to the Tauri store; it is only held in the DOM input long enough to pass to
`save_api_key`. The SvelteKit component never reads it back.

A `× clear key` affordance calls `delete_api_key(stage, provider)` and
removes the stored value.

### Model picker

Visible when a provider that exposes multiple models is active. A `<select>`
dropdown populated from `ProviderMeta.available_models`. The chosen model
is saved in `settings.json` as `<stage>.<provider>.model` (e.g.
`"stt.openai.model": "gpt-4o-transcribe"`).

### Base-URL override

A collapsed `▸ Custom endpoint` expander (default closed). Useful for:
- OpenAI API proxy / Azure OpenAI.
- Self-hosted Ollama remote.
- Corporate API gateway.

Saved as `settings.json` key `"<stage>.<provider>.base_url"`.
When empty the provider uses its default endpoint.

### Test actions

**Test transcription (STT):**

A canned 16-bit WAV clip is bundled as a Tauri resource. The button invokes
`test_stt_transcription()` which sends the clip through the active STT
provider and shows the result inline below the button. The clip is the Hebrew
sentence "שלום, זה בדיקה" (approximately 1.5 s). On success the result
appears in green; on failure the error message appears in red.

**Test prompt (LLM):**

A short text input (max 100 chars). The button invokes `test_llm_prompt(mode,
text)` which sends the text as a single `User` message and shows the response.
The default prompt is "שלום" (a Hebrew greeting) — if the LLM responds in
Hebrew, that is a reasonable signal. On failure the error is shown inline.

Neither test action auto-triggers; the user must click the button. Neither
result is stored.

### Persistence schema (`settings.json`)

New keys added in M7 (existing keys untouched):

```json
{
  "stt.provider": "local-faster-whisper",
  "stt.openai.model": "whisper-1",
  "stt.openai.base_url": "",
  "stt.groq.base_url": "",
  "stt.deepgram.base_url": "",
  "llm.command.provider": "none",
  "llm.command.model": "",
  "llm.chat.provider": "none",
  "llm.chat.model": "",
  "llm.anthropic.base_url": "",
  "llm.openai.base_url": "",
  "llm.groq.base_url": "",
  "llm.minimax.base_url": "",
  "llm.deepseek.base_url": "",
  "llm.mistral.base_url": "",
  "llm.together.base_url": "",
  "llm.openrouter.base_url": "",
  "llm.ollama.base_url": "http://127.0.0.1:11434"
}
```

Defaults: `stt.provider = "local-faster-whisper"` (no regression);
`llm.*.provider = "none"` (LLM not active until the user selects one — cloud
is never the silent default).

### Cloud badge in the Tongue and Conversation panel header

When the active STT provider is cloud (`is_local() == false`):

- The Tongue shows a small `☁` chip overlaid at the top-right of the Lashon
  mark during transcription. The chip uses `--text-muted` and the provider's
  `display_name_key` as a tooltip.
- The Conversation panel header already has a `provider chip`
  (`docs/design-system.md` Conversation panel section). In M7 this chip is
  populated from the active LLM provider's name and gains the `☁` suffix for
  cloud providers.

The badges are implemented via a Tauri event `provider:active-changed` emitted
whenever `set_stt_provider` or `set_llm_provider` is called. The Tongue and
Conversation panel components listen for it.

### The "None" default for LLM

`"none"` is a valid, explicit value for `llm.command.provider` and
`llm.chat.provider`. When `none` is active:

- Dictation mode is unaffected (it never calls the LLM).
- Command mode (M8) will warn at invocation: "No LLM configured for command
  mode — choose a provider in Settings."
- Chat mode (M8) will warn similarly.

This is the same UX pattern as the Hardware section's tier-change restart
note: a configuration gap is surfaced at use time, not at startup.

## Alternatives considered

- **A wizard / separate setup flow for cloud providers** — more hand-holding
  but more disruption. The Hub inline reveal is sufficient for a developer /
  power-user audience; a guided setup can come later if user research shows
  friction.
- **Provider configuration in a separate Settings window** — would require a
  new Tauri window; the Hub's existing single-window pattern is sufficient.
- **Showing the raw API key in a "reveal" mode** — rejected on security
  grounds (see ADR-0020). The `has_api_key` boolean is the only frontend-visible
  signal about key presence.
- **A single "provider" section covering STT + LLM + TTS together** — design
  space becomes cluttered; the per-stage section structure in
  `docs/design-system.md` is already correct, and section 4 (STT) and 5 (LLM)
  are the right granularity.

## Consequences

- Hub `+page.svelte` gains an `'stt'` and an `'llm'` section — two new
  entries in the `SECTIONS` constant and two new `{#if section === 'stt'}` /
  `{#if section === 'llm'}` blocks.
- The `settings.ts` typed module is extended with the new keys.
- `he.json` and `en.json` gain all new Hub strings; key parity must be
  maintained (see ADR-0011).
- `Tongue.svelte` gains a cloud-badge overlay (single conditional element;
  no new state complexity — it listens for `provider:active-changed`).
- The Tauri shell gains `get_stt_providers`, `get_llm_providers`,
  `set_stt_provider`, `set_llm_provider`, `test_stt_transcription`,
  `test_llm_prompt`, `delete_api_key` commands.
- The capability file `capabilities/hub.json` may need the new commands
  declared; follow the same capability-extension pattern as M6's
  `install_wake_model` command.
