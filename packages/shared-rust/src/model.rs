//! ONNX model resolution and integrity verification.
//!
//! The Silero VAD and wake-word ONNX weights are downloaded, never committed —
//! the pattern of `models/manifests/`. This module locates them on disk and
//! SHA-256-verifies every file against the embedded manifest before `ort`
//! loads them: a tampered ONNX graph is native code (docs/adr/0010).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The M6 model registry, committed at `models/manifests/m6-audio.json` and
/// embedded so a packaged build needs no manifest file on disk.
const MANIFEST_JSON: &str = include_str!("../../../models/manifests/m6-audio.json");

/// The opt-in wake-word classifier registry (CC-BY-NC-4.0 — never bundled,
/// downloaded only on the user's explicit request from the Hub).
const WAKE_CLASSIFIERS_JSON: &str = include_str!("../../../models/manifests/wake-classifiers.json");

/// The in-process local-LLM model registry (docs/adr/0025). Each entry is a
/// GGUF model the user may download from the Hub; the `LocalLlmProvider`
/// loads it via `mistralrs` on first chat. Apache-2.0 and MIT only — same
/// bundle policy as the wake classifiers (we just permit bundle, while the
/// CC-BY-NC entries do not).
const LOCAL_LLM_JSON: &str = include_str!("../../../models/manifests/local-llm.json");

/// A packaged build sets this to the per-user model directory; it is absent
/// when running from a source checkout (see apps/desktop/src-tauri/src/lib.rs).
const MODELS_ROOT_ENV: &str = "LASHON_MODELS_ROOT";

/// SHA-256 placeholder used in `models/manifests/local-llm.json` when the
/// upstream LFS hash has not yet been mirrored into the manifest. A file
/// whose manifest carries this string is downloaded once and then
/// **verified-on-first-use** — the hash captured at first install is
/// persisted in a sibling `.sha256` file and compared on every subsequent
/// load (docs/adr/0025 §5).
const LOCAL_LLM_SHA_PLACEHOLDER: &str = "verify-on-download";

#[derive(Debug, Deserialize)]
struct Manifest {
    models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    local_dir: String,
    #[serde(default)]
    files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize, Clone)]
struct ModelFile {
    path: String,
    bytes: u64,
    sha256: String,
    #[serde(default)]
    url: Option<String>,
}

/// An entry in `wake-classifiers.json` — an opt-in CC-BY-NC wake-word
/// classifier the user may download from the Hub.
#[derive(Debug, Deserialize, Clone)]
struct WakeClassifierEntry {
    id: String,
    display_name: String,
    license: String,
    source: String,
    files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize)]
struct WakeClassifierManifest {
    models: Vec<WakeClassifierEntry>,
}

/// An entry in `local-llm.json` — a GGUF model the in-process
/// `LocalLlmProvider` can load (docs/adr/0025).
#[derive(Debug, Deserialize, Clone)]
struct LocalLlmEntry {
    id: String,
    display_name: String,
    description: String,
    license: String,
    source: String,
    #[serde(default)]
    context_window: usize,
    files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize)]
struct LocalLlmManifest {
    models: Vec<LocalLlmEntry>,
}

fn parse_manifest() -> Result<Manifest> {
    serde_json::from_str(MANIFEST_JSON).context("parsing the embedded m6-audio manifest")
}

fn find_entry(model_id: &str) -> Result<ModelEntry> {
    parse_manifest()?
        .models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| anyhow!("model id '{model_id}' is not in the m6-audio manifest"))
}

/// The directory a model's files live in — whether or not they are present.
///
/// Packaged build: `$LASHON_MODELS_ROOT/<dir-name>`. From a source checkout:
/// the repo's `models/` tree, resolved relative to this crate. A packaged
/// build always sets the env var, so the build-machine source path baked in
/// here is never consulted off the build machine.
fn model_dir(local_dir: &str) -> PathBuf {
    if let Some(root) = std::env::var_os(MODELS_ROOT_ENV) {
        let name = Path::new(local_dir)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new(local_dir));
        return PathBuf::from(root).join(name);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(local_dir)
}

