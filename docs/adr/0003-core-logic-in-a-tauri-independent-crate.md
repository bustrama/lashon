# 3. Testable core logic lives in a Tauri-independent crate

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Lashon contributors
- **Relation:** realises the `packages/shared-rust` crate

## Context

M0's Definition of Done requires `cargo test` to be green on the Windows,
macOS, and Linux CI runners. During M0 bring-up, `cargo test` for the Tauri
application crate failed on Windows: the unit-test harness executable
terminated at load time with `STATUS_ENTRYPOINT_NOT_FOUND` (`0xC0000139`)
before any test ran.

The real application binary (`lashon.exe`) builds and launches correctly on the
same machine — it was observed starting, logging, and opening the tongue
window. Only the *test-harness* executables — which link `tauri`, `wry`, and
the WebView2 stack — fail to load. Unit-testing a crate that links the
Tauri/WebView GUI stack is unreliable on Windows.

## Decision

Testable logic lives in a separate crate, `lashon-core`, at
`packages/shared-rust` — the location reserved for shared Rust code. `lashon-core` depends only on the networking/runtime stack
(`tonic`, `prost`, `tokio`) and never on `tauri`.

- `lashon-core` holds `sidecar` (STT sidecar lifecycle and gRPC client) and
  `stt_proto` (generated gRPC bindings), with all of their unit tests, plus the
  end-to-end `tests/healthcheck.rs` smoke test.
- `apps/desktop/src-tauri` is a thin GUI shell — `main.rs` and `lib.rs` only —
  that depends on `lashon-core`. Its `[lib]` and `[[bin]]` set `test = false`,
  so no Tauri-linked test binary is ever built or run.
- The two crates form a Cargo workspace rooted at the repository root.

## Consequences

- `cargo test --workspace` is green on all three runners: `lashon-core`'s test
  binary links no GUI stack and loads everywhere; the `lashon` crate builds no
  test binary.
- This is a deliberate deviation from the working-agreement rule "every Rust
  module under `apps/desktop/src-tauri/src/` has at least one `#[test]`." The
  rule's intent — no untested logic — still holds: `src-tauri/src/` now contains
  only GUI-wiring shims (`run()` and a one-line command delegate), and every
  piece of real logic lives in `lashon-core` and is tested there.
- New non-GUI logic (provider clients, the dictation FSM, the tool runner)
  belongs in `lashon-core`, not in the Tauri crate. This reinforces the
  provider-abstraction seam of [ADR-0001](0001-tauri-sveltekit-rust-stack.md)
  and `docs/architecture.md`.
- The end-to-end sidecar smoke test runs with
  `cargo test -p lashon-core --test healthcheck -- --ignored`.
