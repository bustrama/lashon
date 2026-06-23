---
description: Rust conventions — the lashon-core test boundary, builds, dependency pinning
globs: ["**/*.rs", "**/Cargo.toml"]
---

# Rust

## Tests live in lashon-core

- Testable logic and its `#[test]`s live in `lashon-core`
  (`packages/shared-rust/`). If you are writing logic worth testing, it belongs
  there.
- `apps/desktop/src-tauri/` is a thin shell with `test = false` — **never** add
  unit tests to the Tauri crate. Tauri-linked test binaries fail to load on
  Windows ([ADR-0003](../../docs/adr/0003-core-logic-in-a-tauri-independent-crate.md)).
- `cargo test --workspace` must stay green on all three runners.

## Building

- `cargo check` of the Tauri crate needs the SvelteKit `build/` output to
  exist — run `npm run build` in `apps/desktop/` first.

## Conventions

- Prefer Rust-native crates over shelling out. If you must shell out, leave a
  comment explaining why no crate fit.
- Pin every dependency exactly in `Cargo.toml` — no `^`, `~`, `*`. `Cargo.lock`
  is committed; a version bump is its own deliberate commit.
- `Result` and `?` for fallible paths; reserve panics for genuine invariant
  violations.
