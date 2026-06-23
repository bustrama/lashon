# Onboarding: microphone permission + hardware tier

The final vertical slice of milestone **M4** (Onboarding + settings). Branch
`claude/plan-roadmap-AAppb`.

> **Status: in progress.** This is the third and last M4 slice. The first two
> shipped — the interactive first-run tutorial in `v0.2.0`
> ([`m4-interactive-tutorial.md`](m4-interactive-tutorial.md)) and the Settings
> Hub with he+en i18n in `v0.3.0` ([`m4-settings-hub.md`](m4-settings-hub.md)).
> Landing this slice completes M4.

## Why

The roadmap's onboarding sequence ([`../roadmap.md`](../roadmap.md) §1.8) is
welcome → **mic permission** → **hardware-tier detection** → model download →
hotkey rebind → live test → done. The tutorial slice delivered the
walkthrough and the warm-up; the Settings Hub delivered the persistence and
the hotkey. The two hardware-facing steps were all that remained: a fresh
install never confirmed that Lashon could hear the microphone, and never
detected what the host could actually run — so it could not pick sensible
default models. This slice closes both.

## How

See [ADR-0013](../adr/0013-onboarding-hardware-detection.md) for the decisions.

### Hardware-tier detection (`lashon-core`)

- `packages/shared-rust/src/hardware.rs` — a new module. `classify()` is the
  pure function: a `HardwareProbe` (CUDA, VRAM, RAM, Vulkan) in, a `Tier`
  (A–D) out, exactly the thresholds of [`../tech-stack.md`](../tech-stack.md).
  It carries the unit tests.
- `detect()` fills the probe best-effort — `nvml-wrapper` for the NVIDIA GPU
  and its VRAM, `sysinfo` for RAM, a minimal `ash` Vulkan instance for the
  AMD/Intel GPU path. Every probe degrades to a conservative reading when its
  backend is absent, so detection never fails; the worst case is Tier D.

### Microphone probe (`lashon-core`)

- `audio::probe_input()` opens the default input device and briefly starts a
  capture stream, returning `Ready`, `NoDevice`, or `Unavailable{reason}`.
  There is no portable permission API; opening the stream *is* the test — and
  on macOS, starting it is what raises the OS permission prompt. The callback
  discards every frame and the stream is dropped at once: no audio is retained
  ([`.claude/rules/security.md`](../../.claude/rules/security.md)).

### Tauri commands

- `detect_hardware` and `probe_microphone` in `apps/desktop/src-tauri/src/lib.rs`,
  each handing off to `spawn_blocking` so detection latency — and a first-run
  macOS prompt — never blocks the main thread.

### Onboarding steps

- The tutorial walkthrough gains two steps after `welcome`:
  `welcome → microphone → hardware → tongue → hotkey → practice → done`. The
  step rendering is now keyed by step name, not index, so the inserts do not
  shift the practice step's live-FSM logic.
- The **microphone** step probes on entry — which is also when the macOS
  prompt appears — and shows a `ready` / `no-device` / `blocked` status with a
  "check again" control, reusing the practice step's status-card tones.
- The **hardware** step detects on entry, shows the detected RAM/GPU readings,
  and presents the four tiers in a picker with the detected tier pre-selected
  and badged. The chosen tier persists to `hardware.tier`.

### Settings + the Hub

- `settings.ts` gains `hardware.tier` (`Tier | null`, default `null` until
  onboarding runs).
- The Hub gains a **Hardware** section — the same `TierSelect` picker, the
  detected readings, and a "detect again" control — so the tier is reviewable
  and overridable after onboarding. Lashon never silently changes it.
- `TierSelect.svelte` is the shared picker; `$lib/hardware.ts` carries the
  frontend mirror of the Rust `HardwareReport` / `MicProbe` shapes.

## Acceptance Criteria

- [ ] The first-run tutorial runs welcome → microphone → hardware → tongue →
      hotkey → practice → done; the practice step still recognises a live
      dictation cycle.
- [ ] The microphone step probes the mic and shows a localized ready /
      no-device / blocked status; "check again" re-probes.
- [ ] The hardware step detects a tier, shows the RAM/GPU readings, and lets
      the user pick any of the four tiers; the choice persists across a restart
      and is kept when the tutorial is reopened.
- [ ] The Hub's Hardware section shows the saved tier, re-detects on demand,
      and overrides persist.
- [ ] Every new string resolves through the i18n catalogs; `he.json` and
      `en.json` key sets match; the UI is RTL-native and
      `prefers-reduced-motion` is honoured; `npm run check` is clean.
- [ ] `lashon-core`'s `classify()` carries unit tests; `cargo test --workspace`
      is green; CI is green on `windows-2022`, `macos-14`, `ubuntu-24.04`.

## Files

- `packages/shared-rust/src/hardware.rs` — tier detection (`classify` + tests,
  `detect`).
- `packages/shared-rust/src/audio.rs` — `MicProbe` and `probe_input()`.
- `packages/shared-rust/src/lib.rs`, `Cargo.toml` — the module and the three
  detection dependencies.
- `apps/desktop/src-tauri/src/lib.rs` — the `detect_hardware` and
  `probe_microphone` commands.
- `apps/desktop/src/lib/hardware.ts` — the frontend tier / probe types.
- `apps/desktop/src/lib/settings.ts` — the `hardware.tier` key.
- `apps/desktop/src/lib/components/TierSelect.svelte` — the shared tier picker.
- `apps/desktop/src/routes/tutorial/+page.svelte` — the microphone and hardware
  steps.
- `apps/desktop/src/routes/hub/+page.svelte` — the Hub Hardware section.
- `apps/desktop/src/lib/i18n/locales/{he,en}.json` — the new strings.
- `docs/adr/0013-onboarding-hardware-detection.md` — the decision record.

## Dependencies

Extends the tutorial window from
[ADR-0008](../adr/0008-first-run-tutorial-window.md) and the settings module
and Hub from [`m4-settings-hub.md`](m4-settings-hub.md). `hardware.tier` is
advisory in M4 — model selection reading it against `tiers.json` is M5-and-later
work. Completing this slice meets M4's Definition of Done.
