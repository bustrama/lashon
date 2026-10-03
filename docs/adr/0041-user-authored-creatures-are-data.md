# 41. User-authored creatures are data, never code

## Status

Accepted — 2026-10-04. Decided by the product owner. Builds on
[ADR-0040](0040-the-overlay-becomes-a-living-creature.md).

## Context

Ottid's promise is that every user can make their **own** creature. We ship a
toolkit, and people use it with the local LLM
([ADR-0026](0026-promote-qwen3-4b-as-default-local-llm.md)) or with an external
agent such as Claude Code. Three facts constrain the format:

- **The overlay is a privileged surface.** Its window can call Tauri commands,
  including text injection and, in command mode, keystrokes. A creature that
  ships JavaScript, HTML or a full SVG would run third-party code with the
  user's keyboard.
- **The overlay is the privacy signal.** The lamp shows that Ottid is listening
  (ADR-0040). A careless or hostile creature must not be able to hide that.
- **Small local models are weak at freeform drawing.** They are reliable at
  filling a JSON Schema with constrained decoding (llama.cpp turns a schema into
  a grammar).

## Decision

1. **A creature is one declarative file.** It is `creature.json`, in
   `creatures/<id>/` under the app data directory, and contains nothing
   executable. The default `ottid` creature ships bundled with the app and
   cannot be modified. Its file is just the first example of the format.
   The file:
   - carries a `schema` version
   - is checked against a published JSON Schema
   - has the creature's display name in it, since users name their own
     creatures

   As with recipes, the Rust types in `lashon-core` are the source of the JSON
   Schema.
2. **Two tiers in the same format:**
   - **Kit (parametric).** The file picks values and parts from fixed sets:
     - body radii, wobble and softness
     - limbs and parts from a library we ship (hands first, more later)
     - an eye style
     - body and eye colours
     - per-state poses chosen from the engine's gesture library

     This tier is shaped so a 4B local model can fill it reliably with
     schema-constrained decoding.
   - **Freeform.** Body and part outlines are given as **SVG path data only**:
     the `d` strings, with no elements, scripts, styles, fonts, images,
     `<foreignObject>` or external references. The engine turns the paths into
     its own shapes and animates them with the same springs and states.
3. **The engine owns, and the creature never controls:**
   - the state machine
   - which state is shown and when
   - event timing
   - **the lamp**

   A creature may place the lamp inside its body. It cannot recolour it, hide
   it, shrink it below the minimum the validator sets, or cover it with opaque
   parts. **State colours come from the design tokens, never from the creature
   file.** The props (the dictation notepad and the command code rain) are also
   engine-owned. A creature may restyle them only within what the schema allows.
4. **A complexity budget.** The validator caps each of these, with limits set in
   the schema:
   - file size
   - the number of parts
   - the number of path commands
   - the estimated render cost

   This keeps a creature from making the overlay slow or draining the battery.
5. **One validator, every path in.** Hot reload, Studio saves, import and agent
   writes all go through the same validator. A file that fails is rejected with
   a readable error, in Hebrew and English, and the previous creature stays on
   screen.
6. **The toolkit:**
   - **Files with hot reload.** Editing `creature.json` updates the overlay
     live.
   - **A CLI** with `validate` and `render`. `render` writes a PNG sheet of
     every state in every placement, so an agent can look at its own work.
   - **Studio in the Hub.** Sliders, a live preview of the states, and a "describe
     your creature" box that asks the local LLM to fill the kit schema.
   - **Agent tools.** The existing stdio MCP server
     ([ADR-0028](0028-lashon-as-mcp-server.md)) gains creature tools (schema,
     validate, render, write), and a Claude Code skill teaches the format. The
     running app picks up changes through hot reload, so authoring needs **no
     new network listener**.
   - **Export and import as a single file.** Import validates the file and
     copies it. Nothing in it is ever executed.

## Consequences

- No creature can run code, call a Tauri command, load a network resource or
  hide that Ottid is listening. Reviewing a creature means reading a data file.
- Expressiveness is capped by the engine's vocabulary: its parts, gestures and
  eye styles. A wish that does not fit becomes an engine feature request, not a
  plugin.
- The schema is a public contract. It is versioned, and a breaking change ships
  with a migration for existing files.
- Creatures are portable. A creature written by Claude Code, by the local LLM or
  by hand is the same kind of file.

## Alternatives considered

- **JavaScript or WebAssembly creature plugins.** Rejected. That puts code in a
  privileged window, and even a WASM sandbox needs an API surface and a review
  process we do not want to own.
- **Raw SVG, HTML or CSS.** Rejected. Scripts, `<foreignObject>`, external
  references and CSS animation can all hide the lamp, and sanitizing full SVG is
  a well-known trap. We accept path data only.
- **Sprite sheets or Lottie.** Rejected. They cannot follow continuous inputs,
  the files are large, and models cannot author them reliably.
- **Image-generation models.** Rejected. They are heavy, they do not fit the
  local-first hardware tiers, and their output cannot be animated.
