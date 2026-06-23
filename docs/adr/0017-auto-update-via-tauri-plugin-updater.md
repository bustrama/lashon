# 17. Auto-update via tauri-plugin-updater

- **Status:** Accepted
- **Date:** 2026-05-20
- **Deciders:** Lashon contributors
- **Context source:** `docs/roadmap.md` Phase 4 / M13; milestone auto-update
  (partial M13 slice). Implements the auto-update item from
  [ADR-0006](0006-release-packaging-and-signing.md).

## Context

`docs/roadmap.md` Phase 4 lists `tauri-plugin-updater` with a signed manifest
on GitHub Releases as the auto-update mechanism. The project is unsigned today
(`v0.1.0`–`v0.5.0` are SmartScreen-warned Windows pre-releases;
[ADR-0006](0006-release-packaging-and-signing.md)). `tauri-plugin-updater`'s
update verification is based on **minisign** — a separate keypair from the
Windows EV code-signing certificate (M13). These two signing concerns are
deliberately decoupled:

- The **minisign keypair** (this ADR) authenticates the update manifest and
  installer. It can be set up now, before EV signing is purchased.
- The **Windows EV / Apple Developer ID** (M13) suppresses OS SmartScreen /
  Gatekeeper warnings. It is a money-and-process concern, independent of the
  update mechanism.

The first version that can receive an in-app update is `v0.6.0` — the floor
established by this change. Earlier installs have no updater; users on `v0.5.0`
and below must download manually.

## Decision

### Crate and package

- Rust crate: `tauri-plugin-updater = "=2.10.1"` (pinned exactly, compatible
  with `tauri = "=2.11.2"`).
- JS package: `@tauri-apps/plugin-updater = "2.10.1"` (pinned exactly).

### Configuration

`tauri.conf.json`:
```json
"bundle": {
  "createUpdaterArtifacts": true
},
"plugins": {
  "updater": {
    "pubkey": "<base64-encoded minisign public key>",
    "endpoints": [
      "https://github.com/bustrama/lashon/releases/latest/download/latest.json"
    ],
    "dialog": false
  }
}
```

`dialog: false` disables the native OS update dialog. The Settings Hub About
section drives the flow: a bilingual "בדיקת עדכונים · Check for updates"
button invokes the `check_for_updates` Tauri command, which emits
`updater:progress` events; the Hub renders status inline. On a successful
install the button becomes "Restart to finish update", which calls the
existing `restart_app` command — the user controls when to restart.

### The Rust command

`check_for_updates` (in `apps/desktop/src-tauri/src/lib.rs`):

1. Calls `app.updater()?.check().await?` — the plugin fetches `latest.json`
   from GitHub Releases and verifies the minisign signature against the public
   key in `tauri.conf.json`.
2. On a hit, calls `update.download_and_install(on_chunk, on_install)`.
3. Emits `updater:progress` events with `{status, version, downloaded, total,
   percent}` so the Hub can show inline progress without polling.
4. Returns `"installed"` or `"up-to-date"` as the string result; errors
   surface as `Err(String)` which the Hub maps to an error label.
5. Leaves the relaunch decision to the user; the Hub calls `restart_app` on
   explicit user action.

The command lives in the Tauri crate (thin GUI shell), not `lashon-core`,
because it directly uses `tauri::AppHandle` and the plugin's `UpdaterExt`
trait. There is no testable pure logic to extract (the update check/download
path is integration-only).

### The release workflow

`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are
read from GitHub Actions secrets. `tauri-apps/tauri-action` signs the
installer and generates `latest.json` when these variables are present.
Both secrets must be set before the first `v0.6.0` tag push (see below).

The macOS and Linux per-OS PyInstaller-frozen sidecars are item (3) of the
v0.6.0 backlog (tracked separately). Until that work lands, the non-Windows
build jobs in `release.yml` are present but will fail at the PyInstaller
step on real tag pushes. The Windows job is the critical path for the first
auto-updatable release.

### The keypair

Generated with:
```sh
npx @tauri-apps/cli signer generate --write-keys ~/.tauri/lashon.key --password "" --ci
```

The **public key** is committed in `tauri.conf.json`. It is a base64-encoded
minisign public key (`minisign verify` can read it). **It is safe to commit.**

The **private key** (`~/.tauri/lashon.key`) is **never committed**. It must
be uploaded to GitHub Actions secrets once before cutting `v0.6.0`:

```sh
# Store the private key content:
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/lashon.key

# Set the password (empty string if the key has no password):
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

### Key rotation

If the private key is lost or compromised:

1. Generate a new keypair with the same `signer generate` command.
2. Replace the `pubkey` in `tauri.conf.json` with the new public key.
3. Upload the new private key to GitHub Secrets (replacing the old one).
4. Cut a new release from the updated config. **Clients running any version
   that embeds the old public key cannot verify updates signed with the new
   private key.** Those users will see update-check failures until they
   re-download the app manually. Document the rotation in the release notes.
   In practice, because minisign keys are local secrets (not EV certificates
   with revocation infrastructure), key rotation is rare; treat it as a
   recovery procedure, not routine maintenance.

## Consequences

- `v0.6.0`+ can receive in-app updates. Installs on `v0.5.0` and below
  cannot — those users must download manually.
- The minisign public key is committed and immutable (for the life of this
  keypair). Replacing it requires a coordinated release as described above.
- No vendor lock-in: the update endpoint is a GitHub Releases URL that
  anyone can mirror or replace (the public key governs trust, not the host).
- `dialog: false` means Lashon never shows a system-modal update prompt. The
  flow is purely Hub-driven, consistent with Lashon's chromeless, RTL-native
  UX philosophy.
- Security: the minisign signature prevents a MITM attack on the update
  manifest or installer. GitHub Releases is a trusted distribution point for
  the project. The loopback / local-process isolation between the Tauri shell
  and the STT sidecar is unaffected.
- Licensing: `tauri-plugin-updater` is Apache-2.0 OR MIT. The `minisign-verify`
  crate it pulls in is MIT. Both pass the CI license scan (`cargo deny check
  licenses`).