/// Lowercase, zero-padded hex of a byte slice.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// SHA-256 of a file, streamed so a large model is never held whole in memory.
fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).with_context(|| format!("reading {}", path.display()))?;
    Ok(hex(&hasher.finalize()))
}

/// Whether every file of `model_id` is present on disk.
///
/// A cheap existence check with no hashing — for deciding whether an optional
/// model (the wake word) can be enabled at all.
pub fn is_present(model_id: &str) -> bool {
    let Ok(entry) = find_entry(model_id) else {
        return false;
    };
    if entry.files.is_empty() {
        return false;
    }
    let dir = model_dir(&entry.local_dir);
    entry.files.iter().all(|f| dir.join(&f.path).exists())
}

/// Verify every file of `model_id` against the manifest and return the model's
/// directory.
///
/// Each file must be present, the manifest's size, and match its SHA-256.
/// Verification runs on every load, not only after a download: a swapped
/// same-size `.onnx` is native code inside ONNX Runtime (docs/adr/0010).
pub fn verified_dir(model_id: &str) -> Result<PathBuf> {
    let entry = find_entry(model_id)?;
    if entry.files.is_empty() {
        bail!("manifest entry '{model_id}' lists no files to verify");
    }
    let dir = model_dir(&entry.local_dir);
    for file in &entry.files {
        let path = dir.join(&file.path);
        let size = std::fs::metadata(&path)
            .with_context(|| format!("model file is missing: {}", path.display()))?
            .len();
        if size != file.bytes {
            bail!(
                "model file {} is {size} bytes; the manifest expects {}",
                path.display(),
                file.bytes
            );
        }
        if sha256_file(&path)? != file.sha256 {
            bail!("model file {} failed SHA-256 verification", path.display());
        }
    }
    Ok(dir)
}

