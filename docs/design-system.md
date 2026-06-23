# UI / UX design system

## Identity

**Concept:** "Tongue of Flame × Bezalel Glass" — dark-mode-first glassmorphism
lit with Mediterranean warmth. *Lashon* (לשון) is Hebrew for both *tongue* and
*language*; the dictation widget **is** that tongue — a **tongue of flame**
(לשון אש), the idiom for living, speaking fire. Not a Wispr Flow clone — no
white pill, no chrome. The **Tongue** is a chromeless, fully transparent overlay
that never changes shape, only how it is lit: the Lashon mark, breathing like a
low flame when idle, rising into a warm waveform as it listens, settling to
embers as it transcribes, and flaring gently as it speaks. *Bezalel Glass* —
crafted dark glass, after the Jerusalem craft tradition — is the material of the
larger surfaces (Hub, panels); the Tongue itself carries no glass, only light.

## Color tokens (dark mode primary)

```
--bg-deep         #0B0D12
--bg-glass        rgba(20,24,33,0.6)    + backdrop-filter: blur(24px) saturate(180%)
--bg-elevated     #161A24
--stroke-subtle   rgba(255,255,255,0.08)
--stroke-strong   rgba(255,255,255,0.16)
--text-primary    #F4EFE6                "Jerusalem stone"
--text-secondary  #A8A395
--text-muted      #6B6A63
--accent-citron   #E7D24A                primary CTA, "listening" glow
--accent-aqua     #3FCBC0                "Dead Sea", links, active states
--accent-violet   #7C5CFF                AI / LLM cleanup indicator
--state-recording #FF5470                recording dot, errors
--state-success   #5BD68A
--gradient-aurora linear-gradient(135deg, #E7D24A, #3FCBC0 50%, #7C5CFF)
```

Light-mode variants live in the design-tokens stylesheet. All combinations meet
WCAG AA at 14 px and above.

## Typography

- UI body: **Heebo** (Hebrew + Latin) 400/500/700
- Display: **Rubik** 500/700
- Mono: **JetBrains Mono** + **Miriam Mono CLM** for Hebrew code
- All self-hosted in `/static/fonts/`, no CDN

## Surfaces

| Surface | Purpose | When visible |
|---|---|---|
| **Tongue** | Always-on dictation widget | Always (can hide) |
| **Hub** | Main settings & history window | On-demand (tray click) |
| **Conversation Panel** | Slide-out chat-mode reply view | During/after chat-mode interaction |
| **Agent Panel** | Slide-out terminal for external agents | When an agent is running |
| **Onboarding Overlay** | First-run setup | Once |
| **Confirmation Modal** | Approve risky actions | On-demand |

## Tongue states

The Tongue keeps one form — the Lashon mark — across every state; what changes
is how it is lit and how it moves. Colour carries mode: citron = dictation,
citron+aqua = command, citron+violet = chat, violet = AI/cleanup, aqua = tools,
recording-red = error.

| State | Visual | When |
|---|---|---|
| Idle | The mark breathing like a low flame — a slow citron-warm glow rising and falling | No interaction |
| Preparing | The mark dimmed, a slow guttering pulse — the flame not yet caught | First-run model download |
| Listening (dictation) | The flame leans into the voice — a warm citron waveform tracking the audio, soft halo | Push-to-talk active |
| Listening (command) | The same waveform, lit cobalt blue (the `--garnet` token) | Command hotkey or wake-word |
| Listening (chat) | The same waveform, edged citron→violet | Chat hotkey |
| Transcribing | The flame settles to drifting embers — pulsing violet dots | After release, STT running |
| Polishing | A violet aurora licks across the embers | Cleanup LLM running |
| Tool calling | A turning aqua spark + tool-name caption | Command-mode tool execution |
| Speaking | The flame flares and breathes with the speech — violet wave, mute button visible | TTS playing |
| Confirm | A steady citron ring held around the flame, prompt text + 2 buttons (כן / לא) | Awaiting user confirmation |
| Error | The flame snaps to recording-red — one sharp flicker, dismissable | Failure path |
| Wake-listening | A pilot light — the mark at its smallest, a faint concentric pulse | Wake-word always-on |

## Conversation panel

- Slide from the right edge, 420 px wide, glass-card style.
- Header: mode badge (`💬 שיחה` / `⚙ פקודה`), provider chip
  (`Claude Sonnet 4.6` / `DictaLM 3.0`), cloud indicator if cloud.
- Message stream:
  - User bubble (Hebrew right-aligned, English left-aligned automatically via
    `dir="auto"`) with an audio-replay button.
  - Lashon bubble with streamed text, copy button, audio-replay button.
  - Tool-call cards: collapsed by default, expand to show JSON args and result.
- Input footer: text-input fallback (for typed follow-ups), provider switcher,
  "stop" button.

## Agent panel

- Same slide-out region as the Conversation Panel; tabs at the top:
  `Conversation | claude-code | opencode | …`.
- Per-agent tab: an xterm.js terminal, a status pill
  (`running` / `waiting input` / `exited 0`), and a "send transcript as input"
  button (hold the dictation hotkey, speak, release → the text goes into the
  focused agent's stdin instead of an OS-level paste).
- Multiple agents can run in parallel; tabs are reorderable.

## Hub layout

A left sidebar (260 px) plus a resizable right detail pane (960 × 640 default).

Sections:

1. כללי / General — language, theme, autostart, position
2. קיצורי דרך / Shortcuts — three hotkeys, conflict warnings
3. שמע / Audio — input device, gain, VAD sensitivity, ducking enabled
4. **STT** — provider picker, model picker, "test transcription" button
5. **LLM** — provider picker per mode (cleanup/command/chat), model picker, API-key fields, test prompt
6. **TTS** — provider picker per mode (command/chat), voice picker with sample, streaming toggle
7. **סוכנים / Agents** — detect installed external agents, configure paths, default agent
8. מילון / Dictionary — corrected word table, import/export JSONL; plus a per-user Hebrew→English word list, auto-applied to transcripts, for English words the STT model transliterates into Hebrew letters that the user wants kept in English (e.g. ריליס → release)
9. קטעים / Snippets — shortcut → expansion
10. מילת השכמה / Wake word — current model, sensitivity, train new
11. **כלים / Tools** — per-tool enable + confirmation policy
12. **זיכרון / Memory** — view/edit/delete known facts, export dump
13. **היסטוריה / History** — last 1000 interactions with audio replay
14. **פרטיות / Privacy** — telemetry toggle (off by default), data location, "delete all" button
15. **אודות / About** — version, licenses, model credits

## RTL & accessibility

- `dir="rtl"` when the UI language is Hebrew; logical CSS properties throughout.
- Bidi isolates around mixed Hebrew+English fragments.
- All ARIA-live regions announce state changes.
- Full keyboard navigation; a 3 px aqua focus ring.
- `prefers-reduced-motion` disables springs and the waveform animation.
