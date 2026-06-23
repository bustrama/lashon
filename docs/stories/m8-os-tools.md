# OS-control tools tranche (M8 Phase 2)

Milestone **M8**, Phase 2. Branch `m8-os-tools` (off `main`, opened
after the `m8-command-mode-resilience` PR lands).

> **Status: planned.** All design calls are settled — see "Decisions
> already made" below. Implementation is mostly mechanical: 22 new
> tools, each a copy-and-modify of the existing `LashonTool` shape,
> plus a small extension to the confirmation modal so it renders
> destructive commands legibly.

## Why

Phase 1 (`docs/stories/m8-command-mode.md`) shipped 12 safe tools and
the `m8-command-mode-resilience` PR added a 13th (`read_active_window_text`).
That covers app launch, window focus, basic keyboard/mouse-by-label,
clipboard, web search — enough to drive Spotify or WhatsApp through
a happy-path chain. It does **not** cover:

- The right-click / scroll / drag affordances every desktop app uses.
- Reading what's on screen beyond the foreground window.
- File operations of any kind.
- Shell escape hatch.
- Process control, window management, system controls.
- Reading the browser's URL bar.

The user's framing is "full control over the OS." Phase 2 closes the
gap with one comprehensive tool tranche so the model can answer
"delete the screenshot in my Downloads folder," "close every Slack
channel I'm not in," "run `npm install` in this repo" — actions that
today the LLM has no tool for and either refuses or hallucinates.

## Scope (the 22 tools)

| # | Tool | Args (shape) | Destructive? | Notes |
|---|---|---|---|---|
| 1 | `right_click` | `{ text: string }` | No | UIA-resolve label → `enigo` right click at center of element |
| 2 | `double_click` | `{ text: string }` | No | Same as `right_click`, two `left_click` events ~50ms apart |
| 3 | `scroll` | `{ direction: "up"\|"down"\|"left"\|"right", amount?: number, target?: string }` | No | `amount` defaults to 3 clicks; `target` (UIA label) focuses the scroll over a specific region |
| 4 | `drag` | `{ from: string, to: string }` | No | UIA-resolve both labels, `enigo` mouse-down at `from`, move to `to`, mouse-up |
| 5 | `read_screen` | `{}` | No | Walks every top-level window via `EnumWindows`, returns `Window N: <title>\n  - label\n  - label\n...` per window. Capped at ~4 KB. |
| 6 | `list_open_windows` | `{}` | No | One line per top-level window: `<title> (process)`. Cheaper than `read_screen` when the model only needs titles. |
| 7 | `minimize_window` | `{ title?: string }` | No | Foreground when `title` omitted; otherwise first window whose title contains `title`. |
| 8 | `maximize_window` | `{ title?: string }` | No | Same target rules as `minimize_window`. |
| 9 | `close_window` | `{ title?: string }` | **Yes** | `WM_CLOSE` send. Foreground when `title` omitted. |
| 10 | `file_read` | `{ path: string }` | No | UTF-8 only. ≤32 KB returned; longer → tail + `(truncated, N bytes more)` marker. |
| 11 | `file_write` | `{ path: string, content: string }` | **Yes** | Creates parent dirs. Atomic via `tmp + rename`. |
| 12 | `file_delete` | `{ path: string }` | **Yes** | Files only (refuses dirs in this PR). |
| 13 | `file_move` | `{ from: string, to: string }` | **Yes** | Rename within same volume; copy+delete across volumes. |
| 14 | `list_files` | `{ path: string, pattern?: string }` | No | ≤200 entries. `pattern` is a glob like `*.gguf`. |
| 15 | `run_command` | `{ command: string, cwd?: string, timeout_ms?: number }` | **Yes** | PowerShell on Windows, `/bin/sh -c` elsewhere. 30 s default / 5 min cap. Output ≤4 KB. |
| 16 | `list_processes` | `{}` | No | Top 50 by CPU. `<pid> <name> <cpu%> <ram_mb>` per line. Via `sysinfo` (existing dep). |
| 17 | `kill_process` | `{ pid: number }` | **Yes** | SIGKILL (Windows: `TerminateProcess`). |
| 18 | `set_volume` | `{ percent: number }` | No | 0–100. Win32 `IAudioEndpointVolume`. |
| 19 | `show_notification` | `{ title: string, body: string }` | No | Tauri-side via `tauri-plugin-notification` (already in the manifest). |
| 20 | `lock_screen` | `{}` | **Yes** | Win32 `LockWorkStation`. Recoverable but disruptive — confirm. |
| 21 | `read_browser_url` | `{}` | No | UIA address-bar walk in the foreground browser window. Chrome / Edge / Firefox all expose URL as an Edit element with predictable Name. |
| 22 | `new_browser_tab` | `{ url?: string }` | No | `open` crate already in deps; opens `url` (defaults to `about:blank`) in the default browser, which spawns or focuses-then-new-tab. |

**Eight require confirmation:** `close_window`, `file_write`,
`file_delete`, `file_move`, `run_command`, `kill_process`,
`lock_screen`, plus any new destructive tool a future PR adds. The
existing `EventBasedConfirm` modal handles the round-trip; this PR
mostly inherits that wiring.

## Decisions already made

1. **`run_command` shell**: PowerShell on Windows
   (`powershell.exe -NoProfile -Command <cmd>`), `/bin/sh -c <cmd>`
   on macOS/Linux. 30 s default timeout, 5 min cap. Output capped at
   4 KB (tail preserved; truncation marker appended). Stderr merged
   with stdout. Non-zero exit → `ToolResult::error` so the LLM gets
   the failure context.
2. **Confirmation batching**: one modal per destructive call. No
   batching. Rare-in-practice; the dispatcher's `requires_confirmation`
   gate already fires per call.
