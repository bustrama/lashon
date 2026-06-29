# 35. Live streaming dictation via repeated unary re-decode + client-side LocalAgreement-2

## Status

Accepted — 2026-06-28. Supersedes the abandoned bidirectional-streaming spike
(`spike/local-streaming`, reverted in `8c3cad7`). Implements the live-partial
goal in [`docs/stories/streaming-dictation.md`](../stories/streaming-dictation.md).

> **Amended 2026-06-29 by [ADR-0037](0037-tail-only-windowed-redecode.md):** the
> whole-buffer re-decode below, and the 30 s take cap it implied, are replaced by
> a tail-only **windowed** re-decode that keeps cost bounded for takes of any
> length. Everything else in this ADR (repeated-unary transport, client-side
> LocalAgreement-2, single-flight, the sidecar decode lock, the language latch,
> the authoritative final) still stands.

## Context

Dictation today captures audio, transcribes **once on stop**, then injects. The
user watches a frozen mark for several seconds with no feedback. The goal is
**live partial transcripts** — words appearing as you speak, flicker-free —
while the final injected text stays byte-identical to today's accuracy.

faster-whisper is **not** a streaming model: it decodes a whole utterance at
once. There are two ways to fake live transcription on top of it:

1. **Bidirectional gRPC streaming** — a long-lived `tonic` ⇄ `grpcio` channel
   that streams audio up and partials down, the server sliding a window over the
   incoming audio.
2. **Repeated *unary* re-decode** — the worker already owns the full, growing
   audio buffer locally (`capture.samples_since`); every ~500 ms it fires a
   plain unary `TranscribeBytes(buffer_so_far)` and folds the result into a
   client-side committer.

A prior spike built option 1 end-to-end and it **deadlocked on Windows**:
`client.transcribe_stream().await` hung forever, no error, the `grpcio`
servicer never even entered — a `tonic`+Windows bidi-stream interaction. The
algorithm itself was proven sound on that spike (partials refined smoothly
toward the final; on a Tier-A GPU the last partial landed ~real-time), so the
blocker was purely transport.

A second, quieter hazard surfaced while wiring option 2: the sidecar's gRPC
server runs a `ThreadPoolExecutor(max_workers=4)` and calls `engine.transcribe`
with **no lock**. Today only one decode is ever in flight, so it has never been
exercised concurrently — but streaming adds a second concurrent caller (an
in-flight re-decode overlapping the final-on-stop), and faster-whisper's
`WhisperModel` is not guaranteed thread-safe.

## Decision

### 1. Drive partials with repeated unary `TranscribeBytes`, not bidi streaming

Every ~500 ms of new audio, the dictation worker snapshots the growing buffer
and fires a plain unary `transcribe_bytes(buffer_so_far)` on a background task.
This **reuses the proven, working RPC** and completely sidesteps the
`tonic`+Windows bidi deadlock. Re-decode cost is identical to the bidi
sliding-window — the model re-reads the same audio either way — so nothing is
lost on latency. The spike's `TranscribeStream` sliding-window server code is
abandoned for v1 (the M1 batch shim stays in `server.py`, unused on the hot
path).

### 2. LocalAgreement-2 runs client-side, in Rust

Successive unary hypotheses revise their tail as the model hears more audio;
shown verbatim that tail flickers. The **LocalAgreement-2** policy (CUNI-KIT,
IWSLT-2022) commits a word only once it has appeared, in the same position, in
**two** consecutive hypotheses, and a committed word never changes again. This
is pure logic — no transport, no audio — so it lives in `lashon-core`
(`local_agreement.rs`, 7 tests) and folds the hypothesis stream into a stable
`(committed, provisional)` split. It is Hebrew/RTL-safe by construction: it only
splits on and rejoins whitespace, never reordering tokens or touching combining
marks (niqqud).

### 3. Decode runs off the capture thread, single-flight

The worker thread owns the `!Send` audio stream and wakes every 50 ms for
level/VAD; a synchronous decode there would stall capture (badly on CPU). Each
re-decode runs on a separate `tauri::async_runtime::spawn` task with a shared
`Arc<FasterWhisperProvider>` (transcribe takes `&self`), guarded by an
`AtomicBool` so **only one decode runs at a time** — a slow decode lowers the
partial rate instead of piling work behind the capture thread. The gate (min
1 s of audio before the first decode, then one decode per 0.5 s hop), the
single-flight rule, and the language latch are pure policy and live in
`lashon-core::streaming` (`DecodeScheduler`, `LanguageLatch`; 10 tests, Hebrew +
mixed-script).

### 4. Serialize decodes in the sidecar

A `threading.Lock` now wraps every `engine.transcribe` call in `server.py`, so
an in-flight streaming re-decode and the final-on-stop can never touch the
non-thread-safe model concurrently. Contention is rare (only that one overlap)
and bounded by a single decode; correctness is not worth trading for it. This is
a root-cause fix for a latent hazard the server always had under concurrent
calls — streaming is merely the first caller to trigger it.

### 5. Language is latched after the first detect

