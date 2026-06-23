"""Resolve STT model locations from the stt.json manifest.

Model weights are large and never committed. From source they live under the
repo's ``models/`` tree. In a packaged build the Tauri shell points
``LASHON_MODELS_ROOT`` at a per-user app-data directory, and the weights are
downloaded there on first run (see ``model_download.py``).
"""
from __future__ import annotations

import json
import os
from pathlib import Path

from lashon_stt.paths import manifest_path, repo_root

DEFAULT_MODEL_ID = "ivrit-ai-whisper-large-v3-turbo-ct2"

# Vanilla Whisper tiny, used only for language identification — the ivrit-ai
# fine-tune's own detector is collapsed (see docs/adr/0009).
DETECTOR_MODEL_ID = "faster-whisper-tiny"

# Set by the Tauri shell for a packaged build; absent when run from source.
MODELS_ROOT_ENV = "LASHON_MODELS_ROOT"


def _manifest() -> dict:
    return json.loads(manifest_path("stt.json").read_text(encoding="utf-8"))


def model_entry(model_id: str = DEFAULT_MODEL_ID) -> dict:
    """The manifest entry for a model id. Raises KeyError if the id is unknown."""
    for model in _manifest().get("models", []):
        if model["id"] == model_id:
            return model
    raise KeyError(f"unknown model id: {model_id}")


def model_dir(model_id: str = DEFAULT_MODEL_ID) -> Path:
    """Directory a model's weights live in — whether or not they are present.

    Packaged build: ``$LASHON_MODELS_ROOT/<name>``. From source: the repo path
    from the manifest's ``local_dir``. The caller checks whether the weights
    are actually downloaded (see ``is_downloaded``).
    """
    entry = model_entry(model_id)
    root = os.environ.get(MODELS_ROOT_ENV)
    if root:
        return Path(root) / Path(entry["local_dir"]).name
    return repo_root() / entry["local_dir"]


def is_downloaded(model_id: str = DEFAULT_MODEL_ID) -> bool:
    """True when the model's weights are present on disk."""
    return (model_dir(model_id) / "model.bin").exists()
