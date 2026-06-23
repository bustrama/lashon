"""Generate the gRPC Python stubs from packages/proto/stt.proto.

The generated files (stt_pb2.py, stt_pb2_grpc.py) are build artifacts: they
live in ``_generated/`` and are gitignored. ``ensure_stubs()`` (re)generates
them when missing or stale, so a fresh clone runs without a separate codegen
step. A PyInstaller-frozen build bundles the stubs instead (see PyInstaller.spec).
"""
from __future__ import annotations

from lashon_stt.paths import generated_dir, proto_dir


def _stale() -> bool:
    """True if the stubs are missing or older than the .proto they came from."""
    gen = generated_dir()
    grpc_stub = gen / "stt_pb2_grpc.py"
    msg_stub = gen / "stt_pb2.py"
    if not grpc_stub.exists() or not msg_stub.exists():
        return True
    proto = proto_dir() / "stt.proto"
    if not proto.exists():
        # Frozen build: stubs are bundled, the .proto is not shipped.
        return False
    newest_stub = max(grpc_stub.stat().st_mtime, msg_stub.stat().st_mtime)
    return proto.stat().st_mtime > newest_stub


def ensure_stubs() -> None:
    """Regenerate the gRPC stubs if missing or stale; otherwise do nothing."""
    if not _stale():
        return

    gen = generated_dir()
    gen.mkdir(parents=True, exist_ok=True)
    protos = proto_dir()
    proto_file = protos / "stt.proto"
    if not proto_file.exists():
        raise FileNotFoundError(
            f"cannot generate gRPC stubs: {proto_file} not found "
            "(and no pre-generated stubs are present)"
        )

    # Imported here because grpc_tools is only needed when generating.
    from grpc_tools import protoc

    rc = protoc.main(
        [
            "grpc_tools.protoc",
            f"-I{protos}",
            f"--python_out={gen}",
            f"--grpc_python_out={gen}",
            str(proto_file),
        ]
    )
    if rc != 0:
        raise RuntimeError(f"grpc_tools.protoc failed with exit code {rc}")
