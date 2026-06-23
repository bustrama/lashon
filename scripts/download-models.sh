#!/usr/bin/env bash
# Download Lashon's model weights into models/<stage>/<model>/ and verify them.
# Thin wrapper around scripts/verify-models.py — see models/README.md.
set -euo pipefail
exec python3 "$(dirname "$0")/verify-models.py" --download "$@"
