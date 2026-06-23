"""First-run download of the NVIDIA CUDA runtime for optional GPU acceleration.

The cuDNN and cuBLAS runtime libraries are ~1.2 GB of NVIDIA wheels — far too
large to bundle in the installer. When an NVIDIA GPU is present, the sidecar
downloads them from PyPI on first run (the same place ``pip`` fetches them),
verifies each wheel against ``models/manifests/cuda.json``, and extracts the
DLLs into ``LASHON_CUDA_ROOT`` — ``faster_whisper_engine`` discovers them there.

If no NVIDIA GPU is present, or ``LASHON_CUDA_ROOT`` is unset (a from-source run,
where the virtual environment supplies the libraries), this is a no-op. A
download failure is non-fatal: the engine simply falls back to the CPU.
"""
from __future__ import annotations

import hashlib
import json
import logging
import os
import shutil
import zipfile
from pathlib import Path
from typing import Callable

from lashon_stt.paths import manifest_path

logger = logging.getLogger(__name__)

_CHUNK = 1 << 20

# Set by the Tauri shell for a packaged build; absent when run from source.
CUDA_ROOT_ENV = "LASHON_CUDA_ROOT"

ProgressCallback = Callable[[str], None]


def cuda_runtime_dir() -> Path | None:
    """Directory the CUDA DLLs are extracted into, or None when unmanaged.

    The Tauri shell sets ``LASHON_CUDA_ROOT`` for a packaged build. From source
    it is unset and the venv's nvidia-*-cu12 packages supply the libraries.
    """
    root = os.environ.get(CUDA_ROOT_ENV)
    return Path(root) if root else None


def _has_nvidia_gpu() -> bool:
    """True when an NVIDIA driver is installed — ``nvidia-smi`` is on PATH."""
    return shutil.which("nvidia-smi") is not None


def _installed(cuda_dir: Path) -> bool:
    """True when the cuDNN and cuBLAS DLLs are already extracted."""
    return any(cuda_dir.glob("nvidia/cudnn/bin/*.dll")) and any(
        cuda_dir.glob("nvidia/cublas/bin/*.dll")
    )


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(_CHUNK), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _fetch(url: str, dest: Path) -> None:
    # requests is a huggingface_hub dependency — already installed and bundled.
    import requests

    with requests.get(url, stream=True, timeout=30) as response:
        response.raise_for_status()
        with dest.open("wb") as out:
            for chunk in response.iter_content(chunk_size=_CHUNK):
                out.write(chunk)


def ensure_cuda_runtime(on_progress: ProgressCallback | None = None) -> None:
    """Download and extract the CUDA runtime when an NVIDIA GPU can use it.

    Never raises — GPU acceleration is optional, so any failure is logged and
    swallowed, leaving the engine to run on the CPU.
    """

    def report(text: str) -> None:
        logger.info(text)
        if on_progress is not None:
            on_progress(text)

    cuda_dir = cuda_runtime_dir()
    if cuda_dir is None:
        return  # from source — the virtual environment supplies CUDA
    if not _has_nvidia_gpu():
        logger.info("no NVIDIA GPU detected — the STT engine will use the CPU")
        return
    if _installed(cuda_dir):
        return

    try:
        _download_runtime(cuda_dir, report)
    except Exception as exc:  # optional acceleration — never fatal
        logger.warning("CUDA runtime download failed (%s) — using the CPU", exc)
        report("GPU acceleration unavailable — continuing on the CPU")


def _download_runtime(cuda_dir: Path, report: ProgressCallback) -> None:
    wheels = json.loads(
        manifest_path("cuda.json").read_text(encoding="utf-8")
    )["wheels"]
    total_mb = sum(w["bytes"] for w in wheels) / (1 << 20)
    report(f"downloading GPU acceleration runtime ({total_mb:.0f} MB)")

    cuda_dir.mkdir(parents=True, exist_ok=True)
    for index, wheel in enumerate(wheels, start=1):
        report(f"downloading GPU runtime {index}/{len(wheels)}: {wheel['id']}")
        archive = cuda_dir / wheel["filename"]
        _fetch(wheel["url"], archive)
        if _sha256(archive) != wheel["sha256"]:
            archive.unlink(missing_ok=True)
            raise RuntimeError(f"checksum mismatch for {wheel['filename']}")
        # The wheel is a zip archive; extract just the nvidia/*/bin DLL tree.
        with zipfile.ZipFile(archive) as zf:
            for member in zf.namelist():
                if member.startswith("nvidia/") and "/bin/" in member:
                    zf.extract(member, cuda_dir)
        archive.unlink(missing_ok=True)
    report("GPU acceleration runtime ready")
