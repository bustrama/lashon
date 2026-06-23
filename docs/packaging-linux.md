# Packaging — Linux

How to build the Lashon Linux installer. See
[ADR-0018](adr/0018-cross-os-installer-matrix.md) for the design rationale,
and [releasing.md](releasing.md) for the full release process this fits into.

The release ships **one Linux artifact** — an `.AppImage`. AppImages are
self-contained, run without installation, and need no package manager or root.
No signing is required; they are world-executable on extraction.

**STT device:** CPU by default. If an NVIDIA GPU is present (`nvidia-smi` on
`PATH`), the sidecar downloads the CUDA runtime from PyPI on first run,
exactly as on Windows (see [ADR-0006](adr/0006-release-packaging-and-signing.md)).

## Prerequisites

- Rust 1.95 (`rust-toolchain.toml`), Node 20+, Python 3.11–3.12.
- Tauri system dependencies (Ubuntu / Debian):

  ```sh
  sudo apt-get update
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libasound2-dev
  ```

  On Fedora / RHEL substitute `dnf install webkit2gtk4.1-devel gtk3-devel
  libappindicator-gtk3-devel librsvg2-devel libsoup3-devel alsa-lib-devel`.

- The STT sidecar virtual environment, with the `build` extra:

  ```sh
  cd services/stt-sidecar
  python3 -m venv .venv
  .venv/bin/python -m pip install -e ".[build]"
  ```

  To also build with GPU support (optional, requires CUDA toolkit):

  ```sh
  .venv/bin/python -m pip install -e ".[build,cuda]"
  ```

  The `cuda` extra is optional. Without it, `PyInstaller.spec` detects the
  absence of the `nvidia` namespace package and skips CUDA collection; the
  frozen sidecar will be CPU-only, and CUDA is downloaded on first run when
  `nvidia-smi` is present (same as the Windows installer — see ADR-0006).

## 1. Freeze the STT sidecar

From `services/stt-sidecar`:

```sh
.venv/bin/python -m PyInstaller --noconfirm --clean PyInstaller.spec
```

Output: `dist/lashon-stt/` — a one-folder bundle (`lashon-stt` + `_internal/`).
The binary has no file extension on Linux.

Copy it where the Tauri bundle expects it:

```sh
rm -rf ../../apps/desktop/src-tauri/binaries/lashon-stt
cp -r dist/lashon-stt ../../apps/desktop/src-tauri/binaries/lashon-stt
```

## 2. Build the AppImage

From `apps/desktop`:

```sh
npm install
npm run tauri build -- --bundles appimage
```

Output: `target/release/bundle/appimage/lashon_0.5.0_amd64.AppImage`.
The Cargo workspace places `target/` at the repository root.

Make it executable and run it:

```sh
chmod +x target/release/bundle/appimage/lashon_0.5.0_amd64.AppImage
./target/release/bundle/appimage/lashon_0.5.0_amd64.AppImage
```

## 3. Signing

`.AppImage` files do not require code signing. No signing step is needed for
Linux releases. Users can optionally verify a GPG signature if one is
published with the release (M13 work item).

## Notes

- The frozen sidecar under `binaries/` is a build artifact — git-ignored,
  never committed. A fresh checkout cannot `tauri build` until step 1 has
  produced it.
- The Hebrew STT model is **not** bundled; the app downloads it on first run
  into `~/.local/share/dev.lashon.desktop/models/`.
- The **MIT-licensed "Hey Lashon" wake classifier** (`hey_lashon.onnx`) ships
  inside the AppImage. On first launch the Tauri shell stages it into the
  per-user models directory
  (see `stage_bundled_wake_classifiers` in `apps/desktop/src-tauri/src/lib.rs`).
- The CUDA runtime (`nvidia-cudnn-cu12`, `nvidia-cublas-cu12`) is **not**
  bundled. On a machine with `nvidia-smi` available the sidecar downloads and
  verifies the runtime from PyPI on first run, using the same
  `cuda_download.py` path as Windows.
- Text injection on Linux uses `enigo` (X11/Wayland). If the ydotool fallback
  is needed for a specific Wayland compositor:
  ```sh
  sudo usermod -aG input $USER  # log out and back in
  ```
- `tauri dev` is unaffected — it runs the sidecar from Python source.
