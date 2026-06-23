# Command-mode editioning (free dictation build)

**Status:** active — branch `feat/command-mode-editioning`. Decision:
[ADR-0034](../adr/0034-command-mode-editioning.md).

Gate command mode behind a `command-mode` Cargo feature so a
`--no-default-features` build ships **dictation only**, with command mode
compiled out (not merely hidden). See ADR-0034 for the why + the gate/keep map.

## Build profiles

- **Full (paid):** `default = ["command-mode"]` — current behaviour, unchanged.
- **Free (dictation):** `cargo build --no-default-features` +
  `VITE_LASHON_EDITION=free` + a `tauri.conf` profile that doesn't bundle
  `llama-server`.

## Work, in order

1. **Cargo features.** Add `command-mode` to `lashon-core` (umbrella over
   `local-llm` + `mcp-server`); `default = ["command-mode"]`. Mirror a
   `command-mode` feature in the app crate that forwards to lashon-core's.
2. **lashon-core gating.** `#[cfg(feature = "command-mode")]` on the module
   decls in `lib.rs` for command_mode, llm, llama_server, tool, tools, recipes,
   provider_registry(?), the bins. Gate the dictation-side references to gated
   symbols — the `TakeMode::Command` arm is the main one.
3. **Tauri shell gating.** Gate `mod command_mode/llm/recipes`; the
   command-hotkey handlers in `dictation.rs`; the command wake-slot in
   `wakeword.rs`; the ~22 command-mode `invoke_handler!` entries; the
   `LlamaServerState` + `ActiveDispatch` `.manage()` calls.
4. **Frontend edition flag.** `VITE_LASHON_EDITION`; conditionally render +
   tree-shake the command UI: tongue command/chat modes, and the Hub's LLM
   section, Recipes tab, MCP tab, command/chat hotkey rows, and wake command slot.
5. **Build configs.** A free `tauri.conf` (no llama-server bundle) + the
   release matrix builds both editions.

## Seams that need care

- `TakeMode { Inject, Command }` — the free build is inject-only.
- Hotkey manager — register only the dictation chord.
- Wake worker — dictation slot only.

## Confirm during implementation

- `provider` / `provider_registry`: STT-shared (keep) or LLM-only (gate)?
- `transcript` word-aliases / Voice-corrections: dictation-relevant (keep) or
  command-only (gate)? Lean **keep** — they fix STT output, which helps
  dictation. If kept, move `get_word_aliases` / `set_word_aliases` out of the
  `command_mode` Tauri module so they survive the gate.

## Definition of done

- `cargo check --workspace` green (full) **and**
  `cargo check -p lashon-core --no-default-features` green (free core).
- Full build unchanged — 361 lib tests green.
- Free build: launches, dictation works end-to-end, **zero** command-mode UI in
  the tongue or Hub, and the command-mode Tauri commands are absent.
- Free installer does not bundle `llama-server`.
- Both editions smoke-tested on a real install before either ships.
