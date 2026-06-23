# Command mode (M8 Phase 1)

Milestone **M8**. Branch `feat/m8-command-mode` (based on
`feat/m7-llm-providers` — both branches merge together).

> **Status: in progress.** Phase 1 (the safe-tool catalogue + dispatch
> loop + tongue surfaces) is on `feat/m8-command-mode`. ADR-0024 pins
> the tool trait and the dispatcher contract. Phase 2 — destructive
> tools wired to the confirmation modal, intent-classification auto-route,
> the Conversation panel — is a separate PR.

## Why

M7 made the LLM provider mux load-bearing, but no surface in the app
calls an LLM in production (only the Hub's "test prompt" button). M8
gives the LLM a job: hold the Command-mode hotkey, speak a Hebrew or
English command, let an LLM tool-call loop execute it.

The roadmap entry (`docs/roadmap.md §2`) is sprawling — 30-odd native
tools, the Conversation panel, external-agent delegation, long-term
memory. Phase 1 takes the smallest meaningful slice:

- One dedicated Command-mode hotkey, no intent classification.
- Eight safe tools (no `file_delete`, no `shutdown`, no
  `send_message`).
- A tongue flash + tracing log for the result. The Conversation panel
  is M9.
- The confirmation modal infrastructure is wired even though no
  Phase-1 tool needs it — so M8.2's destructive tools just flip the
  `requires_confirmation` flag.

## Scope

### In scope (Phase 1, this branch)

- `lashon_core::tool::LashonTool` trait + `ToolRegistry`.
- Phase-1 tool catalogue:
  - `click_element` (M8.1) — UI Automation walk of the foreground
    window, finds the first descendant whose Name contains the
    user's substring (case-insensitive, on-screen-only), mouse-clicks
    its bounding-rect centre via `enigo`. Windows-only; macOS / Linux
    stub.
  - `clipboard_get` / `clipboard_set` — `arboard`.
  - `focus_window` — Win32 `EnumWindows` + `SetForegroundWindow`.
  - `open_app` — `cmd /c start` with aliases for `vscode`, `spotify`,
    `chrome`, `firefox`, `notepad`, `calc`, `explorer`, `slack`,
    `discord`, `telegram`, `whatsapp`.
  - `open_url` / `web_search` — `open` crate.
  - `press_keys` — `enigo` with a chord parser supporting
    `Ctrl/Alt/Shift/Win/Cmd` modifiers, named keys (`Enter`, `Tab`,
    `F1`..`F12`, …), and single-character `Unicode` keys.
  - `type_text` — reuses `lashon_core::inject::inject_text` with the
    Hebrew clipboard path.
  - `wait_ms` (M8.1) — `tokio::time::sleep` with a 10000 ms cap, so
    the LLM can wait for apps to finish launching / search results
    to render before the next interactive call.
- `lashon_core::command_mode::dispatch` — the LLM tool-call loop,
  `MAX_TURNS = 8`. Returns a `CommandOutcome { assistant_text,
  tool_summaries, turns }`. **M8.1**: the dispatcher now also takes
  a `CommandProgressHandler` and calls `on_thinking` /
  `on_tool_started` / `on_tool_finished` around every LLM round-trip
  and tool execution so the user sees what's happening.
- `ConfirmHandler` trait with `AlwaysAllow` / `AlwaysDeny` for tests
  and `EventBasedConfirm` in the Tauri shell that emits
  `command:confirm` and awaits a `command:confirm:reply` event (30s
  timeout, denial on timeout).
- `CommandProgressHandler` trait (M8.1) with `NoOpProgress` for
  tests and `EventProgress` in the Tauri shell that emits
  `command:state` (`"thinking"` / `"idle"`) and `command:tool`
  events. The tongue listens for both and renders a three-dot
  spinner + rolling status line so the user no longer waits in
  silence between hotkey and result.
- Dictation worker extension: `TakeMode { Inject, Command }` travels
  with each hotkey press; the `Command` branch hands the transcript
  to `command_mode::dispatch_transcript`.
- Tauri commands: `command_hotkey_pressed`, `command_hotkey_released`,
  `command_mode_status`, `command_mode_dispatch_text` (dev smoke).
- Tongue surfaces:
  - `command:result` event → flash window resize → text for 3.5s →
    restore.
  - `command:confirm` event → confirm modal window resize → Allow /
    Deny → emit `command:confirm:reply` → restore.
  - `command:state` event (M8.1) → window resize + three-dot
    spinner + "Lashon is thinking…" label while the dispatcher is
    waiting on the LLM.
  - `command:tool` event (M8.1) → same widget, label swaps to the
    tool's `display_summary` for ~1.2s after each `finished` event,
    so the user sees each step of the chain land.
- Hub Shortcuts section: a second `HotkeyCapture` bound to
  `hotkeys.command`. Default chord: `CommandOrControl+Backquote`
  (Ctrl+`) — reachable with the left pinky and doesn't clash with
  Ctrl+Space.
- He+en localization for the confirmation modal + the new shortcuts
  copy.
- ADR-0024.

### Explicitly deferred

- **Destructive tools** — `file_delete`, `file_write`, `shutdown`,
  `restart`, `send_message`. Each lands with `requires_confirmation`
  set per the policy in `docs/roadmap.md §2.6`. The modal exists in
  Phase 1 and these tools just flip the flag.
- **Mouse tools** — `click_at`, `move_to`, `screenshot`,
  `read_screen`. Need OCR or coordinates from screen content; M9 work.
- **Intent classification + Dictation→Command auto-route** — the
  verb-lexicon match and the `^(לשון|lashon)` wake-prefix. M8.2.
- **Conversation panel** — the sliding right-edge window
  (`docs/roadmap.md §2.7`). M9.
- **External agent delegation (`delegate_agent`)** — Claude Code,
  OpenCode, Codex, Aider, Goose via PTY. M9.
- **Long-term memory (`remember`)** — SQLite-backed cross-session
  facts. M12.
- **Spoken responses** — Phase 3 / M10–M11. The Phase-1 tongue flash
  is silent text.

## Phased breakdown

Phase 1 is one PR (`feat/m8-command-mode`). Phase 2 lands separately
once Phase 1 + the M7 PR are both merged.

### Phase 1 (this branch)

The eight commits land in order:

1. `lashon-core::tool` trait + `ToolRegistry` + `MockTool`.
2. The Phase-1 tool impls (one commit per tool, or batched — both
   are fine).
3. `lashon-core::command_mode` dispatcher + `ScriptedLlm` test
   harness.
4. Dictation worker `TakeMode` extension.
5. Tauri shell `command_mode.rs` (per-take provider construction +
   `EventBasedConfirm`).
6. Tongue surfaces: command flash + confirm modal in `+page.svelte`
   and `Tongue.svelte`.
7. Hub Shortcuts section: second `HotkeyCapture`.
8. Docs: ADR-0024, story doc, CLAUDE.md.

### Phase 2 (follow-up)

- Destructive tools: `file_delete`, `file_write` (system dirs),
  `shutdown`, `restart`, `send_message`.
- Intent classification: verb-lexicon + wake-prefix; auto-route from
  the dictation hotkey when a transcript looks like a command.
- Cross-platform polish for `open_app` and `focus_window` (macOS
  `open -a` + AXUIElement; Linux `.desktop` lookup + wmctrl).
- Hebrew command corpus tests (`tests/commands.he.yaml`) per the
  M8 DoD (`docs/roadmap.md` Phase 2 DoD #1).

## Test strategy

### Unit tests in `lashon-core` (no real LLM calls)

- **Tool conformance**: per-tool tests that the JSON schema is shaped
  right, missing args error cleanly, and Hebrew args round-trip.
- **`ToolRegistry`**: registration, lookup, duplicate-name panic,
  alphabetical sort, `to_llm_tools()` serialisation.
- **Dispatcher**:
  - Plain-text reply returns immediately.
  - Tool-call → tool_result → final-text chain returns the right
    `CommandOutcome`.
  - Unknown tool feeds an error-shaped tool_result back.
  - Denied confirmation short-circuits with the Hebrew
    "ביטלת את הפעולה" assistant text.
  - `MAX_TURNS` cap returns the
    "(הגעתי לתקרת קריאות הכלים)" text.
- **System-prompt builder**: every registered tool appears in the
  prompt with its description; OS + date + language strings are
  included.

Total new tests on this branch: ~46 (the Phase-1 tool catalogue and
the dispatcher together). The lashon-core test count goes from 113
(M7) to 159 (M8 Phase 1).

### Manual smoke (Windows)

1. **Configure**: open Hub → Language models → pick a provider for
   Command mode (Anthropic / Groq / Ollama local) and save the key.
2. **Simple**: press the Command hotkey, say
   "פתח את Notepad" — Notepad launches; the tongue flashes
   `פתחתי את notepad`.
3. **Chained**: say
   "פתח את Notepad ותכתוב שלום עולם" — Notepad launches,
   focuses, types the text. Hebrew RTL is intact.
4. **Web search**: say
   "חפש imagine dragons" — the default browser opens DuckDuckGo
   results for "imagine dragons".
5. **Spotify**: say
   "פתח את Spotify וחפש Imagine Dragons" — Spotify launches,
   the tool-call chain focuses → presses Ctrl+L → types "Imagine
   Dragons" → presses Enter. The play step is out of Phase-1 scope
   (needs mouse + OCR).
6. **No-LLM**: temporarily set
   `llm.command.provider = "none"`; press the hotkey, speak —
   a clear "no LLM configured" toast appears, nothing executes.

### Manual smoke — confirmation modal

No Phase-1 tool requires confirmation, so the modal is exercised by a
dev-only flag (set `confirm: true` on `MockTool` via a unit test, or
flip a tool's `requires_confirmation` body temporarily in dev). M8.2
ships the first real destructive tool.

## Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| LLM emits a tool name not in the registry | Med | Low | Dispatcher feeds back `error: tool X not available`; the LLM repairs on the next turn. Covered by `dispatch_handles_unknown_tool_by_feeding_back_an_error`. |
| LLM loops on tool calls (cap is reached) | Med | Low | `MAX_TURNS = 8`. The user gets the "תקרת קריאות הכלים" assistant text and a tracing log. |
| `open_app` alias table misses a popular app | High | Low | `cmd /c start "" "<verbatim>"` already handles a lot. Add aliases as users report misses. |
| `focus_window` matches the wrong window when titles overlap | Med | Med | Substring match is first-hit + visible-only. Users see what the LLM matched in the tracing log; an obvious mis-match shows up in M8.2's manual eval. |
| Hotkey conflicts with another global shortcut | Med | Low | The Hub rebinds; the fallback default is reasonably exotic. |
| Window resize jitter for flash / confirm | Low | Low | The resize is a one-shot setSize, not animated. Linux WMs occasionally reject redundant calls — caught in a `try`. |
| User denies a confirmation and the LLM re-attempts | Low | Low | The denied tool_result message reads "do not retry"; if the LLM ignores it, the next prompt still aborts the loop with the `MAX_TURNS` cap. |
| Per-take provider construction reads a stale key after a Hub change | Low | Low | Each dispatch reads `settings.json` + keychain fresh — no caching beyond the LLM trait method. |

## Definition of Done (Phase 1)

- `cargo test --workspace --no-fail-fast` clean.
- `npm run check` clean.
- `cargo check --workspace --all-targets` clean.
- The five manual smoke steps above all work end-to-end on Windows.
- ADR-0024 lands in the same PR.
- CLAUDE.md "current milestone" reflects M8.

## Definition of Done (Phase 2, future)

- 20 Hebrew test commands pass against a real LLM (per
  `docs/roadmap.md` Phase 2 DoD #1).
- The confirmation modal is exercised by at least one
  user-visible destructive tool.
- Intent classification routes plain-language transcripts into
  Command mode without a dedicated hotkey.
