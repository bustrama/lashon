# 26. Promote Qwen3-4B-Q4_K_M as the default local LLM

## Status

Accepted — landed on the `m8-os-tools` branch.

## Context

ADR-0025 shipped the in-process local LLM with Qwen3-1.7B-Q8_0 as the
default. Rationale at the time: smallest variant the official
`Qwen/Qwen3-1.7B-GGUF` repo publishes (~1.83 GB), fast on every
hardware tier, sufficient for the M8.1 tool catalogue (13 tools, a
short system prompt).

M8.2 — the OS-control tools tranche — grew the catalogue from 13 to
35 tools and the system prompt from ~5 K to ~15 K characters. The
chains the user actually wants to run also became longer: a typical
"send a message to X in Discord" is 7–10 turns end-to-end (open app
→ quick switcher → type contact → enter → wait for compose → type
body → enter to send). The 1.7B model could not reliably execute
these multi-turn chains:

- It collapsed multiple interactive steps into one turn without
  seeing results between them — typed the recipient name *before*
  opening the quick switcher, so the text landed in nothing.
- After being constrained to one interactive call per turn (the
  M8.2 dispatcher cap), it consistently stopped 2–3 turns short,
  selecting the recipient and declaring "I sent the message" without
  ever typing the message body or pressing Enter to send.
- Adding Discord/Slack/WhatsApp playbooks to the prompt did not
  meaningfully change the failure mode.

Four iterations of prompt engineering — the cap, the
"don't-claim-done-early" rule, the messaging-app playbooks, the
critical-rule reminder — got further each time but the chain still
broke before sending. This is a model capability ceiling, not a
prompt problem.

The constraint we will not relax: Lashon must stay free and
local-by-default. Switching the default to a cloud provider is not
on the table — it violates the product's core promise. Looking only
at locally-runnable, free, Apache-2.0/MIT models that handle 10+
turn tool chains, the next step up is the 4 B class.

## Decision

Promote **`qwen3-4b-q4_k_m`** to the default local LLM. The 1.7B
variant stays in the picker for users on very weak hardware who
prefer speed over accuracy.

| | Qwen3 1.7B Q8_0 (old default) | Qwen3 4B Q4_K_M (new default) |
|---|---|---|
| On disk | 1.83 GB | 2.5 GB |
| Warm RAM (16 K context) | ~2 GB | ~3.5 GB |
| Tool-use ability (subjective) | Drops 10+ turn chains | Completes 10+ turn chains |
| Hardware floor | Any 8 GB system (CPU works) | Any 8 GB system (CPU works, slow) |
| GPU acceleration | Any modern GPU via Vulkan | Any modern GPU via Vulkan |
| License | Apache-2.0 | Apache-2.0 |
| Cold-launch latency | ~2 s | ~3–4 s |

Both variants ship in the manifest with the 4B listed first so the
Hub renders it as the recommended pick.

## Why not bigger (7B / 8B / 14B)?

Considered. The 4B is the minimum that completes the chains we tested
without falling off. Going larger (7B Q4 ~4.5 GB on disk, 8B Q4 ~5
GB) would not meet "everyone can run it" — CPU-only users on 8 GB
systems would see token rates drop past usable. Once the
`hub-llm-fit-check` PR lands a hardware-graded picker, we can offer
larger models as opt-in upgrades for users with the headroom — but
the *default* belongs in the size class everyone can actually run.

## Why not a different family (Llama, Phi, Mistral, Gemma)?

The Qwen3 family was already vetted and bundled in ADR-0025 — the
model card calls out tool calling as a first-class capability, the
license is clean, and the upstream repo publishes the Q4_K_M quant
directly (no community mirror dependency). Cross-family swaps
(Llama 3.1, Phi-3, etc.) are a bigger blast radius for marginal
gain at the 4B size; the `hub-llm-fit-check` PR's curated catalog
is the right venue for that comparison.

## Migration

- New users: download the 4B GGUF on first Command/Chat use.
- Existing users (testers): their `settings.json` still points at
  `qwen3-1.7b-q8_0`. They keep that pick until they switch via Hub
  → Language models → Local. No forced migration — the user owns
  their setting.
- The first 1.7B install is not auto-removed; users can delete it
  manually if they want to reclaim ~1.83 GB.

## Consequences

- First-launch download bumps from 1.83 GB to 2.5 GB. Reasonable.
- Cold launch latency goes from ~2 s to ~3–4 s on a modern GPU. The
  M8.1 progress UX (the tongue's "thinking" indicator) already
  covers the latency window.
- KV cache memory at 16 K context goes from ~600 MB to ~900 MB. The
  GPU footprint stays well inside any modern dGPU's VRAM.
- The 1.7B variant stays installable as a no-cost option in the
  Hub picker. Documented in the manifest description as "for very
  weak hardware where the 4B is too slow; not recommended for
  multi-step chains".

## Notes

The cause-and-effect chain here matters for future readers:
**catalogue growth caused model insufficiency**. If we ever shrink
the catalogue back (e.g. by splitting Command and Chat modes into
separate prompts), the 1.7B may become viable again. The decision
to promote 4B is a response to the M8.2 prompt size, not a permanent
verdict on the 1.7B's capability.
