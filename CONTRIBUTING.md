# Contributing to Lashon

## External contributions

**Lashon is a solo-maintained project, and external pull requests are not
accepted — they will not be reviewed or merged.** This isn't about the quality
of any given change; it's that a single maintainer keeps the design, the Hebrew
behaviour, and the release process coherent. Please don't invest effort in a PR,
because it won't be merged, and I'd rather not waste your time.

**Bug reports and issues are genuinely welcome.** If something is broken,
behaves incorrectly, or mishandles Hebrew (or mixed Hebrew/English) text, please
[open an issue](https://github.com/bustrama/lashon/issues) with steps to
reproduce — that's the most useful thing you can contribute, and it's read and
appreciated. Lashon is GPL-3.0-only, so you are of course also free to fork the
project and adapt it for your own use under the terms of that license.

The rest of this document describes the **internal** development workflow used
to build Lashon; it is reference for the maintainer, not a contribution guide.

---

Lashon is built milestone by milestone. The spec is a set of focused, living
docs under [`docs/`](docs/) — [`architecture.md`](docs/architecture.md),
[`providers.md`](docs/providers.md), [`design-system.md`](docs/design-system.md)
and the rest — and the build plan is [`docs/roadmap.md`](docs/roadmap.md). Each
doc is authoritative for its own area and is kept current with the code.

## Branch model

- `main` — always green; every milestone merges here via pull request.
- One **feature branch per milestone**, named `mN-slug` (e.g. `m0-bootstrap`,
  `m1-stt-pipeline`).
- One **PR per milestone**. A milestone PR merges only when that milestone's
  Definition of Done is fully met and CI is green on all three runners.
- Short-lived fix branches off a milestone branch are fine; they fold back in
  before the milestone PR merges.

## Milestone Definition of Done

Each milestone (M0–M13) has an explicit DoD in
[`docs/roadmap.md`](docs/roadmap.md); a picked-up milestone is written up as a
story in [`docs/stories/`](docs/stories/). The DoD is the **merge criterion** —
not "looks done", not "passes locally". Before opening a milestone PR:

1. Every DoD bullet for the milestone is demonstrably satisfied.
2. CI is green on `windows-2022`, `macos-14`, and `ubuntu-24.04`.
3. The license-scan job passes — no AGPL or CC-BY-NC contamination. (Lashon is
   GPL-3.0-only as of [ADR-0032](docs/adr/0032-ship-as-open-core-product.md);
   dependencies stay GPLv3-compatible — prefer permissive.)
4. No secrets, no model weights, and no audio fixtures larger than 5 MB are in
   the diff (large fixtures use Git LFS).
5. Hebrew is exercised explicitly by tests at every layer the milestone touches.

## Architecture Decision Records

Any architectural decision, trade-off, or reversal is recorded as an ADR in
[`docs/adr/`](docs/adr/):

- Files are named `NNNN-slug.md`, numbered sequentially from `0001`.
- Keep them concise: context, the decision, alternatives considered,
  consequences.
- Write the ADR in the same PR that makes the decision.

## Commits

- Conventional, imperative mood: `<type>: <subject>`.
- Types: `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `ci`.
- Subject ≤ 72 characters. One concern per commit.
- Example: `feat: add Silero VAD endpointing to audio capture`.

## Versions

- **No floating versions.** Every dependency is pinned exactly — no `^`, `~`,
  or `*` in `Cargo.toml`, `package.json`, or `pyproject.toml`. Lockfiles
  (`Cargo.lock`, `package-lock.json`) are committed.
- Toolchains are pinned too (`rust-toolchain.toml`, `.nvmrc`).
- Bumping a version is a deliberate change with its own commit.

## Code conventions

- **Hebrew is a first-class concern.** Never test English-only. Every layer
  that handles text is tested with Hebrew, including mixed Hebrew/English.
- **Local-first.** Cloud providers are opt-in adapters behind the provider
  traits (see [`docs/architecture.md`](docs/architecture.md)). No code path
  defaults to cloud.
- **Privacy.** No transcripts, audio, or PII leave the machine without explicit
  opt-in. No telemetry by default. API keys live only in the OS keychain.
- Testable Rust logic lives in `packages/shared-rust` (the `lashon-core`
  crate); every module there carries `#[test]`s. `apps/desktop/src-tauri` is a
  thin Tauri shell and is not unit-tested — see
  [ADR-0003](docs/adr/0003-core-logic-in-a-tauri-independent-crate.md).
- Prefer Rust-native APIs over shelling out; document any shell-out with a
  comment explaining why a crate did not fit.

## Local development

| Task | Command |
|---|---|
| Run the desktop app | `cd apps/desktop && npm run tauri dev` |
| Type-check the frontend | `cd apps/desktop && npm run check` |
| Build the frontend | `cd apps/desktop && npm run build` |
| Check Rust | `cd apps/desktop/src-tauri && cargo check` |
| Test Rust | `cd apps/desktop/src-tauri && cargo test` |
| Run the STT sidecar standalone | `cd services/stt-sidecar && python -m lashon_stt.server` |
| List bundled M9 recipes | `cargo run -p lashon-core --bin lashon-recipe -- --list` |
| Run a recipe by id (smoke-test authoring) | `cargo run -p lashon-core --bin lashon-recipe -- <recipe-id> --param=value` |
| Run the Lashon-as-MCP stdio server | `cargo run -p lashon-core --bin lashon-mcp` |

See [`docs/architecture.md`](docs/architecture.md) for how the pieces fit
together.
