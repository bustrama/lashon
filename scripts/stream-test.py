"""Sliding-window streaming-STT benchmark against the local faster-whisper engine.

Replays a Hebrew WAV file and re-runs `FasterWhisperEngine.transcribe` on the
growing buffer between 500 ms hops. Each re-decode emits a "partial" — what the
live tongue preview would show. The final partial after the last slice is FINAL.

This is the latency benchmark the streaming-dictation story calls for (see
`docs/stories/streaming-dictation.md` §4 and ADR-0035): it measures the real
re-decode cost of the present model (`models/stt/whisper-large-v3-turbo-ct2`) on
a growing buffer, on **this** machine, GPU and CPU. The numbers set the
re-decode cadence and confirm CPU viability.

  * Are partials timely? (target: <1 s lag behind audio on a Tier-A GPU)
  * Do they refine smoothly, or do early hypotheses flicker as more context
    arrives? (LocalAgreement-2 in `lashon-core` removes the flicker; this script
    shows the raw, pre-committer hypotheses.)
  * Does the final text match a clean batch decode?

Usage (from the repo root, with the sidecar venv):

    # GPU (default device selection — CUDA if available, else CPU):
    services/stt-sidecar/.venv/Scripts/python scripts/stream-test.py \\
        tests/hebrew-corpus/read/read-001.wav

    # Force CPU, to measure the CPU floor:
    services/stt-sidecar/.venv/Scripts/python scripts/stream-test.py \\
        tests/hebrew-corpus/read/read-001.wav --cpu

Output:

    [audio= 0.5s, decode= 420ms, since_start= 0.92s] partial: "מרטלי"
    [audio= 1.0s, decode= 380ms, since_start= 1.38s] partial: "מרטלי השביע"
    ...
    [audio=END   , decode= 510ms, since_start= 7.21s] FINAL  : "<full>"

    --- summary ---
    audio duration : 6.40 s
    wall-clock     : 7.21 s   (1.13x real-time end-to-end)
    decodes        : 13
    avg decode     : 378 ms
    ...

Drop streaming (degrade to phrase-level pseudo-streaming) if the avg decode time
is comparable to or exceeds the hop interval (no headroom for live updates), or
if the partials reorder/flicker in a way the committer cannot absorb.
"""
from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

import av
import numpy as np

# Windows console defaults to cp1252; Hebrew partials need UTF-8 stdout.
for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8")


# Hop = how often we re-decode the growing buffer. 500 ms gives ~2 Hz partials,
# which feels live; lower hops cost more inference and rarely change what the
# model sees enough to matter. Tune if the benchmark justifies it.
HOP_MS = 500
SAMPLE_RATE = 16_000

# Min audio before the first decode. Sub-second decodes are garbage (the model
# has too little context) and waste a decode slot — the dictation worker gates
# on the same threshold (`MIN_DECODE_SAMPLES` in lashon-core).
MIN_DECODE_S = 1.0


