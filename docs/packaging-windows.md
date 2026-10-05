# Packaging — Windows

How to build the Ottid Windows installer. See
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

Output: `dist/ottid-stt/` — a one-folder bundle (`ottid-stt.exe` +
`_internal/`). Copy it where the Tauri bundle expects it:

```sh
rm -rf ../../apps/desktop/src-tauri/binaries/ottid-stt
cp -r dist/ottid-stt ../../apps/desktop/src-tauri/binaries/ottid-stt
```

## 2. Build the installer

From `apps/desktop`:

```sh
npm install
npm run tauri build
```

Output: `target/release/bundle/nsis/Ottid_0.1.0_x64-setup.exe` — the Cargo
workspace places `target/` at the repository root, not under `src-tauri/`.

The installer is built with NSIS `installMode: "both"` — at install time the
user picks a per-user install (no elevation) or an all-users, machine-wide
install (elevated). See [ADR-0012](adr/0012-portable-distribution-and-all-users-install.md).

### Editions

One source builds two editions ([ADR-0034](adr/0034-command-mode-editioning.md)).
The command above builds the **full** one: it bundles everything in
`tauri.conf.json`'s `bundle.resources`. The **free**, dictation-only edition,
which the release workflow ships, is built like this:

```sh
VITE_OTTID_EDITION=free npm run tauri build -- --config src-tauri/tauri.free.conf.json -- --no-default-features
```

`tauri.free.conf.json` lists the free resources in full (Tauri replaces an
array when it merges configs), leaving out what only the full edition ships:
`llama-server`, `ottid-mcp` and the starter recipes
([ADR-0049](adr/0049-bundle-ottid-mcp-and-the-starters-in-the-full-edition.md)).

Before a **full** build, stage the two binaries it bundles, from the repository
root:

- `llama-server`: mirror it per
  [`binaries/llama-server/README.md`](../apps/desktop/src-tauri/binaries/llama-server/README.md)
  ([ADR-0025](adr/0025-in-process-local-llm.md)).
- `ottid-mcp`: `bash scripts/stage-ottid-mcp.sh` builds it and copies it to
  `apps/desktop/src-tauri/binaries/ottid-mcp/`. When signing, run it before the
  `sign-windows.ps1 -Tree` step in §4.

The starter recipes are bundled straight from `recipes/starters/`; nothing to
stage. The installed full edition has the MCP server at
`<install folder>\binaries\ottid-mcp\ottid-mcp.exe`, which is
`%LOCALAPPDATA%\Programs\Ottid\binaries\ottid-mcp\ottid-mcp.exe` for a
per-user install. Point an MCP host such as Claude Desktop at that file. It
finds the starters by itself, so the host's config needs no
`OTTID_BUNDLED_RECIPES_DIR`.

## 3. Package the portable zip

The portable artifact is the release `ottid.exe` plus the frozen sidecar
bundle — the same bundle the installer ships. Stage them into an `ottid.exe` +
`binaries/ottid-stt/` layout and zip that. Build the zip from the pristine
`src-tauri/binaries/` bundle, **not** from `target/release/binaries/`: a sidecar
run can extract the ~1.7 GB runtime CUDA libraries into the staged release
tree, and CUDA is never shipped. From the repository root, in PowerShell:

```powershell
$stage = "$env:TEMP\ottid-portable"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$stage\binaries" | Out-Null
Copy-Item target\release\ottid.exe $stage
Copy-Item -Recurse apps\desktop\src-tauri\binaries\ottid-stt "$stage\binaries\ottid-stt"
Compress-Archive -Path "$stage\*" -DestinationPath target\release\Ottid-X.Y.Z-windows-x64-portable.zip -Force
```

The portable app runs in place — no installer, no registry writes, no
elevation — and downloads the model (and CUDA runtime) on first run exactly as
the installed app does. It needs the OS WebView2 runtime, so it targets
Windows 11.

## 4. Signing

Only the release workflow signs, with Azure Artifact Signing
([ADR-0043](adr/0043-sign-windows-releases-with-azure-artifact-signing.md)). A
local build like the one above is unsigned, and Windows SmartScreen warns on
its first run.

To sign a local build by hand, you need the `az login` session of an identity
that holds the *Artifact Signing Certificate Profile Signer* role. Set the
environment variables listed in `scripts/sign-windows.ps1`, then:

1. Sign the staged resources:
   `powershell -ExecutionPolicy Bypass -File scripts/sign-windows.ps1 -Tree apps/desktop/src-tauri/binaries`
2. Build with `npm run tauri build -- --config src-tauri/tauri.signing.conf.json`.
   Because `createUpdaterArtifacts` is on, this build also needs the updater
   key in `TAURI_SIGNING_PRIVATE_KEY`.
3. Check the result with
   `scripts/sign-windows.ps1 -Verify <install-or-portable-dir>`.

In the portable zip, sign the staged `ottid.exe` separately. Tauri restores
`target/release/ottid.exe` to its unsigned build after bundling.

## Notes

- The frozen sidecar under `binaries/` is a build artifact — git-ignored, never
  committed. A fresh checkout cannot `tauri build` until step 1 has produced it.
- The Hebrew STT model is **not** bundled; the app downloads it on first run.
- The portable zip in §3 has the free edition's layout. A full-edition zip would
  also need `binaries/ottid-mcp/` and the starters under
  `_up_/_up_/_up_/recipes/starters/`
  ([ADR-0049](adr/0049-bundle-ottid-mcp-and-the-starters-in-the-full-edition.md)).
- The **MIT-licensed "Hey Lashon" wake classifier** (`models/wake/wakewords/hey_lashon.onnx`)
  is listed in `tauri.conf.json`'s `bundle.resources` and ships with the
  installer. On first launch the Tauri shell stages it into
  `$OTTID_MODELS_ROOT/wakewords/` (see `stage_bundled_wake_classifiers` in
  `apps/desktop/src-tauri/src/lib.rs`). The four CC-BY-NC openWakeWord
  classifiers remain opt-in downloads from the Settings Hub — never bundled.
- `tauri dev` is unaffected by all of this — it runs the sidecar from Python
  source (set `OTTID_PYTHON` to the venv interpreter if `python` on `PATH` is
  not the right one).
