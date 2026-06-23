# Packaging — Windows

How to build the Lashon Windows installer. See
[ADR-0006](adr/0006-release-packaging-and-signing.md) for the design rationale,
and [releasing.md](releasing.md) for the full release process this fits into.

The release ships **two Windows artifacts** — a ~66 MB NSIS installer and a
portable zip ([ADR-0012](adr/0012-portable-distribution-and-all-users-install.md)).
GPU acceleration is not bundled: when an NVIDIA GPU is present, the app
downloads the CUDA runtime from PyPI on first run.

## Prerequisites

- Rust 1.95 (`rust-toolchain.toml`), Node 20+, Python 3.11–3.12.
- The STT sidecar virtual environment, with the `build` extra:

  ```sh
  cd services/stt-sidecar
  python -m venv .venv
  .venv/Scripts/python -m pip install -e ".[build]"
  ```

## 1. Freeze the STT sidecar

From `services/stt-sidecar`:

```sh
.venv/Scripts/pyinstaller --noconfirm --clean PyInstaller.spec
```

Output: `dist/lashon-stt/` — a one-folder bundle (`lashon-stt.exe` +
`_internal/`). Copy it where the Tauri bundle expects it:

```sh
rm -rf ../../apps/desktop/src-tauri/binaries/lashon-stt
cp -r dist/lashon-stt ../../apps/desktop/src-tauri/binaries/lashon-stt
```

## 2. Build the installer

From `apps/desktop`:

```sh
npm install
npm run tauri build
```

Output: `target/release/bundle/nsis/Lashon_0.1.0_x64-setup.exe` — the Cargo
workspace places `target/` at the repository root, not under `src-tauri/`.

The installer is built with NSIS `installMode: "both"` — at install time the
user picks a per-user install (no elevation) or an all-users, machine-wide
install (elevated). See [ADR-0012](adr/0012-portable-distribution-and-all-users-install.md).

## 3. Package the portable zip

The portable artifact is the release `lashon.exe` plus the frozen sidecar
bundle — the same bundle the installer ships. Stage them into a `lashon.exe` +
`binaries/lashon-stt/` layout and zip that. Build the zip from the pristine
`src-tauri/binaries/` bundle, **not** from `target/release/binaries/`: a sidecar
run can extract the ~1.7 GB runtime CUDA libraries into the staged release
tree, and CUDA is never shipped. From the repository root, in PowerShell:

```powershell
$stage = "$env:TEMP\lashon-portable"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$stage\binaries" | Out-Null
Copy-Item target\release\lashon.exe $stage
Copy-Item -Recurse apps\desktop\src-tauri\binaries\lashon-stt "$stage\binaries\lashon-stt"
Compress-Archive -Path "$stage\*" -DestinationPath target\release\Lashon-X.Y.Z-windows-x64-portable.zip -Force
```

The portable app runs in place — no installer, no registry writes, no
elevation — and downloads the model (and CUDA runtime) on first run exactly as
the installed app does. It needs the OS WebView2 runtime, so it targets
Windows 11.

## 4. Signing

v0.1.0 ships **unsigned** (ADR-0006). Windows SmartScreen warns on first run.
Signing every binary with a code-signing certificate — Certum Open Source, Azure
Trusted Signing, or an OV/EV certificate — is the v0.1.x follow-up; it slots in
as a final step here, via `tauri.conf.json`'s `bundle.windows` signing options.

## Notes

- The frozen sidecar under `binaries/` is a build artifact — git-ignored, never
  committed. A fresh checkout cannot `tauri build` until step 1 has produced it.
- The Hebrew STT model is **not** bundled; the app downloads it on first run.
- The **MIT-licensed "Hey Lashon" wake classifier** (`models/wake/wakewords/hey_lashon.onnx`)
  is listed in `tauri.conf.json`'s `bundle.resources` and ships with the
  installer. On first launch the Tauri shell stages it into
  `$LASHON_MODELS_ROOT/wakewords/` (see `stage_bundled_wake_classifiers` in
  `apps/desktop/src-tauri/src/lib.rs`). The four CC-BY-NC openWakeWord
  classifiers remain opt-in downloads from the Settings Hub — never bundled.
- `tauri dev` is unaffected by all of this — it runs the sidecar from Python
  source (set `LASHON_PYTHON` to the venv interpreter if `python` on `PATH` is
  not the right one).
