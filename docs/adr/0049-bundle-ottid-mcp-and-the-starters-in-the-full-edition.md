# 49. Bundle ottid-mcp and the starter recipes in the full edition

## Status

Accepted — 2026-10-05. Wires up the bundling that
[ADR-0028](0028-lashon-as-mcp-server.md) described and
[ADR-0034](0034-command-mode-editioning.md)'s two editions were meant to
carry. It also makes `tauri.free.conf.json` apply, which no build did.

## Context

ADR-0028 says the Tauri shell bundles `ottid-mcp` as a resource, and a comment
in `packages/shared-rust/Cargo.toml` says the installer ships a single
`ottid-mcp.exe`. Neither was ever wired up:

- Nothing built `ottid-mcp` for an installer, and `bundle.resources` in
  `tauri.conf.json` listed neither it nor the starter recipes. There is no
  `externalBin` either.
- The runtime reads the starters through `recipe_tools::bundled_recipes_dir()`.
  Its order was `$OTTID_BUNDLED_RECIPES_DIR`, then a path baked in at compile
  time (`CARGO_MANIFEST_DIR/../../recipes/starters`). A doc comment said the
  Tauri shell sets the variable when it spawns `ottid-mcp`, but the shell never
  spawns it (the agent host does) and never set the variable. On a user's
  machine the baked-in path does not exist, so a packaged full build would show
  no starters in the Hub and match no starter intent. `ottid-mcp` had the same
  problem; the website's MCP guide tells source builders to set the variable by
  hand.
- `tauri.free.conf.json` exists to keep the command-mode resources out of the
  free installer, but no build passed it. `release.yml` built the free edition
  from `tauri.conf.json` alone, so the free installer carried the
  `binaries/llama-server/` folder (its README and `.gitkeep`, as nothing stages
  llama-server). The file also set `createUpdaterArtifacts: false`, so applying
  it to a release would have dropped the updater feed.

Only the free edition has ever been released. The full edition has no release
job: it is built from source today, and its paid, signed build is still to come
([ADR-0032](0032-ship-as-open-core-product.md)).

## Decision

### Where things install

Both ship as ordinary `bundle.resources`, the way `ottid-stt` and
`llama-server` do:

| Resource | Source entry | Installed at, relative to the resource dir |
|---|---|---|
| `ottid-mcp` | `binaries/ottid-mcp/**/*`, staged | `binaries/ottid-mcp/ottid-mcp.exe` |
| Starter recipes | `../../../recipes/starters/**/*` | `_up_/_up_/_up_/recipes/starters/<id>/recipe.yaml` |

On Windows the resource dir is the install folder, the one holding
`ottid.exe`. So `ottid-mcp` sits at
`%LOCALAPPDATA%\Programs\Ottid\binaries\ottid-mcp\ottid-mcp.exe` for a
per-user install and `%ProgramFiles%\Ottid\binaries\ottid-mcp\ottid-mcp.exe`
for an all-users one. That is the documented path for an agent host's config.
`_up_` is what Tauri writes for each `..` of a resource path; the bundled
models already land under it.

### Staging

`scripts/stage-ottid-mcp.sh` builds the binary
(`cargo build --release -p ottid-core --bin ottid-mcp --features mcp-server`)
and copies it into `apps/desktop/src-tauri/binaries/ottid-mcp/`, replacing
whatever was staged. The folder is git-ignored apart from a tracked
`.gitkeep`, like the sidecar folders: Tauri fails a glob that matches nothing,
and CI checks the Tauri crate on a clean checkout.

A full build runs the script before `tauri build`, and, when signing, before
`scripts/sign-windows.ps1 -Tree`, so `ottid-mcp.exe` is signed with the other
staged resources. The signer needs no change.

### Finding the starters at run time

- **The app.** `configure_recipes_env`, next to `configure_sidecar_env` in the
  Tauri shell, sets `OTTID_BUNDLED_RECIPES_DIR` to the starters inside the
  resource dir Tauri resolves. It does so in release builds only: a debug
  build keeps reading the live `recipes/starters` of the checkout, because
  tauri-build copies the starters into `target/debug`, where they go stale. A
  value already set wins.
