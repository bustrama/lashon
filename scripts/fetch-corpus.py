#!/usr/bin/env python3
"""Fetch the CC-licensed Hebrew read-speech corpus tier (see docs/testing.md).

Streams a subset of Google FLEURS (config `he_il`, licensed CC-BY-4.0) into
tests/hebrew-corpus/read/ and records the clips in manifest.json. The audio is
written as-is — FLEURS clips are already 16 kHz WAV — so no audio decoder
(torchcodec / soundfile) is required.

Needs `datasets` in a throwaway environment; its version does not affect the
repository (only the fetched WAVs are committed):

  pip install datasets
  python scripts/fetch-corpus.py --count 25
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
CORPUS = REPO_ROOT / "tests" / "hebrew-corpus"
READ_DIR = CORPUS / "read"


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description="Fetch the FLEURS he_il corpus tier.")
    parser.add_argument("--count", type=int, default=25, help="number of clips")
    args = parser.parse_args()

    from datasets import Audio, load_dataset

    READ_DIR.mkdir(parents=True, exist_ok=True)
    print(f"streaming FLEURS he_il (test split) — taking {args.count} clips")
    dataset = load_dataset("google/fleurs", "he_il", split="test", streaming=True)
    # decode=False yields the raw WAV bytes, so no audio codec is needed.
    dataset = dataset.cast_column("audio", Audio(decode=False))

    clips = []
    for index, row in enumerate(dataset):
        if index >= args.count:
            break
        name = f"read-{index + 1:03d}.wav"
        (READ_DIR / name).write_bytes(row["audio"]["bytes"])
        clips.append({"file": f"read/{name}", "transcript": row["raw_transcription"]})
        print(f"  {name}")

    manifest_path = CORPUS / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    manifest["categories"]["read"]["clips"] = clips
    manifest_path.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"\nwrote {len(clips)} clips and updated manifest.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