The first re-decode passes `""` — the companion-model autodetect
([ADR-0009](0009-language-detection-via-a-companion-model.md)). The detected
code is latched and forced on every later re-decode **and the final**, so the
detector never reruns mid-utterance and a noisy chunk can't flip the language.
When no streaming decode ever latched a language (a sub-second take, or a
self-disabled machine — see below), the final falls back to `""`, keeping it
byte-identical to today's one-shot path.

### 6. The final decode stays authoritative; partials are additive

On stop the worker still runs the full final `transcribe` on the trimmed take
and **injects its raw text** — the committer's rejoined display text is never
injected. The committer's `finalize` only settles the on-screen preview. All
existing events (`dictation:state`, `dictation:level`, `dictation:transcript`)
are untouched; `dictation:partial` (the `(committed, provisional)` `Preview`) is
purely additive.

### 7. Interim: live text grows the Tongue

The design system's "the Tongue never changes shape" rule is **relaxed for v1**:
during a take the Tongue grows a text panel (committed solid, provisional muted,
`dir="auto"`, reduced-motion-aware, committed text announced politely via an
ARIA-live region) and collapses on idle. This is a **deliberate, temporary
exception**; if the UX proves out, a proper `docs/design-system.md` redesign
(the alternative was a dedicated companion window) follows.

## The benchmark that set the cadence

`scripts/stream-test.py` (ported from the spike, with a `--cpu` flag) re-decodes
the growing buffer on **this machine** with the shipped
`whisper-large-v3-turbo-ct2` model, on 7.44 s of Hebrew:

| Device | Steady-state re-decode | End-to-end (`--realtime`) | Verdict |
|---|---|---|---|
| **CUDA, RTX 4080** (`int8_float16`) | **~130–155 ms** (first decode 969 ms — one-time CUDA warm-up) | last partial at **7.59 s for 7.44 s audio** — 1.02× real-time, ~0.15 s behind speech | streaming is viable with comfortable headroom under the 500 ms hop |
| **CPU** (`int8`) | **~12.2 s**, ~23× real-time | n/a | non-viable for live re-decode |

The CPU cost is **~constant in buffer length** (11.9 s at a 1 s buffer → 12.5 s
at 7 s): faster-whisper processes a fixed 30 s mel window, so a re-decode costs
roughly the same regardless of how much audio is buffered. At 500 ms cadence a
CPU worker would fall progressively further behind and never catch up.

This constant-cost regime holds **only while the buffer fits one 30 s mel
window**. Past that a whole-buffer re-decode windows the audio sequentially and
grows with the take — which is exactly why this ADR capped a take at 30 s, and
why [ADR-0037](0037-tail-only-windowed-redecode.md) switches to re-decoding only
the uncommitted tail.

**Cadence chosen:** 1 s min-sample gate, 0.5 s re-decode hop — the spike's
numbers, reconfirmed here.

### Graceful degradation on slow machines

Rather than a hardcoded device check, the `DecodeScheduler` measures real decode
latency: a decode that overruns a 2.5 s budget proves the machine can't sustain
live partials, so streaming **self-disables for the session** and takes keep
only their final decode — today's one-shot behaviour. The threshold sits well
above a slow GPU and the one-time ~1 s CUDA warm-up, so only a genuinely
non-viable machine (CPU at ~12 s) trips it. The first take on such a machine
eats one wasted re-decode before the latch engages; every take after is clean.
A future refinement could downgrade to phrase-level pseudo-streaming (one decode
per VAD pause) instead of disabling outright; v1 disables, which is exactly
today's UX on those machines.

## Consequences

- **Live partials on GPU, flicker-free.** Committed words never rewrite;
  the provisional tail refines toward the final.
- **No new transport.** The bidi RPC that deadlocked on Windows is not used; the
  hot path is the same unary `TranscribeBytes` dictation already shipped.
- **Final text unchanged.** Injection still uses the raw final transcript; the
  streaming layer cannot alter what gets typed.
- **The sidecar is now concurrency-safe** under overlapping decodes — a latent
  bug fixed, independent of streaming.
- **CPU users are no worse off than today** — streaming self-disables and the
  one-shot path remains.
- **The turbo→large-v3 model swap stays separate.** The streaming driver is
  model-agnostic; for streaming latency, turbo's speed is an advantage. The
  accuracy swap (WER bench, manifests, CUDA pinning) is its own decision.
- **Design-system debt recorded.** The Tongue-grows-a-panel exception is
  interim and must be revisited.

## Alternatives considered

- **Bidirectional gRPC streaming (the spike).** Rejected: deadlocks on
  `tonic`+Windows (`8c3cad7`). The algorithm was sound; only the transport
  failed. Repeated unary gets the same partials over a transport that works.
- **Server-side commit policy.** Rejected: LocalAgreement-2 is pure logic with
  no need for audio or model state, so it belongs in the tested `lashon-core`
  crate, not the Python sidecar — keeping the sidecar a thin decode service.
- **A dedicated companion window for live text.** Deferred: heavier UX change;
  the interim Tongue-grows-a-panel approach ships the feature now and a proper
  design-system pass can supersede it if it proves out.
