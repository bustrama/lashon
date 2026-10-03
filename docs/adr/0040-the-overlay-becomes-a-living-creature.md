# 40. The overlay becomes a living creature (Ottid)

## Status

Accepted — 2026-10-04. Decided by the product owner. **Amends**
[ADR-0008](0008-first-run-tutorial-window.md) and the design system: the
overlay is no longer shape-locked ("never changes shape, only how it is lit").
ADR-0008's decision, a separate tutorial window, still stands.

## Context

The overlay (window `main`, 104×104, `Tongue.svelte`) is the Lashon mark. It
keeps one shape and shows state through light alone. That was enough for a
dictation widget. It is not enough for where the product is going:

- The overlay should be a **companion with a personality** that can live on the
  desktop by itself, not only a status light.
- Phase C connects it to Claude Code. "Claude needs you", "Claude is done" and
  "thinking" have to be readable at a glance, next to dictation and command
  mode. Colour alone runs out. Body language (where it looks, what its hands do,
  how it moves) carries much more.
- Users will make their own creatures ([ADR-0041](0041-user-authored-creatures-are-data.md)).
  The default creature has to be a good base for that.

We studied Coucou (MIT) for the mechanics of a desktop companion. Coucou's
`LICENSE-ASSETS.md` reserves its character, so the creature has to be our own
design. We iterated with throwaway prototypes: a gallery of six species, a
tongue-shaped drop, a husky, and a polar "bump" body. The owner chose the design
below.

## Decision

The overlay becomes **Ottid**, a small creature with a fixed anatomy and a state
map. The visual spec lives in [`docs/design-system.md`](../design-system.md).
Two self-contained prototypes are the reference implementation until Phase B
ports them. Open them in a browser:

- [`docs/design/ottid/puddle.html`](../design/ottid/puddle.html) shows the base
  form and the three placements.
- [`docs/design/ottid/states.html`](../design/ottid/states.html) shows all twelve
  states, rendered with WebGL.

1. **Silhouette: the puddle.** The body is one dark, gooey shape: a wide
   ellipse (about 29 × 20 units) whose outline wobbles slowly (angular harmonics
   2, 3 and 5). It has **exactly two dough hands**, each a thin arm capsule and
   a round palm, fused into the body with smooth fillets. The default creature
   has nothing else: no ears, tail, spikes, mouth or accessories.
2. **Face: ember eyes.** Two glowing peach (`--peach`) ovals. There is no
   sclera, no pupil and no mouth. Expression comes from four things:
   - **lids**, a clip mask over each eye: the top lid lowers for tired or sad,
     and the bottom lid rises for a smile
   - **gaze**: the eyes slide and foreshorten as if they sat on a sphere
   - **tilt**
   - **blinks**
3. **The lamp.** A soft glow inside the body, at its core, in the state colour.
   On dark backgrounds the creature also casts a halo tinted the same colour,
   and on light backgrounds a drop shadow. The lamp is the **privacy signal**:
   while Ottid listens, the lamp always shows it. ADR-0041 makes the lamp
   engine-owned, so no creature can turn it off.
4. **Palette.** The body is charcoal (`#14181c`) in **both** OS themes. A
   creature is not a surface; it has to read on any wallpaper. A faint rim
   separates it from dark backgrounds. The eyes are peach, and the lamp takes
   the state tokens. Bubbles and cards next to the creature follow the OS theme.
5. **Three placements**, chosen by the user:
   - **taskbar**: sits on the taskbar edge with a flat bottom
   - **float**: free on the desktop, bobbing gently
   - **ceiling**: hangs from the top edge by both hands and swings like a
     pendulum; one hand always holds on while the other gestures
6. **Twelve states.** Each state combines a lamp colour, eyes, hands and body
   motion. The table is in the design system. Two states use **props**:
   - **Dictation**: a notepad and a saffron pencil. The writing speed follows
     the voice level, and the page flips when it is full.
   - **Command mode**: Hebrew letters and digits fall inside the body like
     code rain, faster as the voice gets louder.
