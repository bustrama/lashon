# 45. The creature engine

## Status

Accepted — 2026-10-04. Phase B, part 2. Implements the renderer of
[ADR-0040](0040-the-overlay-becomes-a-living-creature.md) and the kit tier of
[ADR-0041](0041-user-authored-creatures-are-data.md). It fills the stage
contract that ADR-0044 (the click-through overlay window, Phase B part 1)
defines in `lib/creature/types.ts`.

## Context

ADR-0040 designs Ottid and leaves three things to Phase B: the renderer, a
fallback for a WebView without WebGL, and a measured frame cost. ADR-0041 makes
creatures data checked by one validator, with the engine owning the states,
their timing and the lamp. Our own prototypes (`docs/design/ottid/states.html`
and `puddle.html`) are the only visual source.

The overlay window is being rebuilt in parallel (ADR-0044). Its contract gives
the creature a 220 × 120 CSS px stage, a placement, a gaze direction, the voice
level, the wake-word flag, dragging, hover and pokes. The creature only draws.

The frontend had no unit-test framework.

## Decision

1. **The engine and the creature are separate.** `apps/desktop/src/lib/creature/`
   holds:
   - `Creature.svelte`: the component. It takes the contract's props, plus an
     optional validated creature (the bundled one by default) and an `onStats`
     callback.
   - `engine/`: pure modules with no DOM (springs, geometry, the gesture
     library, state → pose, the lamp, the simulation and the per-frame
     description) and the renderers on top of them (`gl.ts`, `canvas2d.ts`,
     `engine.ts` for the frame loop).
   - `data.ts`: the TypeScript mirror of the creature file.
   - `types.ts` and `state.ts`: ADR-0044's contract and state mapping, kept
     identical to that branch.

   The engine reads a creature as numbers and names from closed sets. It never
   evaluates, injects markup from, or fetches anything for a creature file. A
   unit test scans the engine's sources for `eval`, `Function`, `innerHTML`,
   `fetch`, dynamic `import()` and image loads.
2. **Schema 1 is ADR-0041's kit tier.** The bundled creature is
   `creatures/ottid/creature.json`. The Rust types in `ottid_core::creature`
   generate `creatures/schema/ottid-creature.schema.json`, which a snapshot test
   keeps in step. A creature sets its body radii and wobble, its arm and palm
   radii and softness, its eye style, size and position, where its lamp sits
   and how far it glows, and one gesture per state from the engine's thirteen.
   Colours are design-token names (one choice each for now), never values. The
   freeform tier (SVG path data) and the part and path-count budgets come with
   schema 2; schema 1 caps the file at 16 KiB.
3. **One validator, in Rust.** `validate_creature` checks the size cap, then
   parses strictly (unknown fields are errors), then checks every range, then
   how the fields fit together:
   - the palm is wider than the arm;
   - the lamp's centre is in the inner 40% of the body and never under an eye,
     even a startled one looking toward it, and its glow reaches at least the
     body's half-height. The engine clamps how far it scales and shifts an
     eye to the limits the validator checks, and the schema publishes them
     (`x-ottid-eye-motion` on `Eyes`);
   - the eyes sit inside the body;
   - `write` is used only in dictation, the one state with the notepad;
   - the id, which is also the folder name, is not a device name Windows
     reserves (`con`, `nul`, `com1`…);
   - names are plain text: no control characters, line breaks or characters
     Unicode makes invisible (Default_Ignorable_Code_Point, such as zero-width
     spaces, bidi overrides and tag characters), except the two direction marks.

   It reports every problem at once, in Hebrew and English. It lives in Rust
   because the schema's source, the MCP server, the CLI and hot reload all live
   there. The frontend only receives validated creatures; the bundled one is
   validated by the tests in CI.
4. **The lamp is the engine's.** Its colour is the state's token, and its
   strength is 0.18 at rest and clamped to 0.3–0.85 in every other state,
   whatever the glow program asks for. Tests cover every state over time, cycle
   phase, voice level and reduced motion, and show that a different creature
   cannot change the lamp's colour or strength. Idle with the wake word armed
   shows `--state-cloud` at 30% or more, because the microphone is open; this
   follows ADR-0044's stand-in. The wake event flashes in the take's mode colour.
5. **The engine owns state and timing; the creature picks gestures.** Each state
   has a cycle. Events (wake, error, agent done) play once and hold their last
   pose. A test enforces the silhouette rule: in every placement and over time,
   every palm except the pencil hand stays outside 85% of the body's ellipse,
   and on the ceiling one hand always holds on. The notepad and pencil come
   with the `write` gesture, and the code rain with command mode.
