# Story: live streaming dictation (partial transcripts)

**Branch:** `feat/streaming-dictation`
**Status:** wired end-to-end — benchmark, worker driver, sidecar lock, frontend, and
[ADR-0035](../adr/0035-streaming-dictation-via-repeated-unary.md) landed; `cargo
test --workspace` green. Pending live smoke-test on a real install.
**Tracking issue:** "Live streaming dictation: chunked LocalAgreement-2 over ivrit-ai Whisper" (GitHub, bustrama/lashon).

## Goal

Dictation today captures audio, transcribes **once on stop**, then injects — the
user watches a frozen mark for several seconds with no feedback. Ship **live
partial transcripts**: words appear as you speak, flicker-free, with the final
injected text unchanged from today's accuracy.

## The decisive prior art — read this first

A previous spike (`spike/local-streaming`, three `spike:` commits, May 2026)
already proved the hard parts and surfaced the real blocker:

- **The algorithm works.** `scripts/stream-test.py` (on that branch) re-decodes
  the growing buffer every 500 ms against the live engine. On Tier-A CUDA, 7.44 s
  of Hebrew had its last partial land at 7.61 s wall-clock (~real-time), 148 ms
  avg decode across 15 re-decodes; partials refine naturally toward the final.
- **It was reverted for transport, not latency or quality.** The end-to-end wiring
  used a **bidirectional** gRPC stream (`tonic` client ⇄ `grpcio` Python servicer)
  that **deadlocks on Windows**: `client.transcribe_stream().await` hangs forever,
  no error, the servicer is never even entered. See commit `8c3cad7`.

## Architecture decisions (locked for v1)

1. **Drive partials with repeated *unary* `TranscribeBytes`, not bidi streaming.**
   The dictation worker already owns the full, growing audio buffer locally
   (`capture.samples_since`). It does not need to stream audio *up* a long-lived
   bidi channel. Every ~500 ms of new audio, fire a unary `transcribe_bytes(buffer_so_far)`
   on a background task and feed the result into the committer. This reuses the
   proven, working RPC and **completely sidesteps the tonic+Windows bidi deadlock**
   that killed the spike. Re-decode cost is identical to the bidi sliding-window.
   The spike's sliding-window `TranscribeStream` server code is abandoned for v1
   (leave the M1 batch-shim in `server.py` as-is; it is unused on the hot path).

2. **LocalAgreement-2 runs client-side in Rust.** Successive unary hypotheses
   revise their tail; the committer folds them into a stable `(committed,
   provisional)` split so committed words never flicker. Pure logic, no transport.
   **Done** — `packages/shared-rust/src/local_agreement.rs`, 7 tests green
   (Hebrew, code-switching, niqqud, flicker, whitespace, empty).

3. **Live text renders in the Tongue (interim).** Chosen option (b): relax the
   `.claude/rules/frontend.md` "never changes shape" rule for now and let the
   Tongue grow a text panel during a take. If the UX proves out, do a proper
   `docs/design-system.md` redesign then (a dedicated companion window was the
   alternative). Flag this as a temporary, deliberate exception in the ADR.

4. **Decode runs off the capture thread.** The worker thread owns the `!Send`
   audio stream and wakes every 50 ms for level/VAD; a synchronous decode there
   would stall capture (especially on CPU). Run each re-decode on a separate
   task/thread, single-flight (skip a tick if a decode is still in flight), and
   feed the result back. Details below.

## Current state (branch `feat/streaming-dictation`)

- ✅ `packages/shared-rust/src/local_agreement.rs` — `LocalAgreement` committer +
  `Preview { committed, provisional }` (serde-serializable for the Tauri event).
  7 unit tests, all green: `cargo test -p lashon-core --lib local_agreement`.
- ✅ `packages/shared-rust/src/streaming.rs` — `DecodeScheduler` (min-gate, hop
  cadence, single-flight, measured self-disable) + `LanguageLatch`. 10 unit
  tests, Hebrew + mixed-script: `cargo test -p lashon-core --lib streaming`.
- ✅ `packages/shared-rust/src/lib.rs` — `local_agreement` + `streaming` declared.
- ✅ `apps/desktop/src-tauri/src/dictation.rs` — `Streamer` drives single-flight
  off-thread re-decodes, emits `dictation:partial`, latches the language, settles
  the preview on the final, injects raw final text. Both editions compile.
