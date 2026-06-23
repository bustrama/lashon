# M9 Phase 1c — Intent cascade (v1, regex tier)

> **Status: shipped on `main` in PR #78.** The dispatcher
> integration (PR6 — voice → recipe cascade) landed in PR #81,
> so the Tauri shell now calls `try_recipe_cascade` before the
> LLM planner. Tier 1 (regex) only; tiers 2 + 3 deferred.

## What ships

The pre-LLM intent matcher that routes natural-language commands to a
recipe when one fits. The dispatcher's eventual call site:
[`try_recipe_cascade`](packages/shared-rust/src/recipes/cascade.rs) →
on `CommandRoute::Recipe`, return; on `CommandRoute::Planner`, fall
through to `command_mode::dispatch`. On a recipe match, the LLM full
planner is bypassed entirely (0–1 LLM turns vs the 7–10 the planner
would otherwise spend on a Discord-send chain).

### Tier 1 — Regex matcher (lands here)

[`RegexMatcher`](packages/shared-rust/src/recipes/intent.rs) converts
each `Recipe::intents` phrase into an anchored, case-insensitive
regex with `{slot}` tokens translated to non-greedy named captures:

| `intents:` phrase | Compiled regex |
|---|---|
| `"send {body} to {recipient} in discord"` | `(?i)^\s*send\s+(?P<body>.+?)\s+to\s+(?P<recipient>.+?)\s+in\s+discord\s*$` |
| `"lock the screen"` | `(?i)^\s*lock\s+the\s+screen\s*$` |
| `"שלח לדיסקורד ל{recipient} {body}"` | `(?i)^\s*שלח\s+לדיסקורד\s+ל(?P<recipient>.+?)\s+(?P<body>.+?)\s*$` |

Literal whitespace becomes `\s+` so the STT's "send  hi  to" vs
"send hi to" doesn't break the match. Trailing punctuation + outer
whitespace are trimmed before matching.

`regex-lite` (~80 KB) over the full `regex` crate (~500 KB): we don't
need Unicode classes (no `\p{L}` shortcuts), and Hebrew + English
literals work unchanged in both pattern and input.

### Cascade orchestrator

[`try_recipe_cascade`](packages/shared-rust/src/recipes/cascade.rs)
wraps match + execute in one helper the Tauri shell will call before
`command_mode::dispatch`. Returns a `CommandRoute`:

- `Recipe { recipe_id, tier, run }` — cascade matched and the runtime
  executed the recipe. Caller skips the planner.
- `Planner` — no match. Caller falls through to the LLM full planner
  unchanged.

A runtime error during execution (e.g. `AlwaysDeny` on a `run_shell`
step) is surfaced as `Err(RuntimeError)` so the caller can decide
whether to apologise to the user or retry via the planner — the
default Tauri-shell choice will be "apologise; user can re-issue
manually if they meant the shell command to run."

## What this PR does NOT do (deferred)

- **Tier 2 — Embedding matcher.** Needs `multilingual-E5-small`
  (~120 MB Tauri resource) + an inference path. Lands alongside the
  model bundling decision (open question 3 in
  `docs/stories/m9-recipes.md`).
- **Tier 3 — LLM classifier.** Reuses the local Qwen to pick the
  matching recipe and extract slot values in one structured-JSON
  response. Lands after we measure tier 1 hit rate on real
  transcripts and size the prompt-engineering cost.
- **Dispatcher integration.** This PR ships the cascade helper as
  a library function; the Tauri shell will wire it into the
  Command-mode dispatch path in a follow-up so this PR's diff stays
  focused on the matching primitive.
- **MCP `run_recipe` tool.** Adding the cascade as an MCP-callable
  surface (so Claude Desktop can author and run in one loop) is a
  natural follow-up on top of `lashon_core::mcp`.

## Test surface

- 12 unit tests in `recipes::intent::tests`:
  - Two-slot extraction (body + recipient)
  - Case-insensitive matching
  - Trailing punctuation handling
  - Recipe iteration ordering (first match wins)
  - Empty-slot rejection
  - Duplicate-slot-name rejection
  - Cascade default uses regex tier
  - **Hebrew intent phrase matches** with extracted Hebrew slot
    values — the headline cross-script test
  - Literal regex meta-character escaping
  - Whitespace collapse in literals
- 4 integration tests in `recipes::cascade::tests`:
  - Matched recipe runs via the runtime and returns `Recipe` route
  - Unmatched transcript returns `Planner` route
  - Shell recipe + `AlwaysDeny` surfaces the runtime error
  - Empty recipe list cleanly returns `Planner`

## Test plan

- [x] `cargo test -p lashon-core --lib recipes` — 42 passed (26 from
  Phase 1a/1b + 16 new for intent + cascade)
- [x] `cargo check --workspace --all-targets` clean
- [ ] CI green on all three runners
- [ ] Manual: write a recipe with an `intents:` phrase that matches
  a spoken transcript, run via the future dispatcher integration

## Definition of done

- `RegexMatcher` + `CascadeMatcher` + `try_recipe_cascade` land in
  `lashon_core::recipes`
- All 16 new tests green; no regressions in the 26 existing recipe
  tests
- Story doc committed (this file)
- CLAUDE.md branch-summary paragraph updated
- Dispatcher integration is a follow-up PR — out of scope here
