# 13. Hardware-tier detection and the microphone permission probe

- **Status:** Accepted
- **Date:** 2026-05-18
- **Deciders:** Lashon contributors
- **Context source:** Milestone M4, the onboarding-hardware slice
  ([`../stories/m4-onboarding-hardware.md`](../stories/m4-onboarding-hardware.md))

## Context

M4's onboarding sequence ([`../roadmap.md`](../roadmap.md) §1.8) is
welcome → mic permission → hardware-tier detection → model download → hotkey
rebind → live test → done. The interactive tutorial and the Settings Hub
shipped earlier slices; what remained were the two hardware-facing steps —
**microphone permission** and **hardware-tier detection**.

Two questions needed deciding: how to detect the host's capability tier, and
how to handle the microphone permission across Windows, macOS, and Linux —
which differ sharply, and where Tauri 2 offers no permission plugin.

`docs/tech-stack.md` already defines four tiers (A–D) and their thresholds, and
`apps/desktop/src-tauri/tiers.json` maps each to default models. The detection
itself did not exist.

## Decision

**Tier detection is pure-classify-plus-best-effort-probe, in `lashon-core`.**
A new `hardware` module exposes `classify(&HardwareProbe) -> Tier` — pure, the
exact thresholds of `tech-stack.md`, fully unit-tested — and `detect()`, which
fills a `HardwareProbe` from three probes:

- **NVIDIA GPU + VRAM** via `nvml-wrapper`. NVML init failing is the ordinary
  non-NVIDIA case, not an error.
- **System RAM** via `sysinfo`.
- **A Vulkan GPU** via a minimal `ash` instance — the AMD / Intel path that
  separates Tier C from Tier D. A software rasterizer (`CPU` device type) does
  not count.

Every probe degrades to a conservative reading when its backend is absent, so
`detect()` never fails — the worst case is Tier D.

**Microphone permission is a capture-stream probe, not a permission API.**
There is no portable "is the mic permitted" call. The honest test is to open
the default input device and start a capture stream: `audio::probe_input()`
returns `Ready`, `NoDevice`, or `Unavailable{reason}`. On macOS, *starting* a
capture stream is itself what raises the OS permission prompt on first use — so
the probe doubles as the permission request. The stream's callback discards
every frame and the stream is dropped immediately; no audio is retained
([`.claude/rules/security.md`](../../.claude/rules/security.md)).

**Both are `#[tauri::command]`s run on a blocking thread.** `detect_hardware`
and `probe_microphone` hand off to `tauri::async_runtime::spawn_blocking`, so
NVML/Vulkan latency — and, on a first-run macOS prompt, the wait for the user —
never blocks the main thread.

**The result persists as `hardware.tier`; the user always chooses.** The
detected tier is written to the `settings.json` store and shown pre-selected in
a four-card picker. Lashon never silently up- or downgrades — onboarding and
the Hub's new **Hardware** section both present the picker, and an override is
kept across a tutorial reopen.

**The steps extend the tutorial window.** Per
[ADR-0008](0008-first-run-tutorial-window.md) the tutorial window is the
first-run surface; the mic and hardware steps slot into its walkthrough
(welcome → microphone → hardware → tongue → …) rather than into a new window.

## Alternatives considered

- **Per-OS GPU enumeration (WMI / DXGI on Windows, Metal on macOS).** More code
  on every platform for no extra signal — the CUDA-vs-Vulkan split that the
  tiers actually turn on is already covered by NVML plus a Vulkan instance.
- **A real microphone-permission API.** Tauri 2 ships no mic-permission plugin;
  a true macOS check is `AVCaptureDevice authorizationStatus` via a native
  call. The stream probe is simpler, portable, and — unlike a status query —
  also raises the first-run prompt, which is the point of the step.
- **A dedicated onboarding window.** Rejected: the tutorial window already is
  the first-run surface, and ADR-0008 set the separate-window pattern. A second
  window would duplicate the gating and the chrome.
- **Skipping Vulkan detection.** Would collapse Tier C into Tier D for every
  AMD / Intel machine — a real misclassification for office hardware. `ash`
  loads the Vulkan loader at runtime, so it costs nothing on a host without it.

## Consequences

- Three new `lashon-core` dependencies — `nvml-wrapper`, `sysinfo`, `ash` — all
  MIT / Apache-2.0, so the `cargo-deny` allow-list is unchanged. Each loads its
  backend at runtime, so the crate still builds and tests on a GPU-less CI
  runner; `classify()` is unit-tested, the probes are not.
- `hardware.tier` is **advisory in M4**: it is detected, shown, overridable, and
  stored, but no code consumes it yet. Wiring the tier to model selection
  against `tiers.json` is M5-and-later work.
- On macOS the permission prompt needs `NSMicrophoneUsageDescription` in the
  app's `Info.plist`; that string lands with M13's macOS packaging and
  entitlements (Phase 4). On Windows and Linux the probe works as shipped.
- The probe opens a real capture stream. It is brief and silent, but it does
  touch the microphone — acceptable, since hearing the mic is exactly what the
  onboarding step verifies.
- The tier picker (`TierSelect.svelte`) and the detection command are shared by
  the tutorial step and the Hub's Hardware section, so a re-detect or an
  override behaves identically in both.
