# 24. Command-mode tool registry and dispatch loop

- **Status:** Accepted
- **Date:** 2026-05-22
- **Deciders:** Lashon contributors
- **Context source:** Milestone M8 — tool registry + command mode
  ([`../roadmap.md §2.2`–`§2.6`](../roadmap.md)). Builds on
  [ADR-0019](0019-provider-mux-traits.md) (the `LLMProvider` trait),
  [ADR-0020](0020-keychain-integration.md), and
  [ADR-0022](0022-cloud-opt-in-and-badging.md).

## Context

M7 wired the LLM provider mux but no surface in the app actually calls
it. The only LLM exercise in M7 is the Hub's "test prompt" button.

M8 makes the LLM useful: the user presses a Command-mode hotkey, speaks
("פתח את Spotify וחפש Imagine Dragons"), and Lashon executes the
result by chaining native tools. Three things need pinning before the
first line of M8 code lands:

1. **The tool abstraction.** What does a "tool" look like? How is its
   schema published to the LLM? How does it execute? How does it gate
   on user confirmation?
2. **The dispatch loop.** How many LLM turns are allowed per take?
   What happens when the LLM emits an unknown tool name, denies a
   confirmation, or loops forever?
3. **The transcript→dispatch hand-off.** How does the dictation worker
   (Phase 1) end up routing a take to the LLM dispatcher instead of
   the text injector?

## Decision

### The `LashonTool` trait

```rust
pub trait LashonTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn requires_confirmation(&self, _args: &Value) -> bool { false }
    fn execute<'a>(&'a self, args: &'a Value)
        -> BoxFuture<'a, Result<ToolResult>>;
}
```

The `parameters()` return value is JSON Schema — both Anthropic's
`input_schema` and OpenAI's `function.parameters` accept this shape,
so `ToolRegistry::to_llm_tools()` forwards it verbatim into the
`llm::Tool` array on every `chat()` call.

`ToolResult` carries two strings: `content` (what the LLM sees on its
next turn, stringified) and `display_summary` (what the tongue flashes
to the user, Hebrew-friendly). `ToolResult::silent(content)` builds a
result whose action need not be echoed to the user — e.g.
`clipboard_get`.

`requires_confirmation(args)` is per-call rather than per-tool so
M8.2's `file_delete` can return `true` only for system-directory
targets while leaving home-directory deletes ungated.

### Tool registration

A `ToolRegistry` holds `Arc<dyn LashonTool>` by name. `register()`
panics on duplicate names — a registry is configured at startup and
a name collision is a programmer error caught at review time, not at
runtime.

The Phase-1 catalogue is wired in `lashon_core::tools::phase_one_registry`:

| Tool | Where it lives | Cross-platform |
|---|---|---|
| `clipboard_get` / `clipboard_set` | arboard | Yes |
| `focus_window` | Win32 `EnumWindows` + `SetForegroundWindow` | Windows only in Phase 1; mac/Linux return a clear error |
| `open_app` | `cmd /c start` (Win), aliases for `vscode`/`spotify`/… | Windows only in Phase 1 |
| `open_url` / `web_search` | `open` crate (xdg-open / `open` / cmd start) | Yes |
| `press_keys` | enigo, chord parser (`Ctrl+Shift+S`, `Enter`, `F4`, …) | Yes |
| `type_text` | reuses `lashon_core::inject` — Hebrew clipboard path | Yes |

None of the Phase-1 tools require confirmation. The
confirmation-modal infrastructure (below) is wired so M8.2's
destructive tools (`file_delete`, `shutdown`, `send_message`) flip
the flag without touching the dispatcher.

### The dispatch loop

`lashon_core::command_mode::dispatch(provider, registry, confirm,
transcript, ui_language)` runs the loop:

```text
1. Build the system prompt: identity + OS + today + language +
   per-tool descriptions.
2. messages = [system, user(transcript)]
3. Loop up to MAX_TURNS (= 8):
   a. completion = provider.chat(&messages, &llm_tools).await
   b. (text, calls) = split_content(completion.content)
   c. messages.push(assistant turn)
   d. if calls.is_empty(): return CommandOutcome{ text, summaries, turn }
   e. for call in calls:
      - tool = registry.get(call.name); if absent, append "error: tool
        not available" and continue.
      - if tool.requires_confirmation(args):
          decision = confirm.confirm(name, args).await
          if Deny: append "user denied", short-circuit, return.
      - result = tool.execute(args).await
      - append tool_result message
4. Cap reached: return "(הגעתי לתקרת קריאות הכלים)" with summaries.
```

