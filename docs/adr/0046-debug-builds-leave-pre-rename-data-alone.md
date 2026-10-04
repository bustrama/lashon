# 46. Debug builds leave the pre-rename data alone

## Status

Accepted — 2026-10-04. Narrows the carry-over of
[ADR-0042](0042-rename-the-product-to-ottid.md) to release builds, plus debug
builds that opt in. Release builds behave as before.

## Context

ADR-0042 carries an existing *Lashon* install over to *Ottid*. Three consumers
each move their own part of the old data the first time they need it:

| Consumer | What it moves |
|---|---|
| The Tauri shell (`adopt_legacy_app_dirs`, before the builder runs) | The `dev.lashon.desktop` dirs under each per-user base dir (settings, WebView storage, logs, models) |
| `ottid_core::keychain::read_key`, on a miss | API keys from the keychain service `lashon` to `ottid`. The old entry is then deleted. `store_key` and `delete_key` delete it too. |
| `ottid_core::mcp::recipe_tools::user_recipes_dir` | `<data-local>/lashon/recipes` to `<data-local>/ottid/recipes` |

All three ran in every build. A developer's machine usually has a real install
as well, so `npm run tauri dev` moved the developer's real data. On 2026-10-04
a dev run moved three of the owner's API keys (`llm.anthropic`, `llm.openai`,
`llm.opencode-go`) to the new service and deleted the originals.

There was no safe way to run a dev build on such a machine:

- A `--config` identifier override doesn't help. The shell always reads from
  the legacy identifier, so the data still moves.
- Skipping only the dir move still lets the keychain and the recipes resolver
  move their part.

## Decision

- **One switch.** `ottid_core::legacy::carry_over_enabled()` decides for every
  consumer. Each one asks it before it reads, moves or deletes pre-rename data.
- **Release builds: always on.** Users run release builds, so ADR-0042's
  carry-over is unchanged for them. A release build ignores the opt-in
  variable.
- **Debug builds: off, unless `OTTID_ADOPT_LEGACY=1`.** This covers
  `npm run tauri dev`, `cargo run` and `cargo test`.
  - Only the exact value `1` opts in. Anything else leaves the data alone,
    because a guard on someone's installed app should fail closed.
  - "Debug build" means `cfg!(debug_assertions)` as compiled into `ottid-core`.
    The release profile turns it off, and the workspace sets no profile
    overrides.
- **Read once per process.** The value is cached on first use, so the shell,
  the keychain and the recipes resolver always agree.
- **With the switch off:**
  - The shell skips the dir move. At startup it logs one line that says the
    carry-over is off and names the opt-in variable.
  - `read_key` treats a miss as "no key" and never reads the old service.
    `store_key` and `delete_key` leave the old entry too.
  - `user_recipes_dir` returns the new path without moving anything.
- **Tests.** The rule and each consumer's off path are unit-tested in
  `ottid-core`. The keychain cases need a real OS keychain, so they are
  `#[ignore]`d like the existing keychain tests. They pass the switch in
  explicitly, so they don't depend on the build kind.

## Alternatives considered

- **Gate only the dir move in the shell.** The keychain and the recipes would
  still move. That is the gap that cost the keys on 2026-10-04.
- **Opt out instead of opt in** (for example `OTTID_SKIP_LEGACY=1`). This fails
  open: forgetting the variable is exactly what moved the keys.
- **Give dev builds their own identifier and keychain service.** That would also
  keep dev runs away from the developer's real *Ottid* data. It is a wider
  change: every dev run would start without models or keys. It doesn't conflict
  with this gate and can still be added later.
- **A CLI flag or setting instead of an environment variable.** The shell would
  have to parse it before the builder runs, and `cargo test` couldn't pass it.
  An environment variable works the same way for all three ways of running a
  debug build.

## Consequences

- **Testing the carry-over itself needs a release build or the opt-in.** The
  installer smoke test over a real v1.1.0 install that ADR-0042 requires is a
  release build, so it is unaffected. A debug run with `OTTID_ADOPT_LEGACY=1`
  moves the developer's real data, as every dev run did before.
- **A dev run still creates the new `app.ottid.desktop` dirs.** When a release
  build later runs on the same machine, it merges the old dirs into the new
  ones instead of renaming them. Under the merge rule of `legacy::adopt_dir`,
  entries the dev run created (settings, WebView storage) win. For a clean
  carry-over on a machine with real *Lashon* data, delete the dev run's dirs
  first.
- **A key saved in a dev run lands under `ottid` only.** The old `lashon` entry
  of the same name stays. Nothing reads it while the new entry exists, and the
  next `store_key` or `delete_key` in a release build removes it.
- **Data that already moved stays moved.** That includes the three keys from
  2026-10-04. This decision only stops further moves.
- `CONTRIBUTING.md` documents the variable for developers.
