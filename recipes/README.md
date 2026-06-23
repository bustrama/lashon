# Lashon Recipes

Pre-recorded parameterised desktop workflows. Each recipe lives in its own
directory with a `recipe.yaml` that declares parameters and per-OS step
lists. The format is specified in
[`docs/stories/m9-recipes.md`](../docs/stories/m9-recipes.md) and parsed by
[`lashon_core::recipes`](../packages/shared-rust/src/recipes/mod.rs).

## Layout

```
recipes/
├── README.md                        # This file
├── schema/
│   └── lashon-recipe.schema.json    # Auto-derived JSON Schema; snapshot-tested
└── starters/                        # The 10 bundled starter recipes
    ├── <recipe-id>/
    │   └── recipe.yaml              # The recipe spec
    └── ...
```

## Authoring

The schema in `recipe.yaml` blends three formats — see the module docs at
[`packages/shared-rust/src/recipes/mod.rs`](../packages/shared-rust/src/recipes/mod.rs)
for the lineage. Validate authored recipes with the unit test:

```sh
cargo test -p lashon-core --lib recipes
```

The validator surfaces every issue at once (unknown interpolations, dangling
parameters, missing OS variants, missing `shell.run` permission for shell
steps) so a single edit/test loop is enough.

The recommended authoring path is via **Claude Desktop / Cursor / any
MCP host** connected to the `lashon-mcp` stdio server — the agent can
read existing recipes via `get_recipe`, validate drafts via
`validate_recipe`, and save via `save_recipe`. Wire-up:
[`docs/stories/m9-mcp-server.md`](../docs/stories/m9-mcp-server.md#manual-smoke-test-today).

## Running

From the CLI (testing surface; doesn't go through voice):

```sh
cargo run -p lashon-core --bin lashon-recipe -- --list
cargo run -p lashon-core --bin lashon-recipe -- send-discord-message --recipient=kuki --body=hi
```

From voice — speak any of the recipe's `intents:` phrases at the
Command-mode hotkey. The cascade (regex tier) matches and the
runtime executes deterministically in 0–1 LLM turns. The user's
`stt.word_aliases` settings are applied between STT and the cascade
to catch recurring misrecognitions (e.g. "claude → cloud") —
configure in Hub → Voice corrections.

## Status

**M9 Phases 1a, 1b, 1c (tier 1), 1d (incl. Steps panel), 1g
shipped on `main`.** The author→test loop works end-to-end:
write a recipe → list/run via the Hub or `lashon-recipe` CLI →
trigger by voice once an `intents:` phrase matches the transcript.

Deferred to future milestones:

- **Phase 1e — in-Hub Creator UI.** Claude Desktop via MCP
  `save_recipe` is the recommended authoring path; the in-Hub
  creator can ship later if non-MCP users ask for it.
- **Cascade tier 2 (embedding)** + **tier 3 (LLM classifier).**
  The regex tier has 14 patterns per messaging recipe and proven
  coverage on real Hebrew + English utterances.
- **Phase 1g Hub MCP Server tab.** Users currently wire MCP by
  hand-editing `claude_desktop_config.json` per the manual.
- **`lashon-recipes` GitHub org marketplace.** Wait until there's
  an authoring community outside Lashon.
