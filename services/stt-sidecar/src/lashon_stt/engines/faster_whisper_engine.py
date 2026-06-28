"""faster-whisper STT engine — loads the ivrit-ai Hebrew CT2 model."""
from __future__ import annotations

import importlib.util
import logging
import math
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np

from lashon_stt.model_registry import DEFAULT_MODEL_ID, DETECTOR_MODEL_ID, model_dir
from lashon_stt.postprocess import sanitize

logger = logging.getLogger(__name__)


def _register_cuda_dll_dirs() -> None:
    """Make the CUDA runtime DLLs discoverable on Windows before ctranslate2 loads.

    ctranslate2 loads cuDNN with the legacy Windows DLL search, which consults
    PATH but not os.add_dll_directory entries — so both are registered here.

    - Packaged app: the CUDA runtime is downloaded on first run (see
      cuda_download) into $LASHON_CUDA_ROOT; register nvidia/*/bin beneath it.
    - From source: the nvidia-*-cu12 packages ship the DLLs under
      site-packages/nvidia/<lib>/bin.

    With no NVIDIA GPU the directories are simply absent and this is a no-op.
    """
    if sys.platform != "win32":
        return

    cuda_root = os.environ.get("LASHON_CUDA_ROOT")
    if cuda_root:
        roots = [d for d in sorted(Path(cuda_root).glob("nvidia/*/bin")) if d.is_dir()]
    else:
        spec = importlib.util.find_spec("nvidia")
        if spec is None or not spec.submodule_search_locations:
            return
        nvidia_root = Path(next(iter(spec.submodule_search_locations)))
        roots = [d for d in sorted(nvidia_root.glob("*/bin")) if d.is_dir()]

    for root in roots:
        try:
            os.add_dll_directory(str(root))
        except OSError:
            pass
    if roots:
        os.environ["PATH"] = os.pathsep.join(
            [str(d) for d in roots] + [os.environ.get("PATH", "")]
        )


# 16 kHz mono — the sample rate Whisper models expect.
SAMPLE_RATE = 16000


@dataclass(frozen=True)
class Transcript:
    """Result of a single transcription."""

    text: str
    language: str
    confidence: float
    inference_ms: int


class FasterWhisperEngine:
    """A loaded faster-whisper model, transcribing Hebrew-first PCM audio."""

    def __init__(
        self, device: str, compute_type: str, model_id: str | None = None
    ) -> None:
        # Imported here so module import stays cheap and dependency-free.
        from faster_whisper import WhisperModel

        self.device = device
        self.compute_type = compute_type
        # The transcription model. ``None`` loads the shipped DEFAULT_MODEL_ID;
        # an explicit id is the seam for A/B model evaluations (e.g. turbo vs
        # non-turbo large-v3 — scripts/wer-bench.py, scripts/stream-test.py).
        self.model_id = model_id or DEFAULT_MODEL_ID
        self._model = WhisperModel(
            str(model_dir(self.model_id)), device=device, compute_type=compute_type
        )
        # The ivrit-ai Hebrew fine-tune's language detector is collapsed — it
        # reports 'he' for every language, English included. A small vanilla
        # Whisper does the language ID; it never transcribes (docs/adr/0009).
        self._detector = WhisperModel(
            str(model_dir(DETECTOR_MODEL_ID)),
            device=device,
            compute_type=compute_type,
        )

    def transcribe(self, pcm: np.ndarray, language: str | None = None) -> Transcript:
        """Transcribe 16 kHz mono float32 PCM (samples in [-1.0, 1.0]).

        With no explicit ``language``, the spoken language is identified by the
        companion detector model and forced for the decode — the transcription
        model's own auto-detection cannot be trusted (docs/adr/0009).
        """
        # frombuffer arrays are read-only; ascontiguousarray gives a clean copy.
        audio = np.ascontiguousarray(pcm, dtype=np.float32)
        if not language:
            language = self._detect_language(audio)
        start = time.monotonic()
        segments, info = self._model.transcribe(
            audio,
            language=language,
            beam_size=5,
            vad_filter=False,
        )
        # faster-whisper decodes lazily — iterating the generator runs inference.
        texts: list[str] = []
        logprobs: list[float] = []
        for segment in segments:
            texts.append(segment.text.strip())
            logprobs.append(segment.avg_logprob)
        elapsed_ms = int((time.monotonic() - start) * 1000)
        return Transcript(
            text=sanitize(" ".join(t for t in texts if t)),
            language=info.language or language,
            confidence=_confidence(logprobs),
            inference_ms=elapsed_ms,
        )

    def _detect_language(self, audio: np.ndarray) -> str:
        """Identify the spoken language with the companion detector model."""
        language, probability, _ = self._detector.detect_language(audio)
        # A language code and its probability are metadata, not transcript
        # content — safe to log (.claude/rules/security.md).
        logger.info("language detected: %s (p=%.2f)", language, probability)
        return language


def _confidence(logprobs: list[float]) -> float:
    """Map mean segment log-probability (<= 0) to a confidence in (0.0, 1.0]."""
    if not logprobs:
        return 0.0
    return round(min(1.0, math.exp(sum(logprobs) / len(logprobs))), 4)


def load_engine(
    cpu_only: bool = False, model_id: str | None = None
) -> FasterWhisperEngine:
    """Load the STT engine.

    By default the GPU is preferred, with a CPU fallback. With ``cpu_only`` —
    hardware tiers C/D, or a user who overrode to them — the CUDA device is not
    attempted at all and the engine loads straight on the CPU (docs/adr/0014).

    ``model_id`` selects the transcription model; ``None`` loads the shipped
    ``DEFAULT_MODEL_ID``. A non-default id is the seam for A/B model
    evaluations and is never passed on the shipped hot path.
    """
    # The CUDA runtime is downloaded on first run, so register its DLL
    # directories now — not at import, which is before that download.
    _register_cuda_dll_dirs()
    candidates = (
        (("cpu", "int8"),)
        if cpu_only
        else (("cuda", "int8_float16"), ("cpu", "int8"))
    )
    for device, compute_type in candidates:
        try:
            engine = FasterWhisperEngine(
                device=device, compute_type=compute_type, model_id=model_id
            )
            logger.info(
                "STT engine loaded on %s (%s) — model %s",
                device,
                compute_type,
                engine.model_id,
            )
            return engine
        except Exception as exc:  # device probing — ctranslate2 raises varied errors
            logger.warning("STT engine load on %s failed: %s", device, exc)
    raise RuntimeError("could not load the STT engine")
