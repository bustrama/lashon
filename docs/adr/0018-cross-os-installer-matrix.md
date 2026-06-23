# 18. Cross-OS installer matrix: per-OS PyInstaller freeze + macOS DMG + Linux AppImage

- **Status:** Partially paused by
  [ADR-0033](0033-focus-on-windows-for-v1.md) — the macOS `.dmg` / Linux
  `.AppImage` outputs are deferred until a tester for those platforms exists;
  the Windows half of the matrix continues. The design below stands for when
  they resume.
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` (Phase 4 — build, packaging, signing),
  `docs/adr/0006-release-packaging-and-signing.md`,
  `docs/adr/0012-portable-distribution-and-all-users-install.md`

## Context

After M6 (`v0.5.0`) the Rust + SvelteKit frontend compiles on all three CI
runners (Windows / macOS / Linux), but the release pipeline only produces a
Windows NSIS installer. The STT sidecar — a PyInstaller-frozen Python process —
was only frozen on the Windows runner.

Three problems block macOS and Linux releases:

1. **PyInstaller cannot cross-compile.** Each OS must produce its own frozen
   sidecar. A Windows runner cannot produce a macOS or Linux binary.
2. **The sidecar binary name is OS-specific.** The existing
   `configure_sidecar_env` in `apps/desktop/src-tauri/src/lib.rs` hard-coded
   `lashon-stt.exe` — the right name on Windows, wrong on macOS/Linux.
3. **`tauri.conf.json` declared only `nsis`.** The `dmg` and `appimage` Tauri
   bundle targets were absent.

A fourth constraint is CUDA: the `nvidia-cublas-cu12` and `nvidia-cudnn-cu12`
packages in the `[cuda]` extra are NVIDIA/x86_64 binaries that cannot install
on Apple Silicon. The `PyInstaller.spec` must not attempt to collect them on
macOS.

## Decision

### Per-OS PyInstaller freeze in the release workflow

A new `.github/workflows/release.yml` runs on `v*` tag pushes with a
three-runner matrix: `windows-2022`, `macos-14`, `ubuntu-24.04`. Each runner:

1. Installs the sidecar's `[build]` extra (PyInstaller, GPL-isolated from
   runtime deps per ADR-0006).
2. Runs `pyinstaller PyInstaller.spec` to produce `dist/lashon-stt/`.
3. Stages the bundle into `apps/desktop/src-tauri/binaries/lashon-stt/`.
4. Runs `tauri-apps/tauri-action` (pinned to a commit SHA) to build and upload
   the OS-appropriate installer to the GitHub Release.

The `[cuda]` extra is intentionally omitted on all three runners. On Windows
and Linux, CUDA is downloaded from PyPI on first run when an NVIDIA GPU is
present (ADR-0006). On macOS, CUDA is never applicable.

### PyInstaller spec — platform-aware CUDA collection

`PyInstaller.spec` now probes `importlib.util.find_spec("nvidia")` at
spec-parse time. If the `nvidia` namespace package is absent (macOS, or any
runner where `[cuda]` was not installed), CUDA collection is skipped. This is a
pure-Python check — no platform conditionals — so the spec remains a single
file and the CPU-only path is the default for all OSes.

### Sidecar binary path — OS-specific via `cfg!`

`configure_sidecar_env` in `apps/desktop/src-tauri/src/lib.rs` now uses
`cfg!(target_os = "windows")` to select the sidecar path:

```rust
#[cfg(target_os = "windows")]
let sidecar_rel = "binaries/lashon-stt/lashon-stt.exe";
#[cfg(not(target_os = "windows"))]
let sidecar_rel = "binaries/lashon-stt/lashon-stt";
```

The rest of the function — model/CUDA directory creation and
`stage_bundled_wake_classifiers` — is unchanged.

### Tauri bundle targets

`tauri.conf.json` is updated to declare `["nsis", "dmg", "appimage"]`. In the
release workflow, each runner passes `--bundles <target>` to `tauri-action` to
produce only its OS bundle. The list in `tauri.conf.json` is the superset; CI
never produces all three on a single runner.

A macOS `entitlements.plist` is added to `apps/desktop/src-tauri/` declaring
microphone access (`com.apple.security.device.audio-input`) and Accessibility
automation (`com.apple.security.automation.apple-events`). These entitlements
are required but are not enforced until the app is notarized.

### Installer formats chosen per OS

| OS | Format | Signing | Notes |
|---|---|---|---|
| Windows | NSIS `.exe` + portable `.zip` | Unsigned (M13) | Per ADR-0006 and ADR-0012 |
| macOS | `.dmg` | Unsigned (M13) | Gatekeeper warns; right-click → Open bypasses |
| Linux | `.AppImage` | Not required | Self-contained; runs without install |

**macOS — unsigned rationale.** An Apple Developer ID costs money and requires
identity validation. Until the Developer Program purchase is made (M13 gate),
macOS releases ship unsigned and unnotarized. The release notes document the
Gatekeeper bypass. This is the same deliberate, time-boxed exception pattern
as the Windows unsigned preview (ADR-0006).

**Linux — AppImage over deb / rpm / snap / flatpak.** AppImage is
distro-agnostic, requires no root, and bundles all dependencies. A `.deb` would
only serve Debian/Ubuntu; `.rpm` only Fedora/RHEL; Snap and Flatpak add
sandboxing complexity that conflicts with Lashon's need for Accessibility API
access (text injection) and microphone access at the OS level. AppImage is the
least-friction packaging for a first release.

### feat/auto-update interaction

A sibling branch (`feat/auto-update`) is wiring `tauri-plugin-updater`. That
branch will need to add `TAURI_SIGNING_PRIVATE_KEY` signing to the release
workflow. Proposed merge order: `feat/cross-os-installers` first, then
`feat/auto-update` on top, so the auto-update branch adds its signing step to
an already-functional per-OS matrix. The auto-update branch should reference
this ADR and the comment in `release.yml` when resolving the conflict.

ADR-0017 (`auto-update-via-tauri-plugin-updater`) covers that work and is
reserved for that branch. This ADR is therefore numbered 0018.

## Consequences

- **New docs:** `docs/packaging-macos.md` and `docs/packaging-linux.md` mirror
  `docs/packaging-windows.md`'s structure.
- The Windows portable zip step now runs as a post-build PowerShell step in the
  release workflow (previously a manual runbook step only).
- A fresh checkout still cannot `tauri build` without first running PyInstaller —
  the constraint from ADR-0006 is unchanged.
- The macOS sidecar is CPU-only. A future ADR will add `whisper.cpp` + Metal
  as an alternative `SttProvider` for macOS GPU users. That is explicitly out
  of scope here — the focus is a buildable, shippable macOS installer.
- CI (`ci.yml`) is unchanged — it does `cargo check` and tests on all three
  runners but does not freeze the sidecar or build an installer.

## Alternatives considered

- **Cross-compile the sidecar from Windows.** PyInstaller cannot cross-compile;
  attempting it produces a binary that crashes on the target OS.
- **Artifact upload + download across jobs.** An alternative approach would
  freeze the sidecar in separate per-OS jobs and pass the result to a single
  `tauri-action` job. Rejected: `tauri-action` requires the sidecar to be in
  place before building, and parallelism across OSes already exists in the
  matrix — combining them in a single job is cleaner.
- **`.deb` or `.rpm` for Linux.** Distro-specific. AppImage is universal.
- **Flatpak / Snap.** Sandboxing breaks Accessibility API text injection and
  requires significant additional packaging work. Revisit if distribution via
  the Flathub store becomes a goal.
- **Universal macOS binary (`universal-apple-darwin`) in CI.** Would require
  a cross-compilation from macOS 14 arm64 to x86_64. Deferred — arm64-only is
  sufficient for the Apple Silicon majority and the `x86_64` target can be
  added later.