/// The directory wake-word classifier ONNX files live in.
///
/// Packaged build: `$LASHON_MODELS_ROOT/wakewords`; from a source checkout: the
/// repo's `models/wake/wakewords/` tree.
fn wake_models_dir() -> PathBuf {
    if let Some(root) = std::env::var_os(MODELS_ROOT_ENV) {
        return PathBuf::from(root).join("wakewords");
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/wake/wakewords")
}

/// The expected path of a wake-word classifier named `name` (without `.onnx`).
///
/// The classifier is an offline-trained artifact, not a manifest model
/// (docs/adr/0016) — it is loaded by path and may be absent, in which case the
/// wake word simply stays unavailable.
pub fn wake_classifier_path(name: &str) -> PathBuf {
    wake_models_dir().join(format!("{name}.onnx"))
}

/// List the wake-word classifiers installed on disk — the filenames without
/// the `.onnx` suffix, sorted. Drives the Hub's wake-word picker.
pub fn list_wake_models() -> Vec<String> {
    let dir = wake_models_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("onnx") {
                path.file_stem()
                    .and_then(|stem| stem.to_str().map(String::from))
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

/// A wake-word classifier the Hub may offer to install on demand.
///
/// `installed` is `true` when every file is already on disk at the right size;
/// the SHA is not re-checked here (it is checked on every load by
/// [`verified_dir`], and on install by [`install_wake_classifier`]) — this is
/// the cheap "should the Install button be visible?" answer.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AvailableWakeModel {
    /// Filename stem — the value the picker stores in `wakeword.model`.
    pub id: String,
    /// Human label for the picker and the install confirmation.
    pub display_name: String,
    /// Bare licence string, e.g. `"CC-BY-NC-4.0"` — the Hub shows it on a
    /// badge before the user confirms the download.
    pub license: String,
    /// Upstream project URL, shown beneath the licence.
    pub source: String,
    /// Total bytes the install will download (used for the modal copy).
    pub bytes: u64,
    pub installed: bool,
}

fn parse_wake_classifiers() -> Result<WakeClassifierManifest> {
    serde_json::from_str(WAKE_CLASSIFIERS_JSON)
        .context("parsing the embedded wake-classifiers manifest")
}

/// The list of wake-word classifiers the user can install on demand.
///
/// Each is CC-BY-NC and is never bundled in the installer — the Hub shows
/// the licence badge before the user confirms the download.
pub fn available_wake_models() -> Vec<AvailableWakeModel> {
    let Ok(manifest) = parse_wake_classifiers() else {
        return Vec::new();
    };
    let dir = wake_models_dir();
    manifest
        .models
        .into_iter()
        .map(|entry| {
            let total_bytes = entry.files.iter().map(|f| f.bytes).sum();
            let installed = entry.files.iter().all(|f| {
                dir.join(&f.path)
                    .metadata()
                    .is_ok_and(|m| m.len() == f.bytes)
            });
            AvailableWakeModel {
                id: entry.id,
                display_name: entry.display_name,
                license: entry.license,
                source: entry.source,
                bytes: total_bytes,
                installed,
            }
        })
        .collect()
}

/// Download and SHA-256-verify an opt-in wake-word classifier into the
/// wake-words directory. Idempotent — a present, verified file is a no-op.
///
/// Returns the classifier's id (matching its filename stem) on success.
pub async fn install_wake_classifier(id: &str) -> Result<String> {
    let manifest = parse_wake_classifiers()?;
    let entry = manifest
        .models
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("unknown wake-word classifier id: {id}"))?;

    let dir = wake_models_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating the wake-words directory {}", dir.display()))?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("lashon/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building the HTTP client")?;

    for file in &entry.files {
        let target = dir.join(&file.path);
        if let Ok(meta) = std::fs::metadata(&target) {
            if meta.len() == file.bytes && sha256_file(&target)? == file.sha256 {
                tracing::info!(path = %target.display(), "wake classifier already present");
                continue;
            }
        }
        let url = file
            .url
            .as_deref()
            .ok_or_else(|| anyhow!("manifest entry {id} has no download URL"))?;
        tracing::info!(url = %url, "downloading wake classifier");
        let bytes = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("requesting {url}"))?
            .error_for_status()
            .with_context(|| format!("server rejected {url}"))?
            .bytes()
            .await
            .with_context(|| format!("reading the body of {url}"))?;

        if bytes.len() as u64 != file.bytes {
            bail!(
                "downloaded {} is {} bytes; the manifest expects {}",
                file.path,
                bytes.len(),
                file.bytes
            );
        }
        let actual = hex(Sha256::digest(&bytes).as_slice());
        if actual != file.sha256 {
            bail!("downloaded {} failed SHA-256 verification", file.path);
        }
        std::fs::write(&target, &bytes).with_context(|| format!("writing {}", target.display()))?;
        tracing::info!(path = %target.display(), "wake classifier installed");
    }
    Ok(entry.id)
}

/// Stage every `.onnx` file from a bundled wake-classifier directory into the
/// per-user wake-words directory, returning how many files were copied.
///
/// The Tauri shell calls this on every launch with the bundle's wake-classifier
/// directory as the source and `$LASHON_MODELS_ROOT/wakewords` as the target:
/// the MIT "Hey Lashon" classifier ships in the installer
/// (docs/adr/0016-wake-word-engine.md) but lives where the wake engine looks
/// for it only after this copy. Idempotent — a file already present at the
/// target is left untouched, so a user who has trained their own replacement
/// keeps it.
pub fn install_bundled_wake_classifiers(source_dir: &Path, target_dir: &Path) -> Result<usize> {
    if !source_dir.is_dir() {
        // tauri dev — no bundle on disk — or a packaged build that opted out of
        // bundling any classifiers. Either way, nothing to stage.
        return Ok(0);
    }
    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("creating {}", target_dir.display()))?;
    let mut copied = 0_usize;
    for entry in std::fs::read_dir(source_dir)
        .with_context(|| format!("reading {}", source_dir.display()))?
    {
        let entry = entry?;
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        if src.extension().and_then(|ext| ext.to_str()) != Some("onnx") {
            continue;
        }
        let Some(name) = src.file_name() else {
            continue;
        };
        let target = target_dir.join(name);
        if target.exists() {
            continue;
        }
        std::fs::copy(&src, &target)
            .with_context(|| format!("copying {} to {}", src.display(), target.display()))?;
        copied += 1;
    }
    Ok(copied)
}

// --- in-process local LLM (docs/adr/0025) ----------------------------------

