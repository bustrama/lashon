# Download Lashon's model weights into models/<stage>/<model>/ and verify them.
# Thin wrapper around scripts/verify-models.py — see models/README.md.
$ErrorActionPreference = 'Stop'
python "$PSScriptRoot/verify-models.py" --download @args