- ✅ `services/stt-sidecar/src/lashon_stt/server.py` — `engine.transcribe`
  serialized behind a `threading.Lock` (concurrent-decode safety).
- ✅ Frontend — `+page.svelte` listens to `dictation:partial`; `Tongue.svelte`
  grows a panel (committed solid / provisional muted, `dir="auto"`,
  reduced-motion, polite ARIA-live), collapses on idle. `npm run check` clean.
- ✅ `scripts/stream-test.py` — ported with a `--cpu` flag; benchmark recorded
  below and in ADR-0035.
- ✅ [ADR-0035](../adr/0035-streaming-dictation-via-repeated-unary.md) written.

## Remaining work

### 1. Streaming driver in the dictation worker (`apps/desktop/src-tauri/src/dictation.rs`)

The `listen` hands-free loop already wakes every `LEVEL_INTERVAL` (50 ms) and has
the growing buffer via `capture.samples_since`. Add streaming around it:

- **Single-flight off-thread decode.** Keep an `AtomicBool decode_in_flight` (or a
  `Option<JoinHandle>`/result-channel). Each ~500 ms tick, if not in flight and the
  buffer is past the min-sample gate, snapshot `capture.samples_since(0)` into an
  owned `Vec<f32>` (Send) and spawn a decode (`tauri::async_runtime::spawn` with a
  shareable provider/sidecar handle, or a dedicated decode thread + mpsc). On
  return, send the raw hypothesis text back to the worker.
- **Min-sample gate:** don't decode until ≥ ~1 s (16 000 samples) is buffered — tiny
  decodes are garbage. Then re-decode every ~500 ms of new audio.
- **Commit + emit:** the worker owns a `LocalAgreement`; on each hypothesis call
  `observe()` and `app.emit("dictation:partial", preview)`. (If the decode task
  emits directly, share the committer as `Arc<Mutex<LocalAgreement>>`; single-flight
  means no real contention.)
- **Language:** the first decode passes `""` (companion-detector autodetect,
  ADR-0009); capture the detected language and pass it **explicitly** to subsequent
  re-decodes and the final, so the detector model doesn't run on every chunk.
