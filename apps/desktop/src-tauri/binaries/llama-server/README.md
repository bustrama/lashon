# Bundled llama-server

Prebuilt `llama-server.exe` and the minimum DLL set Lashon needs to run
the in-process local LLM (docs/adr/0025). Mirrored from the
`ggml.llamacpp` winget release — a Vulkan-only build that runs on any
modern GPU (NVIDIA, AMD, Intel) and falls back to CPU on hosts without
one.

## Source
Upstream: <https://github.com/ggml-org/llama.cpp>
Distribution: <https://winget.run/pkg/ggml/llamacpp>

## License
llama.cpp and ggml are licensed under the MIT License. Lashon's own code is
GPL-3.0-only (see the project root's `LICENSE`); llama.cpp's MIT notice is
reproduced in the project root's `NOTICE` file. This directory ships binary
form only.

## Updating
Reinstall the upstream package (`winget upgrade ggml.llamacpp`) and
re-copy the file set listed in `apps/desktop/src-tauri/tauri.conf.json`'s
`bundle.resources` entry. Refresh the size + version note in the ADR if
the totals change materially.
