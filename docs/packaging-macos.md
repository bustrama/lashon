# Packaging — macOS

How to build the Lashon macOS installer. See
[ADR-0018](adr/0018-cross-os-installer-matrix.md) for the design rationale,
and [releasing.md](releasing.md) for the full release process this fits into.

The release ships **one macOS artifact** — a `.dmg` disk image. Unlike Windows,
there is no portable distribution; `.dmg` is the macOS portable format.

**Signing status:** macOS releases are currently **unsigned and unnotarized**.
Gatekeeper will show a warning on first launch. The bypass is right-click →
Open. Signed, notarized releases are gated on the Apple Developer Program
purchase (M13). See [ADR-0018](adr/0018-cross-os-installer-matrix.md).

**STT device:** macOS runs STT on the CPU only. CUDA is an NVIDIA technology
unavailable on Apple Silicon. A future ADR will add `whisper.cpp` + Metal as
a macOS GPU path (explicitly out of scope for this release).

## Prerequisites

- Rust 1.95 (`rust-toolchain.toml`) and the `aarch64-apple-darwin` target
  (present by default on Apple Silicon Macs; install with
  `rustup target add aarch64-apple-darwin`).
- Node 20+, Python 3.11–3.12.
- Xcode command-line tools: `xcode-select --install`.
- The STT sidecar virtual environment, with the `build` extra:

  ```sh
  cd services/stt-sidecar
  python3 -m venv .venv
  .venv/bin/python -m pip install -e ".[build]"
  ```

  Note: the `cuda` extra is **not** installed on macOS. CUDA packages
  (`nvidia-cublas-cu12`, `nvidia-cudnn-cu12`) are NVIDIA/x86_64 only and will
  not install on Apple Silicon. The `PyInstaller.spec` detects their absence
  and skips CUDA collection automatically.

## 1. Freeze the STT sidecar

From `services/stt-sidecar`:

```sh
.venv/bin/python -m PyInstaller --noconfirm --clean PyInstaller.spec
```

Output: `dist/lashon-stt/` — a one-folder bundle (`lashon-stt` + `_internal/`).
The binary has no file extension on macOS (unlike `lashon-stt.exe` on Windows).

Copy it where the Tauri bundle expects it:

```sh
rm -rf ../../apps/desktop/src-tauri/binaries/lashon-stt
cp -r dist/lashon-stt ../../apps/desktop/src-tauri/binaries/lashon-stt
```

## 2. Build the DMG

From `apps/desktop`:

```sh
npm install
npm run tauri build -- --bundles dmg
```

Output: `target/release/bundle/dmg/Lashon_0.5.0_aarch64.dmg` (arm64 host) or
`Lashon_0.5.0_x64.dmg` (Intel host). The Cargo workspace places `target/` at
the repository root.

For a universal binary (arm64 + x86_64 in one `.dmg`), use:

```sh
rustup target add x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin --bundles dmg
```

A universal build requires both Rust targets installed and takes roughly
twice the compile time. The release CI builds arm64 only (macOS 14 runner =
Apple Silicon); cross-compilation to x86_64 is left for future work.

## 3. Signing and notarization (M13)

v0.5.x ships unsigned. When an Apple Developer ID is obtained (M13):

1. Add `APPLE_SIGNING_IDENTITY` and `APPLE_CERTIFICATE` (base64 `.p12`) as
   GitHub Actions secrets.
2. Add `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password), and
   `APPLE_TEAM_ID` for `xcrun notarytool submit`.
3. Pass them to `tauri-action` via `env:` in the release workflow; Tauri's
   macOS bundler calls `codesign` and `xcrun notarytool` automatically when
   these variables are present.
4. Update `entitlements.plist` in `apps/desktop/src-tauri/` as needed for
   any additional capabilities.

## Notes

- The frozen sidecar under `binaries/` is a build artifact — git-ignored,
  never committed. A fresh checkout cannot `tauri build` until step 1 has
  produced it.
- The Hebrew STT model is **not** bundled; the app downloads it on first run
  into `~/Library/Application Support/dev.lashon.desktop/models/`.
- The **MIT-licensed "Hey Lashon" wake classifier** (`hey_lashon.onnx`) is
  listed in `tauri.conf.json`'s `bundle.resources` and ships with the DMG.
  On first launch the Tauri shell stages it into the per-user models directory
  (see `stage_bundled_wake_classifiers` in `apps/desktop/src-tauri/src/lib.rs`).
- CUDA is never downloaded on macOS — `nvidia-smi` is absent on Apple Silicon
  and the sidecar's CUDA probe skips the runtime download automatically.
- `tauri dev` is unaffected — it runs the sidecar from Python source.
- **Future GPU path:** `whisper.cpp` compiled with Metal (Apple GPU) is the
  planned GPU-accelerated STT option for macOS. It will arrive as a new
  `SttProvider` trait implementation in a future ADR, without changing the
  current CPU sidecar.