- **Finalize:** on stop (existing path), do the final full `transcribe`, call
  `la.finalize(final_text)`, emit a final `dictation:partial { is_final-ish }` or a
  terminal state, then **inject the raw final transcript** (not the committer's
  rejoined text — keep today's injection exactly). Reset the committer per take.
- Keep the existing `dictation:state`, `dictation:level`, `dictation:transcript`
  events intact. `dictation:partial` is additive.
- **Concurrency caveat:** confirm the sidecar client (`SidecarState` / `ready_sidecar`)
  is cheaply shareable across the decode task. If not, add an `Arc` handle or a
  small decode-thread that owns its own client. Don't serialize decodes *through*
  the capture thread.

### 2. Rust seam (`packages/shared-rust/src/stt.rs`)

- Consider a small helper on the provider or a free function that the worker calls
  for a single re-decode (it can just reuse `transcribe`). Keep the provider trait
  as the seam; don't bind the worker to faster-whisper directly. No bidi method.
- If any pure orchestration logic emerges (e.g. a "should I decode now" gate, the
  language-latch), put it in `lashon-core` with tests (`.claude/rules/rust.md`:
  tests live in lashon-core, never the Tauri crate).

### 3. Frontend (`apps/desktop/src/routes/+page.svelte` + Tongue components)

- Listen to `dictation:partial`; render `committed` solid and `provisional` muted.
- RTL-native: `dir="auto"` on the text container, logical CSS props, bidi-isolate
  mixed fragments (`.claude/rules/frontend.md`). Honour `prefers-reduced-motion`.
- Interim: grow the Tongue to show a text panel during a take, collapse on idle
  (option b). The spike's `+page.svelte` (commit `9bc0296`) did a 520×140 expand —
  reuse as a starting point but fit the current mark-centric visual language and
  the design tokens (no hardcoded colours).
- Announce partial/committed updates via an ARIA-live region (politely).

### 4. Latency benchmark (the issue's first task — do early)

- Resurrect/port `scripts/stream-test.py` from `spike/local-streaming` and run it on
  **this** machine to get the real number: re-decode latency of the *present* model
  (`models/stt/whisper-large-v3-turbo-ct2`) on a growing buffer (1/2/4/8/15 s), CPU
  and GPU if available. The venv is at `services/stt-sidecar/.venv`.
- This sets the re-decode cadence and confirms CPU viability. If CPU commit-latency
  is too high, the same architecture degrades cleanly to **phrase-level
  pseudo-streaming** (decode once per VAD pause) — no code-path change, just a
  longer cadence. Document the number in the ADR.

**Result (this machine, `whisper-large-v3-turbo-ct2`, 7.44 s Hebrew):**

| Device | Steady-state re-decode | End-to-end (`--realtime`) |
|---|---|---|
| CUDA, RTX 4080 (`int8_float16`) | ~130–155 ms (first 969 ms = one-time CUDA warm-up) | last partial at 7.59 s / 7.44 s audio — 1.02× real-time |
| CPU (`int8`) | ~12.2 s, ~23× real-time | non-viable |

CPU cost is ~constant in buffer length (30 s mel window), so it can never catch
up — streaming **self-disables** on it (measured latency > 2.5 s budget) and the
take keeps only its final decode. (That constant-cost holds while a re-decode
fits one 30 s mel window; tail-only windowing keeps it there for takes of any
length — [ADR-0037](../adr/0037-tail-only-windowed-redecode.md).) **Cadence
chosen:** 1 s gate, 0.5 s hop. Full analysis in
[ADR-0035](../adr/0035-streaming-dictation-via-repeated-unary.md).

### 5. Story/ADR/tests housekeeping (`.claude/rules/workflow.md`)

- Write an **ADR** for: repeated-unary-over-bidi (with the spike-deadlock rationale),
  client-side LocalAgreement-2, and the interim Tongue-shape exception. Next free
  number: **0035** (last is 0034).
- Keep `cargo test --workspace` green on all three runners. Add Hebrew + mixed-script
  cases for any new pure logic (`.claude/rules/hebrew.md`).
- Update this story as the work lands.

## Model / quality note (resolved — see ADR-0036)

The shipped model is `ivrit-ai/whisper-large-v3-turbo-ct2` (turbo). The non-turbo
`large-v3` swap recommended by #1 was held as a **separate decision** so it would not
block streaming. It has now been evaluated and **declined** — see
[ADR-0036](../adr/0036-keep-turbo-over-large-v3-for-hebrew-stt.md) and
[#9](https://github.com/bustrama/lashon/issues/9): on a Tier-A GPU `large-v3` gives only
a modest Hebrew-WER gain (`read` 23.9% → 22.2%) and is slightly *worse* on
code-switching, while decoding **4.6–6× slower** (775 ms–2.1 s vs 167–343 ms re-decode —
over the 500 ms hop and the streaming self-disable budget) for a near-doubled
(1.6 → 3.1 GB) download. Turbo's speed is exactly what keeps streaming viable. The
driver stays model-agnostic, so the conclusion is revisitable if streaming is dropped or
a clean-dictation (`studio`) corpus shows a larger gap. Reference weights:
https://huggingface.co/ivrit-ai/whisper-large-v3

## Risks / open questions

- Sidecar client shareability across the decode task (see §1 caveat).
- Re-decode load on CPU — bounded by single-flight + cadence; pseudo-streaming is the
  graceful floor.
- Interim Tongue-shape exception vs the design system — temporary, ADR-recorded.
- `transcribe_bytes` re-sends audio each tick over loopback — negligible bytes,
  and on long takes the re-decode now covers only the uncommitted tail (not the
  whole buffer), with the take bounded by a 5-minute backstop
  ([ADR-0037](../adr/0037-tail-only-windowed-redecode.md)).

## Definition of done

- Live partials render during a take, flicker-free (committed never rewrites).
- Final injected text is byte-identical to today's one-shot path.
- `cargo test --workspace` green on all three runners; Hebrew + mixed cases covered.
- ADR-0035 written; this story updated; benchmark number recorded.
