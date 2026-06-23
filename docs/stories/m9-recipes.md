# M9 — Recipes

> **Status: Phases 1a–1d + 1g shipped on `main`.** Cascade tier 1
> (regex) wired into the Command-mode dispatcher and proven
> end-to-end against the live Discord client. See per-phase
> story docs for what landed: `m9-recipe-runtime.md`,
> `m9-intent-cascade.md`, `m9-mcp-server.md`,
> `m9-hub-recipes-tab.md`, `m9-steps-panel.md`. Cascade tier 2
> (embedding) + tier 3 (LLM classifier) + Phase 1e (in-Hub Creator
> UI) are deferred — the bar for "is the cascade pulling its weight"
> is now real-user usage data on tier 1.

## Why

The local model (Qwen3-4B-Q4_K_M) plateaued on multi-step
desktop-control chains after four prompt-engineering iterations.
Per the deep-research recommendation, the highest-leverage fix is
**recipe replay for the 80% common case** — pre-recorded
parameterized workflows that bypass LLM planning. The Discord
send-message benchmark collapses from 7–10 LLM turns to 0–1 (slot
extraction only) on the recipe path.

This is the most important single piece of the M8.3 structural
refactor. PR #67 (KV cache reuse) and the next session's prompt
restructuring fix the LLM-planned path; recipes fix the
fallback-to-replay path.

## Direction set at end of 2026-05-25 session

Format and product decisions made by the user (with research
backing):

1. **Build inside Lashon as the reference implementation** — not
   a separate-repo standalone project. The recipe format is
   designed to be portable from day one (so it could extract to a
   spec later), but v1 lives in `packages/shared-rust/src/recipes/`.