6. **Two stacked canvases.** A WebGL 1 fragment shader draws the body's signed
   distance field together with the lamp, the rim, the code rain and the halo
   (dark backgrounds) or drop shadow (light backgrounds). The rain samples a
   texture of 32 glyphs (Hebrew letters and digits) drawn once in the page's
   monospace font, near their on-screen size and without mipmaps, which speckle
   at the cells' edges. The numbers the shader gets stay small however long the
   app runs: the wobble's phases wrap, and the rain counts from the start of the
   state, so 32-bit floats keep their precision. A 2D canvas on top draws the
   eyes, the notepad and the pencil.
7. **A 2D fallback.** Without WebGL, or while the GPU context is lost, the 2D
   canvas draws the silhouette as an ellipse with arms and palms. It keeps the
   lamp, rim, halo or shadow, eyes and props, and drops the wobble, the smooth
   fillets and the rain. It returns to WebGL when the context is restored.
   Unmounting a creature releases its WebGL context at once, so remounting
   never waits on garbage collection for a free one.
8. **It draws only while it is seen.** The frame loop runs only while the page
   is visible and the creature is on screen. At rest it drops to 30 fps. Under
   reduced motion the springs snap, ambient motion stops, the lamp holds a
   steady strength for the state, and the engine draws only when what it shows
   changes. It redraws sharp when the device pixel ratio changes, as when the
   window moves to a screen with another scale. A frame that throws does not
   stop the loop. It reports frames per second and the mean and worst CPU time
   per frame over the last five seconds.

   Measured in Chrome with ANGLE on Direct3D 11 (an RTX 4080):

   | Case | Frames/s | CPU per frame, mean / worst |
   |---|---|---|
   | One creature at rest | 30 | 0.16 / 0.50 ms |
   | Dictation, with the voice | 60 | 0.16 / 0.60 ms |
   | Command, with the rain | 60 | 0.13 / 0.80 ms |
   | Hanging, agent needs you | 60 | 0.11 / 0.40 ms |
   | Twelve creatures at once | 30–60 | 0.04 / 0.50 ms each |
   | Reduced motion | 0 when nothing changes | — |
   | Hidden | 0 | — |

   WebGL 1 has no GPU timer here, so GPU time is not measured; the shader runs
   over a 220 × 120 stage.
9. **Design tokens only.** New tokens in `app.css` cover the body, rim, shadow,
   rain head and the dictation props. The engine reads them from the page and
   parses them into numbers. A unit test checks every token it uses is defined,
   and that its sources contain no colour literals.
10. **Vitest 4.1.11** runs the frontend unit tests in Node (`npm test`), and CI
    runs it after the type-check.
11. **Integration.** ADR-0044's overlay (`lib/overlay/Overlay.svelte`) mounts
    the creature through `lib/creature/index.ts`, which exports
    `Creature.svelte`. The overlay route maps the lifecycle events to its state
    through `state.ts`, the overlay feeds it the smoothed `dictation:level`,
    `wake:detected` shows the wake event for 900 ms, and the wake-word setting
    shows as wake-armed. The overlay's live region announces the twelve states
    (`creature.states.*`, Hebrew and English), and at rest with the wake word
    armed it says the microphone is listening for it (`creature.wakeArmed`).
12. **A development lab.** `npm run dev` serves `/creature-lab`: every state
    side by side, in any placement, with a simulated voice, gaze, hover, pokes
    and drag, and the frame cost per creature. A release build compiles it out,
    and the route answers 404 there.

## Consequences

- WebGL is a dependency of the overlay, with a fallback that keeps the state
  readable.
- Adding a gesture, an eye style or a colour choice is an engine change: the
  Rust enum, the schema snapshot, the TypeScript mirror and the gesture library
  move together, and the unit tests fail if they drift apart.
- The overlay's privacy signal does not depend on the creature file: the lamp's
  colour, minimum and timing are enforced in code and tested.
- The frame loop costs a fraction of a millisecond per frame. At rest it draws
  30 frames a second, and nothing while hidden or still.

## Alternatives considered

- **Validating in TypeScript too.** Rejected. Two validators drift apart, and
  ADR-0041 requires one. The frontend gets creatures that Rust has checked.
- **Everything in WebGL.** Rejected. The eyes, notepad and pencil are simpler
  and sharper on a 2D canvas, and only the rain needs glyphs in the shader.
- **A CSS blur for the halo.** Rejected. A filter on a canvas costs an extra
  compositing pass every frame, while the shader already knows the distance to
  the outline.
- **A rendering library** (three.js, PixiJS, regl). Rejected. One fragment
  shader does not justify the dependency.
- **A pre-traced contour for the fallback**, as ADR-0040 suggested. Rejected.
  Drawing the same geometry in 2D needs no asset and still follows every pose.
- **Jest or `node:test`.** Rejected. Vitest reuses the Vite and SvelteKit
  configuration, including TypeScript and the `$lib` alias, with no extra
  transform.