def load_wav(path: Path) -> np.ndarray:
    """Decode any WAV (PCM or float) as 16 kHz mono float32 PCM in [-1, 1].

    Uses PyAV (already a faster-whisper dep) so we handle every WAV subtype
    the corpus contains, including IEEE float (format tag 3).
    """
    with av.open(str(path)) as container:
        stream = container.streams.audio[0]
        resampler = av.AudioResampler(format="flt", layout="mono", rate=SAMPLE_RATE)
        chunks: list[np.ndarray] = []
        for frame in container.decode(stream):
            for resampled in resampler.resample(frame):
                chunks.append(resampled.to_ndarray().flatten().astype(np.float32))
        # Drain the resampler.
        for resampled in resampler.resample(None):
            chunks.append(resampled.to_ndarray().flatten().astype(np.float32))
    return np.concatenate(chunks) if chunks else np.zeros(0, dtype=np.float32)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("wav", type=Path, help="Path to a 16 kHz mono WAV.")
    parser.add_argument(
        "--language",
        default="he",
        help="Force the language (default 'he' — skips detection, as the live "
             "driver does after the first decode latches it).",
    )
    parser.add_argument(
        "--hop-ms",
        type=int,
        default=HOP_MS,
        help=f"Audio per decode step (default {HOP_MS}).",
    )
    parser.add_argument(
        "--cpu",
        action="store_true",
        help="Force the CPU device (cpu_only) to measure the CPU floor. "
             "Default selection prefers CUDA when available.",
    )
    parser.add_argument(
        "--model",
        default=os.environ.get("LASHON_STT_MODEL_ID"),
        help="Transcription model id to benchmark (default: the shipped model, "
             "or $LASHON_STT_MODEL_ID). Pass the non-turbo large-v3 id to "
             "compare re-decode latency against turbo (see ADR-0036).",
    )
    parser.add_argument(
        "--realtime",
        action="store_true",
        help="Pace decodes to wall-clock (simulates a live mic). "
             "Off by default — decodes run back-to-back so you can see the "
             "model's raw throughput.",
    )
    args = parser.parse_args(argv)

    # Lazy import — the engine pulls in faster-whisper, ctranslate2, etc.
    sys.path.insert(
        0,
        str(Path(__file__).resolve().parent.parent / "services" / "stt-sidecar" / "src"),
    )
    from lashon_stt.engines.faster_whisper_engine import load_engine

    print("loading engine... ", end="", flush=True)
    t0 = time.monotonic()
    engine = load_engine(cpu_only=args.cpu, model_id=args.model)
    print(
        f"{(time.monotonic() - t0):.1f}s "
        f"(device={engine.device}, compute={engine.compute_type}, "
        f"model={engine.model_id})"
    )

    pcm = load_wav(args.wav)
    audio_duration_s = len(pcm) / SAMPLE_RATE
    hop_samples = int(SAMPLE_RATE * args.hop_ms / 1000)
    min_samples = int(SAMPLE_RATE * MIN_DECODE_S)
    print(
        f"loaded {args.wav.name}: {audio_duration_s:.2f} s, "
        f"{len(pcm)} samples, hop={args.hop_ms} ms ({hop_samples} samples), "
        f"min-decode={MIN_DECODE_S:.1f} s"
    )
    print()

    partials: list[str] = []
    decode_times_ms: list[int] = []
    start = time.monotonic()
    # Skip the min-sample gate's worth of audio before the first decode, then
    # advance one hop at a time — the live driver's single-flight cadence.
    cursor = min(min_samples, len(pcm))
    step = 0

    while True:
        step += 1
        audio_so_far_s = cursor / SAMPLE_RATE
        is_final = cursor >= len(pcm)

        # In realtime mode, wait until wall-clock catches up to the audio we've
        # "consumed" (the live-mic illusion).
        if args.realtime:
            wall_now = time.monotonic() - start
            if wall_now < audio_so_far_s:
                time.sleep(audio_so_far_s - wall_now)

        decode_start = time.monotonic()
        result = engine.transcribe(pcm[:cursor], language=args.language)
        decode_ms = int((time.monotonic() - decode_start) * 1000)
        decode_times_ms.append(decode_ms)
        partials.append(result.text)

        since_start = time.monotonic() - start
        tag = "FINAL  " if is_final else "partial"
        audio_label = "END   " if is_final else f"{audio_so_far_s:4.1f}s"
        print(
            f"[audio={audio_label}, decode={decode_ms:4d}ms, "
            f"since_start={since_start:5.2f}s] {tag}: {result.text}"
        )

        if is_final:
            break
        cursor = min(cursor + hop_samples, len(pcm))

    print()
    total_decode_s = sum(decode_times_ms) / 1000
    wall_total = time.monotonic() - start
    print("--- summary ---")
    print(f"device           : {engine.device} ({engine.compute_type})")
    print(f"audio duration   : {audio_duration_s:.2f} s")
    print(
        f"wall-clock       : {wall_total:.2f} s   "
        f"({wall_total / audio_duration_s:.2f}x real-time end-to-end)"
    )
    print(f"decodes          : {len(decode_times_ms)}")
    print(f"total decode     : {total_decode_s:.2f} s")
    print(f"avg decode       : {int(sum(decode_times_ms) / len(decode_times_ms))} ms")
    print(f"max decode       : {max(decode_times_ms)} ms")
    print(f"final text       : {partials[-1]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