2. **Adopt the extend-don't-invent stack from the research:**
   - `SKILL.md` frontmatter envelope (Anthropic Agent Skills
     standard — Apache-2.0, supported by 32+ tools in 2026).
   - `recipe.yaml` with Goose-compatible `parameters:` schema
     (Apache-2.0 under Linux Foundation's Agentic AI Foundation).
   - **Lashon-specific `os_steps:`** block for the OS-UI
     primitives nobody else has standardized (key chord, type
     Unicode, click label, focus window, screenshot, clipboard).
3. **Windows-first runtime.** macOS / Linux adapters deferred (no
   test hardware on the dev machine; will use a VM for Ubuntu
   later).
4. **Marketplace = Git repo convention** (no web app yet).
   `lashon-recipes` GitHub org with `recipe.yaml` per directory.
5. **Hub integration in scope** — browser for installed recipes,
   one-click run, slot-fill UI.
6. **Creator UI in scope** — use Claude or GPT (cloud LLM, opt-in
   via existing Hub provider config) to author a recipe from a
   natural-language description. The cloud LLM never runs the
   recipe — it only generates the YAML.
7. **Naming: "Lashon Recipes"** externally, `recipe.yaml` as the
   file name (Goose-compatible). The "Scrolls / Glyphs" Hebrew-
   branded alternatives were considered and deferred.
8. **Sigstore / signing / sandboxing / permissions: M11+ stretch.**
   v1 recipes are user-trusted (same as shell scripts they
   downloaded).

## Scope (Phase 1 — this milestone)

### In scope
- **Recipe spec.** `SKILL.md` + `recipe.yaml` (Goose-compatible
  parameters + Lashon `os_steps:`) + optional `assets/`. Published
  as JSON Schema in `packages/shared-rust/src/recipes/schema/`.
- **Recipe runtime.** Rust executor in `lashon-core::recipes::`.
  Per-step type → native call (clipboard, key chord, type
  Unicode), UIA call (focus window, click label), or shell-out.
  No MCP IPC on the hot path.
- **Intent cascade for recipe matching:**
  1. Regex (~10 ms) — fastest deterministic match
  2. Embedding match (~50 ms, multilingual-E5-small ~120 MB) —
     paraphrase tolerance
  3. LLM classifier (~300 ms, local Qwen3-4B) — "which of these
     10 recipes fits, or none?" Just the recipe descriptions, no
     planning.
  4. LLM full planner (~3–5 s/turn, existing M8 dispatcher) —
     last resort when no recipe matches.
- **Hub: Recipe browser tab.** Lists installed recipes from
  `~/.config/lashon/recipes/` and `<bundled>/recipes/`. Shows
  name + description + parameter schema. "Run" button opens slot-
  fill modal.
- **Hub: Recipe creator UI.** Natural-language prompt → cloud
  LLM (opt-in, uses existing Hub provider config) → generated
  YAML preview → user-approves → saved to local recipes dir.
- **10 starter recipes** (proposed; refine in session):
  1. Send Discord message to recipient X with body Y
  2. Send Slack message to recipient X with body Y
  3. Send Telegram message to recipient X with body Y
  4. Send WhatsApp message to contact X with body Y
  5. Open app and focus (any app by name)
  6. Search Spotify for query and play first result
  7. Take screenshot of region to clipboard
  8. Batch-rename files in directory matching pattern
  9. Browser: search query and open first result
  10. Translate clipboard contents to target language (uses
      Hub-configured LLM)
- **OCR fallback** flagged but not implemented in Phase 1 — the
  research warned this is needed for Skia/Flutter/Electron apps
  with no a11y info. Add `--fallback ocr` knob in spec; ship the
  implementation in Phase 2.
- **`rtl_safe: true` flag** on `type_unicode` steps for Hebrew
  in Electron apps that mangle synthetic BiDi — triggers
  clipboard-paste instead of synthetic key events.
- **Dispatcher integration.** Command-mode dispatcher gains a
  pre-LLM intent-cascade pass; if a recipe matches, it runs the
  recipe instead of invoking the LLM planner.

### Out of scope (explicitly)
- macOS adapter (no test hardware)
- Linux adapter (deferred to VM testing later)
- Marketplace web app (Git-repo convention only in v1)
- Sigstore signing + permission manifests
- Sandboxing / first-use approval UI
- Vision-language model OCR fallback (research recommends, but
  Phase 2)
- Record-by-demonstration UI (OpenAdapt-style; future)
- **Lashon as MCP client** (connecting outbound to other people's
  MCP servers from inside a recipe — separate ADR if pursued)

## Phase 1g — Lashon as MCP server (~1.5 weeks)

**Added end of 2026-05-25 session.** Bigger architectural piece —
listed last because it's the most disjoint from the
recipe-runtime work; can run in parallel with the rest of M9.

### Why

The Hub creator UI (Phase 1e) authors recipes by calling a cloud
LLM directly. That works for the immediate ask, but the **MCP
server route is structurally better** for the same goal:

- Claude Desktop / Cursor / GPT-via-MCP-client / any 2026-era
  agent host can connect to Lashon and **see Lashon's own tool
  catalog as MCP tools.** The user asks Claude Desktop "help me
  write a Lashon recipe that opens YouTube Music and plays a
  song" — Claude calls `lashon.read_active_window` to see state,
  `lashon.get_recipe('open_app_and_focus')` to see the template,
  then drafts a `recipe.yaml` and saves it via
  `lashon.save_recipe`.
- It validates the **recipe-format-as-portable-spec** direction:
  if Claude Desktop can author Lashon recipes through MCP, the
  format is provably portable beyond Lashon's runtime.
- It makes Lashon's tools available to other agent systems
  generally — Lashon becomes an MCP server that exposes "Hebrew
  voice + Windows desktop control" to any client.

### Scope

**Crate:** `rmcp` (official Anthropic Rust SDK for MCP). Pin
exact version.

**Transport:**
- **stdio** — primary. Ships as a separate binary
  `lashon-mcp.exe` that Claude Desktop spawns. The user adds an
  entry to `claude_desktop_config.json` and Claude has Lashon
  tools.
- **HTTP/SSE** — optional second binding inside the main Lashon
  process so HTTP-MCP clients (Cursor, etc.) can connect.
  Loopback-only, same security posture as the STT sidecar's
  loopback transport.

**Tools exposed (always-on safe set):**

| Tool | Source |
|---|---|
| `lashon.read_active_window` | reuse `tools/read_active_window_text.rs` |
| `lashon.read_screen` | reuse `tools/read_screen.rs` |
| `lashon.list_open_windows` | reuse `tools/list_open_windows.rs` |
| `lashon.list_files` | reuse `tools/list_files.rs` (path-gated) |
| `lashon.file_read` | reuse `tools/file_read.rs` (path-gated) |
| `lashon.list_processes` | reuse `tools/list_processes.rs` |
| `lashon.clipboard_get` | reuse `tools/clipboard.rs` |
| `lashon.list_recipes` | new — installed recipes from `~/.config/lashon/recipes/` |
| `lashon.get_recipe(name)` | new — full YAML for a named recipe |
| `lashon.validate_recipe(yaml)` | new — runs schema validator from Phase 1a |
| `lashon.save_recipe(name, yaml)` | new — saves to per-user recipes dir; gated by confirmation modal |
| `lashon.list_recipe_step_types` | new — what step types are available + their schemas |

**Tools NOT exposed by default (opt-in toggle in Hub):**

- All interactive tools: `click_element`, `right_click`,
  `double_click`, `drag`, `scroll`, `press_keys`, `type_text`.
- All destructive tools: `file_write`, `file_delete`,
  `file_move`, `run_command`, `kill_process`, `lock_screen`,
  `close_window`.

The user can opt into either category via the Hub. Even when
opted-in, the confirmation modal still gates every destructive
call — the existing M8 confirm infra works unchanged for MCP
callers.

**Hub UI:**
- New tab "MCP Server"
- Status pill: "Running on stdio + HTTP :NNNN" / "Stopped"
- Toggle: enable/disable
- Per-category exposure toggles (safe reads / interactive /
  destructive)
- "Connect Claude Desktop" button — copies a config snippet to
  the clipboard for the user to paste into
  `claude_desktop_config.json`
- "Connection log" — last N MCP requests with tool name +
  duration (NOT arg values, per security rule)

**Security:**
- Confirmation modal applies to every destructive MCP call exactly
  as it does for Lashon's own dispatcher calls.
- Path safety guard applies to every `file_*` call.
- Loopback-only binding for HTTP transport.
- stdio transport is per-process — Claude Desktop's spawn is the
  trust boundary.
- The same `LASHON_DEBUG_TOOL_ARGS` flag controls arg-value
  logging for MCP requests.

**Files:**
- `packages/shared-rust/src/mcp/` (new module)
- `packages/shared-rust/src/mcp/server.rs`
- `packages/shared-rust/src/mcp/tool_bridge.rs` — adapter
  exposing existing `LashonTool` impls as MCP tools
- `packages/shared-rust/src/mcp/recipe_tools.rs` — the
  recipe-specific tools (list/get/validate/save)
- `apps/desktop/src-tauri/src/bin/lashon-mcp.rs` — stdio
  binary entry point
- `apps/desktop/src-tauri/src/mcp.rs` — HTTP-binding glue + Hub
  Tauri commands
- `apps/desktop/src/routes/hub/mcp/+page.svelte` — Hub tab
- ADR-0028 — Lashon as MCP server (transport choice, tool
  surface, security model)

### Effort breakdown

- `rmcp` integration + server skeleton: 2 days
- Adapter from existing `LashonTool` to MCP tools + safe-set
  exposure: 2 days
- Recipe-management MCP tools (list/get/validate/save): 2 days
- Hub MCP Server tab + status / toggles: 2 days
- stdio binary + Claude Desktop config snippet generator: 1 day
- Integration test: spawn `lashon-mcp.exe`, call
  `lashon.list_recipes` via JSON-RPC, assert response: 1 day

**Total: ~1.5 person-weeks.** Brings M9 to ~5 person-weeks
total.

### How this changes Phase 1e (Hub creator UI)

The Hub creator UI (Phase 1e, ~4 days) becomes optional / a
thin wrapper:

- If MCP server is running, the Hub creator UI can be a "Click
  to copy MCP config snippet for your AI assistant" button +
  some example prompts ("Try asking Claude: write me a Lashon
  recipe to send a message in Discord").
- The actual recipe-authoring intelligence lives in whatever
  agent client the user picked (Claude Desktop, Cursor, etc.) —
  Lashon doesn't have to ship a cloud-LLM integration just for
  this.

Decision for next session: **keep the in-Hub creator UI as a
fallback for users who don't want to install Claude Desktop /
configure MCP, but make the MCP path the recommended one.** The
in-Hub creator stays simple (small prompt → cloud LLM via
existing M7 provider config → YAML preview → save) and shrinks
to ~2 days.

### Defers / explicitly out of scope for Phase 1g

- **Lashon as MCP client** (outbound — calling other people's
  MCP servers from inside a recipe). Separate ADR; future
  capability. Recipes today are pure `os_steps:` chains; the
  decision of "can a recipe step call an MCP tool from another
  server?" can wait.
- **Discovery / registry of Lashon-exposed tools.** Phase 1g
  exposes a fixed set. A future PR could add user-configurable
  tool subsets per-MCP-client (e.g., "Claude Desktop gets safe
  reads + recipe tools; Cursor gets safe reads only").
- **Signed MCP requests / per-client auth.** Same as recipe
  signing — M11+ stretch.

## Phased breakdown

### Phase 1a — Schema + validator (~3 days)
- Define JSON Schema for `recipe.yaml` (Goose-compatible
  parameters + Lashon `os_steps:` per OS)
- `lashon_core::recipes::schema` module with `serde` deserialisers
- CLI validator: `lashon validate-recipe <path>`
- Unit tests covering the 10 starter recipe shapes

### Phase 1b — Runtime executor (~1 week)
- `lashon_core::recipes::runtime` — step types: `key_chord`,
  `type_unicode`, `click_label`, `focus_window`, `wait_for_window`,
  `screenshot_to_clipboard`, `run_shell` (gated)
- Per-OS adapters (Windows only Phase 1)
- Slot interpolation — `{{ recipient }}`, `{{ body }}`
- `rtl_safe: true` triggers clipboard-paste path
- Reuse existing `tools/path_safety.rs` for any file operations
- Integration test: 4 destructive flows (file write, shell, etc.)
  through the runtime under `AlwaysAllow` and `AlwaysDeny`

### Phase 1c — Intent cascade + dispatcher integration (~3 days)
- `lashon_core::recipes::intent` — regex + embedding + LLM-
  classifier
- Add pre-LLM intent-cascade pass to `command_mode::dispatch`
- Cascade short-circuits to recipe runtime on match; falls
  through to LLM planner on no match
- Tests: each cascade tier in isolation + full cascade with mock
  recipes

### Phase 1d — Hub UI: browser + run modal (~3 days)
- New Hub tab: "Recipes"
- Lists installed recipes from per-user + bundled dirs
- "Run" button → slot-fill modal → dispatches via existing
  Tauri command bridge
- Hot-reload on directory change

### Phase 1e — Hub UI: creator (~4 days)
- New "Create recipe" button in the Recipes tab
- Natural-language input + cloud-provider picker (re-uses M7
  provider config — opt-in cloud, never default)
- LLM generates YAML, validator runs, preview shown to user
- User approves → saved to `~/.config/lashon/recipes/`
- Generated recipes are user-trusted (same model as the user
  authoring by hand)

### Phase 1f — Starter recipe library (~3–4 days)
- Author the 10 starter recipes by hand
- Test each end-to-end on Windows
- Document recipe authoring conventions in
  `docs/recipes-authoring.md`
- Optional: publish `lashon-recipes` GitHub org with the 10
  starters

**Subtotal Phases 1a–1f: ~3.5 person-weeks.** Plus Phase 1g
(MCP server) ~1.5 weeks = **~5 person-weeks total for M9.**
Phase 1g is the most parallelizable — assign to a separate
work-stream if multiple people are on it.

## Open questions — resolutions

Most of these landed with decisions baked into shipped code; left
here as a record of how the project answered them.

1. **Where does the intent cascade live?** ✓ **Pre-pass** in
   `command_mode::dispatch` (PR6, in #81). Couples recipes to
   Command mode but lets the cascade short-circuit *before* the
   expensive Local-LLM spawn. ADR-0029 covers the rationale.
2. **Bundled recipes or downloaded?** ✓ **Bundled** with the
   installer at `recipes/starters/`. The MCP `save_recipe` path
   lets users author into the per-user dir; no on-first-run
   download flow shipped.
3. **Embedding model choice for intent cascade tier 2.** *Open* —
   tier 2 was deferred; the regex tier covers all current
   messaging-recipe cases (14 intent variants × 4 recipes).
   `multilingual-E5-small` remains the working default if tier 2
   is picked up.
4. **Recipe permission declarations.** ✓ **Spec'd as descriptive
   `permissions: [...]`** (ADR-0027). v1 enforcement is partial:
   the validator complains on `run_shell` without `shell.run`.
   Full sandboxing remains M11+.
5. **Versioning + upgrade.** ✓ **Bundled upgrades silently, user
   shadows bundled, user wins on id collision.** Documented in
   ADR-0027 + implemented in `recipes::storage`. No conflict UX
   shipped (designer agreed). Future "outdated copy" badge on
   user-shadowed recipes is a Phase 1d.5 polish.
6. **MCP server: stdio binary build path.** ✓ **`[[bin]]` in
   `lashon-core` under the `mcp-server` feature** (ADR-0028).
   Binary stays ~5 MB stripped because it doesn't link Tauri.
7. **MCP server: tool naming convention.** ✓ **Unprefixed
   snake_case** to match the internal dispatcher names
   (ADR-0028). The MCP client UI prefixes by server name anyway.
8. **MCP server: default-on or default-off?** ✓ **Default off.**
   Hub MCP Server tab (deferred Phase 1g UI half) is the opt-in
   surface.

## Deferred — picked up in a future milestone

- **Cascade tier 2 (embedding)** + **tier 3 (LLM classifier)** —
  the regex tier has 14 patterns per messaging recipe and
  proven coverage; reach for the LLM classifier when real-user
  data shows the regex misses. ADR for the embedding model
  choice will land then.
- **Phase 1e — in-Hub Creator UI** — the design existed but
  Claude Desktop via MCP `save_recipe` is the recommended
  authoring path (proven this session). The in-Hub creator can
  ship later if non-MCP users ask for it.
- **Phase 1f — `lashon-recipes` GitHub org** — 10 starters
  ship bundled; the marketplace-org idea waits until there's an
  outside-Lashon authoring community.
- **Phase 1g Hub MCP Server tab** — status pill, toggles,
  Claude-Desktop snippet generator. Users currently wire MCP by
  hand-editing `claude_desktop_config.json` per the manual in
  `docs/stories/m9-mcp-server.md`.
- **`wait_for_focus_change` for Electron** — the step type
  ships (PR #81) but Electron's UIA opacity defeats it.
  Useful for future native-Win32 recipes (Notepad, File
  Explorer, settings dialogs); revisit when those recipes
  appear.

## Definition of Done (M9)

- Schema published as JSON Schema in the repo, with validator CLI
- Runtime executes all 10 starter recipes end-to-end on Windows
- MCP server (Phase 1g) spawns via `lashon-mcp.exe`, Claude
  Desktop connects, can call `lashon.list_recipes` +
  `lashon.read_active_window` + `lashon.save_recipe`, end-to-end
- ADR-0028 lands documenting the MCP server's transport, tool
  surface, security posture
- Intent cascade routes >70% of a hand-labeled 50-command Hebrew
  + English test set to recipes (LLM planner falls through on
  the rest)
- Hub Recipes tab lists + runs recipes via slot-fill modal
- Hub Creator UI generates a working recipe from a Hebrew
  natural-language description, validator-clean, user-approved,
  saved
- `cargo test --workspace --no-fail-fast` green
- `cargo check --workspace --all-targets` clean
- `npm run check` clean
- ADR-0027 written documenting the format-extension decisions
  (SKILL.md envelope + Goose params + Lashon os_steps)
- CLAUDE.md branch-summary paragraph for whichever PR(s) land
  the work (this may be 2–3 PRs given the scope)

## References

- The motivating research (Command-mode brief, the OS-action-marketplace
  survey, the session handoff) is kept in the project's internal notes,
  off-repo.
- Anthropic Agent Skills: https://agentskills.io
- Goose Recipes: https://github.com/aaif-goose/goose (formerly
  block/goose; donated to AAIF / Linux Foundation Dec 2025)
- MCP spec: https://modelcontextprotocol.io
