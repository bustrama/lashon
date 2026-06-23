# 33. Focus on Windows for v1.0; pause macOS and Linux

## Status

Accepted — 2026-06-11. Pauses the macOS / Linux halves of
[ADR-0018](0018-cross-os-installer-matrix.md) and narrows the open-core v1.0
plan to Windows.

## Context

The open-core pivot ([ADR-0032](0032-ship-as-open-core-product.md)) makes
shipping a sellable **Windows** binary the near-term goal. The maintainer can
only **test on Windows** — there is no Mac or Linux machine in the loop to
validate a build, reproduce a bug, or smoke-test an installer before release.

Lashon's deepest features are already Windows-first: Command mode's UIA tooling,
`open_app` / `focus_window`, the recipe runtime's PowerShell shell-outs, the
Win32 Job Object sidecar lifecycle, and the NSIS / portable-zip packaging.
macOS / Linux have build targets and per-OS code paths (`inject/mac.rs`,
`inject/linux.rs`, `.dmg` / `.AppImage` outputs) but no test coverage by a human
who can see them fail.

Shipping an *unsigned, untested* Mac or Linux installer is worse than shipping
nothing there: it invites bug reports on platforms we cannot reproduce, and a
paid binary we cannot Gatekeeper-notarize would convert poorly anyway
(ADR-0032 already deferred Apple Developer ID).

## Decision

For v1.0, **Windows is the only shipping platform.**

- **Packaging / release:** the tagged-release matrix produces **Windows only**
  (NSIS + portable zip). The macOS `.dmg` and Linux `.AppImage` jobs are paused
  — re-enabled, with signing / notarization, only when a tester for that
  platform exists. (The workflow edit lands with the `v1.0.0` release setup, not
  now — no release is being cut this turn.)
- **Signing:** Windows via SignPath Foundation is the only signing on the
  critical path (ADR-0032). Apple Developer ID stays deferred.
- **Keep the codebase cross-platform.** The macOS / Linux `cfg` branches,
  injection profiles, and packaging docs (`docs/packaging-macos.md`,
  `docs/packaging-linux.md`) are **retained, not deleted** — cheap to keep and
  the head start for resuming those platforms later. "Focus on Windows" is a
  shipping / effort decision, not a code deletion.
- **Keep cross-platform CI for now.** The `macos-14` / `ubuntu-24.04` runners
  stay green as a free portability safety net (CONTRIBUTING DoD #2 unchanged).
  If Mac / Linux CI breakage starts blocking Windows work, demote those jobs to
  non-blocking rather than chasing fixes on untestable platforms — a separate,
  reversible call for when it actually bites.

## Consequences

- v0.6.0's headline — the macOS + Linux installer matrix (ADR-0018) — is paused;
  the auto-update half ([ADR-0017](0017-auto-update-via-tauri-plugin-updater.md))
  is Windows-relevant and stays. The next release is the open-core **`v1.0.0`**
  (Windows), not a cross-OS v0.6.0.
- Resuming macOS / Linux is gated on (a) a tester for the platform and (b)
  Windows traction justifying the signing spend — a post-v1.0 decision.
