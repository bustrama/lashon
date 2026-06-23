# 14. The hardware tier sets the STT device mode

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** Lashon contributors
- **Context source:** Milestone M5, the STT-device slice
  ([`../stories/m5-stt-device-tier.md`](../stories/m5-stt-device-tier.md))

## Context

M4 detects the host's hardware tier (A–D) and stores it as `hardware.tier`
([ADR-0013](0013-onboarding-hardware-detection.md)), and the Settings Hub
presents an A/B/C/D **picker** so the user can override the detected value.
But nothing consumed the setting — the tier was advisory.

A selectable picker that changes nothing is misleading: the user picks a tier,
something *looks* like it happened, and nothing did. "Informative" and
"interactive" contradict each other. Either the selection does something, or it
should not be a selection.

Investigation found that, for an *auto-detected* tier, there is genuinely
nothing to wire: `faster-whisper`'s engine loader already probes CUDA and falls
back to CPU, and the CUDA-runtime download already no-ops when no NVIDIA GPU is
present. The engine adapts itself. But the Hub's picker also allows an
**override** — and an override has no honest meaning unless it does something.

(M5 reached this form after two earlier ones: an optional LLM cleanup pass —
cut as a product decision; and a whisper.cpp STT engine — abandoned because
`pywhispercpp` has no Windows wheel and fails to build on the Windows CI
runner, see PR #32.)

## Decision

**The hardware tier sets the STT device mode**, so the Hub's picker is honest.

- A new env var, `LASHON_STT_DEVICE`: `auto` (probe the GPU, fall back to CPU)
  for tiers A/B, `cpu` (CPU only) for tiers C/D. The Tauri shell reads
  `hardware.tier` at startup and sets it; the sidecar obeys it.
- `Tier::stt_device()` in `lashon-core::hardware` is the map — pure and
  unit-tested. `Tier::from_code()` parses the stored code.
- The sidecar's `faster_whisper_engine.load_engine()` takes `cpu_only`: when
  set it does not attempt the CUDA device at all, and the warm-up skips the
  CUDA-runtime download.
- No saved tier (a fresh install) resolves to `auto` — the sidecar's existing
  behaviour, unchanged.

**The honest scope.** For a user whose tier matches their hardware — the common
case — this wiring is a confirmed no-op: the engine would probe to the same
device anyway. Its real effect is the **override**: a user who picks Tier C/D
on a machine that *has* a CUDA GPU now forces speech recognition onto the CPU.
That is the case the picker visually promises and that nothing honoured before.

## Alternatives considered

- **Leave the tier informative; remove the picker.** Honest, but discards
  ADR-0013's deliberate "Lashon never silently up- or downgrades — the user
  chooses" override, and removes a real capability (forcing the CPU path).
- **A second STT engine (whisper.cpp) selected by tier.** `pywhispercpp` ships
  no Windows wheel and its source build fails on the Windows CI runner;
  abandoned in PR #32. Lashon is Windows-first.
- **Wire only the auto-detected default.** Redundant — the engine already
  probes GPU→CPU. The override is the non-redundant part, so the wiring is
  framed around it.

## Consequences

- **No new dependency.** Pure faster-whisper, which the project already ships;
  installs on every platform — none of what blocked the whisper.cpp attempt
  applies.
- A tier change in the Hub takes effect on the **next app launch** — the STT
  sidecar is spawned once per session and `LASHON_STT_DEVICE` is read at
  startup.
- For most users (tier == detected hardware) the change is a verified no-op.
  The single behavioural change is honouring an explicit override to the CPU.
- The Hub's tier picker — and the onboarding tier step — are now honest:
  selecting a tier changes how speech recognition runs.
- This closes the "advisory in M4" gap ADR-0013 left, for STT. The tier will
  carry more weight at M7 (provider mux) and M10/M11 (TTS per tier).
