"""Lashon STT sidecar — gRPC server.

Implements HealthCheck, TranscribeBytes, and TranscribeStream over a loopback
gRPC server (docs/roadmap.md §1.2; transport per ADR-0002, auth per
ADR-0010). The Hebrew STT
model is loaded in a background thread on boot, so the sidecar is reachable
immediately and reports ``model_ready`` once warm-up completes.
"""
from __future__ import annotations

import logging
import os
import secrets
import sys
import threading
from concurrent import futures

import grpc
import numpy as np

from lashon_stt import __version__, codegen, cuda_download, model_download
from lashon_stt.engines.faster_whisper_engine import load_engine
from lashon_stt.model_registry import DETECTOR_MODEL_ID
from lashon_stt.paths import generated_dir

# Force UTF-8 stdio — Hebrew in logs must never hit a cp1252 Windows console.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8")
    except (AttributeError, ValueError):
        pass

logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s")
logger = logging.getLogger("lashon_stt")

# Contract: the Rust host parses these exact prefixes from the sidecar's
# stdout — the token line first, then the port line that signals "listening".
# See ADR-0002 (transport) and ADR-0010 (the per-process auth token).
TOKEN_LINE_PREFIX = "LASHON_STT_TOKEN="
PORT_LINE_PREFIX = "LASHON_STT_PORT="

# gRPC metadata key the auth token rides in. Must match AUTH_METADATA_KEY in
# packages/shared-rust/src/sidecar.rs.
_AUTH_METADATA_KEY = "x-lashon-auth"

# Longest a transcription RPC waits for the model warm-up to finish.
MODEL_WAIT_SECONDS = 120.0


def _load_stubs():
    """Generate (if needed) and import the gRPC stubs — see codegen.ensure_stubs."""
    codegen.ensure_stubs()
    gen = str(generated_dir())
    if gen not in sys.path:
        sys.path.insert(0, gen)
    import stt_pb2
    import stt_pb2_grpc

    return stt_pb2, stt_pb2_grpc


def _make_servicer(stt_pb2, stt_pb2_grpc, token: str):
    class SttServicer(stt_pb2_grpc.SttServicer):
        def __init__(self) -> None:
            self._token = token
            self._engine = None
            self._ready = threading.Event()
            # Human-readable warm-up state, surfaced through HealthCheck so the
            # tongue can show "preparing" while the model downloads on first run.
            self._status = "starting"
            self._status_lock = threading.Lock()
            threading.Thread(
                target=self._warm_up, name="stt-warmup", daemon=True
            ).start()

        def _set_status(self, text: str) -> None:
            with self._status_lock:
                self._status = text
            logger.info("STT warm-up: %s", text)

        def _status_text(self) -> str:
            with self._status_lock:
                return self._status

        def _warm_up(self) -> None:
            # The host sets LASHON_STT_DEVICE from the hardware tier
            # (docs/adr/0014): "cpu" forces the CPU path and skips the CUDA
            # runtime; anything else probes the GPU first.
            cpu_only = (
                os.environ.get("LASHON_STT_DEVICE", "").strip().lower() == "cpu"
            )
            try:
                self._set_status("locating the Hebrew STT model")
                model_download.ensure_model(on_progress=self._set_status)
                model_download.ensure_model(
                    DETECTOR_MODEL_ID,
                    on_progress=self._set_status,
                    label="the language detector",
                )
                if not cpu_only:
                    cuda_download.ensure_cuda_runtime(on_progress=self._set_status)
                self._set_status("loading the Hebrew STT model")
                self._engine = load_engine(cpu_only=cpu_only)
                self._set_status("stt-sidecar ready")
                self._ready.set()
            except Exception as exc:
                logger.exception("STT engine warm-up failed")
                self._set_status(f"STT warm-up failed: {exc}")

        def _await_engine(self, context):
            if not self._ready.wait(timeout=MODEL_WAIT_SECONDS):
                context.abort(grpc.StatusCode.UNAVAILABLE, self._status_text())
            return self._engine

        def _require_token(self, context) -> None:
            """Abort the RPC unless it carries the shared loopback auth token.

            The loopback bind (ADR-0002) limits *where* a caller sits; this
            token limits *who* may call. It is minted per process and handed
            to the Rust host over the stdout pipe — a channel no other local
            process can read — so a co-resident process cannot forge a call.
            See docs/adr/0010.
            """
            presented = dict(context.invocation_metadata() or ()).get(
                _AUTH_METADATA_KEY, ""
            )
            if not secrets.compare_digest(presented, self._token):
                context.abort(
                    grpc.StatusCode.UNAUTHENTICATED,
                    "missing or invalid Lashon STT auth token",
                )

        def HealthCheck(self, request, context):
            self._require_token(context)
            return stt_pb2.HealthCheckResponse(
                status=stt_pb2.SERVING_STATUS_SERVING,
                detail=self._status_text(),
                version=__version__,
                model_ready=self._ready.is_set(),
            )

        def TranscribeBytes(self, request, context):
            self._require_token(context)
            engine = self._await_engine(context)
            pcm = np.frombuffer(request.pcm_f32, dtype=np.float32)
            # Empty language → let Whisper auto-detect (Hebrew, English, mixed).
            result = engine.transcribe(pcm, language=request.language or None)
            return stt_pb2.TranscribeResponse(
                text=result.text,
                language=result.language,
                confidence=result.confidence,
                inference_ms=result.inference_ms,
            )

        def TranscribeStream(self, request_iterator, context):
            self._require_token(context)
            engine = self._await_engine(context)
            chunks: list[np.ndarray] = []
            for chunk in request_iterator:
                if chunk.pcm_f32:
                    chunks.append(np.frombuffer(chunk.pcm_f32, dtype=np.float32))
                if chunk.end_of_utterance:
                    break
            pcm = (
                np.concatenate(chunks)
                if chunks
                else np.zeros(0, dtype=np.float32)
            )
            result = engine.transcribe(pcm)
            # M1 yields a single final result; incremental partials arrive later.
            yield stt_pb2.TranscribePartial(
                text=result.text, is_final=True, language=result.language
            )

    return SttServicer()


def serve() -> grpc.Server:
    """Build, bind, and start the gRPC server; return the running server.

    Prints the ``LASHON_STT_TOKEN=`` and ``LASHON_STT_PORT=`` handshake lines
    to stdout once listening. The STT model warms up in a background thread,
    so the server is reachable immediately.
    """
    stt_pb2, stt_pb2_grpc = _load_stubs()
    # Per-process auth token. The loopback bind (ADR-0002) limits where a
    # caller can sit; this token limits who may call — without it a
    # co-resident process cannot drive STT. See ADR-0010.
    token = secrets.token_hex(32)
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    stt_pb2_grpc.add_SttServicer_to_server(
        _make_servicer(stt_pb2, stt_pb2_grpc, token), server
    )
    # Ephemeral loopback port — the OS picks a free one. See ADR-0002.
    port = server.add_insecure_port("127.0.0.1:0")
    server.start()
    # Hand the token to the Rust host, then the port. Both lines go to stdout,
    # a pipe only our parent process can read — that is what keeps the token
    # secret. The host waits for the port line, so the token must precede it.
    print(f"{TOKEN_LINE_PREFIX}{token}", flush=True)
    print(f"{PORT_LINE_PREFIX}{port}", flush=True)
    logger.info("STT sidecar listening on 127.0.0.1:%d", port)
    return server


def main() -> int:
    server = serve()
    try:
        server.wait_for_termination()
    except KeyboardInterrupt:
        server.stop(grace=1.0)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
