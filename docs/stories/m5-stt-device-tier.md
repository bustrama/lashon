# STT device selection by hardware tier

Milestone **M5** (final form). Branch `m5-stt-device`.

> **Status: in progress.** M5 went through three forms: an optional LLM
> cleanup pass (cut as a product decision); a whisper.cpp STT engine (abandoned
> — `pywhispercpp` has no Windows wheel, PR #32); and this — wiring the
> hardware tier to the STT device mode.

## Why

M4 detects the hardware tier and the Hub presents an A/B/C/D **picker** to
override it ([ADR-0013](../adr/0013-onboarding-hardware-detection.md)) — but
nothing read `hardware.tier`. A picker that changes nothing misleads the user:
it looks interactive, they choose, and nothing happens.

For an *auto-detected* tier there is nothing to wire — faster-whisper already
probes GPU→CPU and the CUDA download self-gates on GPU presence. But the picker
also allows an **override**, and an override is dishonest unless it does
something. This milestone makes it do something.

## How

See [ADR-0014](../adr/0014-stt-device-by-hardware-tier.md) for the decisions.

### Rust

- `lashon-core::hardware` — `Tier::from_code()` (parse the stored code) and
  `Tier::stt_device()` (A/B → `auto`, C/D → `cpu`), both pure and unit-tested.
- `apps/desktop/src-tauri/src/lib.rs` — at startup the shell reads
  `hardware.tier` from the settings store and sets `LASHON_STT_DEVICE` for the
  sidecar. No saved tier resolves to `auto`.

### Python sidecar

- `faster_whisper_engine.load_engine(cpu_only=False)` — with `cpu_only` the
  CUDA device is not attempted; the engine loads straight on the CPU.
- `server.py` — the warm-up reads `LASHON_STT_DEVICE`; `cpu` skips the
  CUDA-runtime download and passes `cpu_only=True`.

### Hub feedback

- The Hub's Hardware section shows what the selected tier means for speech
  recognition (GPU or CPU). When the tier is changed it shows a note and a
  **Restart now** button — the `restart_app` command relaunches the app so the
  change takes effect at once, rather than asking the user to restart by hand.
- The section's intro copy is reworded; the tier is no longer described as
  merely informative.

## Acceptance Criteria

- [x] `lashon-core::hardware` has unit tests for `from_code` (round-trip) and
      `stt_device` (the tier→device map); `cargo test --workspace` is green.
- [x] `cargo check --workspace` / `cargo clippy` clean; the sidecar imports
      cleanly and `load_engine` carries the `cpu_only` parameter.
- [x] The faster-whisper path is unchanged for an auto-detected tier — a
      verified no-op; the only behavioural change is an explicit CPU override.
- [ ] Manual check: override the tier to C on a GPU machine, restart, confirm
      the sidecar log reports the engine loaded on CPU (not run — no display
      in the build environment).
- [ ] CI green on `windows-2022`, `macos-14`, `ubuntu-24.04`.

## Files

- `packages/shared-rust/src/hardware.rs` — `from_code` / `stt_device` (+ tests).
- `apps/desktop/src-tauri/src/lib.rs` — `LASHON_STT_DEVICE` resolution and the
  `restart_app` command.
- `services/stt-sidecar/src/lashon_stt/engines/faster_whisper_engine.py` —
  `load_engine(cpu_only=…)`.
- `services/stt-sidecar/src/lashon_stt/server.py` — the device-aware warm-up.
- `apps/desktop/src/routes/hub/+page.svelte` — the Hardware-section feedback
  (the device note and the restart-to-apply line).
- `apps/desktop/src/lib/i18n/locales/{he,en}.json` — the Hardware copy.
- `docs/adr/0014-stt-device-by-hardware-tier.md` — the decision record.

## Dependencies

Builds on M4's hardware-tier detection (ADR-0013). No new runtime dependency —
pure faster-whisper. Unlike the abandoned whisper.cpp attempt, nothing here is
platform-specific.
