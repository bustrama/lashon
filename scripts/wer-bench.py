#!/usr/bin/env python3
"""Word-error-rate benchmark for the Lashon STT pipeline (see docs/testing.md).

Transcribes every clip in tests/hebrew-corpus/manifest.json that is present on
disk, computes WER against the ground-truth transcript, and reports per-tier
results. Exits non-zero if any populated tier regresses past its wer_target.

Run in the STT sidecar environment (faster-whisper + the `bench` extra):

  python scripts/wer-bench.py
"""
from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CORPUS = REPO_ROOT / "tests" / "hebrew-corpus"


def normalize(text: str) -> str:
    """Normalize text for Hebrew WER: drop punctuation, collapse whitespace.

    Hebrew is caseless, so this is the whole normalization. Applied identically
    to reference and hypothesis, so it cannot bias the score.
    """
    return " ".join(re.sub(r"[^\w\s]", " ", text, flags=re.UNICODE).split())


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8")

    import jiwer
    from faster_whisper.audio import decode_audio

    from lashon_stt.engines.faster_whisper_engine import SAMPLE_RATE, load_engine

    manifest = json.loads((CORPUS / "manifest.json").read_text(encoding="utf-8"))
    # LASHON_STT_MODEL_ID selects the transcription model for an A/B run (e.g.
    # turbo vs non-turbo large-v3); unset loads the shipped default. See ADR-0036.
    engine = load_engine(model_id=os.environ.get("LASHON_STT_MODEL_ID"))
    print(f"engine: {engine.device} ({engine.compute_type}) — model {engine.model_id}\n")

    gate_ok = True
    gates_scored = 0
    for tier, category in manifest["categories"].items():
        clips = [c for c in category["clips"] if (CORPUS / c["file"]).exists()]
        if not clips:
            print(f"[{tier}] no clips on disk — skipped")
            continue
        # A code-switching tier runs with language detection ON, exercising the
        # companion detector (docs/adr/0009); the others force Hebrew, where the
        # benchmark measures transcription WER rather than detection.
        detect = category.get("detect_language", False)
        refs, hyps = [], []
        for clip in clips:
            audio = decode_audio(str(CORPUS / clip["file"]), sampling_rate=SAMPLE_RATE)
            result = (
                engine.transcribe(audio)
                if detect
                else engine.transcribe(audio, language="he")
            )
            ref, hyp = normalize(clip["transcript"]), normalize(result.text)
            refs.append(ref)
            hyps.append(hyp)
            if detect:
                # A code-switching run is read by eye — show each clip, the
                # detected language, and the transcription against the truth.
                print(f"  {clip['file']}  detected: {result.language}")
                print(f"    ref: {ref}")
                print(f"    got: {hyp}")
        wer = jiwer.wer(refs, hyps)
        target = category["wer_target"]
        if category.get("gate", False):
            gates_scored += 1
            ok = wer <= target
            gate_ok &= ok
            label = "OK" if ok else "FAIL"
        else:
            label = "baseline"  # informational — does not gate the build
        print(
            f"[{tier}] {len(clips)} clips — WER {wer:.1%} "
            f"(target <= {target:.0%})  {label}"
        )

    print()
    if gates_scored == 0:
        print("WER BENCH: no gating tier scored — record studio clips to verify the M1 DoD")
        return 1
    print("WER BENCH: PASS" if gate_ok else "WER BENCH: FAIL")
    return 0 if gate_ok else 1


if __name__ == "__main__":
    sys.exit(main())
