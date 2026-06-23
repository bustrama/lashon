# Hub local-LLM catalog + hardware-fit recommendations

Branch `hub-llm-catalog` (off `main`, after the
`m8-command-mode-resilience` PR lands). Runs in parallel with
`m8-os-tools` — see "Parallel work" at the bottom.

> **Status: planned.** A focused PR (~1.5 days). No on-the-fly
> math; the work is curation + a simple threshold lookup + a UI
> annotation. Replaces an earlier "port the inference calculator"
> design that was rejected as over-engineered — we want **a
> curated list of models we recommend, with their hardware
> requirements as data, not as formulas**.

## Why

The Hub's "Local (built-in)" LLM chip lists models from
`models/manifests/local-llm.json` — today exactly two entries
(Qwen3-1.7B-Q8_0, Qwen3-4B-Q4_K_M). Users have:

- No signal for "which of these fits my GPU?"
- No way to discover models beyond those two
- No notion that other model families (Llama 3, Phi, Mistral)
  are even reachable

[canirun.ai](https://www.canirun.ai/) validates the design: a
curated model catalogue + hardware grades is what users need.
But canirun.ai has no API and no open data; we curate our own,
sourced from Ollama's library + HuggingFace + community
benchmarks.

The result the user sees in the Hub:

```
[Recommended for your hardware]
  Qwen3 4B (Q4_K_M)     ★★★★  ~3.2 GB VRAM  ·  fits on your RTX 4080 (16 GB)  [Download]
  Llama 3.1 8B (Q4_K_M) ★★★★  ~5.6 GB VRAM  ·  fits on your RTX 4080 (16 GB)  [Download]
  Qwen3 1.7B (Q8_0)     ★★★   ~2.0 GB VRAM  ·  fits comfortably               [Installed]

[Will run on your hardware]
  Phi-3 14B (Q4_K_M)    ★★    ~9.1 GB VRAM  ·  tight on 16 GB                  [Download]

[Won't fit on your hardware — install anyway?]
  Llama 3.3 70B (Q4_K_M) ☆    ~42 GB VRAM   ·  needs 48 GB+ VRAM               [Download anyway]
```

## Scope

### 1. Replace local-llm.json with the catalog

The existing `models/manifests/local-llm.json` becomes the
catalog — same file, extended schema. Every entry gains:

- `family: string` — e.g. `"Qwen3"`, `"Llama 3.1"`, `"Phi-3"`
- `params_billions: number`
- `quantisation: string` (already implicit in the id; pulled out)
- `context_window: number` (already in the schema; kept)
- `min_vram_gb: number` — below this, won't load on GPU
- `recommended_vram_gb: number` — above this, runs smoothly
- `min_ram_gb: number` — for the CPU fallback path
- `capabilities: string[]` — tags from `["chat", "tool_use", "hebrew", "code", "vision", "reasoning"]`
- `bundled: boolean` — `true` for the entries Lashon recommends
  at install time (currently the Qwen3 1.7B + 4B pair).
  Catalog defaults to `false`; user has to explicitly install.

The current `files: [...]` block stays — every catalog entry is
installable; downloads land in `$LASHON_MODELS_ROOT/local-llm/`
the same way as today.

**Sourcing the v1 catalogue:** hand-curated. ~15 entries to
start. Pull min/recommended VRAM from Ollama's library
(`ollama.com/library/<model>`); cross-check params + license
against HuggingFace; SHA-256 from upstream LFS pointers
(same discipline as the existing 2 entries). The first PR ships
this baseline; later PRs add models as users ask for them.

Initial v1 catalogue target (subject to refinement during the PR):

| Family | Sizes (quants) | Why |
|---|---|---|
| Qwen3 | 1.7B (Q8_0), 4B (Q4_K_M), 7B (Q4_K_M) | Existing default + larger variants for higher-VRAM users |
| Llama 3.1 | 8B (Q4_K_M, Q8_0) | The de-facto open chat baseline |
| Phi-3 | mini (3.8B, Q4_K_M), medium (14B, Q4_K_M) | Microsoft's tool-use-strong small models |
| Mistral | 7B-Instruct (Q4_K_M, Q8_0) | Apache-2.0 strong baseline |
| Gemma 2 | 9B (Q4_K_M) | Google's open chat tier |
| DeepSeek | R1-Distill-Qwen-7B (Q4_K_M) | Reasoning category |

Hebrew capability is editorial — none of the small open-source
models benchmark well on Hebrew; we tag based on training-mix
language coverage and our own smoke tests. ADR-0022 Invariant 3
still applies: default rating is `Basic` until measured.

### 2. New module: `lashon-core::llm_catalog`

Pure-lookup, no math, no async. Public API:

```rust
pub struct CatalogEntry {  // mirrors a manifest row, deserialised
    pub id: String,
    pub family: String,
    pub display_name: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub params_billions: f64,
    pub quantisation: String,
    pub context_window: u32,
    pub min_vram_gb: f64,
    pub recommended_vram_gb: f64,
    pub min_ram_gb: f64,
    pub capabilities: Vec<String>,
    pub bundled: bool,
    pub files: Vec<CatalogFile>,  // {path, url, bytes, sha256}
    pub installed: bool,
}

pub struct HardwareSpec {
    pub gpu_vram_gb: Option<f64>,
    pub system_ram_gb: f64,
    pub gpu_name: Option<String>,  // e.g. "NVIDIA GeForce RTX 4080"
}

pub enum FitVerdict {
    Recommended,   // gpu_vram >= recommended_vram_gb
    Works,         // gpu_vram >= min_vram_gb (but < recommended)
    CpuOnly,       // no GPU or gpu_vram < min, but system_ram >= min_ram (slow fallback)
    WontFit,       // can't run, period
}

pub struct GradedEntry {
    pub entry: CatalogEntry,
    pub verdict: FitVerdict,
}

pub fn load_catalog() -> Result<Vec<CatalogEntry>>;
pub fn grade(entries: &[CatalogEntry], hw: &HardwareSpec) -> Vec<GradedEntry>;
```

Verdict logic (threshold comparison, no math):

```rust
match hw.gpu_vram_gb {
    Some(vram) if vram >= entry.recommended_vram_gb => Recommended,
    Some(vram) if vram >= entry.min_vram_gb => Works,
    Some(_) | None if hw.system_ram_gb >= entry.min_ram_gb => CpuOnly,
    _ => WontFit,
}
```

Unit tests cover every verdict transition + the bundled Qwen3
pair against a 16 GB RTX 4080 (both `Recommended`) and against
a 4 GB GPU (1.7B `Works`, 4B `Tight` then `CpuOnly`).

### 3. Background refresh from Ollama

Best-effort, non-blocking. On Hub open (or app start), fire a
fetch against Ollama's library endpoint. For each catalog entry
whose `family + params_billions + quantisation` Ollama also
publishes, refresh `min_vram_gb` / `recommended_vram_gb` /
`min_ram_gb` from Ollama's data. Cache the result in
`$LASHON_MODELS_ROOT/local-llm-catalog-cache.json` with a 24-hour
TTL.

The bundled JSON is the floor — if Ollama is unreachable or
the user is offline, the Hub renders from the baked-in
catalogue. Refresh is graceful enhancement only.

The actual Ollama endpoint to query needs verification during
implementation — `ollama.com/api/tags` is the local-daemon API;
the public library has an undocumented JSON schema you scrape
out of the page (or use community wrappers). If no clean source
exists, skip the refresh in v1 and just ship the bundled
catalogue. Document the gap; revisit.

### 4. Plumb the verdict through to the Hub

- `packages/shared-rust/src/model.rs`:
  `available_local_llm_models` returns `Vec<AvailableLocalLlmModel>`;
  extend the struct with `family`, `params_billions`, `capabilities`,
  `bundled`, and the four fit fields (`min_vram_gb`,
  `recommended_vram_gb`, `min_ram_gb`, `verdict` as a lowercase
  string). The grading happens here by reading
  `lashon-core::hardware::probe()` once per call.
- `apps/desktop/src-tauri/src/llm.rs`: extend
  `LocalLlmModelMeta` + the `From<AvailableLocalLlmModel>` impl
  with the new fields.
- `apps/desktop/src/routes/hub/+page.svelte`: the existing
  local-llm section becomes a three-tier list (Recommended,
  Works, Won't fit), each chip showing family + size + VRAM cost
  + verdict copy + GPU name. WontFit chips get a "Download
  anyway" button override + a small warning icon.
- i18n strings for the new copy go under `hub.llm.catalog.*` in
  both `he.json` + `en.json`.

## Decisions already made — DO NOT relitigate

1. **No formula-based math.** No params×quant computation, no KV
   cache prediction. Data, not derivation. (User explicitly
   rejected the calculator-port approach.)
2. **Catalog is single source of truth.** `local-llm.json` becomes
   the catalog; the existing 2 entries get extended with the new
   fields + `bundled: true`. No second manifest.
3. **Hand-curated baseline + background Ollama refresh.** Ship a
   versioned JSON, refresh from Ollama opportunistically. Cache
   in per-user data dir with 24h TTL. Falls back cleanly to
   baked-in catalogue when offline.
4. **v1 surfaces fit only.** No tok/s estimates, no quality
   tiers. Verdict copy is the speed signal. Tok/s + quality come
   in a follow-up PR once we have measurements.
5. **VRAM/RAM thresholds come from Ollama's library** as the
   primary source; our own smoke tests fill gaps. SHA-256 +
   bytes still come from upstream HF LFS pointers, same as
   existing entries.
6. **WontFit doesn't block install.** The button changes to
   "Download anyway" so users can install for a future hardware
   upgrade or for archival.
7. **No internet at runtime is OK.** Refresh is best-effort;
   never blocks the Hub's chip render.

## Files this PR touches

- `models/manifests/local-llm.json` (extend schema; grow to ~15
  entries)
- `packages/shared-rust/src/llm_catalog.rs` (new, ~250 lines)
- `packages/shared-rust/src/lib.rs` (one `pub mod` line)
- `packages/shared-rust/src/model.rs` (extend
  `AvailableLocalLlmModel`; call the grader)
- `apps/desktop/src-tauri/src/llm.rs` (extend
  `LocalLlmModelMeta` + its `From` impl)
- `apps/desktop/src/routes/hub/+page.svelte` (three-tier list +
  verdict colour + "Download anyway")
- `apps/desktop/src/lib/i18n/locales/{he,en}.json`
  (`hub.llm.catalog.*` strings)
- `CLAUDE.md` (one-paragraph branch summary)

Disjoint from the `m8-os-tools` PR's file list except for
`CLAUDE.md`. See "Parallel work" below.

## Definition of done

- Catalog grows to ~15 entries, each with full schema fields +
  SHA-256 / bytes from upstream LFS.
- `llm_catalog` module landed with unit tests covering every
  verdict transition + the 2 bundled entries on a 16 GB GPU.
- Background Ollama refresh wired (or explicitly skipped with a
  note if no clean data source exists in time).
- Hub renders the three-tier list with verdict colour + GPU name
  in the copy.
- WontFit chips render "Download anyway" on the button.
- `cargo test -p lashon-core --lib` green.
- `cargo check -p lashon` clean.
- `npm run check` clean.
- Manual: open Hub on the dev machine, confirm both Qwen3
  entries land in "Recommended", confirm at least one
  larger-than-16-GB entry lands in "Won't fit".
- `CLAUDE.md` branch-summary paragraph updated.

## Parallel work

The `m8-os-tools` PR is being built in parallel from the same
`main` base. Conflict surface is small:

- **`CLAUDE.md`**: both PRs add a branch-summary paragraph. Last
  to merge resolves trivially.
- **`packages/shared-rust/src/lib.rs`**: m8-os-tools adds no new
  `pub mod`; this PR adds one. No conflict.
- **`apps/desktop/src-tauri/src/llm.rs`**: this PR extends
  `LocalLlmModelMeta`. m8-os-tools does not touch this file.
- **`models/manifests/local-llm.json`**: this PR extends the
  schema. m8-os-tools doesn't touch it.

Either PR can land first; the other rebases cleanly.
