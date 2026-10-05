# Releasing Ottid

The end-to-end runbook for cutting a release. `v0.1.0` was the first; the
packaging design behind it is [ADR-0006](adr/0006-release-packaging-and-signing.md).

## What a release is

Two Windows artifacts published on
[GitHub Releases](https://github.com/bustrama/ottid/releases): an NSIS
installer (`Ottid-X.Y.Z-windows-x64-setup.exe`, ~66 MB) and a portable zip
(`Ottid-X.Y.Z-windows-x64-portable.zip`) —
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
to `apps/desktop/src-tauri/binaries/ottid-stt/`, then `npm run tauri build`.

Rename the installer for the release asset:

```sh
cd target/release/bundle/nsis
mv Ottid_X.Y.Z_x64-setup.exe Ottid-X.Y.Z-windows-x64-setup.exe
```

Then package the portable zip — `packaging-windows.md` §3. It is assembled from
the frozen sidecar bundle, never the staged `target/release/binaries/` tree, so
runtime-downloaded CUDA cannot leak into the artifact.

## 3. Verify the artifacts

Install it per-user and check it:

```sh
Ottid-X.Y.Z-windows-x64-setup.exe /S /CurrentUser
```

The installer uses `installMode: "both"`, so NSIS ignores `/D=`. A per-user
install goes to `%LOCALAPPDATA%\Programs\Ottid`, unless an earlier install is
found, in which case it reuses that folder.

- `ottid.exe` and `binaries\ottid-stt\ottid-stt.exe` exist there.
- `binaries\ottid-stt\_internal\nvidia` does **not** exist — CUDA is fetched at
  runtime, never bundled.
- Launch it: the tongue appears, shows the dim "preparing" pulse while it
  downloads the model on first run, then settles to idle.
- Speak a Hebrew passage and an English one — both should paste correctly.

Then extract `Ottid-X.Y.Z-windows-x64-portable.zip` to a fresh folder and
launch `ottid.exe` from it — the tongue should behave identically, with no
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
  --title "Ottid vX.Y.Z — <summary>" \
  --notes-file <notes.md> \
  "target/release/bundle/nsis/Ottid-X.Y.Z-windows-x64-setup.exe" \
  "target/release/Ottid-X.Y.Z-windows-x64-portable.zip"
```

The release notes should tell users: download and run, that a new release can
still meet a SmartScreen notice for its first days while its reputation builds,
and that the first run downloads the model (and the CUDA runtime on NVIDIA
machines).

A locally built installer is unsigned. Signed builds come only from the release
workflow (see **Signing** below).

## Signing

A release tag (`v*`) runs `.github/workflows/release.yml`, which signs twice,
independently ([ADR-0043](adr/0043-sign-windows-releases-with-azure-artifact-signing.md)):

- **Authenticode, with Azure Artifact Signing.** It signs every PE image in the
  build: the app exe, the installer and uninstaller, the NSIS plugins, and every
  unsigned exe/dll/pyd of the frozen sidecar (and of `llama-server` in the full
  edition). The workflow logs in to Azure over GitHub OIDC, so there is no
  signing secret. The tooling is `scripts/sign-windows.ps1`, which Tauri calls
  through `apps/desktop/src-tauri/tauri.signing.conf.json`.
- **The updater's minisign signature.** It covers the finished installer and
  `latest.json`, so in-app auto-update can verify them
  ([ADR-0017](adr/0017-auto-update-via-tauri-plugin-updater.md)). The public key
  is committed in `tauri.conf.json` (`plugins.updater.pubkey`). The private key
  is never committed.

Both live in the GitHub environment **`release`**. Its deployment rules admit
tags `v*` and `main`.

| Kind | Name |
|---|---|
| secret | `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`, `AZURE_SUBSCRIPTION_ID` |
| variable | `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE` |
| secret | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` |

ADR-0043 lists the one-time Azure setup behind these values.

**A run fails before the build starts when any of them is missing.** The one
exception is an explicitly unsigned pre-release:

- a tag containing `-unsigned` (e.g. `v1.2.0-unsigned.1`), or
- a manual run with **allow_unsigned** ticked.

Such a build skips both signatures and is released as a GitHub pre-release,
which keeps it out of `/releases/latest`, the updater's feed. Its release notes
say it is unsigned.

**Every run ends in a draft release:**

1. A signed run verifies itself first. It silently installs the installer and
   checks that every PE image installed, including the uninstaller, carries a
   valid signature.
2. Install the draft's installer for real (fresh, and over the previous
   release), check the publisher in the UAC prompt, and run the smoke test in
   §3.
3. Only then publish the draft. **Never publish a draft whose run is red.**

## Notes

- The frozen sidecar under `binaries/`, and everything under `target/`, are
  build artifacts — git-ignored, never committed.
- Only the small installer is uploaded to GitHub; the model and CUDA runtime
  are fetched from Hugging Face and PyPI at first run.
- The release workflow builds the **free** edition: `--no-default-features`,
  `VITE_OTTID_EDITION=free`, and `tauri.free.conf.json` merged over
  `tauri.conf.json`, which leaves out `llama-server`, `ottid-mcp` and the
  starter recipes. The full edition has no release job yet;
  [ADR-0049](adr/0049-bundle-ottid-mcp-and-the-starters-in-the-full-edition.md)
  says what one needs.
- `tauri dev` is unaffected by any of this — it runs the sidecar from Python
  source.
