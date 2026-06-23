# 28. Lashon as MCP server (stdio)

## Status

Accepted — landed on the `m9-mcp-server` branch as M9 Phase 1g v1.

## Context

M9 Phase 1a (ADR-0027) introduced **recipes**: parameterised desktop
workflows authored as `recipe.yaml` and run by the M9 intent cascade
before the LLM full-planner takes over. The headline value of recipes
depends on a steady supply of well-authored recipes for the apps users
care about. Two paths to that supply:

1. **In-Lashon Hub creator UI** (Phase 1e of M9). The user types a
   natural-language description, the Hub sends it to the configured
   cloud LLM, gets back YAML, validates, saves.
2. **Any 2026-era agent host** — Claude Desktop, Cursor, GPT clients
   with MCP support — could do the same thing if Lashon exposes its
   recipe-management primitives as MCP tools.

The second path is strictly more general: the user's primary
agent-authoring tool of choice already understands Lashon's recipe
format, has the user's context loaded, and can iterate on the draft
with `validate_recipe` calls in a way the in-Hub UI can't replicate
without rebuilding a whole chat surface. It also validates the
"format-as-portable-spec" direction (ADR-0027) — if Claude Desktop can
author Lashon recipes through MCP, the format is provably portable
beyond Lashon's runtime.

The cost is a stdio binary, an rmcp dependency, and a Phase-1g-shaped
amount of integration work.

## Decision

Ship `lashon-mcp` — a standalone stdio MCP server binary built from
`lashon-core` — exposing Lashon's recipe-management toolset to any MCP
client. The Tauri shell ships the binary as a resource; the Hub MCP
Server tab (follow-up PR) generates a Claude Desktop config snippet
the user pastes into `claude_desktop_config.json`.

### Crate + transport

