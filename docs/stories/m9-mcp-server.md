# M9 Phase 1g — Lashon as MCP server (stdio v1)

> **Status: shipped on `main` in PR #74.** First slice of Phase 1g per
> [`docs/stories/m9-recipes.md`](m9-recipes.md). ADR-0028.

## What ships

The headline: any 2026-era agent host (Claude Desktop, Cursor,
GPT-via-MCP-client) can now author and manage Lashon recipes through
MCP. The slice that lands here:

- `packages/shared-rust/src/mcp/` — `rmcp` integration. The
  `LashonMcpServer` struct + `#[tool_router]` impl carrying the
  five Phase-1g-v1 tools, and a thin `ServerHandler` glue.
- `packages/shared-rust/src/bin/lashon_mcp.rs` — the `lashon-mcp`
  stdio binary. Runs the `LashonMcpServer` over
  `rmcp::transport::stdio()`, pumps tracing to stderr (stdout is
  the MCP transport).
- `[features].mcp-server` — Cargo feature gating the rmcp dep + the
  binary; default-on so CI exercises the path. A library-only
  consumer of `lashon-core` can opt out with `--no-default-features`.
- ADR-0028 — transport choice, tool surface, security posture, the
  three open M9 questions resolved (#6 binary build path, #7 tool
  naming, #8 default-on/off).
- `tests/lashon_mcp_stdio.rs` — end-to-end integration test that
  spawns the built `lashon-mcp` binary, drives a real JSON-RPC
  handshake over its stdin/stdout, and asserts that `tools/list`
  returns all five tools and `tools/call list_recipes` returns the
  10 bundled starters.

## Tool roster (Phase 1g v1 — five safe tools)

| Tool | Args | Purpose |
|---|---|---|
| `list_recipes` | — | Enumerate installed recipes (bundled + user), with source tag and on-disk path |
| `get_recipe` | `id: string` | Read full `recipe.yaml` for an id; per-user shadows bundled |
| `validate_recipe` | `yaml: string` | Validate a draft against the ADR-0027 schema; returns `ok` or multi-line issues |
| `save_recipe` | `id: string, yaml: string, overwrite?: bool` | Persist a draft to the per-user dir; validates before writing |
| `list_recipe_step_types` | — | Emit the JSON Schema for `Step` so authors discover the OS-UI vocabulary |

All five are safe — they only read or write `recipe.yaml` files.
Interactive/destructive tool exposure is opt-in, gated by the Hub
MCP Server tab (follow-up PR) + the existing confirmation modal.

## How a user wires this up (eventual flow — Hub follow-up PR)

1. Open the Hub → MCP Server tab → toggle "Enable MCP server".
2. Click "Connect Claude Desktop" → snippet copied to clipboard.
3. Paste into `~/Library/Application Support/Claude/claude_desktop_config.json`
   (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows).
4. Restart Claude Desktop.
5. Ask Claude: "List my Lashon recipes" — Claude calls
   `list_recipes`, surfaces them in chat.
6. Ask Claude: "Write me a Lashon recipe that opens YouTube Music
   and plays a song" — Claude calls `list_recipe_step_types` to see
   the vocabulary, drafts a `recipe.yaml`, calls `validate_recipe`
   to check it, then `save_recipe` to persist.
7. The recipe appears in the Hub Recipes tab and is callable via
   voice on the next Command-mode invocation.

Today, with this PR, steps 1–4 happen via manual `claude_desktop_config.json`
editing — the Hub tab is the follow-up that adds the snippet generator.
The MCP binary itself is fully functional.

## Manual smoke test (today)

```jsonc
// claude_desktop_config.json
{
  "mcpServers": {
    "lashon": {
      "command": "C:/path/to/lashon/target/debug/lashon-mcp.exe",
      "env": {
        "LASHON_BUNDLED_RECIPES_DIR": "C:/path/to/lashon/recipes/starters"
      }
    }
  }
}
```

Restart Claude Desktop. Ask "list my Lashon recipes" → expect the
ten starters.

## What's not in this PR

Explicitly deferred per ADR-0028 §"What's not in Phase 1g v1", in
priority order for follow-up PRs:

1. **Hub MCP Server tab** — status, toggles, snippet generator
2. **HTTP/SSE transport** — for Cursor / non-stdio MCP clients
3. **Safe-read tool exposure** — `read_active_window_text`,
   `clipboard_get`, `list_open_windows`, etc.
4. **Interactive/destructive opt-in** — confirmation-modal-gated
5. **Lashon as MCP client (outbound)** — separate ADR
6. **Per-client tool subsets** — future PR
7. **Signed MCP requests / per-client auth** — M11+

## Test plan

- [x] `cargo test -p lashon-core --lib` — 305 passed, 6 ignored (no
  regression from Phase 1a)
- [x] `cargo test -p lashon-core --test lashon_mcp_stdio` — 1 passed
  (end-to-end stdio + initialize + tools/list + tools/call)
- [x] `cargo test -p lashon-core --test recipe_starters` — 3 passed
- [x] `cargo test -p lashon-core --test recipe_schema_snapshot` — 1
  passed
- [x] `cargo check --workspace --all-targets` clean
- [ ] CI green on all three runners
- [ ] Manual: wire `lashon-mcp` into Claude Desktop and call
  `list_recipes`, `get_recipe send-discord-message`,
  `validate_recipe` with a hand-written draft, `save_recipe` with
  a new recipe id.

## Definition of done

- `lashon-mcp` binary builds + runs on Windows
- All five Phase-1g-v1 tools functional end-to-end through stdio
- Integration test asserts the JSON-RPC handshake + tool call
  round-trip
- ADR-0028 committed
- Story doc committed (this file)
- CLAUDE.md branch-summary paragraph updated