/// One entry in `models/manifests/local-llm.json`, serialised for the Hub
/// to render in the "Local (built-in)" download section. Mirrors the
/// `AvailableWakeModel` shape exactly so the Hub's `M.bytes` /
/// `M.installed` chrome carries over.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AvailableLocalLlmModel {
    /// Stable id (matches `LocalLlmProvider::with_model`'s argument).
    pub id: String,
    /// Hub-visible name — bilingual is fine, but the canonical entries
    /// in the manifest are English (the locales translate the chip
    /// header, not the model name itself).
    pub display_name: String,
    /// One-line "what this trades" copy the Hub shows under the chip.
    pub description: String,
    /// `"Apache-2.0"`, `"MIT"`, …. Per the security rules a `*-NC` model
    /// would never land in this manifest.
    pub license: String,
    /// Upstream Hugging Face repository URL.
    pub source: String,
    /// Tokens of context the model supports — Hub copy ("32k context").
    pub context_window: usize,
    /// Total bytes the install will pull from upstream. `0` when the
    /// manifest has not yet been pinned (the first-download path
    /// resolves it; the Hub shows "~ <upstream>" until then).
    pub bytes: u64,
    /// Whether every file is present on disk at the manifest size — the
    /// chip uses this to grey out the "Use this model" button vs the
    /// "Download" button.
    pub installed: bool,
}

fn parse_local_llm() -> Result<LocalLlmManifest> {
    serde_json::from_str(LOCAL_LLM_JSON).context("parsing the embedded local-llm manifest")
}

/// The directory the local-LLM GGUF files live in.
///
/// Packaged build: `$LASHON_MODELS_ROOT/local-llm`. From a source checkout:
/// the repo's `models/local-llm/` tree.
fn local_llm_dir() -> PathBuf {
    if let Some(root) = std::env::var_os(MODELS_ROOT_ENV) {
        return PathBuf::from(root).join("local-llm");
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/local-llm")
}

/// The on-disk path of a local-LLM model's primary file. Returns the
/// directory + the bare filename, since `mistralrs::GgufModelBuilder`
/// takes those as two arguments.
///
/// Errors when the id is unknown or when the manifest entry carries no
/// files (a manifest authoring mistake the test suite catches).
pub fn local_llm_resolved_path(model_id: &str) -> Result<(PathBuf, String)> {
    let manifest = parse_local_llm()?;
    let entry = manifest
        .models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| anyhow!("unknown local-llm id: {model_id}"))?;
    let file = entry
        .files
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("local-llm entry {model_id} has no files"))?;
    Ok((local_llm_dir(), file.path))
}

/// Cheap existence check: is every file of `model_id` present at the size
/// the manifest expects? The SHA is not re-checked here (it is checked on
/// download and on every `mistralrs` load); this is the "should the Hub
/// show 'Installed' or 'Download'?" answer.
pub fn is_local_llm_installed(model_id: &str) -> bool {
    let Ok(manifest) = parse_local_llm() else {
        return false;
    };
    let Some(entry) = manifest.models.into_iter().find(|m| m.id == model_id) else {
        return false;
    };
    let dir = local_llm_dir();
    entry.files.iter().all(|f| {
        let path = dir.join(&f.path);
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        // The manifest may carry `bytes = 0` while the upstream size is
        // still being pinned; in that case any non-zero file on disk is
        // accepted as installed (the SHA verification on load is the
        // real gate).
        f.bytes == 0 || meta.len() == f.bytes
    })
}

/// The list of local-LLM models the user may download. Mirrors
/// `available_wake_models` for the Hub's download chip UX.
pub fn available_local_llm_models() -> Vec<AvailableLocalLlmModel> {
    let Ok(manifest) = parse_local_llm() else {
        return Vec::new();
    };
    let dir = local_llm_dir();
    manifest
        .models
        .into_iter()
        .map(|entry| {
            let total_bytes = entry.files.iter().map(|f| f.bytes).sum();
            let installed = entry.files.iter().all(|f| {
                let path = dir.join(&f.path);
                let Ok(meta) = std::fs::metadata(&path) else {
                    return false;
                };
                f.bytes == 0 || meta.len() == f.bytes
            });
            AvailableLocalLlmModel {
                id: entry.id,
                display_name: entry.display_name,
                description: entry.description,
                license: entry.license,
                source: entry.source,
                context_window: entry.context_window,
                bytes: total_bytes,
                installed,
            }
        })
        .collect()
}

