# Releasing Lashon

The end-to-end runbook for cutting a release. `v0.1.0` was the first; the
packaging design behind it is [ADR-0006](adr/0006-release-packaging-and-signing.md).

## What a release is

Two Windows artifacts published on
[GitHub Releases](https://github.com/bustrama/lashon/releases): an NSIS
installer (`Lashon-X.Y.Z-windows-x64-setup.exe`, ~66 MB) and a portable zip
(`Lashon-X.Y.Z-windows-x64-portable.zip`) —
[ADR-0012](adr/0012-portable-distribution-and-all-users-install.md). The STT
model (~1.6 GB) and, on NVIDIA machines, the CUDA runtime (~1.2 GB) are
**downloaded on first run** — never bundled, never uploaded to GitHub.

## Prerequisites

- The build toolchains and the STT sidecar virtual environment with the `build`
  extra — see [packaging-windows.md](packaging-windows.md).
- `gh` (the GitHub CLI), authenticated.

## 1. Branch and bump the version

Branch off `main` (`mN-slug` for a milestone, otherwise `release-X.Y.Z`), then
set the new `X.Y.Z` in every version field:

- `apps/desktop/package.json` and `apps/desktop/package-lock.json` (two places)
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/tauri.conf.json`
- `packages/shared-rust/Cargo.toml`
- `services/stt-sidecar/pyproject.toml`

Run `cargo check --workspace` once to refresh `Cargo.lock`, and refresh the
`## Current milestone` section of `CLAUDE.md`.

## 2. Freeze the sidecar, build the installer and the portable zip

Follow [packaging-windows.md](packaging-windows.md): freeze the sidecar, copy it
to `apps/desktop/src-tauri/binaries/lashon-stt/`, then `npm run tauri build`.

Rename the installer for the release asset:

```sh
cd target/release/bundle/nsis
mv Lashon_X.Y.Z_x64-setup.exe Lashon-X.Y.Z-windows-x64-setup.exe
```

Then package the portable zip — `packaging-windows.md` §3. It is assembled from
the frozen sidecar bundle, never the staged `target/release/binaries/` tree, so
runtime-downloaded CUDA cannot leak into the artifact.

## 3. Verify the artifacts

Install to a scratch directory and check it:

```sh
Lashon-X.Y.Z-windows-x64-setup.exe /S /D=C:\lashon-check
```

- `C:\lashon-check\lashon.exe` and `binaries\lashon-stt\lashon-stt.exe` exist.
- `binaries\lashon-stt\_internal\nvidia` does **not** exist — CUDA is fetched at
  runtime, never bundled.
- Launch it: the tongue appears, shows the dim "preparing" pulse while it
  downloads the model on first run, then settles to idle.
- Speak a Hebrew passage and an English one — both should paste correctly.

Then extract `Lashon-X.Y.Z-windows-x64-portable.zip` to a fresh folder and
launch `lashon.exe` from it — the tongue should behave identically, with no
install step.

## 4. Commit, push, open the PR

Conventional commits, one concern each. Push the branch and open the PR against
`main`. Wait for CI to pass on all three runners plus the license scan.

## 5. Merge and publish

```sh
gh pr merge <PR-number> --merge

gh release create vX.Y.Z \
  --target main \
  --prerelease \
  --title "Lashon vX.Y.Z — <summary>" \
  --notes-file <notes.md> \
  "target/release/bundle/nsis/Lashon-X.Y.Z-windows-x64-setup.exe" \
  "target/release/Lashon-X.Y.Z-windows-x64-portable.zip"
```

The release notes should tell users: download and run, the SmartScreen
"Run anyway" step while the build is unsigned, and that the first run downloads
the model (and the CUDA runtime on NVIDIA machines).

## Auto-update signing (tauri-plugin-updater)

The release workflow signs the installer and
`latest.json` manifest with a minisign keypair so in-app auto-update can
verify authenticity. The public key is committed in `tauri.conf.json`
(`plugins.updater.pubkey`). The private key lives **only** in the developer's
`~/.tauri/lashon.key` and in GitHub Actions secrets — never committed.

Before pushing the **first signed release tag**, run once:

```sh
# Store the private key (generated with `npm run tauri signer generate`):
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/lashon.key

# Set the password (empty string if the key has no password):
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

See [ADR-0017](adr/0017-auto-update-via-tauri-plugin-updater.md) for key
rotation instructions and security notes.

## Code signing

`v0.1.x`–`v0.5.x` ship **unsigned** — Windows SmartScreen warns on first run
(ADR-0006). Once a code-signing certificate is in hand (Certum Open Source or
Azure Trusted Signing), wire it into `tauri.conf.json` under `bundle.windows`
and drop `--prerelease` for the first signed, stable release. Note: the
minisign key (above) is **independent** of the Windows EV certificate — both
are needed for a fully hardened release, but either can be set up without the
other.

## Notes

- The frozen sidecar under `binaries/`, and everything under `target/`, are
  build artifacts — git-ignored, never committed.
- Only the small installer is uploaded to GitHub; the model and CUDA runtime
  are fetched from Hugging Face and PyPI at first run.
- `tauri dev` is unaffected by any of this — it runs the sidecar from Python
  source.