- **`ottid-mcp`.** It runs as the agent host's child, so no app sets anything
  for it. It resolves the directory as: the env var; then the starters beside
  the binary, when its own path ends in `binaries/ottid-mcp/ottid-mcp[.exe]`
  (the resource dir is three levels up, matched case-insensitively); then the
  compile-time checkout path. The middle step fires only for that exact
  layout, so a binary built into `target/` is never mistaken for an installed
  one.
- The compile-time path stays as the last resort, for `cargo run` and the
  tests. A shipped build never relies on it.

The logic lives in `ottid_core::mcp::recipe_tools` and is unit-tested there;
the shell only calls it.

### Editions

`tauri.conf.json` is the full configuration, as before: it already lists
`llama-server`. `tauri.free.conf.json` lists the free resources in full,
because Tauri merges configs as JSON Merge Patch (RFC 7396), which replaces an
array rather than adding to it. It leaves out all three full-only resources:
`llama-server`, `ottid-mcp` and the starters.

- `packages/shared-rust/tests/bundle_layout.rs` asserts that the free list is
  the full list minus exactly those three, and that the install paths the
  runtime looks under match the entries in `tauri.conf.json`.
- `release.yml` now applies the free config for its free matrix entry
  (`tauri_config`). `createUpdaterArtifacts: false` is gone from that file, so
  merging it leaves the updater feed alone; an unsigned pre-release already
  skips updater artifacts through `--no-sign`.

### Left for later

- **A full-edition release job.** It needs the llama-server mirror (manual
  today, [ADR-0025](0025-in-process-local-llm.md)), the signing and
  pricing decisions of ADR-0032 and ADR-0043, and a matrix entry with no
  `tauri_config`. The order is: stage the sidecar, stage `ottid-mcp`, sign the
  staged tree, `tauri build`.
- **The portable zip** is assembled from `ottid.exe` and `binaries/ottid-stt`
  only. A full-edition zip would also need `binaries/ottid-mcp` and the starters
  under `_up_/`.
- **The Hub's MCP tab** (an ADR-0028 follow-up) can show the binary's path from
  `resource_dir()`.

## Consequences

- A packaged full build finds its starters. An agent host can run `ottid-mcp`
  from a fixed path with no environment variable. The website guide's
  `OTTID_BUNDLED_RECIPES_DIR` is then only for builds from source.
- The free installer drops the empty `llama-server` folder and bundles neither
  `ottid-mcp` nor recipes. Everything else in it is as before.
- The full installer grows by the `ottid-mcp` binary (ADR-0028 estimated about
  5 MB stripped) and the starters, a few dozen small files.
- Ottid gains no subprocess spawn: the agent host starts `ottid-mcp`, as
  ADR-0028 says, so the `CREATE_NO_WINDOW` rule has nothing new to cover.
- The starters and `ottid-mcp` are trusted as far as the install folder is:
  whoever can write there can already replace `ottid.exe`. The env override
  existed before.
- An installer built with `tauri build --debug` has debug assertions on, so
  the app would look for the starters in the checkout. Release builds are what
  ship.
- `BUNDLED_RECIPES_RESOURCE_DIR` and `MCP_RESOURCE_DIR` in `recipe_tools` have
  to match the resource entries. `bundle_layout.rs` fails when they drift.

## Alternatives considered

- **`externalBin`** would install `ottid-mcp.exe` beside `ottid.exe`, a shorter
  path. But it needs a file named with the target triple at the build of
  *every* Tauri crate: tauri-build fails when it is missing, so CI's
  `cargo check` and every `tauri dev` would need a staged binary, and the free
  config would have to cancel it. Folders under `binaries/` with a `.gitkeep`
  are the existing pattern.
- **Copy the starters into `binaries/`** with the staging script. That avoids
  the `_up_` path, but copies committed files, and a free build would still
  ship the empty folder. A resource list that leaves them out is the only real
  exclusion.
- **`beforeBundleCommand`** runs after the signing step, so the binary would be
  unsigned, and it would also run for a free build.
- **A new `tauri.full.conf.json`** would restate the whole array a third time.
  `tauri.conf.json` is already the full configuration.
- **A `OnceLock` in core instead of the env var.** The variable already exists
  as the override, and `configure_sidecar_env` sets its variables the same way.
- **Resolving the app's starters from the executable's path in core.** The
  resource dir differs on macOS and Linux, and Tauri's resolver is
  authoritative. `ottid-mcp` has no Tauri, so it is the one case that does.
