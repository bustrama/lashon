# Settings Hub + i18n

A vertical slice of milestone **M4** (Onboarding + settings). Branch
`m4-settings-hub`.

> **Status: in progress.** This is the second M4 slice — the first, the
> interactive first-run tutorial, shipped in `v0.2.0` (see
> [`m4-interactive-tutorial.md`](m4-interactive-tutorial.md)). The remaining M4
> work after this slice — **mic permission** and **hardware-tier detection** —
> is the onboarding-hardware slice and extends the settings store and the Hub
> built here.

## Why

A fresh install gives the user no way to change anything: the dictation hotkey
is a hardcoded `Control+Space` constant in `+page.svelte`, and every UI string
is hardcoded Hebrew — an English speaker is locked out on first launch. M4's
Definition of Done requires that "settings persist" and the UI is available in
"i18n he+en". This slice delivers the two halves of that: a **persistent
Settings Hub** and full **Hebrew/English localization**, with a **rebindable
dictation hotkey** as the first real setting the Hub exposes.

It also lays the foundation the onboarding-hardware slice needs: a typed
settings module and a Hub window with a section layout to hang
mic-permission and hardware-tier UI onto.

## How

### Internationalization

- A small, dependency-free i18n store in `apps/desktop/src/lib/i18n/` — a
  `locale` writable, a `t` lookup store derived from it, and `applyLanguage`,
  plus `locales/he.json` and `locales/en.json`. Hebrew is the **fallback** — an
  unkeyed string degrades to Hebrew, never English. The catalogs are imported
  statically, so `$t` is synchronous from first paint — no loading flash.
  (`svelte-i18n` was the first attempt; see
  [ADR-0011](../adr/0011-localization-architecture.md) for why it was dropped.)
- Every user-facing string in `Tongue.svelte`, `DebugSurface.svelte`,
  `routes/+page.svelte`, `routes/tutorial/+page.svelte`, and the new Hub
  resolves through `$t(...)`. `he.json` and `en.json` carry matching key sets.
- The root `routes/+layout.svelte` initializes i18n, applies the persisted
  language, and sets `document.documentElement.{lang,dir}` (`he`→`rtl`,
  `en`→`ltr`). Each window is its own webview, so this runs per window.

### Settings

- `apps/desktop/src/lib/settings.ts` — a typed wrapper over the
  `tauri-plugin-store` `settings.json`. Keys: `ui.language`,
  `hotkeys.dictation`, plus the existing `tutorial.completed` and
  `tongue.position`. It degrades gracefully when the Tauri APIs are absent (so
  routes can be opened in a plain browser for review).
- `snap.ts` and `routes/tutorial/+page.svelte` are migrated off their ad-hoc
  `load('settings.json')` calls onto the typed module.
- A change in one window broadcasts a `settings:changed` event; other open
  windows listen and re-apply (live language switch, live hotkey re-register).

### The Hub window

- A dedicated `hub` window — frameless, transparent, glass, hidden until
  summoned — following the separate-window pattern set by
  [ADR-0008](../adr/0008-first-run-tutorial-window.md). Declared in
  `tauri.conf.json`; permissions scoped by `capabilities/hub.json`.
- `routes/hub/+page.svelte` — a sidebar plus a detail pane, RTL-native, design
  tokens only, `prefers-reduced-motion` honoured, section changes announced via
  `aria-live`. M4 sections: **General** (language), **Shortcuts** (the
  dictation hotkey), **About** (version, repo, license note). The full
  fifteen-section Hub in [`design-system.md`](../design-system.md) fills in
  over later milestones.
- The Rust `setup()` hook gains a tray entry that calls `show_hub()`; closing
  the window hides it rather than destroying it, exactly as the tutorial does.

### Hotkey rebind

- A chord-capture control in the Shortcuts section turns a key press into a
  Tauri accelerator string (`Control+Space`).
- Validation is real logic, so it lives in `lashon-core`:
  `packages/shared-rust/src/hotkey.rs` exposes `validate_accelerator()` with
  `#[test]`s — it rejects an empty chord, a chord with no modifier, and the
  genuinely OS-reserved chords. A `validate_hotkey` Tauri command exposes it to
  the Hub, called once per save (not per keystroke).
- `routes/+page.svelte` reads `hotkeys.dictation` from settings and registers
  that chord instead of the hardcoded constant; on a `settings:changed` event
  it unregisters and re-registers, so a rebind takes effect with no restart.
- `validate_hotkey` is a policy gate, not a registrability check: a chord can
  pass it yet still be rejected by the OS at `register()` time (typically a
  conflict with another running app). When that happens the tongue logs a
  warning and falls back to the default chord, so dictation is never left
  without a working hotkey. Surfacing that failure back in the Hub is a known
  follow-up.

## Acceptance Criteria

- [ ] A "Settings" tray entry opens the Hub window; closing it hides (never
      destroys) it; reopening works and the tray entry is localized.
- [ ] The Hub has General, Shortcuts, and About sections — RTL-native, design
      tokens only, `prefers-reduced-motion` disables motion, `npm run check`
      clean.
- [ ] Switching language he↔en in the Hub updates every open window live (the
      tongue, the tutorial if open, the Hub), flips `dir`, and persists across
      an app restart.
- [ ] Every user-facing string in the tongue, the debug surface, the tutorial,
      and the Hub resolves through the i18n catalogs — no hardcoded Hebrew
      remains in those components; `he.json` and `en.json` key sets match.
- [ ] The dictation hotkey can be rebound in the Hub; an invalid chord (empty,
      no modifier, or OS-reserved) is rejected with a localized reason; a valid
      rebind takes effect with no restart and persists.
- [ ] `lashon-core`'s hotkey validator carries unit tests; `cargo test
      --workspace` is green; CI is green on `windows-2022`, `macos-14`,
      `ubuntu-24.04`.

## Files

- `apps/desktop/src/lib/i18n/index.ts`, `locales/he.json`, `locales/en.json` —
  the i18n store and the two catalogs.
- `apps/desktop/src/lib/settings.ts` — the typed settings module.
- `apps/desktop/src/lib/hotkey.ts` — accelerator capture and display helpers.
- `apps/desktop/src/lib/snap.ts` — migrated onto the settings module.
- `apps/desktop/src/routes/+layout.svelte` — i18n init, `dir`/`lang` application.
- `apps/desktop/src/routes/+page.svelte` — the configured-hotkey registration.
- `apps/desktop/src/routes/tutorial/+page.svelte` — localized.
- `apps/desktop/src/routes/hub/+page.svelte` — the Hub.
- `apps/desktop/src/lib/components/HotkeyCapture.svelte` — the chord-capture
  control.
- `apps/desktop/src/lib/components/Tongue.svelte`, `DebugSurface.svelte` —
  localized.
- `apps/desktop/src-tauri/tauri.conf.json` — the `hub` window.
- `apps/desktop/src-tauri/capabilities/hub.json` — its capability.
- `apps/desktop/src-tauri/src/lib.rs` — the tray entry, `show_hub`, and the
  `validate_hotkey` command.
- `packages/shared-rust/src/hotkey.rs`, `lib.rs` — the accelerator validator.
- `docs/adr/0011-localization-architecture.md` — the decision record.

## Dependencies

Depends on the `hub`-style separate window pattern from
[ADR-0008](../adr/0008-first-run-tutorial-window.md) and the `tauri-plugin-store`
wiring from M0–M3. Does not block any other milestone. Unblocks the M4
onboarding-hardware slice (mic permission + hardware-tier detection), which
hangs new sections on this Hub and new keys on this settings module.
