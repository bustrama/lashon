# llama-server slot-stats tracing

> **Status: shipped on `main` in PR #71.** Follow-up to `llama-server-kv-cache` (#67) — item
> 3 of that story's "What this PR does NOT do" list, picked up so we
> can measure #67's impact from Lashon's own logs instead of grepping
> subprocess output.

## Why

[`llama-server-kv-cache`](llama-server-kv-cache.md) (#67) enabled
`--cache-reuse 256` and predicted **60–90% reduction in turn-2+
prefill cost**. Confirming that prediction (and detecting any future
regression) required tailing llama-server's stdout for
`slot update_slots: id N | task M | … n_past = X, n_tokens = Y` lines
and doing arithmetic by hand. Workable for one debugging session,
useless as a permanent metric.

This PR pumps those lines into Lashon's `tracing` subscriber as a
structured INFO event with the reuse percentage already computed.
A live `RUST_LOG=info` Lashon session now surfaces:

```text
INFO lashon::llama_server::slot slot=0 task=12 prompt_tokens=5234 \
     prefilled=1734 cached=3500 reuse_pct="66.9" stream="stdout" \
     llama-server slot turn
```

per Command-mode turn. The reuse percentage answers `--cache-reuse`'s
effectiveness directly:

- Turn 0 (cold): `reuse_pct ≈ 0` — nothing cached yet.
- Turn 2+ steady state (working as designed): `reuse_pct ≥ 60`.
- A regression in cache stability (e.g. a future change interleaving
  dynamic content into the system prompt prefix) drops the percentage
  visibly — the metric becomes the alarm.

## What this PR does

1. **Drains llama-server stdout/stderr into Lashon's tracing.**
   `tokio::process::Command` was already piping both
   (`Stdio::piped()`), but nothing was reading them — a latent hang
   waiting to fire once the OS pipe buffer filled (~64 KB on Windows,
   reachable after thousands of turns). The forwarder eliminates the
   risk while making the data useful.
2. **Parses the slot-stats lines** with a four-field extractor
   (`id`, `task`, `n_past`, `n_tokens`) tolerant of llama.cpp's
   right-aligned numeric fields and reshuffled prefix decoration
   across releases.
3. **Computes cache reuse** as `n_past − n_tokens`, surfaces both the
   absolute count and the percentage.
4. **Routes the firehose to DEBUG.** Lines that don't match the slot
   pattern (startup banner, model load, tokenizer warmup) fall through
   to `lashon::llama_server` at DEBUG so the default INFO log stays
   focused on the metric. `RUST_LOG=lashon::llama_server=debug` opens
   the firehose when needed.

## What this PR does NOT do

- **No `/slots` HTTP poll.** A REST poll would give richer per-slot
  state (current task, idle/busy, prompt cached), but adds an HTTP
  round-trip per chat call and a new failure mode. The stdout
  forwarder is zero-overhead and surfaces the per-turn datum that
  actually matters for cache measurement.
- **No history accumulator.** Lashon's tracing is consumed by the
  user's `tail -f` (or the dev console). A Prometheus-style metric
  store would belong in a future observability ADR if anyone needs
  cross-session aggregates.
- **No structured event format.** The fields are tracing fields —
  human-readable in the formatted output, machine-readable to any
  JSON subscriber, but not a dedicated event type. If the recipes
  intent cascade (M9 Phase 1c) ends up needing the same metric, we
  can promote it to a named event then.

## Test plan

- `cargo test -p lashon-core --lib llama_server` — 8 tests (3
  existing + 5 new for the parser + reuse arithmetic), all pass.
- `cargo test -p lashon-core --lib` — full suite, no regressions
  (292 tests).
- `cargo check --workspace --all-targets` clean.
- Manual: restart Lashon, dictate a 2-turn Command-mode chain,
  observe two INFO events on `lashon::llama_server::slot`. Turn 0
  shows `reuse_pct ≈ 0`; turn 1 shows `reuse_pct ≥ 60`.

## Definition of done

- Forwarder wired into `llama_server::spawn`.
- Parser + reuse arithmetic unit-tested.
- All three CI runners green.
- Story doc committed (this file).
- CLAUDE.md branch-summary paragraph updated.
