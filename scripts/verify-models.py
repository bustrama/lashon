#!/usr/bin/env python3
"""Download, record, and verify Lashon's model weights.

Model weights are never committed; `models/manifests/*.json` is the source of
truth for what to fetch and how to verify it.

  python scripts/verify-models.py              verify downloaded models
  python scripts/verify-models.py --download   download (if missing), then verify
  python scripts/verify-models.py --record     download, then write SHA-256 sums
                                               back into the manifest
"""
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
MANIFEST_DIR = REPO_ROOT / "models" / "manifests"
_CHUNK = 1 << 20


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def manifest_paths() -> list[Path]:
    return sorted(MANIFEST_DIR.glob("*.json"))


def download(model: dict) -> None:
    local_dir = REPO_ROOT / model["local_dir"]
    files = model.get("files", [])

    # M6 audio models (Silero VAD, openWakeWord) are not on Hugging Face — each
    # file carries a direct `url`. STT models are Hugging Face snapshots.
    if any("url" in entry for entry in files):
        import urllib.request

        local_dir.mkdir(parents=True, exist_ok=True)
        for entry in files:
            url = entry.get("url")
            if url is None:
                continue
            dest = local_dir / entry["path"]
            dest.parent.mkdir(parents=True, exist_ok=True)
            print(f"  downloading {entry['path']}")
            urllib.request.urlretrieve(url, dest)  # noqa: S310 — pinned https URLs
        return

    # huggingface_hub is a build/setup dependency, imported lazily.
    from huggingface_hub import snapshot_download

    print(f"  downloading {model['repo']}@{model.get('revision', 'main')}")
    snapshot_download(
        repo_id=model["repo"],
        revision=model.get("revision", "main"),
        local_dir=str(local_dir),
    )


def model_files(model: dict) -> list[Path]:
    """Every file under the model's local_dir, excluding HF cache metadata."""
    local_dir = REPO_ROOT / model["local_dir"]
    return sorted(
        p
        for p in local_dir.rglob("*")
        if p.is_file() and ".cache" not in p.relative_to(local_dir).parts
    )


def record(model: dict) -> dict:
    # Recompute the verification set from the files on disk, preserving each
    # file's download `url` (direct-URL models — Silero VAD, openWakeWord).
    local_dir = REPO_ROOT / model["local_dir"]
    urls = {f["path"]: f["url"] for f in model.get("files", []) if "url" in f}
    recorded = []
    for path in model_files(model):
        rel = path.relative_to(local_dir).as_posix()
        entry: dict = {"path": rel}
        if rel in urls:
            entry["url"] = urls[rel]
        entry["bytes"] = path.stat().st_size
        entry["sha256"] = sha256(path)
        recorded.append(entry)
    model["files"] = recorded
    return model


def verify(model: dict) -> bool:
    local_dir = REPO_ROOT / model["local_dir"]
    files = model.get("files", [])
    if not files:
        print(f"  ! {model['id']}: manifest lists no files — run with --record")
        return False
    ok = True
    for entry in files:
        path = local_dir / entry["path"]
        if not path.exists():
            print(f"  missing  {entry['path']}")
            ok = False
        elif sha256(path) != entry["sha256"]:
            print(f"  CHANGED  {entry['path']}")
            ok = False
        else:
            print(f"  ok       {entry['path']}")
    return ok


def main() -> int:
    parser = argparse.ArgumentParser(description="Download/verify Lashon models.")
    parser.add_argument("--download", action="store_true", help="download before verifying")
    parser.add_argument(
        "--record",
        action="store_true",
        help="download, then write SHA-256 sums back into the manifest",
    )
    args = parser.parse_args()

    all_ok = True
    for manifest_path in manifest_paths():
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
        for model in data.get("models", []):
            print(f"{model['id']} ({model['license']})")
            if args.download or args.record:
                download(model)
            if args.record:
                record(model)
            elif not verify(model):
                all_ok = False
        if args.record:
            manifest_path.write_text(
                json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
            )
            print(f"recorded SHA-256 sums into {manifest_path.name}")

    if args.record:
        return 0
    print("OK" if all_ok else "FAILED")
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
