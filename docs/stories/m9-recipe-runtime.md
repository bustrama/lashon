# M9 Phase 1b — Recipe runtime executor

> **Status: shipped on `main` in PR #75.** Follows Phase 1a (PR #72 — schema + parser +
> validator + 10 starters). Phase 1c (intent cascade) and Phase 1d
> (Hub Recipes tab) layer on top of this runtime.

## What ships

The runtime that turns a validated `Recipe` into actual OS-UI work,
plus a small CLI driver so authored recipes can be tested today
without waiting for the Hub integration or voice-cascade work.

- **`lashon_core::recipes::runtime`** — the executor.
  - `execute_recipe(recipe, args, confirm)` — the public async entry
    point. Walks the host-OS step list, runs each step against
    Lashon's existing primitives, threads recipe-local variables
    through interpolation.
  - `ConfirmHandler` trait + `AlwaysAllow` / `AlwaysDeny` impls.
    Currently consulted only for `run_shell` (the one destructive
    step type in v1); the Tauri shell will plug its existing
    confirmation modal here in Phase 1d.
  - `RuntimeError` enum with the failure modes the dispatcher and
    Hub will surface: `NoStepsForOs`, `UnknownInterpolation`,
    `StepNotImplemented`, `StepFailed`, `Denied`, `Timeout`.
  - `interpolate(text, vars)` — `{{ key }}` substitution with
    step-local variable support.
- **Step backings.** Eleven of twelve `Step` variants execute against
  real OS calls; one is left as `StepNotImplemented` for v1.1:

  | Step | Backing |
  |---|---|
  | `KeyChord` | `enigo` synthetic keypress (factored out as `tools::press_keys::execute_chord`, shared with the LLM tool of the same name) |
  | `TypeUnicode` | `inject::inject_text` — Hebrew clipboard path baked in |
  | `FocusWindow` | `tools::focus_window::try_focus` |
  | `WaitForWindow` | poll-loop on `try_focus` until match or timeout |
  | `WaitMs` | `tokio::time::sleep` |
  | `ScreenshotToClipboard` | PowerShell shell-out — `System.Windows.Forms.Clipboard::SetImage` |
  | `ClipboardSet` / `ClipboardGetInto` | `arboard` |
  | `RunShell` | PowerShell on Windows / `/bin/sh` elsewhere — gated by `ConfirmHandler` |
  | `OpenUrl` | `open::that` |
  | `OpenApp` | `cmd /c start "" "<name>"` on Windows |
  | `ClickLabel` | `StepNotImplemented` — UIA wiring lands in v1.1 (`click_element.rs` has the primitive; the runtime adapter just needs the wire-up) |

- **`lashon-recipe` CLI binary** at
  `packages/shared-rust/src/bin/lashon_recipe.rs`. Lets the user run
  a recipe by id from the shell:

  ```sh
  lashon-recipe send-discord-message --recipient=kuki --body="hi"
  lashon-recipe lock-workstation
  lashon-recipe batch-rename-files --directory=C:\tmp \
      --pattern="*.txt" --find=old --replace=new --allow-shell
  lashon-recipe --list
  ```

  Per ADR-0028 §"What's not in Phase 1g v1" precedent: `run_shell`
  steps are **denied by default**; the user passes `--allow-shell` to
  opt in. Authored recipes can be tested immediately without Hub or
  voice integration; the same env vars
  (`LASHON_BUNDLED_RECIPES_DIR`, `LASHON_USER_RECIPES_DIR`) the MCP
  server uses point the CLI at the right starters + per-user dirs.

- **Tests.**
  - `recipes::runtime::tests` — 8 unit tests covering interpolation,
    OS dispatch, error surfaces, click-label-not-yet-implemented,
    denial path.
  - `tests/recipe_runtime.rs` — 6 integration tests with real
    `arboard` clipboard round-trip and real PowerShell / sh
    invocations: clipboard set→get round trip, `run_shell` with
    `AlwaysAllow` actually captures stdout, `AlwaysDeny` aborts
    without side effects, slot interpolation reaches the captured
    output, unknown slot aborts before any side effect,
    `wait_ms` actually sleeps.

## How the user tests today

1. Build: `cargo build -p lashon-core --bin lashon-recipe`.
2. List: `cargo run -p lashon-core --bin lashon-recipe -- --list`.
3. Run a safe one: `cargo run -p lashon-core --bin lashon-recipe -- screenshot-to-clipboard`.
4. Run a parameterised one (real desktop side effects):
   `cargo run -p lashon-core --bin lashon-recipe -- send-discord-message --recipient=kuki --body="hello from Lashon"`.

For recipes that include `run_shell` steps (`batch-rename-files`),
add `--allow-shell` after the slot args.

## What this PR does NOT do (deferred per the M9 story)

- **Phase 1c — intent cascade + dispatcher integration.** Wires the
  runtime into voice / Command-mode: regex → embedding → LLM
  classifier → LLM full planner with the recipe path
  short-circuiting on a match. The runtime itself is ready; the
  cascade is the next PR.
- **Phase 1d — Hub Recipes tab.** Browser + slot-fill modal + Run
  button. The Tauri shell calls into `execute_recipe` exactly as
  the CLI does; the modal becomes the `ConfirmHandler`.
- **MCP `run_recipe` tool.** Adding the runtime as an MCP-callable
  surface so Claude Desktop can author + run in one loop. Defers
  until PR #74 (MCP server) merges so the two pieces don't conflict.
  Will be a small follow-up: one new `#[tool]` method on
  `LashonMcpServer` calling `execute_recipe`, with the same
  `LASHON_MCP_ALLOW_SHELL` env-var-gated default as the CLI.
- **`ClickLabel` step.** Returns `StepNotImplemented` in v1; the
  UIA primitive exists in `tools::click_element` and the adapter is
  a small follow-up.
- **`ScreenshotToClipboard` region capture.** v1 captures the primary
  screen; the `region: Some(Region { ... })` field is honoured in a
  follow-up that adds the bounded-rectangle path.
- **Step-by-step confirmation.** v1 only gates on `run_shell`. A
  future PR could let recipes mark individual steps as
  `requires_confirmation: true` for sensitive non-shell actions.

## Test plan

- [x] `cargo test -p lashon-core --lib recipes` — 26 passed (18 from
  Phase 1a + 8 new in runtime module), no regressions
- [x] `cargo test -p lashon-core --test recipe_runtime` — 6 passed
  (real powershell.exe / arboard exercised)
- [x] `cargo test -p lashon-core --test recipe_starters` — 3 passed
  (still green — runtime additions don't touch the starters)
- [x] `cargo test -p lashon-core --test recipe_schema_snapshot` — 1
  passed
- [x] `cargo check --workspace --all-targets` clean
- [x] Manual: `lashon-recipe --list` enumerates all 10 starters
- [ ] CI green on all three runners
- [ ] Manual desktop smoke test: run `send-discord-message` against
  a real Discord install with a throwaway recipient

## Definition of done

- Runtime executes ten of twelve step types end-to-end on Windows;
  `ClickLabel` returns the documented `StepNotImplemented` error
- `lashon-recipe` CLI lists + runs recipes by id with slot args
- `ConfirmHandler` trait carries the denial path for `run_shell`
- Integration tests exercise the AlwaysAllow + AlwaysDeny paths
- Story doc committed (this file)
- CLAUDE.md branch-summary paragraph updated
