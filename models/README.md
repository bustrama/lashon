# Models

Model weights are **not committed** — they are large and licensed separately
from the Lashon source. This directory holds only:

- `manifests/` — per-stage model registries (`stt.json`, …) recording each
  model's repo, revision, license, and per-file SHA-256.
- this README.

Weights download into `models/<stage>/<model>/` (git-ignored):

```sh
python scripts/verify-models.py --download   # or scripts/download-models.{sh,ps1}
```

and are verified against the manifest with `python scripts/verify-models.py`.

## License policy

Only MIT/Apache-licensed models are bundled in release installers
(`"bundle": "installer"` in a manifest). CC-BY-NC and CPML models are offered as
opt-in downloads with a non-commercial badge, never bundled — see
`docs/providers.md`.
