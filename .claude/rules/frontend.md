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
- Honour `prefers-reduced-motion` — it disables springs and the waveform
  animation.
- Announce state changes via ARIA-live regions.

## Design system

- Use the design tokens; do not hardcode colours. Colour carries mode
  (citron = dictation, citron+aqua = command, citron+violet = chat, …).
- The Tongue is a chromeless transparent overlay — it never changes shape, only
  how it is lit.