`MAX_TURNS = 8` matches `docs/roadmap.md §2.4` ("Hard cap: 8 tool
calls per command"). A run that hits the cap is rare — it means the
LLM is looping rather than concluding.

The `ConfirmHandler` is trait-injected. `AlwaysAllow` and `AlwaysDeny`
exist for tests; the Tauri shell ships `EventBasedConfirm` which
emits `command:confirm` and awaits `command:confirm:reply` with a
30s timeout (denial on timeout).

### Transcript routing

The dictation worker (`apps/desktop/src-tauri/src/dictation.rs`) is
extended with a `TakeMode { Inject, Command }` that travels with each
hotkey press. After STT, the worker branches:

- `Inject` → existing behaviour (text injection at cursor).
- `Command` → `command_mode::dispatch_transcript(app, transcript)`,
  which spawns the dispatcher on Tauri's async runtime and emits the
  result as a `command:result` event.

Two new Tauri commands wrap the existing edge protocol:
`command_hotkey_pressed`, `command_hotkey_released`. They tag the
worker message with `TakeMode::Command`; everything downstream
(capture, VAD endpointing, STT) is unchanged.

### Output surface

Per the M8 product decision (this PR), Command mode shows its result
as a **tongue flash + tracing log**:

- The tongue window resizes from 104×104 to ~360×132 for the flash,
  renders the `assistant_text` (or fallback tool summary) for ~3.5s,
  then restores to its idle size.
- A `tracing::info!` line records `turns` and `tool_count` —
  **never** the transcript content (`.claude/rules/security.md`).
- The Conversation panel (a sliding right-edge window with streaming
  bubbles, tool-call cards, agent tabs, per `docs/roadmap.md §2.7`)
  is **deferred to M9** — it's a different UI surface entirely.

### Confirmation modal

When `ConfirmHandler::confirm` is called the tongue window resizes to
~380×168 and the Tongue component renders the modal — tool name,
truncated args preview, **Allow** (citron) and **Deny** (subtle)
buttons. Esc denies.

The modal's text comes from `command.confirm.*` in the locale files.
The args preview is `serde_json::to_string` truncated at 96 chars;
the full args are visible only in tracing logs.

## Alternatives considered

- **Per-app native-tool sub-protocols** — Spotify's CLI, Slack's CLI,
  per-app browser extensions. Rejected: every app would need its own
  integration, and most apps don't expose one. The generic
  `open_app + focus_window + press_keys + type_text` chain covers
  ~90% of consumer software at the cost of a few extra LLM turns.
- **A single `execute_command(transcript)` LLM-prompt-only interface**
  with no tool schema — let the LLM emit shell. Rejected on security
  grounds: arbitrary shell execution is a sledgehammer, and the LLM
  has no way to know which apps the user has installed.
- **Per-take confirmation modal for every tool** — too disruptive.
  Phase-1 tools are all safe; the user has already consented by
  pressing the Command hotkey.
- **Synchronous tool dispatch on the dictation worker thread** — the
  worker owns the audio stream, which is `!Send`. Spawning on Tauri's
  async runtime keeps the worker free to start the next take while
  the LLM is still thinking.
- **A separate "command" audio worker** — duplicates cpal capture
  logic. Carrying the mode on the existing take is simpler.

## Consequences

- New modules in `lashon-core`: `tool`, `tools`, `command_mode`.
- New deps in `lashon-core`: `chrono =0.4.42` (date in the system
  prompt) and `open =5.3.4` (the `open_url` and `web_search` tools).
  The `windows` crate gains the `Win32_UI_WindowsAndMessaging`
  feature for `focus_window`'s `EnumWindows`.
- New deps in the Tauri shell: `tokio =1.52.3` (for
  `tokio::sync::oneshot` + `tokio::time::timeout` in the
  confirmation handler).
- `apps/desktop/src-tauri/src/command_mode.rs` is the per-take
  builder: it reads `settings.json` for `llm.command.provider` +
  `llm.command.model` + base URL, constructs a fresh provider, calls
  `dispatch`, and emits `command:result`. No long-lived
  `dyn LLMProvider` state.
- `apps/desktop/src/routes/+page.svelte` registers the
  `hotkeys.command` chord (default `CommandOrControl+Backquote`, i.e.
  Ctrl+`) and resizes the tongue window for the flash / confirm modal.
- The Hub's Shortcuts section gains a second `HotkeyCapture` bound
  to `hotkeys.command`. The existing `validate_hotkey` rule covers
  both.
- The cloud-opt-in invariants (ADR-0022) carry through: the LLM is
  only ever invoked when the user has explicitly chosen a Command
  mode provider in the Hub *and* stored an API key (or is using
  Ollama). With `"llm.command.provider" == "none"` the dispatcher
  refuses to run with a clear toast.
- All 38 new tool-related tests run in `cargo test -p lashon-core`
  with no real LLM API calls. The `ScriptedLlm` mock runs the
  dispatcher through tool-call → tool_result → final-text chains.
