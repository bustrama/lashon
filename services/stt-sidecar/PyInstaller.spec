# -*- mode: python ; coding: utf-8 -*-
#
# PyInstaller spec for the Lashon STT sidecar.
#
# Produces a one-folder bundle of the gRPC speech-to-text server with
# faster-whisper + ctranslate2 frozen in. See docs/adr/0006, docs/adr/0018,
# docs/packaging-windows.md, docs/packaging-macos.md, docs/packaging-linux.md.
#
#   pyinstaller PyInstaller.spec
#
# Output:  dist/lashon-stt/lashon-stt[.exe]  (+ _internal/)
#          -> copied to apps/desktop/src-tauri/binaries/lashon-stt/ and shipped
#             by the Tauri bundle as a resource (tauri.conf.json).
#
# One-folder, not one-file: a one-file build would re-extract the whole payload
# to a temp directory on every launch.
#
# The NVIDIA CUDA runtime is NOT bundled — it is ~1.2 GB and is downloaded from
# PyPI on first run when an NVIDIA GPU is present (see cuda_download.py). This
# is invariant across every target: even on Windows x64 (where the [cuda] extra
# may be installed in the build venv for development), the frozen sidecar must
# stay slim and pull the runtime at first launch — that is the whole point of
# the download-on-first-run pattern in ADR-0006.
#
# Platform matrix:
#   Windows x64  — CPU + CUDA download-on-first-run
#   macOS arm64  — CPU only; CUDA is NVIDIA/x86_64, unavailable on Apple Silicon
#   Linux x86_64 — CPU + CUDA download-on-first-run same as Windows

from pathlib import Path

from PyInstaller.utils.hooks import collect_all

_root = Path(SPECPATH)
_src = _root / "src"
_repo = _root.parent.parent  # services/stt-sidecar -> services -> repo root

datas = []
binaries = []
hiddenimports = []

# Native-heavy packages PyInstaller cannot fully trace on its own. The gRPC
# stubs (stt_pb2*) are loaded dynamically and import google.protobuf, so it is
# collected explicitly rather than discovered.
_base_pkgs = (
    "grpc",
    "google.protobuf",
    "ctranslate2",
    "faster_whisper",
    "av",
    "onnxruntime",
    "tokenizers",
    "huggingface_hub",
)

# CUDA runtime packages are NEVER collected — see the top-of-file comment.
# If the [cuda] extra is installed in the build venv (typical on Windows
# development boxes), collect_all("nvidia.cublas") would pull ~1.7 GB of
# DLLs into the bundle. The runtime download path (cuda_download.py,
# ADR-0004 / ADR-0006) is the canonical path on every OS.

for _pkg in _base_pkgs:
    _d, _b, _h = collect_all(_pkg)
    datas += _d
    binaries += _b
    hiddenimports += _h

# Bundled gRPC stubs (generated before this build) and the download manifests
# (stt.json, cuda.json). The frozen sidecar resolves both via
# lashon_stt.paths.base_dir().
datas += [
    (str(_src / "lashon_stt" / "_generated"), "_generated"),
    (str(_repo / "models" / "manifests"), "manifests"),
]

a = Analysis(
    [str(_src / "lashon_stt" / "server.py")],
    pathex=[str(_src)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    # grpc_tools is only used to regenerate stubs from source; the frozen build
    # ships the stubs and never imports it. tkinter is unused.
    excludes=["grpc_tools", "tkinter"],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="lashon-stt",
    console=True,
    disable_windowed_traceback=False,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    name="lashon-stt",
)
