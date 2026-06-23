# 12. A portable distribution and an all-users installer

- **Status:** Accepted
- **Date:** 2026-05-19
- **Deciders:** Lashon contributors
- **Context source:** issues #10 and #11;
  [ADR-0006](0006-release-packaging-and-signing.md)

## Context

[ADR-0006](0006-release-packaging-and-signing.md) settled the release on a
single NSIS installer that installs for the current user only. Two user
requests followed:

- **#10 — all-users install.** The installer should be able to install
  machine-wide, for every account, not just the user who runs it.
- **#11 — portable version in a zip.** A copy that runs without installing —
  unzip and launch — for locked-down machines and USB-stick use.

## Decision

### All-users install: NSIS `installMode: "both"`

`bundle.windows.nsis.installMode` is set to `"both"` in `tauri.conf.json`. The
installer now offers a choice at install time: a per-user install (no
elevation, the path `v0.1.0`–`v0.3.0` shipped) or an all-users, machine-wide
install (requires elevation). `"both"` rather than `"perMachine"` keeps the
no-admin per-user path while adding the all-users option #10 asks for.

### Portable distribution: a zip of the app and the frozen sidecar

The portable distribution is the release `lashon.exe` (from `target/release/`)
plus the frozen STT sidecar bundle (`apps/desktop/src-tauri/binaries/lashon-stt/`
— the same bundle the installer ships), staged into a `lashon.exe` +
`binaries/lashon-stt/` layout and zipped as
`Lashon-X.Y.Z-windows-x64-portable.zip`. It runs in place: no installer, no
registry writes, no elevation. Resource resolution is unchanged — Tauri
resolves `BaseDirectory::Resource` next to the executable, the same as an
installed copy.

The zip is assembled from that pristine frozen bundle, **not** from the
`target/release/binaries/` tree `tauri build` stages: a sidecar run can extract
the ~1.7 GB runtime CUDA libraries into that staged tree, and CUDA is never
shipped — it is fetched on first run (ADR-0006).

The portable build is **not** a separate Tauri bundle target; Tauri has none
for Windows. It is a post-build packaging step, documented in
[`packaging-windows.md`](../packaging-windows.md).

Like the installed app, the portable app downloads the Hebrew model and — on
NVIDIA hardware — the CUDA runtime into per-user app-data on first run; that
behaviour is unchanged. It relies on the OS-provided WebView2 runtime (present
on Windows 11); the NSIS installer's WebView2 bootstrapper has no portable
equivalent, so the portable build targets Windows 11.

## Consequences

- GitHub Releases now hosts **two** Windows artifacts per release: the NSIS
  installer and the portable zip. ADR-0006's "GitHub Releases only ever hosts
  the small installer" is superseded for the artifact list — the model and the
  CUDA runtime are still never uploaded.
- No code changes. `installMode` is configuration; the portable zip is a
  packaging step. `tauri dev` and `lashon-core` are untouched.
- The portable app has no Start Menu entry, no uninstaller, and no auto-update
  path — auto-update (ADR-0006, Phase 4) will apply to the installed app only.

## Alternatives considered

- **`installMode: "perMachine"`** — force every install machine-wide. Rejected:
  it forces a UAC prompt on every install and removes the no-admin per-user
  path. `"both"` lets the user choose.
- **A dedicated Tauri portable bundle target.** Tauri has no portable target
  for Windows; the runnable `target/release/` tree is the de-facto portable
  layout, so a documented zip step is simpler than a custom bundler.
- **A single-file self-extracting portable exe.** Rejected: it would
  re-extract the frozen sidecar on every launch — the same cost ADR-0006
  rejected the one-file PyInstaller build for.
