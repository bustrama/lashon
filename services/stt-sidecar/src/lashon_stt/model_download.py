"""First-run download of the STT model weights, verified against stt.json.

A packaged build ships no model — it is ~1.6 GB and Apache-2.0-licensed
separately from the Lashon source. ``ensure_model`` downloads it on first run
into ``LASHON_MODELS_ROOT``, streaming each file so the warm-up UI can report
real byte-level progress.

Every present file is SHA-256-verified against the manifest on every boot,
not only right after a download: a swapped same-size ``model.bin`` is native
code inside ``ctranslate2``, so a size check alone is not an integrity gate
(docs/adr/0010). A file that fails verification is re-downloaded. A partial
file left by an interrupted run is resumed via an HTTP ``Range`` request
rather than refetched.
"""
from __future__ import annotations

import hashlib
import logging
import time
from pathlib import Path
from typing import Callable
from urllib.parse import quote

from lashon_stt.model_registry import DEFAULT_MODEL_ID, model_dir, model_entry

logger = logging.getLogger(__name__)

_CHUNK = 1 << 20

# Hugging Face file endpoint; the `resolve` path follows LFS pointers server-side.
_HF_ENDPOINT = "https://huggingface.co"
# Network attempts per file before giving up — a partial file is resumed.
_MAX_ATTEMPTS = 4

# Called with a short human-readable status line as the download progresses.
ProgressCallback = Callable[[str], None]


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _present(path: Path, entry: dict) -> bool:
    """Cheap check — the file exists and is the expected size. Run every boot."""
    return path.exists() and path.stat().st_size == entry["bytes"]


def _verified(path: Path, entry: dict) -> bool:
    """Full check — size and SHA-256 match the manifest. Run after a download."""
    return _present(path, entry) and _sha256(path) == entry["sha256"]


def _download_file(
    url: str, dest: Path, size: int, on_bytes: Callable[[int], None]
) -> None:
    """Stream ``url`` to ``dest``, resuming a partial file and retrying on
    network errors. ``on_bytes`` receives the file's cumulative byte count as
    it grows, so the caller can report progress.
    """
    # requests ships with huggingface_hub — already installed and bundled.
    import requests

    for attempt in range(1, _MAX_ATTEMPTS + 1):
        have = dest.stat().st_size if dest.exists() else 0
        if have >= size:  # a stale, oversized leftover — start fresh
            have = 0
        headers = {"Range": f"bytes={have}-"} if have else {}
        try:
            with requests.get(
                url, headers=headers, stream=True, timeout=60
            ) as response:
                response.raise_for_status()
                # 206 means the server honoured the Range and we append.
                resuming = response.status_code == 206
                written = have if resuming else 0
                on_bytes(written)
                with dest.open("ab" if resuming else "wb") as out:
                    for chunk in response.iter_content(chunk_size=_CHUNK):
                        out.write(chunk)
                        written += len(chunk)
                        on_bytes(written)
            return
        except requests.RequestException as exc:
            if attempt == _MAX_ATTEMPTS:
                raise
            logger.warning("download of %s failed (%s) — retrying", dest.name, exc)
            time.sleep(2 * attempt)


def ensure_model(
    model_id: str = DEFAULT_MODEL_ID,
    on_progress: ProgressCallback | None = None,
    label: str = "the Hebrew STT model",
) -> Path:
    """Ensure the model's weights are present and verified against the manifest.

    Absent, wrong-size, or hash-mismatched files are (re)downloaded. ``label``
    names the model in the human-readable progress lines.

    Returns the model directory. Raises on a download or verification failure.
    """
    entry = model_entry(model_id)
    target = model_dir(model_id)
    files = entry.get("files", [])

    def report(text: str) -> None:
        logger.info(text)
        if on_progress is not None:
            on_progress(text)

    if not files:
        raise RuntimeError(
            f"manifest for '{model_id}' lists no files — cannot download"
        )
    # A file is stale — it needs (re)downloading — when it is absent, the
    # wrong size, or, though present at the right size, fails the manifest
    # SHA-256. Hashing present files here, not merely sizing them, is what
    # catches a swapped same-size file (a tampered model.bin is native code
    # inside ctranslate2) on every boot, not only right after a download.
    # See docs/adr/0010.
    stale: list[dict] = []
    verifying = False
    for file in files:
        path = target / file["path"]
        if not _present(path, file):
            stale.append(file)
            continue
        if not verifying:
            report(f"verifying {label}")
            verifying = True
        if _sha256(path) != file["sha256"]:
            logger.warning(
                "%s failed integrity verification — re-downloading",
                file["path"],
            )
            stale.append(file)
    if not stale:
        return target

    target.mkdir(parents=True, exist_ok=True)
    total = sum(f["bytes"] for f in stale)
    repo = entry["repo"]
    revision = entry.get("revision", "main")
    report(f"downloading {label} ({total / (1 << 20):.0f} MB)")

    done = 0  # bytes from files already finished
    last_pct = -1

    def on_bytes(file_done: int) -> None:
        # Report only on a percent change — at most 100 lines for the whole
        # download, however many 1 MB chunks stream past.
        nonlocal last_pct
        pct = min(100, (done + file_done) * 100 // total)
        if pct != last_pct:
            last_pct = pct
            report(f"downloading {label} — {pct}%")

    for file in stale:
        dest = target / file["path"]
        dest.parent.mkdir(parents=True, exist_ok=True)
        # `resolve` redirects LFS-backed files to the CDN; requests follows it.
        url = f"{_HF_ENDPOINT}/{repo}/resolve/{revision}/{quote(file['path'])}"
        _download_file(url, dest, file["bytes"], on_bytes)
        if not _verified(dest, file):
            dest.unlink(missing_ok=True)  # drop the bad file so a retry refetches it
            raise RuntimeError(
                f"downloaded model file failed verification: {file['path']}"
            )
        done += file["bytes"]

    report("model download complete")
    return target