- **Crate:** [`rmcp`](https://crates.io/crates/rmcp) v1.7.0, the
  official Anthropic / `modelcontextprotocol` Rust SDK. Pinned
  exactly (`=1.7.0`) per the Lashon dep policy
  (`.claude/rules/rust.md`).
- **Transport:** **stdio only** in Phase 1g v1. JSON-RPC over
  stdin/stdout — the canonical MCP transport, the only one Claude
  Desktop currently understands without extra config, and the
  simplest security model (trust = the spawning process).
- **HTTP/SSE transport deferred** to a follow-up. The Hub MCP Server
  tab will offer it as an opt-in for clients that need network
  reachability; same loopback-only posture as the STT sidecar.

### Tool surface — Phase 1g v1 (five tools, all safe)

| Tool | Args | Behaviour |
|---|---|---|
| `list_recipes` | — | Walk bundled + per-user dirs, return `[ { id, description, source, path } ]` |
| `get_recipe` | `id: string` | Read full `recipe.yaml` for an id; per-user shadows bundled |
| `validate_recipe` | `yaml: string` | Parse + run the full validator from ADR-0027; return `ok` or multi-line issues |
| `save_recipe` | `id: string, yaml: string, overwrite?: bool` | Validate then write to `<data-local>/lashon/recipes/<id>/recipe.yaml`; refuses overwrite by default |
| `list_recipe_step_types` | — | Emit the JSON Schema for `Step` so authors discover the OS-UI step vocabulary |

All five are **safe**: they read or write `recipe.yaml` files,
nothing else. A malicious caller can write a syntactically-valid
recipe; nothing happens until the user triggers that recipe through
Hub or voice. The Phase 1g v1 surface does NOT expose:

- **Interactive tools** (`click_element`, `right_click`,
  `double_click`, `drag`, `scroll`, `press_keys`, `type_text`) —
  these would need an opt-in toggle and the existing confirmation
  modal; both live in the Tauri shell and the stdio binary has no
  IPC path to them.
- **Destructive tools** (`file_write`, `file_delete`, `file_move`,
  `run_command`, `kill_process`, `lock_screen`, `close_window`) —
  same constraint, plus stronger gating requirements (M11+ stretch).

The Hub MCP Server tab follow-up adds the opt-in toggles; even with
toggles on, the confirmation modal continues to gate destructive
calls exactly as it does for the Command-mode dispatcher today.

### Tool naming

Unprefixed snake_case (`list_recipes`, not `lashon.list_recipes`).
M9 story open question #7 picked between the two; the decision:

- MCP clients label tools by server in their UI ("Lashon: list_recipes")
  so prefixing is redundant.
- Anthropic's reference MCP servers (`filesystem`, `git`, `github`)
  all use unprefixed snake_case.
- Matching Lashon's internal dispatcher names means a recipe author
  uses the same name from voice and from MCP without translation.

### Default-off

M9 story open question #8 — default off, consistent with "cloud is
opt-in." The Tauri shell never spawns `lashon-mcp` automatically.
The user enables it from the Hub MCP Server tab (follow-up PR),
copies the generated config snippet, pastes into
`claude_desktop_config.json`, restarts Claude Desktop. Claude
Desktop then spawns `lashon-mcp` as a child process and the MCP
connection is live.

### Binary location

M9 story open question #6 — the binary lives at
`packages/shared-rust/src/bin/lashon_mcp.rs` as a `[[bin]]` entry on
the `lashon-core` crate, gated behind the `mcp-server` Cargo feature
(default-on). This places it where it can depend on `lashon-core`'s
recipe types directly without pulling in the Tauri runtime — the
binary stays ~5 MB stripped instead of the ~80 MB it would weigh
if it linked Tauri. The Tauri shell bundles the built binary as a
resource (same `[bundle].resources` mechanism the STT sidecar uses)
so the installer ships a single `lashon-mcp.exe`.

The Cargo workspace's lockfile pins `lashon-core`'s exact dep set;
the `mcp-server` feature on by default means CI exercises the binary
on every commit.

## Security posture

Symmetric to the STT sidecar's trust boundary (`docs/adr/0010`) but
adapted from loopback TCP to stdio process parentage:

- **The agent host's identity is the trust boundary.** stdio MCP
  runs as a child process of the host; only that host can write to
  the binary's stdin. There is no inbound network port, no shared
  secret, no listening socket. The host's authentication of *itself*
  (Claude Desktop's user login, etc.) is Lashon's only trust signal,
  and it's enough because compromise of the host implies compromise
  of the system anyway.
- **Safe tool surface only.** Phase 1g v1 exposes recipe-management
  only. The user has to actively trigger a saved recipe through Hub
  or voice for any OS effect to land — saving alone is not enough.
- **Validation gates the only write path.** `save_recipe` parses +
  validates before writing. A malicious caller can't smuggle past
  the validator: the file format itself constrains what the recipe
  can do.
- **No transcript / audio / PII** ever touches the MCP server.
  `LASHON_DEBUG_TOOL_ARGS` (the opt-in logging exception) is
  honoured by the dispatcher, not by the MCP server, so the
  existing security rule (`.claude/rules/security.md`) holds
  unchanged.
- **Spawning a different binary is the user's problem.** If the
  user pastes a config snippet that spawns a Trojan with the same
  name, that's a system-level compromise — Lashon can't and
  shouldn't try to defend against it.

## Why not the in-Hub creator UI alone?

The Hub creator UI (Phase 1e) still ships — it's the simpler path
for users who don't run an MCP-capable agent host. The MCP route
adds value on top:

- A user *already using* Claude Desktop or Cursor doesn't have to
  context-switch into Lashon to author a recipe.
- The host carries the user's project / file context, which often
  matters for "the recipe needs to know paths in *my* dev tree."
- Multi-turn refinement ("here's the recipe; the third step needs
  to wait longer for the Electron app") is the host's native shape.
- Future agent hosts get free Lashon support without Lashon shipping
  per-host integrations.

The in-Hub creator UI becomes the fallback; the recommended path is
"install Claude Desktop, paste the snippet."

## Schemars versions

`lashon-core` already uses `schemars = "=1.2.1"` for the recipe JSON
Schema export (ADR-0027). `rmcp` bundles its own `schemars` for tool
parameter schemas (currently v0.x at rmcp 1.7.0). Cargo resolves both
versions side-by-side without conflict — we don't share `JsonSchema`
impls across the two trees:

- Recipe `Step` JSON Schema (returned by `list_recipe_step_types`)
  is generated with the v1 schemars from `lashon-core::recipes`.
- MCP tool parameter schemas (`GetRecipeArgs`, `SaveRecipeArgs`, etc.)
  are generated with the v0 schemars rmcp re-exports.

Both are JSON Schema Draft 7 wire format; clients can't tell which
crate generated which schema.

## Consequences

- **MCP discoverability.** Lashon is now an addressable surface for
  any 2026-era agent system. Adds Lashon to the small set of
  desktop-control servers a Claude Desktop user can wire up.
- **Format adoption pressure.** Once Claude Desktop is in the loop,
  the recipe format starts feeling pressure from outside Lashon.
  Future schema bumps (`version: 2`) need to consider clients that
  authored against the older spec.
- **Versioning interplay.** The advertised MCP server version
  tracks `lashon-core` (which is `0.7.0` as of this PR), not the
  user-facing Lashon app version. The Hub's MCP tab snippet
  generator includes the version comment so the user knows what
  they're connecting to.
- **Binary lifecycle = host lifecycle.** Claude Desktop spawning +
  killing `lashon-mcp` is fine; the binary is stateless and the
  per-user recipes dir is the only shared state. No coordination
  needed with the main Tauri shell.
- **Win32 Job Object NOT used here.** `lashon-mcp` runs as a child
  of the agent host (Claude Desktop), not of Lashon's Tauri shell.
  The job-object kill-on-close pattern that wraps the STT sidecar
  + llama-server doesn't apply — the agent host owns the lifecycle
  end-to-end. If Claude Desktop crashes, the OS reaps the child
  through the host's normal process tree, not Lashon.

## What's not in Phase 1g v1

Explicitly deferred to follow-up PRs in priority order:

1. **Hub MCP Server tab** — status pill, enable/disable toggle,
   per-category exposure toggles (safe reads / interactive /
   destructive), "Connect Claude Desktop" snippet-generator button.
   Story doc + ticket attached to the main M9 story.
2. **HTTP/SSE transport** — second binding inside the main Lashon
   process for HTTP-MCP clients (Cursor). Loopback-only.
3. **Safe-read tool exposure** — `read_active_window_text`,
   `read_screen`, `list_open_windows`, `list_files`, `file_read`,
   `list_processes`, `clipboard_get`. The Phase 1g v1 surface is
   recipe-only because that's the headline value; the read tools
   add ergonomic value without changing the security model and can
   land any time after.
4. **Interactive/destructive opt-in** — toggle in the Hub MCP tab
   that exposes the existing `LashonTool` interactive + destructive
   surface to MCP callers. Requires plumbing the confirmation modal
   across the process boundary; M11+ stretch.
5. **Lashon as MCP client (outbound)** — calling other people's MCP
   servers from inside a recipe. Separate ADR; future capability.
6. **Per-client tool subsets** — "Claude Desktop gets safe reads +
   recipe tools; Cursor gets safe reads only." Future PR.
7. **Signed MCP requests / per-client auth.** M11+ stretch.

## Notes

The single-binary-per-server convention (vs. multiplexing servers
inside one binary) is deliberate. It matches how `npx @anthropic/...`
and the Python `mcp-server-*` packages work, makes the Claude Desktop
config snippet trivially understandable, and means a future
Lashon-as-MCP-client doesn't accidentally land in the same binary as
Lashon-as-MCP-server.
