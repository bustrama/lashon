# UI / UX design system

## Identity

**Concept: "The Lamp" × Ottid.** The room is a cool slate study at night and
cool bone parchment by day. In it lives **Ottid**, a small, dark, gooey creature:
a puddle with two dough hands and glowing ember eyes. It sits on the taskbar,
floats on the desktop or hangs from the top edge. A lamp glows inside it, and
the lamp's colour tells you what it is doing. Peach is the only warm note in the
room. It is the locked brand tone, the colour of Ottid's eyes and of its resting
lamp.

The product was called Lashon until v1.1
([ADR-0042](adr/0042-rename-the-product-to-ottid.md)). The overlay is the
`main` window, drawn today by `Tongue.svelte`; older docs call it "the tongue".
The creature itself is
[ADR-0040](adr/0040-the-overlay-becomes-a-living-creature.md), and user-made
creatures are [ADR-0041](adr/0041-user-authored-creatures-are-data.md).

The visual reference is two self-contained prototypes that open in any browser.
They are the spec until Phase B ports them into the app:

- [`design/ottid/puddle.html`](design/ottid/puddle.html) shows the base form and
  the three placements.
- [`design/ottid/states.html`](design/ottid/states.html) shows all twelve states,
  rendered with WebGL, in both OS themes.

## Color tokens

The tokens live in [`apps/desktop/src/app.css`](../apps/desktop/src/app.css),
which is authoritative. Use the tokens and never hardcode a colour.

```
/* Ink: dark mode, cool slate */
--ink        #0b1216    --ink-2  #141c22    --ink-3  #1d272f    --ink-4  #27333d
--ink-line   rgba(221,228,233,.08)          --ink-line-2 rgba(221,228,233,.16)
--ink-text   #dde4e9    --ink-mute  62%     --ink-faint  38%

/* Bone: light mode, cool off-white */
--vellum     #ecede9    --vellum-2 #e1e3df  --vellum-3 #d3d6d1
--vellum-line rgba(14,20,24,.10)            --vellum-line-2 rgba(14,20,24,.18)
--vellum-text #0e1418   --vellum-mute 62%   --vellum-faint 38%

/* Brand: locked */
--peach      #f7c8a3    eyes, resting lamp, confirm

/* Mode chroma: evenly spaced hues, so one glance reads the mode */
--saffron    #e8b14a    dictation: gold, pen and ink
--garnet     #4d8df0    command: cobalt blue (the name is historical)
--indigo     #a47bd9    chat: wisteria violet
--hearth     #d97a4a    recipe match: terracotta, solid, never pulsing
--aqua       #3fcbc0    an agent needs you (Phase C)

/* System */
--state-success #5fb887 done
--state-error   #e8625a error ("rose")
--state-cloud   #7a8590 working: transcribing, thinking

/* The creature (same in both OS themes) */
--creature-charcoal   #14181c               the body
--creature-rim-dark   rgba(247,200,163,.3)  rim on dark backgrounds
--creature-rim-light  rgba(255,255,255,.1)  rim on light backgrounds
--creature-shadow     rgba(10,14,18,.3)     drop shadow on light backgrounds
--creature-rain-head  #d7e6ff               command rain's lead glyph

/* Dictation's props (the pencil's body is --saffron) */
--prop-paper #efe7da  --prop-binding #2a3036  --prop-ring   #8a939c
--prop-ink   rgba(38,42,48,.85)               --prop-lead   #3a2f22
--prop-eraser #e89a8a --prop-shadow rgba(0,0,0,.35)
```

Each mode chroma has a `-glow` variant at 55% alpha.

**Legacy aliases** (`--bg-*`, `--text-*`, `--accent-*`, `--state-recording`)
map the pre-Lamp names onto this palette so older windows still compile. Do not
use them in new code. Note that `--accent-aqua` resolves to **cobalt**, not to
`--aqua`.

Contrast: text tokens meet WCAG AA at 14 px and above on their own background.

## Typography

- **Heebo** 400/500/700 (SIL OFL), self-hosted in `static/fonts/` with Hebrew
  and Latin subsets. It covers every Hebrew use.
- Display, sans and mono stacks are `--font-he-display`, `--font-he-sans`,
  `--font-lat-sans` and `--font-mono`. Latin display and mono fall back to
  system fonts until those families are self-hosted.
- Never load a font from a CDN at runtime (`.claude/rules/security.md`).

