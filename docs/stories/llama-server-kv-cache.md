# llama-server KV cache reuse

> **Status: shipped on `main` in PR #67.** Tiny config change with
> outsized effect. The empirical follow-up that measures the savings
> (slot-stats forwarding to Lashon's tracing) shipped in PR #71.

## Why

The M8.2 dispatch sends a stable, repeated prefix to the local LLM
every turn:

- ~5 K-token system prompt (identity, behaviour rules, worked
  examples, messaging-app playbooks)
- ~600-token tool-catalogue schema

…plus a growing conversation tail (tool results, intermediate
assistant turns). A Discord send-message chain runs 7–10 turns. On
turn 0 the model prefills the whole prompt — call it ~6 K tokens.
On turns 1–9 the dispatcher sends *the same prefix again*, plus the
new turn's tail.

Without prefix caching, llama-server re-prefills the entire stack
every turn — ~50 K cumulative prefilled tokens for a 10-turn chain
where ~40 K of those are *the same prefix*. At 200 tok/s prefill
speed that's ~250 seconds of avoidable prefill work per chain.

llama.cpp's `--cache-reuse N` (with the `/slots` endpoint surfacing
stats) reuses any prefix ≥ N tokens that matches a prior request.
The system prompt + tool catalogue is a static ~5–6 K-token prefix
across every turn of a chain — the cache will reuse multi-K-token
matches, far above the 256-token floor we set. Expected reduction
in turn-2+ prefill cost: **60–90%**, comparable to Anthropic's
prompt-caching savings.

## What this PR does

Adds two flags to the `llama-server` spawn args in
`packages/shared-rust/src/llama_server.rs`:

- `--cache-reuse 256` — enables intra-slot prefix reuse for any
  continuation sharing ≥ 256 tokens with a prior request. 256 is
  the floor (most matches in Lashon's chain will be multiple K
  tokens); the flag does not cap the actual reuse length.
- `--slots` — exposes the `/slots` REST endpoint so a future PR
  can save / load slot KV state across app restarts (warmup on
  first chat) and inspect slot-reuse stats during debugging. No
  functional change today beyond endpoint availability.

Plus a tracing-log field documenting the cache config so a quick
`grep cache_reuse` in the logs confirms activation on launch.

## What this PR does NOT do

Out of scope, in priority order for future PRs:

1. **Prompt restructuring for cache stability.** Today the
   dispatcher interleaves dynamic content into the message list;
   for maximum cache hit rate, *all* dynamic content (tool results,
   working memory) must live at the tail and the prefix must be
   strictly stable. Worth a separate measurement-driven PR.
2. **Slot-save-path persistence across app restarts.** `--slots`
   alone exposes the endpoint; a follow-up PR can call
   `/slots/<id>?action=save` on shutdown and `/slots/<id>?action=restore`
   on next launch for a warm-cold-start.
3. **Cache-reuse measurement.** llama-server logs slot reuse stats
   when `--slots` is enabled; surface these in Lashon's tracing
   output as a new structured field per chat call so we can prove
   the savings empirically.
4. **`--cram N` host-memory cache.** Lets the host RAM hold cached
   prefix KV beyond what VRAM holds; load to active KV on next
   match. Useful when running multiple GGUF-format models in
   sequence (Command + Chat modes with different defaults).

These are all addressed in the broader command-mode research brief (kept in the
project's internal notes, off-repo).

## Test plan

- `cargo test -p lashon-core --features local-llm --lib llama_server` — 3 tests, all pass; new flags don't affect existing test surface.
- `cargo test -p lashon-core --features local-llm --lib` — full suite, no regressions.
- `cargo check -p lashon --all-targets` clean.
- `npm run check` clean.
- Manual: restart Lashon, dictate a 2-turn Command-mode chain, observe llama-server log line `slot update_slots: n_past = N, n_tokens = M` where M ≪ N on turn 2 (cache hit). Pre-cache, M ≈ N.

## Definition of done

- Flags wired into the spawn args.
- Tracing log surfaces the cache config.
- All three CI runners green.
- Story doc committed.
- CLAUDE.md branch-summary paragraph updated.
