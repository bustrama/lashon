# 8. A dedicated window for the first-run interactive tutorial

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Context source:** GitHub issue #9 (Tutorial); `docs/roadmap.md` §1.8
  (Onboarding); `docs/design-system.md` ("Onboarding Overlay" surface)

## Context

A first-time user installs Lashon and is met by a 104×104 chromeless,
transparent, always-on-top tongue — and nothing else. There is no surface that
explains what Lashon is or how to dictate. Issue #9 asks for an interactive
tutorial shown on first run, with a skip option.

The tongue window cannot host this. It is deliberately shape-locked — the
design system states it "never changes shape, only how it is lit" — and at
104×104 it has no room for instructional content. The roadmap's full M4
onboarding (mic permission, hardware-tier detection, model download, hotkey
rebind) is a larger milestone; the *interactive tutorial* is a self-contained
vertical slice of it that can ship early, the same way the `v0.1.0` packaging
slice of M13 shipped ahead of its milestone.

## Decision

Ship the tutorial as a **second Tauri window**, `tutorial`, separate from the
tongue:

- Declared in `tauri.conf.json` as a **frameless, transparent**, centred
  760×600 window with `"visible": false`. It is created at startup but stays
  hidden. The page paints only a floating glass card — there is no OS chrome,
  so the tutorial reads as a dialog, not a window; the card header carries a
  `data-tauri-drag-region` so it can still be moved.
- The SvelteKit route `/tutorial` renders a five-step walkthrough: welcome, the
  tongue, the dictation hotkey, an **interactive practice step**, and a
  closing tips card. The practice step subscribes to the `dictation:state`
  events the Rust worker already broadcasts, so it reflects the real FSM —
  the user dictates for real, inside the tutorial.
- **First-run gating** is a single boolean, `tutorial.completed`, in the
  existing `tauri-plugin-store` `settings.json`. The Rust `setup()` hook reads
  it and reveals the tutorial window only when it is unset; the frontend writes
  it on finish *or* skip. The tongue stays visible underneath throughout — the
  tutorial's practice step needs the live tongue on screen. Skipping is always
  available and is treated identically to finishing — the tutorial never
  reappears uninvited.
- The window is **hidden, never destroyed**, on dismissal. A tray menu entry
  ("Tutorial · מדריך") re-reveals it and emits `tutorial:open` so the page
  rewinds to step one.
- A `capabilities/tutorial.json` capability scopes the new window's permissions
  (`core:default`, `core:window:allow-hide`, `core:window:allow-start-dragging`,
  `store:default`).

## Consequences

- Lashon becomes a **multi-window** app. The tongue stays pristine and
  shape-locked; instructional UI lives entirely in its own window.
- The completion flag is shared cross-language through one store file — the
  Rust shell reads it, the frontend writes it. The key `tutorial.completed` is
  a flat string, identical on both sides.
- The practice step subscribes to the worker's `dictation:state` events; the
  worker also emits a `dictation:transcript` event (the transcribed text) so
  the step can echo back what Lashon heard. Both broadcast to every window, so
  the tutorial needs no window-targeting. The transcript event is in-process
  only and never logged (see `.claude/rules/security.md`).
- A failed store write only risks the tutorial showing again; it never blocks
  the user from leaving. First-run gating fails *open* (shows the tutorial)
  rather than trapping a returning user.
- The full M4 onboarding (permissions, hardware tier, model-download UI,
  hotkey rebind, i18n) is still owed. This window is the surface that work
  will extend, not throwaway scaffolding.
