# 11. Localization architecture (Hebrew / English UI)

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** Lashon contributors
- **Context source:** Milestone M4, the Settings Hub slice
  ([`../stories/m4-settings-hub.md`](../stories/m4-settings-hub.md))

## Context

Until M4 every UI string was a hardcoded Hebrew literal, and `<html>` was
fixed to `lang="he" dir="rtl"`. M4's Definition of Done requires the interface
in **he + en**. That needs four decisions: a localization library (or none),
the catalog shape, how the language is chosen and persisted, and how the
several Tauri windows — each its own webview — stay in sync when it changes.

## Decision

**A small in-house i18n store — no library.** `apps/desktop/src/lib/i18n/`
holds the two catalogs as static JSON (`locales/he.json`, `locales/en.json`)
and a compact `index.ts`: a `locale` writable, a `t` store derived from it that
resolves a dotted key (`tutorial.steps.welcome.kicker`) against the active
catalog, and `applyLanguage`. Hebrew is the **fallback** — a key missing from a
catalog resolves against the Hebrew catalog, then echoes the key itself, so a
gap is visible, never silently English. Components use `$t('key')` exactly as
they would a library's store.

**Bundled catalogs.** The JSON is imported statically, so `$t` resolves
synchronously from first paint — no loading state, no async locale fetch.

**Language is a persisted setting.** `ui.language` lives in the `settings.json`
store, read through the typed `$lib/settings` module. The root
`routes/+layout.svelte` applies it on load and sets
`document.documentElement.{lang,dir}` (`he`→`rtl`, `en`→`ltr`).

**Cross-window sync via a Tauri event.** Each window is a separate webview, so
the i18n module initializes per window. When the Hub changes the language it
writes the setting and broadcasts `settings:changed`; every window's layout
listens and re-applies the locale live. The same event carries a rebound
hotkey to the tongue window.

**The tray menu is bilingual, not re-localized.** The tray is built once in
Rust; its items carry `he · en` labels. Re-localizing it would mean reading the
language setting in Rust and rebuilding the menu on every change —
disproportionate for OS chrome that is not an app surface.

## Alternatives considered

- **`svelte-i18n`.** The initial choice — it is named in
  [`../tech-stack.md`](../tech-stack.md) and is the obvious library for a
  Svelte app — and the implementation first used it. Rejected on two grounds.
  First, its `intl-messageformat` → `tslib` dependency chain does not resolve
  under the project's pinned Vite 8 / Rolldown bundler: the production build
  fails with `Rolldown failed to resolve import "tslib"`. Second, and the
  reason not to fight that with bundler configuration: every Lashon UI string
  is static — no ICU interpolation, no pluralization, no runtime locale
  negotiation — so svelte-i18n's machinery is far more than the feature needs.
  A dotted-key lookup store is the right size and pulls in no dependency.
- **`paraglide-js`** — compile-time, type-safe messages, but a heavier
  build-tooling change for the same static-string need.
- **English as the fallback locale** — rejected: Lashon is Hebrew-first; a
  missing key must surface in Hebrew.
- **A dynamically re-localized tray** — rejected as above; it pulls i18n into
  the Rust shell for little gain.

## Consequences

- No new runtime dependency — the i18n store is `svelte/store` plus two JSON
  files. `tech-stack.md`, which had listed `svelte-i18n`, is corrected.
- `he.json` and `en.json` must keep matching key sets; an English gap renders
  Hebrew. The story's acceptance criteria assert parity.
- New UI must route every string through `$t` — a hardcoded literal is a
  localization regression, and Hebrew literals are easy to add by reflex.
- The store does plain key lookup, no interpolation. A string that must embed
  a value interpolates in the component (as the tutorial's warm-up percentage
  already does); if that grows common, the `t` store gains a small format step.
- The tray stays bilingual and is unaffected by the in-app language toggle.
- Per-window initialization means the always-visible tongue applies the
  persisted language a frame after first paint. The tongue carries no visible
  text — only brief ARIA-live state announcements — and starts in Hebrew, the
  fallback; the lone consequence is that an English user could, rarely, hear
  one Hebrew state announcement if a dictation state change races the locale
  load. The Hub and tutorial windows ship hidden, so their locale is settled
  before they are shown.
- Hot path untouched: i18n is frontend-only. The dictation FSM, the sidecar,
  and `lashon-core` are unaffected — the one Rust addition for M4, the `hotkey`
  accelerator validator, is unrelated to localization.
