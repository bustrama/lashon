---
description: SvelteKit frontend — Svelte 5 runes, RTL, the design system
globs: ["apps/desktop/src/**"]
---

# Frontend

The desktop UI is SvelteKit 5 / Svelte 5. Visual spec in
[`docs/design-system.md`](../../docs/design-system.md).

## Svelte 5

- Use runes (`$state`, `$derived`, `$effect`) — not Svelte 4 stores or `$:`.
- The frontend holds no dictation state of its own. Lifecycle state comes from
  the Rust FSM via Tauri events; the UI renders it.

## RTL & accessibility

- The UI is RTL-native. Use `dir="auto"` on user-text containers, logical CSS
  properties (`margin-inline`, not `margin-left`), and bidi isolates around
  mixed Hebrew/English fragments.
- Honour `prefers-reduced-motion`. It snaps springs to their targets and stops
  the creature's ambient motion (bob, swing, wobble, writing, code rain). The
  lamp still shows the state.
- Announce state changes via ARIA-live regions.

## Design system

- Use the design tokens; never hardcode colours. Colour carries the state:
  `--saffron` for dictation, `--garnet` (cobalt) for command, `--indigo` for
  chat, `--state-cloud` for working, `--aqua` when an agent needs you, and so on.
  Don't use the legacy aliases in new code. `--accent-aqua` is cobalt.
- The overlay is **Ottid**, a living creature
  ([ADR-0040](../../docs/adr/0040-the-overlay-becomes-a-living-creature.md)). It
  changes pose with state, but the **lamp is engine-owned and always shows the
  state colour**. Never hide, dim below the minimum, or recolour it; it is how
  the user knows Ottid is listening.
- The silhouette rule: the body is one dark shape, so a hand in front of it
  disappears. Gestures happen on the outline (the sides, above the head, the
  floor). Props such as the notepad may sit in front.
- Creatures are **data, never code**
  ([ADR-0041](../../docs/adr/0041-user-authored-creatures-are-data.md)). Never
  `eval`, inject HTML or SVG from, or fetch resources for a creature file.
  Every path in goes through the one validator.