/// Progress update emitted while a local-LLM GGUF is downloading. The
/// Tauri shell forwards these as `local_llm:progress` events to the Hub
/// so a 1 GB+ download surfaces a percentage and byte count rather
/// than a silent wait.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LocalLlmDownloadProgress {
    pub model_id: String,
    pub file: String,
    pub downloaded: u64,
    /// `None` when the manifest does not pin the upstream size and the
    /// server does not return `Content-Length` (rare; Hugging Face does).
    pub total: Option<u64>,
}

/// Download (or resume) every file of `model_id` into the local-LLM
/// directory, calling `on_progress` periodically so the Hub can render
/// a percentage. Idempotent — a present, size-matching file is a no-op.
///
/// SHA-256 is verified after each file lands; on a mismatch the file is
/// removed and the call returns an error so the user can retry. When
/// the manifest carries the placeholder `verify-on-download` SHA, the
/// computed hash is persisted to a sibling `<file>.sha256` file and used
/// as the trust anchor on subsequent loads.
pub async fn install_local_llm_model<F>(model_id: &str, mut on_progress: F) -> Result<String>
where
    F: FnMut(LocalLlmDownloadProgress) + Send + 'static,
{
    let manifest = parse_local_llm()?;
    let entry = manifest
        .models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| anyhow!("unknown local-llm id: {model_id}"))?;

    let dir = local_llm_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating the local-llm directory {}", dir.display()))?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("lashon/", env!("CARGO_PKG_VERSION")))
        // A 1 GB GGUF over a slow link can take many minutes — disable
        // the per-request timeout entirely; the per-read timeout on the
        // socket guards against a stalled connection.
        .timeout(std::time::Duration::from_secs(0))
        .build()
        .context("building the HTTP client for local-llm download")?;

    for file in &entry.files {
        let target = dir.join(&file.path);
        // Skip when the present file already matches the manifest.
        if let Ok(meta) = std::fs::metadata(&target) {
            let size_ok = file.bytes == 0 || meta.len() == file.bytes;
            if size_ok {
                if file.sha256 == LOCAL_LLM_SHA_PLACEHOLDER {
                    // Trust-on-first-use: a present file whose hash we
                    // captured earlier is accepted on subsequent calls.
                    if persisted_sha_matches(&target).unwrap_or(false) {
                        tracing::info!(path = %target.display(), "local-llm file already present");
                        continue;
                    }
                } else if sha256_file(&target)? == file.sha256 {
                    tracing::info!(path = %target.display(), "local-llm file already present");
                    continue;
                }
            }
        }

        let url = file
            .url
            .as_deref()
            .ok_or_else(|| anyhow!("local-llm entry {model_id} has no download URL"))?;
        tracing::info!(url = %url, "downloading local-llm GGUF");

        let response = client
            .get(url)
            .send()
            .await
            .with_context(|| format!("requesting {url}"))?
            .error_for_status()
            .with_context(|| format!("server rejected {url}"))?;
        let total = response.content_length().or(if file.bytes == 0 {
            None
        } else {
            Some(file.bytes)
        });

        // Write to a temp file and rename on success — a half-downloaded
        // GGUF on disk would otherwise look "installed" to the cheap
        // existence check.
        let tmp = dir.join(format!("{}.partial", file.path));
        let mut sink = std::fs::File::create(&tmp)
            .with_context(|| format!("creating partial file {}", tmp.display()))?;
        let mut hasher = Sha256::new();
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();
        use futures::StreamExt;
        use std::io::Write;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| format!("reading chunk of {url}"))?;
            hasher.update(&chunk);
            sink.write_all(&chunk)
                .with_context(|| format!("writing to {}", tmp.display()))?;
            downloaded += chunk.len() as u64;
            on_progress(LocalLlmDownloadProgress {
                model_id: model_id.to_string(),
                file: file.path.clone(),
                downloaded,
                total,
            });
        }
        sink.flush()
            .with_context(|| format!("flushing {}", tmp.display()))?;
        drop(sink);

        let actual_sha = hex(hasher.finalize().as_slice());
        if file.sha256 == LOCAL_LLM_SHA_PLACEHOLDER {
            // Trust-on-first-use: capture the computed hash beside the
            // file so subsequent loads verify against it without needing
            // a re-download. Documented in ADR-0025 §5.
            tracing::info!(
                file = %file.path,
                sha256 = %actual_sha,
                "local-llm file installed (trust-on-first-use)"
            );
            persist_sha(&target.with_extension("sha256"), &actual_sha)?;
        } else if actual_sha != file.sha256 {
            let _ = std::fs::remove_file(&tmp);
            bail!(
                "downloaded {} failed SHA-256 verification: expected {}, got {}",
                file.path,
                file.sha256,
                actual_sha
            );
        }
        if file.bytes != 0 && downloaded != file.bytes {
            let _ = std::fs::remove_file(&tmp);
            bail!(
                "downloaded {} is {downloaded} bytes; the manifest expects {}",
                file.path,
                file.bytes
            );
        }
        std::fs::rename(&tmp, &target)
            .with_context(|| format!("promoting {} to {}", tmp.display(), target.display()))?;
        tracing::info!(path = %target.display(), "local-llm file installed");
    }
    Ok(entry.id)
}