## Surfaces

| Surface | Purpose | When visible |
|---|---|---|
| **Ottid** (overlay, window `main`) | The creature: state, privacy lamp, companion | Always (can hide) |
| **Bubble / card** | Short text next to Ottid: captions, the approval card (Phase B) | On demand |
| **Hub** | Settings, history, recipes, the creature Studio (ADR-0041) | On demand (tray click) |
| **Tutorial** | First-run walkthrough, in its own window ([ADR-0008](adr/0008-first-run-tutorial-window.md)) | First run, or from the tray |
| **Conversation panel** | Slide-out reply view for chat mode | During and after a chat |
| **Agent panel** | Slide-out terminal for external agents | While an agent runs |

The approval card shows the **full** command and arms its buttons only after a
700 ms delay, so a stray keypress cannot approve it. Phase B finalizes it.

## Ottid, the creature

### Form

- **Body.** One dark, gooey shape: a wide ellipse about **29 × 20 units**. In the
  prototypes 1 unit is 1.5 CSS px; Phase B sets the scale in the app. Its outline wobbles slowly through angular harmonics
  2, 3 and 5. The wobble grows with the voice level and with a "jiggle" spring
  after a poke.
- **Hands.** Exactly two, made of dough. Each is a thin arm (capsule radius 2.6)
  ending in a round palm (radius 4.6), fused into the body with smooth fillets
  (`smin` 4.5 for the arm, 2.5 for the palm). The shoulders sit at the sides.
- **Nothing else.** The default creature has no ears, tail, spikes, mouth or
  accessories. User creatures can add parts from the kit (ADR-0041).

### Face: ember eyes

- Two glowing **peach** ovals with a soft peach halo. There is no sclera, no
  pupil and no mouth.
- **Lids** are a clip mask over each eye. The top lid (`lt`) lowers for tired,
  sad or focused, and the bottom lid (`lb`) rises for a smile.
- **Gaze.** The eyes slide toward the target and foreshorten as if they sat on a
  sphere. At rest they wander slowly.
- **Tilt** tips the eyes together for curious, pleading or sad looks.
- **Blink** comes every 2.4–6 s, at random.

### The lamp

A radial glow inside the body, centred at its core (5.5 units below the centre),
in the **state colour**:

- Its strength sits at 0.18 at idle and between 0.3 and 0.85 in every active
  state, so **while Ottid listens, the lamp is never below 30%**.
- On dark backgrounds the body also casts a halo in the state colour, and its
  blur grows with the glow. On light backgrounds it casts a neutral drop shadow
  instead.

The lamp is the **privacy signal**. The engine owns it: no creature can recolour
it, hide it or shrink it (ADR-0041).

### Palette

- The **body** is charcoal (`--creature-charcoal`) in **both** OS themes. A
  creature is not a surface; it has to read on any wallpaper.
- The **rim** is a hairline at the edge: peach at 30% on dark backgrounds
  (`--creature-rim-dark`), white at 10% on light ones (`--creature-rim-light`).
- The **eyes** are `--peach`. The **lamp** takes the state token.
- **Bubbles and cards** next to Ottid follow the OS theme (`--ink-*` or
  `--vellum-*`).

### Placements

The user chooses one of three:

| Placement | Anchor | Motion |
|---|---|---|
| **Taskbar** | Sits on the taskbar's top edge. The bottom is flattened by a smooth intersection with the floor line. | Hands rest on the floor beside the body. Hops land on the floor. |
| **Float** | Free on the desktop | Gentle bob (about 2.6 units at 1.8 rad/s). Hands paddle slowly. |
| **Ceiling** | Hangs from the screen's top edge by both hands. The edge is a smooth union with the ceiling, evaluated in screen space so it stays put while the body swings. | Pendulum swing. **One hand always holds on**, and the other does the state's gesture. |

### The silhouette rule

The body is one dark silhouette, so **a hand in front of it disappears**. Every
gesture happens on the **outline**: at the sides, above the head or on the
floor. Never design a pose that needs a hand in front of the belly.

**Props** have their own colours and **may** sit in front of the body:

- **Notepad** (dictation): cream paper, a dark binding with rings, and graphite
  lines (the `--prop-*` tokens). It is held at the creature's right side, tilted
  slightly, and comes with the `write` gesture.
