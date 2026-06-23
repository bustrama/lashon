# Interactive first-run tutorial

A vertical slice of milestone **M4** (Onboarding + settings). Branch
`claude/add-interactive-tutorial-sgELC`. Resolves issue #9.

> **Status: shipped in `v0.2.0`** (PR #13). Every acceptance criterion below is
> met; the window has since gained a first-run warm-up display with byte-level
> model-download progress. M3 (the Tongue UI) has since shipped. The remaining
> M4 work — mic permission, hardware-tier detection, hotkey rebind, i18n, a
> persistent settings surface — is the next milestone and has no story yet.

## Why

A fresh install drops the user straight into the chromeless tongue with no
explanation of what Lashon is or how to dictate. The full M4 onboarding
(mic permission, hardware-tier detection, model download, hotkey rebind) is a
larger milestone; this story carves out the **interactive tutorial** — the
"learn how to use Lashon" half — so first-time users are not left guessing.
It must be skippable and must never reappear once finished or skipped.

## How

A dedicated `tutorial` window, separate from the shape-locked tongue — see
[ADR-0008](../adr/0008-first-run-tutorial-window.md) for the rationale.

- **Window** — declared in `apps/desktop/src-tauri/tauri.conf.json` as a
  frameless, transparent, centred 760×600 window with `"visible": false`. The
  page paints only a floating glass card (no OS chrome); its header carries a
  `data-tauri-drag-region` so the window can still be moved. Permissions are
  scoped by `apps/desktop/src-tauri/capabilities/tutorial.json`.
- **Route** — `apps/desktop/src/routes/tutorial/+page.svelte`, a five-step
  walkthrough (welcome → the tongue → the hotkey → interactive practice →
  closing tips). RTL-native, design tokens only, `prefers-reduced-motion`
  honoured, step changes announced via `aria-live`.
- **Interactive step** — suggests a phrase to dictate (any speech works),
  subscribes to the worker's `dictation:state` and `dictation:transcript`
  events (`apps/desktop/src-tauri/src/dictation.rs`), reports live progress,
  and echoes back the transcribed text. Observing a `capturing` →
  `transcribing` cycle marks the practice done; proceeding is never
  hard-blocked on it.
- **First-run gating** — a flat `tutorial.completed` boolean in the
  `tauri-plugin-store` `settings.json`. The Rust `setup()` hook
  (`apps/desktop/src-tauri/src/lib.rs`) reveals the tutorial window only when
  the flag is unset; the frontend writes it on finish *or* skip. The tongue
  stays on screen underneath throughout. Gating fails open.
- **Re-entry** — dismissal hides (never destroys) the window. A tray entry
  re-reveals it and emits `tutorial:open` to rewind the page to step one.

## Acceptance Criteria

- [x] A clean install shows the tutorial window on first launch; a second
      launch does not.
- [x] Every non-final step offers a "skip" control; skipping closes the
      tutorial and suppresses it on subsequent launches.
- [x] The interactive step reflects live `dictation:state` and recognises a
      completed Hebrew dictation cycle.
- [x] The tray "Tutorial" entry reopens the walkthrough from step one at any
      time.
- [x] UI is RTL-native and Hebrew-first; `prefers-reduced-motion` disables the
      animations; `npm run check` is clean.

## Files

- `apps/desktop/src-tauri/tauri.conf.json` — the `tutorial` window.
- `apps/desktop/src-tauri/capabilities/tutorial.json` — its capability.
- `apps/desktop/src-tauri/src/lib.rs` — first-run gating, tray entry.
- `apps/desktop/src/routes/tutorial/+page.svelte` — the walkthrough.
- `docs/adr/0008-first-run-tutorial-window.md` — the decision record.

## Dependencies

Depends on the `dictation:state` events from M0–M2 (already landed). Does not
block M3. The remaining M4 work (permissions, hardware tier, model-download UI,
hotkey rebind, i18n, settings persistence) extends this window rather than
replacing it.
