"""Filesystem paths for the STT sidecar — works frozen or run from source.

When PyInstaller-frozen, ``sys.frozen`` is set and resources sit next to the
executable. When run from source, paths resolve relative to this file. The
``base_dir`` pattern is lifted from docs/roadmap.md §1.2.
"""
from __future__ import annotations

import sys
from pathlib import Path

# .../services/stt-sidecar/src/lashon_stt
_PACKAGE_DIR = Path(__file__).resolve().parent


def base_dir() -> Path:
    """Directory that anchors bundled resources (generated stubs, manifests).

    When PyInstaller-frozen, bundled data is unpacked under ``sys._MEIPASS`` —
    the ``_internal`` folder of a one-dir build. When run from source, it is
    this package's directory. The pattern is from docs/roadmap.md §1.2.
    """
    if getattr(sys, "frozen", False):
        meipass = getattr(sys, "_MEIPASS", None)
        if meipass:
            return Path(meipass).resolve()
        return Path(sys.executable).resolve().parent
    return _PACKAGE_DIR


def generated_dir() -> Path:
    """Directory holding the generated gRPC stubs (stt_pb2*.py)."""
    return base_dir() / "_generated"


def manifest_path(name: str) -> Path:
    """Path to a download manifest (e.g. ``stt.json``, ``cuda.json``).

    A frozen build ships the manifests next to its other resources; from source
    they live in the repository's ``models/manifests/`` tree.
    """
    bundled = base_dir() / "manifests" / name
    if bundled.exists():
        return bundled
    return repo_root() / "models" / "manifests" / name


def repo_root() -> Path:
    """Repository root. Only meaningful when running from source."""
    # _PACKAGE_DIR == <root>/services/stt-sidecar/src/lashon_stt
    #   parents[0] src  parents[1] stt-sidecar  parents[2] services  parents[3] root
    return _PACKAGE_DIR.parents[3]


def proto_dir() -> Path:
    """Directory containing the shared .proto contracts (source builds only)."""
    return repo_root() / "packages" / "proto"