- **Pencil** (dictation): a saffron body, a dark graphite tip and a pink eraser.
  The palm grips it about 5.6 units above the tip, so the tip stays visible.
- **Code rain** (command): Hebrew letters and digits fall *inside* the body,
  clipped to the silhouette's alpha and added on top as light. The head glyph is
  near-white blue (`--creature-rain-head`), and the trail is cobalt.

### Motion

Everything moves on damped springs, `a = −ω²(x − target) − 2ζωv` with
`ω = 2π / response`, integrated in fixed 240 Hz sub-steps:

| Spring | Response (s) | Damping ζ |
|---|---|---|
| Shoulder | 0.30 | 0.60 |
| Hand | 0.22 | 0.50 |
| Rotation (swing) | 1.10 | 0.45 |
| Squash and stretch | 0.38 | 0.25 |
| Hop | 0.42 | 0.38 |
| Jiggle (wobble boost) | 0.50 | 0.30 |
| Scale | 0.25 | 0.70 |
| Lids, tilt | 0.12–0.18 | 0.90 |
| Gaze | 0.22 | 0.85 |

Squash and stretch keep the volume of a 3D body: the x scale is the inverse
square root of the y scale. A **poke** gives the hop (on the ceiling, the
squash), squash and jiggle springs an impulse. A click pokes Ottid and makes it
smile briefly. Events (wake, agent needs you, agent done) use the same impulse
without the smile.

### Rendering

The engine is `apps/desktop/src/lib/creature/`
([ADR-0045](adr/0045-the-creature-engine.md)). `Creature.svelte` takes the
overlay's props; the engine draws whatever creature file it is given, and the
bundled one is [`creatures/ottid/creature.json`](../creatures/ottid/creature.json).

- The body is a 2D **signed-distance field**, evaluated per pixel by a **WebGL**
  fragment shader. Limbs join by smooth union. The floor is a smooth
  intersection and the ceiling a smooth union. The lamp, the rim, the code rain
  and the halo or drop shadow are in the same shader.
- Eyes and props draw on a 2D canvas on top, in the body's local transform.
- **Without WebGL** (or after a lost GPU context), a 2D fallback draws the same
  silhouette as an ellipse with arms and palms, with the lamp, rim, halo, eyes
  and props. It drops the wobble, the smooth fillets and the rain.
- It draws only while the page and the creature are visible. At rest it drops to
  30 fps; under reduced motion it draws only when what it shows changes. The
  engine reports what its frames cost (`onStats`); about 0.1–0.2 ms of CPU per
  frame on a desktop GPU.
- Colours come from the tokens above, read from the page; never from literals
  and never from a creature file.
- In development, `/creature-lab` shows every state side by side in any
  placement, with the frame cost. The CPU path in `puddle.html` is a readable
  reference, not for the app.

### Reduced motion and accessibility

- `prefers-reduced-motion` snaps springs to their targets and stops the bob,
  swing, wobble, writing and rain. **The lamp still shows the state.**
- ARIA-live regions announce every state change. The creature is decoration on
  top of that, never the only signal.
- Ottid never takes keyboard focus from the user's app. Phase B sets up
  non-activating focus and click-through on transparent pixels.

## States

Each state combines four parts: **lamp colour**, **eyes**, **hands** and
**body**. In dictation and command mode the **voice level** drives the body's
ripple, its stretch and the lamp's strength. It also sets the writing speed
(dictation) and the rain speed (command).

| State | Lamp | Eyes | Hands and body | When |
|---|---|---|---|---|
| **Idle** | `--peach`, dim and steady. With the wake word armed, `--state-cloud` at 30% or more: the microphone is open. | Wander slowly, blink | Rest. Sometimes waves hello and smiles. | No interaction |
| **Preparing** | `--saffron`, slowly ramping up | Lids mostly open | Stretches with both hands overhead, then rests | Model loading or first-run download |
| **Dictation** | `--saffron` × level | On the notepad | Writes on the notepad with the pencil, faster when the voice is louder. Flips the page when it is full. | Dictation listening |
| **Command** | `--garnet` × level | Forward, slight tilt | Both hands raised, ready. Code rain falls inside the body, faster when the voice is louder. | Command listening |
| **Transcribing** | `--state-cloud`, flowing | Down at the side | One hand scribbles beside the body | After release, STT running |
| **Thinking** | `--state-cloud`, slow pulse | Up and away, heavy lids | One hand scratches the head (above the outline) | LLM planning or cleanup |
| **Tool** | `--garnet`, ticking | Darting, down | Both hands hammer in turn, like pistons | Command-mode tool running |
| **Confirm** | `--peach`, steady | Pleading tilt, wide open | Both hands open toward the user ("ok?") | Waiting for approval; the card is next to Ottid |
| **Wake** *(event)* | Flash in the mode's colour, then steady | Wide | Startled hop with both hands up, then the listening pose | Wake word heard |
| **Error** *(event)* | `--state-error`, flash that fades | Sad: lids low, tilted | Flinches and squashes, hands droop | Any failure |
| **Agent needs you** | `--aqua`, fast pulse | Wide, toward the user | Waves again and again, hopping | Claude Code waits for permission or input (Phase C) |
| **Agent done** *(event)* | `--state-success`, flash that fades | Smiling | Cheers with both hands up and hops | Claude Code finished (Phase C) |