7. **The silhouette rule.** The body is one dark silhouette, so a hand in front
   of it disappears. Every gesture therefore happens **on the outline**: at the
   sides, above the head, or on the floor. Props have their own colours and may
   sit in front of the body.
8. **Rendering.** The body is a 2D signed-distance field evaluated per pixel on
   the **GPU** (a WebGL fragment shader in the overlay window):
   - Limbs fuse into the body by smooth union (`smin`).
   - The taskbar seat is a smooth intersection with the floor line.
   - The ceiling grip is a smooth union with the ceiling half-plane, evaluated in
     screen space so it does not tilt with the swing.

   Eyes, props and the code rain draw on a 2D canvas on top. The rain is masked
   by the body's alpha. The prototype's CPU path (`puddle.html`) is a reference
   only and is too slow for the app. The overlay renders only while it is
   visible, and drops its frame rate when idle (battery). Phase B measures the
   cost.
9. **Motion.** Everything that moves runs through damped springs,
   `a = −ω²(x − target) − 2ζωv` with `ω = 2π / response`, integrated in fixed
   240 Hz sub-steps. The response and damping values are in the design system.
10. **Originality.** The creature must not read as an existing mascot. A
    recolour of an existing character is still a copy. These are excluded:
    - a pink or purple blob with dot eyes and a thin smile
    - a dark rounded square with large eyes (Coucou's reserved character)
    - round ears on a round dark body

    New parts for the default creature, and kit parts we ship, are checked
    against this rule.

## Consequences

- The "never changes shape, only how it is lit" rule is retired from
  `docs/design-system.md`, `.claude/rules/frontend.md` and the reasoning in
  ADR-0008.
- **The overlay window grows.** The arms reach above the body, the ceiling
  placement needs headroom, and the props stick out, so the 104×104 window will
  not fit. Phase B moves to a larger transparent window whose transparent pixels
  pass clicks through, using Rust cursor-poll hit-testing. Phase B also ports the
  non-activating focus and island behaviour from Coucou.
- **WebGL becomes a dependency** of the overlay. Phase B defines the fallback
  for a WebView without WebGL, for example a pre-traced contour drawn in 2D at
  lower fidelity.
- The current overlay components (`Tongue.svelte`, `Mark.svelte`,
  `StateGlyph.svelte` and `Waveform.svelte`) are replaced in Phase B. The window
  label (`main`) stays.
- A new colour token, `--aqua` (`#3fcbc0`), marks "an agent needs you"
  (Phase C). Claude-done reuses `--state-success`.
- Accessibility does not change: state changes are still announced through
  ARIA-live regions, and the creature sits on top of that. Under
  `prefers-reduced-motion`, springs snap to their targets and the bob, swing,
  writing and rain stop. The lamp still shows the state.

## Alternatives considered

- **Keep the shape-locked, lit mark.** Rejected. It cannot carry the agent
  states, and the owner wants a companion, not a status light.
- **A tongue-shaped creature** (a drop, after the app's old name). Prototyped
  and rejected: its motion read as crooked.
- **A species to choose from** (flame, ember, firefly, the letter yod, parrot,
  bat, husky). Rejected in favour of one simple body that suits user-made
  creatures (ADR-0041).
- **A polar "bump" body.** Rejected because of its spikes. A signed-distance
  smooth union gives round, gooey joins instead.
- **Ears or a tail on the default creature.** Rejected. The owner chose "hands
  only", and round ears drift toward a well-known mouse.
- **Recolouring an existing character.** Rejected, because a recolour is still
  a copy.
- **Sprite sheets or Lottie.** Rejected. They cannot follow continuous inputs
  (voice level, gaze, springs), and users or local LLMs cannot author them.
- **CPU canvas rendering.** Rejected for the app: a per-pixel field at 60 fps
  costs too much battery. It is kept only as the readable reference.