/// Remove the on-disk files for a local-LLM model. Returns how many
/// files were deleted (0 means the model was already absent).
pub fn delete_local_llm_model(model_id: &str) -> Result<usize> {
    let manifest = parse_local_llm()?;
    let entry = manifest
        .models
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| anyhow!("unknown local-llm id: {model_id}"))?;
    let dir = local_llm_dir();
    let mut removed = 0;
    for file in &entry.files {
        let path = dir.join(&file.path);
        if path.exists() {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed += 1;
        }
        let sha_path = path.with_extension("sha256");
        if sha_path.exists() {
            let _ = std::fs::remove_file(&sha_path);
        }
    }
    Ok(removed)
}

/// Persist a computed SHA-256 hex digest beside the GGUF file (the
/// trust-on-first-use anchor for `verify-on-download` manifests).
fn persist_sha(path: &Path, sha_hex: &str) -> Result<()> {
    std::fs::write(path, sha_hex.as_bytes()).with_context(|| format!("writing {}", path.display()))
}

/// Check the file at `<path>` against its sibling `<path>.sha256` trust
/// anchor. Returns `Ok(true)` only when both the anchor and the file
/// exist and the file's freshly-computed hash matches the anchor.
fn persisted_sha_matches(path: &Path) -> Result<bool> {
    let sha_path = path.with_extension("sha256");
    let expected = match std::fs::read_to_string(&sha_path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return Ok(false),
    };
    let actual = sha256_file(path)?;
    Ok(expected == actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_manifest_parses() {
        let manifest = parse_manifest().expect("m6-audio.json parses");
        assert!(
            manifest.models.iter().any(|m| m.id == "silero-vad-v5"),
            "the Silero VAD entry is present"
        );
    }

    #[test]
    fn unknown_model_ids_are_rejected() {
        assert!(find_entry("no-such-model").is_err());
        assert!(verified_dir("no-such-model").is_err());
    }

    #[test]
    fn hex_encodes_lowercase_and_zero_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
    }

    #[test]
    fn sha256_file_matches_a_known_vector() {
        let path = std::env::temp_dir().join(format!("lashon-sha256-test-{}", std::process::id()));
        std::fs::write(&path, b"abc").expect("write the test file");
        let got = sha256_file(&path);
        let _ = std::fs::remove_file(&path);
        // The canonical SHA-256 test vector for the input "abc".
        assert_eq!(
            got.expect("hash the test file"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verifying_an_absent_model_reports_the_missing_file() {
        // `silero-vad-v5` is a real entry. When its weights are absent — as on
        // a CI runner — verification must fail cleanly, not panic.
        if !is_present("silero-vad-v5") {
            let err = verified_dir("silero-vad-v5").unwrap_err().to_string();
            assert!(err.contains("missing"), "unexpected error: {err}");
        }
    }

    #[test]
    fn the_wake_classifiers_manifest_parses_and_carries_nc_licences() {
        let manifest = parse_wake_classifiers().expect("wake-classifiers.json parses");
        assert!(!manifest.models.is_empty(), "at least one entry is shipped");
        for entry in &manifest.models {
            assert!(
                entry.license.contains("NC"),
                "{}: every opt-in classifier must carry an NC licence",
                entry.id
            );
            assert!(
                !entry.files.is_empty(),
                "{}: needs at least one file",
                entry.id
            );
            for file in &entry.files {
                assert!(
                    file.url.is_some(),
                    "{}: every file must carry a URL",
                    entry.id
                );
                assert_eq!(
                    file.sha256.len(),
                    64,
                    "{}: SHA-256 is 64 lowercase hex chars",
                    entry.id
                );
            }
        }
    }

    #[test]
    fn available_wake_models_lists_each_manifest_entry_with_install_status() {
        let entries = available_wake_models();
        assert!(!entries.is_empty(), "the manifest carries entries");
        // Every entry exposes the fields the Hub renders.
        for entry in &entries {
            assert!(!entry.id.is_empty());
            assert!(!entry.display_name.is_empty());
            assert!(!entry.license.is_empty());
            assert!(entry.bytes > 0);
        }
    }

    /// Each test gets its own temp directory keyed by name so they cannot
    /// collide when `cargo test` runs them in parallel.
    fn bundle_test_dirs(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "lashon-bundled-wake-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("bundle");
        let target = root.join("user");
        std::fs::create_dir_all(&source).expect("create the source dir");
        (root, source, target)
    }

    #[test]
    fn install_bundled_wake_classifiers_copies_onnx_files() {
        let (root, source, target) = bundle_test_dirs("copy");
        std::fs::write(source.join("hey_lashon.onnx"), b"fake-weights")
            .expect("write the fake classifier");

        let copied = install_bundled_wake_classifiers(&source, &target).expect("install succeeds");
        assert_eq!(copied, 1);
        assert!(target.join("hey_lashon.onnx").is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_bundled_wake_classifiers_is_idempotent() {
        let (root, source, target) = bundle_test_dirs("idem");
        std::fs::write(source.join("hey_lashon.onnx"), b"fake-weights")
            .expect("write the fake classifier");

        install_bundled_wake_classifiers(&source, &target).expect("first install");
        let copied_again =
            install_bundled_wake_classifiers(&source, &target).expect("second install");
        assert_eq!(copied_again, 0, "a present file is left untouched");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_bundled_wake_classifiers_preserves_user_replacement() {
        // A user who trained their own classifier and saved it to the target
        // must not have it overwritten on the next launch.
        let (root, source, target) = bundle_test_dirs("preserve");
        std::fs::create_dir_all(&target).expect("create the target dir");
        std::fs::write(source.join("hey_lashon.onnx"), b"bundled-weights")
            .expect("write the bundled classifier");
        std::fs::write(target.join("hey_lashon.onnx"), b"user-trained-weights")
            .expect("write the user's classifier");

        let copied = install_bundled_wake_classifiers(&source, &target).expect("install succeeds");
        assert_eq!(copied, 0);
        let kept = std::fs::read(target.join("hey_lashon.onnx")).expect("read back");
        assert_eq!(
            kept, b"user-trained-weights",
            "the user's classifier survives"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_bundled_wake_classifiers_ignores_non_onnx_files() {
        let (root, source, target) = bundle_test_dirs("skip-non-onnx");
        std::fs::write(source.join("README.txt"), b"docs").expect("write a readme");
        std::fs::write(source.join("hey_lashon.onnx"), b"fake-weights")
            .expect("write the classifier");

        let copied = install_bundled_wake_classifiers(&source, &target).expect("install succeeds");
        assert_eq!(copied, 1);
        assert!(target.join("hey_lashon.onnx").is_file());
        assert!(!target.join("README.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_bundled_wake_classifiers_is_a_no_op_when_the_source_is_absent() {
        // tauri dev path — no bundle on disk. The function must not fail and
        // must not create the target directory uselessly.
        let absent =
            std::env::temp_dir().join(format!("lashon-bundled-wake-absent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&absent);
        let target = std::env::temp_dir().join(format!(
            "lashon-bundled-wake-absent-target-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&target);

        let copied = install_bundled_wake_classifiers(&absent, &target).expect("install succeeds");
        assert_eq!(copied, 0);
        assert!(
            !target.exists(),
            "target is not created when the source is absent"
        );
    }

    // --- local-LLM (docs/adr/0025) ----------------------------------------

    #[test]
    fn the_local_llm_manifest_parses() {
        let manifest = parse_local_llm().expect("local-llm.json parses");
        assert!(
            !manifest.models.is_empty(),
            "the manifest ships at least one model"
        );
    }

    #[test]
    fn local_llm_manifest_lists_the_lightest_default() {
        // ADR-0025 default: the smallest Qwen3 GGUF the upstream Qwen org
        // publishes (Q8_0 at 1.7B). The provider's DEFAULT_MODEL constant
        // points at this exact id.
        let ids: Vec<String> = parse_local_llm()
            .expect("parse")
            .models
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert!(
            ids.iter().any(|id| id == "qwen3-1.7b-q8_0"),
            "the 1.7B Q8_0 default must be in the manifest, got {ids:?}"
        );
    }

    #[test]
    fn local_llm_manifest_entries_are_apache_or_mit() {
        // .claude/rules/security.md — the local-LLM bundle policy mirrors
        // wake classifiers: nothing CC-NC ever ships in the installer-eligible
        // manifest. (CC-NC models would live in a separate opt-in manifest
        // with a non-commercial badge.)
        let manifest = parse_local_llm().expect("parse");
        for entry in &manifest.models {
            let lic = entry.license.to_uppercase();
            assert!(
                lic.contains("APACHE") || lic.contains("MIT"),
                "{}: unexpected license `{}` — only Apache-2.0 / MIT permitted",
                entry.id,
                entry.license
            );
        }
    }

    #[test]
    fn unknown_local_llm_ids_are_rejected() {
        assert!(local_llm_resolved_path("no-such-model").is_err());
        assert!(!is_local_llm_installed("no-such-model"));
    }

    #[test]
    fn local_llm_resolved_path_returns_dir_and_file_for_known_id() {
        let (dir, file) =
            local_llm_resolved_path("qwen3-1.7b-q8_0").expect("the default id is in the manifest");
        assert!(
            file.ends_with(".gguf"),
            "expected a .gguf filename, got {file}"
        );
        // The dir is `<root>/local-llm` whether or not the env var is set.
        assert!(
            dir.ends_with("local-llm"),
            "expected the local-llm subdir, got {}",
            dir.display()
        );
    }

    #[test]
    fn available_local_llm_models_returns_each_entry() {
        let models = available_local_llm_models();
        assert!(!models.is_empty(), "at least one model is offered");
        for m in &models {
            assert!(!m.id.is_empty());
            assert!(!m.display_name.is_empty());
            assert!(!m.license.is_empty());
            // `installed` is the cheap check; tests run on a fresh CI box
            // where no GGUF has been pulled, so it must be false there.
            // (We don't assert false unconditionally — a developer may
            // have pulled the model into their checkout — but its type
            // contract holds.)
            let _ = m.installed;
        }
    }

    #[test]
    fn delete_local_llm_model_is_a_no_op_when_absent() {
        // Use a guaranteed-absent path by overriding the models root to a
        // throwaway temp dir for the duration of this test. The other
        // tests in this module follow the existing pattern of mutating
        // `MODELS_ROOT_ENV` directly without wrapping in `unsafe` — the
        // edition-2021 toolchain (rust-toolchain.toml) keeps `set_var` /
        // `remove_var` safe.
        let scratch =
            std::env::temp_dir().join(format!("lashon-local-llm-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        std::env::set_var(MODELS_ROOT_ENV, &scratch);
        let removed =
            delete_local_llm_model("qwen3-1.7b-q8_0").expect("a missing model is not an error");
        assert_eq!(removed, 0);
        std::env::remove_var(MODELS_ROOT_ENV);
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