3. **`read_screen` scope**: every top-level window via `EnumWindows`,
   title + small visible-label sample per window, total capped at
   ~4 KB. Distinct from `read_active_window_text` (foreground only,
   different use case).
4. **File-path safety**: every `file_*` tool resolves its `path`
   argument with `std::fs::canonicalize` before any operation. The
   canonical path must live under the user's home directory OR under
   `$env:TEMP` / `/tmp`. Anything else → `ToolResult::error("path
   outside the allowed roots")`. This prevents
   `file_delete("../../../Windows/System32/...")` even before the
   confirmation modal fires. (A future PR can add a "trusted paths"
   setting; out of scope here.)
5. **Confirmation modal copy for `run_command`**: the modal preview
   shows the full literal command + the resolved cwd, no truncation,
   rendered as `<code>` so the user can read every character. The
   existing modal already renders args as JSON; for shell commands
   we'll add a code-block render path. Self-contained tweak.

## Implementation pattern (copy-and-modify)

Every existing tool in `packages/shared-rust/src/tools/` follows the
same shape — see [`wait_for_element.rs`](../../packages/shared-rust/src/tools/wait_for_element.rs)
as the canonical example. Each new tool is roughly 100 lines:

```rust
//! `<tool_name>` — one-line description.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct ToolName;

impl ToolName { pub fn new() -> Self { Self } }
impl Default for ToolName { fn default() -> Self { Self::new() } }

impl LashonTool for ToolName {
    fn name(&self) -> &str { "tool_name" }
    fn description(&self) -> &str { /* what the model needs to know */ }
    fn parameters(&self) -> Value { json!({ /* JSON schema */ }) }
    // OVERRIDE only for destructive tools:
    fn requires_confirmation(&self, _args: &Value) -> bool { true }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            // ... do the work ...
            Ok(ToolResult { content: String::from("..."), display_summary: Some("…".into()) })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn metadata_matches_spec() { /* assert name, requires_confirmation, schema */ }
    #[test] fn missing_required_arg_errors() { /* JSON schema enforcement */ }
}
```

Wire each new module in [`packages/shared-rust/src/tools.rs`](../../packages/shared-rust/src/tools.rs)'s
`register_phase_one_tools` (one `Arc::new(tool::ToolName::new()),`
line per tool) and add the tool's `name()` to the expected-tools
test. The `to_llm_tools` count assertion goes from 13 → 35.

## Security invariants (must hold)

These are the lines that **cannot** be crossed even by an LLM that
asks. They're enforced in code, not by the system prompt.

- **Path roots**: `file_*` tools refuse paths outside
  `~/`, `$env:TEMP`, `/tmp`. Enforced by canonicalising and checking
  prefix BEFORE any I/O. The confirmation modal is a secondary gate;
  the path check is primary.
- **No shell-out hidden in arg values**: `file_write` content is
  treated as bytes, not a script. `type_text` already does this.
- **`run_command` timeout is mandatory**: the child process is killed
  on timeout via `Child::kill`. No `--no-timeout` escape hatch.
- **No logged secrets**: per `.claude/rules/security.md`, no tool
  may log arg values or result content beyond what the existing
  command-mode dispatcher already structurally logs (names, lengths,
  durations).
- **No tool may call another tool**: tools are leaves. Composition
  happens in the LLM's chain, not in tool internals.

## Test plan

- [ ] All ~35 lib tests pass (`cargo test -p lashon-core --features local-llm --lib`).
- [ ] Per-tool unit tests cover: metadata correctness, missing required arg → error, basic happy-path execution (mocked where the OS surface is required — e.g. mock the UIA tree walk for `read_screen`).
- [ ] One integration test that exercises the destructive flow end-to-end against `AlwaysAllow` and `AlwaysDeny` confirmation handlers, to verify the modal infra survives the new gated tools.
- [ ] `cargo check -p lashon` clean.
- [ ] `npm run check` clean (no Hub UI changes expected in this PR).
- [ ] Manual end-to-end through Command mode: "delete the file named X in my Downloads" — confirmation modal appears, user clicks Approve, file is gone.
- [ ] Manual end-to-end: "open Notepad and write hello to a temp file" — chain spans `open_app` → `wait_for_window` → `file_write` (confirmation) → optional `read_screen` to verify.

## Out of scope

- Vision / screenshot model support (`screenshot` would just return an
  unused base64 today; deferred until a vision-capable local LLM ships).
- A "trusted paths" Hub setting to extend `file_*` reach beyond
  `~`/`$TEMP`/`/tmp`.
- Batch confirmation modal (one-call-one-modal stands).
- macOS / Linux platform implementations beyond the trivial cases —
  `run_command`, `file_*`, `list_files`, `list_processes`,
  `set_volume`, `show_notification` ship cross-platform; mouse,
  window, browser, `read_screen` are Windows-only until UIA's macOS
  AXUIElement / Linux AT-SPI peer modules land (separate ADR).
- Memory of past tool calls (the LLM gets the current turn's tool
  results back; longer-horizon recall is M9).

## Definition of done

- 22 new tools registered, each with unit tests passing.
- The 8 destructive tools each trigger the confirmation modal on
  the first invocation in a fresh session; denial short-circuits the
  chain cleanly.
- The system prompt in [`command_mode.rs::build_system_prompt`](../../packages/shared-rust/src/command_mode.rs)
  gains one worked example showcasing a destructive flow (e.g.
  "delete the screenshot in Downloads") so the model knows what to
  expect when the user grants/denies confirmation.
- `CLAUDE.md`'s branch-summary paragraph updated.
- PR opened against `main`, all three CI runners green.