States not designed yet are built from the same vocabulary: a lamp colour from
the tokens plus a pose made of existing gestures. They never get a new body
shape.

- **Chat listening** (`--indigo`)
- **Speaking** (TTS, Phase D): the lamp follows the speech amplitude
- **Recipe match** (`--hearth`, solid)

## Conversation panel

- Slides in from the right edge, 420 px wide, in glass-card style.
- Header: a mode badge (`💬 שיחה` / `⚙ פקודה`), a provider chip
  (`Claude Sonnet 4.6` / `DictaLM 3.0`), and a cloud indicator if the provider
  is in the cloud.
- Message stream:
  - The user's bubble with an audio-replay button. Hebrew is right-aligned and
    English left-aligned automatically, via `dir="auto"`.
  - The assistant's bubble with streamed text, a copy button and an
    audio-replay button.
  - Tool-call cards, collapsed by default, that expand to show the JSON
    arguments and result.
- Input footer: a text-input fallback for typed follow-ups, a provider switcher
  and a "stop" button.

## Agent panel

- Uses the same slide-out region as the Conversation panel, with tabs at the
  top: `Conversation | claude-code | opencode | …`.
- Each agent tab has:
  - an xterm.js terminal
  - a status pill (`running` / `waiting input` / `exited 0`)
  - a "send transcript as input" button: hold the dictation hotkey, speak and
    release, and the text goes into the focused agent's stdin instead of an
    OS-level paste
- Several agents can run in parallel, and the tabs can be reordered.

## Hub layout

A left sidebar (260 px) plus a resizable right detail pane (960 × 640 default).

Sections:

1. כללי / General: language, theme, autostart, position
2. קיצורי דרך / Shortcuts: three hotkeys, with conflict warnings
3. שמע / Audio: input device, gain, VAD sensitivity, ducking
4. **STT**: provider picker, model picker, a "test transcription" button
5. **LLM**: a provider picker per mode (cleanup, command, chat), a model picker,
   API-key fields and a test prompt
6. **TTS**: a provider picker per mode (command, chat), a voice picker with a
   sample, a streaming toggle
7. **סוכנים / Agents**: detect installed external agents, configure their
   paths, choose the default agent
8. מילון / Dictionary:
   - a table of corrected words, with JSONL import and export
   - a per-user Hebrew→English word list, applied automatically to transcripts.
     It is for English words that the STT model writes in Hebrew letters and
     that the user wants kept in English (e.g. ריליס → release).
9. קטעים / Snippets: shortcut → expansion
10. מילת השכמה / Wake word: the current model, its sensitivity, training a new
    one
11. **כלים / Tools**: enable each tool and set its confirmation policy
12. **זיכרון / Memory**: view, edit and delete known facts; export a dump
13. **היסטוריה / History**: the last 1000 interactions, with audio replay
14. **פרטיות / Privacy**: telemetry toggle (off by default), data location, a
    "delete all" button
15. **אודות / About**: version, licences, model credits

The creature Studio (ADR-0041) joins the Hub when Phase B lands.

## RTL & accessibility

- `dir="rtl"` when the UI language is Hebrew, and logical CSS properties
  throughout.
- Bidi isolates around mixed Hebrew and English fragments.
- ARIA-live regions announce every state change.
- Full keyboard navigation, with a 3 px cobalt focus ring.
- `prefers-reduced-motion` disables the springs and the creature's ambient
  motion (see above).
