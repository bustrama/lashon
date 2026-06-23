//! Tauri-side wiring for the M7 LLM provider mux (`docs/adr/0019`,
//! `docs/adr/0020`, `docs/adr/0021`).
//!
//! This module owns the catalogue of known providers and constructs a fresh
//! provider instance on each `test_llm_prompt` call from the user's persisted
//! settings — base URL, model, and (where required) the API key the
//! `lashon-core::keychain` module fetches.
//!
//! The `lashon-core::provider_registry::ProviderRegistry` type is not
//! materialised here; it is the M8 callers (Command mode, Chat mode) that
//! will wire it in. M7's Hub never holds a long-lived `dyn LLMProvider` —
//! every dispatch reads the latest persistence and constructs ad-hoc.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use lashon_core::keychain;
use lashon_core::llama_server::{
    ready_llama_server, resolve_server_exe, LlamaServerState, SpawnConfig,
};
use lashon_core::llm::{
    anthropic::{
        AnthropicLlmProvider, AVAILABLE_MODELS as ANTHROPIC_MODELS,
        DEFAULT_MODEL as ANTHROPIC_DEFAULT_MODEL,
    },
    local::{
        LocalLlmProvider, AVAILABLE_MODELS as LOCAL_LLM_MODELS,
        CONTEXT_WINDOW as LOCAL_LLM_CONTEXT, DEFAULT_MODEL as LOCAL_LLM_DEFAULT,
        DISPLAY_NAME_KEY as LOCAL_LLM_DISPLAY_KEY, PROVIDER_ID as LOCAL_LLM_ID,
        RUNTIME_AVAILABLE as LOCAL_LLM_RUNTIME_AVAILABLE,
    },
    openai_compat::{
        detect_ollama as detect_ollama_core, OllamaDetection, OpenAiCompatConfig,
        OpenAiCompatLlmProvider, ALL_VENDORS,
    },
    LLMProvider, Msg,
};
use lashon_core::model::{
    available_local_llm_models, delete_local_llm_model as delete_local_llm_core,
    install_local_llm_model as install_local_llm_core, is_local_llm_installed,
    local_llm_resolved_path, AvailableLocalLlmModel,
};
use lashon_core::provider::{Confidence, ProviderMeta};

/// LLM mode — the persistence schema is keyed off this
/// (`llm.command.provider`, `llm.chat.provider`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmMode {
    Command,
    Chat,
}

impl LlmMode {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "command" => Ok(Self::Command),
            "chat" => Ok(Self::Chat),
            other => Err(anyhow!("unknown LLM mode: {other}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Chat => "chat",
        }
    }
}

/// What this catalogue knows about a single provider — both the metadata the
/// Hub renders and the construction recipe we use when we need a live impl.
struct ProviderDescriptor {
    id: &'static str,
    display_name_key: &'static str,
    is_local: bool,
    supports_hebrew: Confidence,
    supports_tool_use: bool,
    context_window: usize,
    default_model: &'static str,
    available_models: Vec<String>,
    /// Hub's "מומלץ / recommended" marker. The Tauri layer forwards
    /// it verbatim — it points at an entry in `available_models`.
    recommended_model: Option<&'static str>,
    kind: ProviderKind,
}

enum ProviderKind {
    Anthropic,
    OpenAiCompat(OpenAiCompatConfig),
    /// In-process local LLM via `mistralrs` (docs/adr/0025) — no
    /// external daemon required.
    LocalLlm,
}

/// The full catalogue, materialised at first use. Pure data — no I/O.
fn catalogue() -> Vec<ProviderDescriptor> {
    let mut entries: Vec<ProviderDescriptor> = vec![
        // Local-first: the in-process LLM tops the chip grid so users
        // who pick Lashon for its local-first ethos see it before any
        // cloud chip (docs/adr/0025, docs/adr/0022 Invariant 1).
        ProviderDescriptor {
            id: LOCAL_LLM_ID,
            display_name_key: LOCAL_LLM_DISPLAY_KEY,
            is_local: true,
            supports_hebrew: Confidence::Basic,
            supports_tool_use: true,
            context_window: LOCAL_LLM_CONTEXT,
            default_model: LOCAL_LLM_DEFAULT,
            available_models: LOCAL_LLM_MODELS.iter().map(|s| s.to_string()).collect(),
            recommended_model: Some(LOCAL_LLM_DEFAULT),
            kind: ProviderKind::LocalLlm,
        },
        ProviderDescriptor {
            id: "anthropic",
            display_name_key: "provider.llm.anthropic",
            is_local: false,
            supports_hebrew: Confidence::Excellent,
            supports_tool_use: true,
            context_window: 200_000,
            default_model: ANTHROPIC_DEFAULT_MODEL,
            available_models: ANTHROPIC_MODELS.iter().map(|s| s.to_string()).collect(),
            recommended_model: None,
            kind: ProviderKind::Anthropic,
        },
    ];
    for vendor in ALL_VENDORS {
        entries.push(ProviderDescriptor {
            id: vendor.name,
            display_name_key: vendor.display_name_key,
            is_local: vendor.is_local,
            supports_hebrew: vendor.supports_hebrew,
            supports_tool_use: vendor.supports_tool_use,
            context_window: vendor.context_window,
            default_model: vendor.default_model,
            available_models: vendor
                .available_models
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            recommended_model: vendor.recommended_model,
            kind: ProviderKind::OpenAiCompat(*vendor),
        });
    }
    entries
}

/// Settings-store key conventions, captured in one place
/// (`docs/adr/0021` Persistence Schema).
fn key_active_provider(mode: LlmMode) -> String {
    format!("llm.{}.provider", mode.as_str())
}
fn key_active_model(mode: LlmMode) -> String {
    format!("llm.{}.model", mode.as_str())
}
fn key_base_url(provider: &str) -> String {
    format!("llm.{provider}.base_url")
}

fn read_string(app: &AppHandle, key: &str) -> Option<String> {
    let store = app.store("settings.json").ok()?;
    store
        .get(key)
        .and_then(|value| value.as_str().map(|s| s.to_string()))
}

/// Build a fresh `dyn LLMProvider` for `descriptor` with the user's
/// persisted overrides folded in. Returns `None` when the descriptor's id
/// is `"none"`.
fn build_provider(
    descriptor: &ProviderDescriptor,
    base_url_override: Option<String>,
    model_override: Option<String>,
) -> Box<dyn LLMProvider> {
    let model = model_override.unwrap_or_else(|| descriptor.default_model.to_string());
    let base_url = base_url_override.unwrap_or_default();
    match descriptor.kind {
        ProviderKind::Anthropic => {
            let mut provider = AnthropicLlmProvider::new();
            if !base_url.is_empty() {
                provider = provider.with_base_url(base_url);
            }
            provider = provider.with_model(model);
            Box::new(provider)
        }
        ProviderKind::OpenAiCompat(config) => {
            let mut provider = OpenAiCompatLlmProvider::new(config);
            if !base_url.is_empty() {
                provider = provider.with_base_url(base_url);
            }
            provider = provider.with_model(model);
            Box::new(provider)
        }
        ProviderKind::LocalLlm => {
            // `base_url` is the loopback URL of the Lashon-managed
            // `llama-server` subprocess. Resolved by the async caller
            // via `ensure_local_llm_base_url` before this sync helper
            // runs (the spawn cannot happen here because we have no
            // tokio runtime hand-off and `build_provider` would have
            // to become async, cascading through every caller). When
            // the URL is empty, the provider's `chat` returns a clear
            // error and the Hub renders that state honestly.
            let mut provider = LocalLlmProvider::default().with_model(model);
            if !base_url.is_empty() {
                provider = provider.with_base_url(base_url);
            }
            Box::new(provider)
        }
    }
}

/// Return the ProviderMeta summaries the Hub renders into the chip grid.
/// The `mode` argument is informational — Hebrew/Cloud badges don't differ
/// between command and chat — but it lets the frontend share one code path
/// for both pickers.
#[tauri::command]
pub async fn get_llm_providers(_mode: String) -> Result<Vec<ProviderMeta>, String> {
    let entries = catalogue();
    let metas = entries
        .into_iter()
        .map(|descriptor| {
            let key_name = format!("llm.{}", descriptor.id);
            let has_api_key = match descriptor.kind {
                ProviderKind::OpenAiCompat(config) if !config.requires_api_key => true,
                // The in-process LLM needs no key — but it does need the
                // GGUF to be on disk. Surface "no key required" here; the
                // Hub queries `local_llm_status` for the install state.
                ProviderKind::LocalLlm => true,
                _ => keychain::has_key(&key_name),
            };
            ProviderMeta {
                id: descriptor.id.to_string(),
                display_name_key: descriptor.display_name_key.to_string(),
                is_local: descriptor.is_local,
                supports_hebrew: descriptor.supports_hebrew,
                has_api_key,
                default_model: descriptor.default_model.to_string(),
                available_models: descriptor.available_models,
                context_window: descriptor.context_window,
                supports_tool_use: descriptor.supports_tool_use,
                recommended_model: descriptor.recommended_model.map(|s| s.to_string()),
            }
        })
        .collect();
    Ok(metas)
}

/// Set the active provider for `mode`. Persists to `settings.json` and
/// returns the new provider id. The frontend rebroadcasts to the tongue
/// via `settings:changed`.
#[tauri::command]
pub async fn set_llm_provider(app: AppHandle, mode: String, id: String) -> Result<String, String> {
    let mode = LlmMode::from_str(&mode).map_err(|e| e.to_string())?;
    let known = id == "none" || catalogue().iter().any(|descriptor| descriptor.id == id);
    if !known {
        return Err(format!("unknown LLM provider id: {id}"));
    }
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    store.set(
        key_active_provider(mode),
        serde_json::Value::String(id.clone()),
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(id)
}

/// Store an API key in the OS keychain. The raw `secret` is consumed by this
/// command — the Tauri shell never echoes it back through any event or
/// return value (`docs/adr/0020`).
#[tauri::command]
pub async fn save_api_key(stage: String, provider: String, secret: String) -> Result<(), String> {
    if !["llm", "stt", "tts"].contains(&stage.as_str()) {
        return Err(format!("unknown provider stage: {stage}"));
    }
    if provider.is_empty() {
        return Err("provider name must not be empty".to_string());
    }
    let key_name = format!("{stage}.{provider}");
    keychain::store_key(&key_name, &secret).map_err(|e| format!("{e:#}"))
}

/// Whether a key is stored for `(stage, provider)`. The frontend's only
/// read path — the raw value never crosses the JS bridge.
#[tauri::command]
pub async fn has_api_key(stage: String, provider: String) -> Result<bool, String> {
    if provider.is_empty() {
        return Ok(false);
    }
    let key_name = format!("{stage}.{provider}");
    Ok(keychain::has_key(&key_name))
}

/// Remove a stored key.
#[tauri::command]
pub async fn delete_api_key(stage: String, provider: String) -> Result<(), String> {
    if provider.is_empty() {
        return Err("provider name must not be empty".to_string());
    }
    let key_name = format!("{stage}.{provider}");
    keychain::delete_key(&key_name).map_err(|e| format!("{e:#}"))
}

/// Probe Ollama at the user's configured base URL (or the loopback default).
/// Used by the Hub's Ollama chip to grey itself out when the daemon is
/// absent and to populate the model picker when it is present.
#[tauri::command]
pub async fn detect_ollama(app: AppHandle) -> Result<OllamaDetection, String> {
    let base = read_string(&app, &key_base_url("ollama-local"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());
    Ok(detect_ollama_core(&base).await)
}

/// Dispatch a one-shot test prompt against the mode's active provider. The
/// Hub button calls this and renders the returned text inline.
#[tauri::command]
pub async fn test_llm_prompt(app: AppHandle, mode: String, text: String) -> Result<String, String> {
    if text.is_empty() {
        return Err("test prompt is empty".into());
    }
    let mode = LlmMode::from_str(&mode).map_err(|e| e.to_string())?;
    let provider_id = read_string(&app, &key_active_provider(mode))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "no LLM provider configured for this mode".to_string())?;
    if provider_id == "none" {
        return Err("no LLM provider configured for this mode".into());
    }
    let descriptor = catalogue()
        .into_iter()
        .find(|descriptor| descriptor.id == provider_id)
        .ok_or_else(|| format!("unknown LLM provider id: {provider_id}"))?;
    let model = read_string(&app, &key_active_model(mode))
        .filter(|s| !s.is_empty())
        .or_else(|| Some(descriptor.default_model.to_string()));
    let base_url = match descriptor.kind {
        // Local: the URL is the loopback port of the spawned
        // llama-server, not a user-configurable setting.
        ProviderKind::LocalLlm => {
            let active_model = model
                .clone()
                .unwrap_or_else(|| LOCAL_LLM_DEFAULT.to_string());
            Some(ensure_local_llm_base_url(&app, &active_model).await?)
        }
        _ => read_string(&app, &key_base_url(descriptor.id)),
    };
    let provider = build_provider(&descriptor, base_url, model);
    let completion = provider
        .chat(&[Msg::user(text)], &[])
        .await
        .with_context(|| format!("LLM provider {provider_id} chat call"))
        .map_err(|e| format!("{e:#}"))?;
    Ok(completion.content.to_plain_text())
}

/// Result of a `fetch_provider_models` call. The Hub renders the model
/// dropdown from `models`; when `source == "fallback"` (the remote
/// fetch failed) it also surfaces `error` as an inline hint so the
/// user can correct a bad key or rate-limited endpoint without
/// guessing.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderModelsResult {
    /// Filtered, sorted, capped model ids — the list rendered in the
    /// picker. Always non-empty: falls back to the provider's static
    /// `available_models` when the remote call fails.
    pub models: Vec<String>,
    /// `"remote"` when the list came from the provider's `/v1/models`
    /// endpoint, `"fallback"` when the static list was used because the
    /// remote call errored.
    pub source: &'static str,
    /// Number of models the provider actually exposed before
    /// filter-and-cap. Used by the Hub for the "showing N of M" hint.
    /// Equals `models.len()` on `"fallback"` (the static list isn't
    /// trimmed).
    pub total_count: usize,
    /// When `source == "fallback"`, a short user-readable message
    /// describing the failure (`401 Unauthorized`, `rate-limited`, …).
    /// `None` on `"remote"`.
    pub error: Option<String>,
}

/// Fetch the live model list from a provider. Used by the Hub to
/// populate the model dropdown after the user pastes an API key — so
/// brand-new models (or org-private fine-tunes) show up without a
/// Lashon update. Falls back to the static `available_models()` list
/// when the remote call fails (no key saved, rate-limited, offline,
/// vendor doesn't expose `/v1/models`).
#[tauri::command]
pub async fn fetch_provider_models(
    app: AppHandle,
    provider_id: String,
) -> Result<ProviderModelsResult, String> {
    let descriptor = catalogue()
        .into_iter()
        .find(|descriptor| descriptor.id == provider_id)
        .ok_or_else(|| format!("unknown LLM provider id: {provider_id}"))?;
    // The base URL the user may have overridden in the Hub — same path
    // as `test_llm_prompt` so a corporate proxy override applies to the
    // model-discovery call too.
    let base_url = match descriptor.kind {
        ProviderKind::LocalLlm => {
            // Local-LLM only ever serves the one model llama-server was
            // launched with — short-circuit with the static list rather
            // than spawning the server just to read its name.
            let models = descriptor.available_models.clone();
            let total_count = models.len();
            return Ok(ProviderModelsResult {
                models,
                source: "fallback",
                total_count,
                error: None,
            });
        }
        _ => read_string(&app, &key_base_url(descriptor.id)),
    };
    let provider = build_provider(&descriptor, base_url, None);
    match provider.fetch_remote_models().await {
        Ok(remote) if !remote.models.is_empty() => Ok(ProviderModelsResult {
            models: remote.models,
            source: "remote",
            total_count: remote.total,
            error: None,
        }),
        Ok(_) => {
            // Empty remote list — likely a misbehaving proxy. Fall back
            // to the static list rather than render an empty dropdown.
            let models = descriptor.available_models.clone();
            let total_count = models.len();
            Ok(ProviderModelsResult {
                models,
                source: "fallback",
                total_count,
                error: Some("provider returned an empty model list".into()),
            })
        }
        Err(err) => {
            let models = descriptor.available_models.clone();
            let total_count = models.len();
            Ok(ProviderModelsResult {
                models,
                source: "fallback",
                total_count,
                error: Some(format!("{err:#}")),
            })
        }
    }
}

// --- in-process local LLM (docs/adr/0025) -----------------------------------

/// Summary of one local-LLM model the Hub renders in the download card.
/// Mirrors the wake-classifier surface so the Hub reuses the same chip
/// chrome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalLlmModelMeta {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub license: String,
    pub source: String,
    pub context_window: usize,
    pub bytes: u64,
    pub installed: bool,
}

impl From<AvailableLocalLlmModel> for LocalLlmModelMeta {
    fn from(value: AvailableLocalLlmModel) -> Self {
        Self {
            id: value.id,
            display_name: value.display_name,
            description: value.description,
            license: value.license,
            source: value.source,
            context_window: value.context_window,
            bytes: value.bytes,
            installed: value.installed,
        }
    }
}

/// Overall status of the in-process LLM — drives the Hub chip's
/// "Download required" / "Ready" copy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalLlmStatusReport {
    /// True when `lashon-core` was compiled with the `local-llm`
    /// Cargo feature. False makes the Hub render a "this build does not
    /// include the in-process LLM" notice (the binary still works for
    /// every other provider).
    pub runtime_available: bool,
    /// Active model id (read from `settings.json llm.command.model`, or
    /// the descriptor default when unset).
    pub active_model: String,
    /// Whether `active_model`'s files are on disk at the manifest size.
    pub active_installed: bool,
    /// Every model the manifest lists, with install state — the Hub
    /// renders the picker rows from this.
    pub models: Vec<LocalLlmModelMeta>,
}

/// Read the active local-LLM model id from settings — Command-mode pick
/// takes precedence over Chat-mode, with the descriptor default as the
/// fallback. The Hub passes the chosen id to `install_local_llm` /
/// `delete_local_llm` so the user always operates on what they see in
/// the chip.
fn active_local_llm_model(app: &AppHandle) -> String {
    read_string(app, "llm.command.model")
        .or_else(|| read_string(app, "llm.chat.model"))
        .filter(|s| !s.is_empty() && LOCAL_LLM_MODELS.contains(&s.as_str()))
        .unwrap_or_else(|| LOCAL_LLM_DEFAULT.to_string())
}

/// Status of the in-process LLM. Cheap — no I/O beyond `std::fs::metadata`
/// per file.
#[tauri::command]
pub async fn local_llm_status(app: AppHandle) -> Result<LocalLlmStatusReport, String> {
    let active = active_local_llm_model(&app);
    let active_installed = is_local_llm_installed(&active);
    let models: Vec<LocalLlmModelMeta> = available_local_llm_models()
        .into_iter()
        .map(LocalLlmModelMeta::from)
        .collect();
    Ok(LocalLlmStatusReport {
        runtime_available: LOCAL_LLM_RUNTIME_AVAILABLE,
        active_model: active,
        active_installed,
        models,
    })
}

/// Download (or resume) a local-LLM model into the per-user
/// `local-llm/` directory. Emits `local_llm:progress` events as the
/// download proceeds so the Hub can render a percentage bar.
#[tauri::command]
pub async fn install_local_llm(app: AppHandle, model_id: String) -> Result<String, String> {
    if model_id.is_empty() {
        return Err("model id must not be empty".into());
    }
    if !LOCAL_LLM_MODELS.contains(&model_id.as_str()) {
        return Err(format!("unknown local-llm model id: {model_id}"));
    }
    let progress_app = app.clone();
    install_local_llm_core(&model_id, move |progress| {
        // Best-effort — a missed event just means the Hub stays at the
        // previous percentage for a tick. Never block the download.
        let _ = progress_app.emit(
            "local_llm:progress",
            serde_json::json!({
                "model_id": progress.model_id,
                "file": progress.file,
                "downloaded": progress.downloaded,
                "total": progress.total,
            }),
        );
    })
    .await
    .map_err(|e| format!("{e:#}"))
}

/// Delete a local-LLM model's files from disk. Returns how many files
/// were removed (0 means the model was already absent).
#[tauri::command]
pub async fn delete_local_llm(model_id: String) -> Result<usize, String> {
    if model_id.is_empty() {
        return Err("model id must not be empty".into());
    }
    if !LOCAL_LLM_MODELS.contains(&model_id.as_str()) {
        return Err(format!("unknown local-llm model id: {model_id}"));
    }
    delete_local_llm_core(&model_id).map_err(|e| format!("{e:#}"))
}

/// Bundled `llama-server.exe` path inside the Tauri resource dir. For
/// `tauri dev` runs this resolves under `apps/desktop/src-tauri/`; for
/// installed builds, under the resource dir of the install (NSIS:
/// `<install>\resources\binaries\llama-server\llama-server.exe`).
fn bundled_llama_server_exe(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resolving Tauri resource dir: {e}"))?;
    let exe_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    Ok(resource_dir
        .join("binaries")
        .join("llama-server")
        .join(exe_name))
}

/// Ensure the Lashon-managed `llama-server` subprocess is running and
/// pointed at the GGUF for `model_id`. Returns the loopback base URL
/// (`http://127.0.0.1:<port>/v1`) for `LocalLlmProvider::with_base_url`.
///
/// Idempotent — if the server is already running against the same
/// GGUF, returns the existing URL without restarting.
pub async fn ensure_local_llm_base_url(app: &AppHandle, model_id: &str) -> Result<String, String> {
    if !is_local_llm_installed(model_id) {
        return Err(format!(
            "local-llm model '{model_id}' is not installed — \
             click 'Download' in the Hub first"
        ));
    }
    let (dir, file_name) = local_llm_resolved_path(model_id).map_err(|e| format!("{e:#}"))?;
    let model_path = dir.join(&file_name);

    let bundled = bundled_llama_server_exe(app)?;
    let server_exe = resolve_server_exe(bundled).map_err(|e| format!("{e:#}"))?;

    let state = app.state::<LlamaServerState>();
    let server = ready_llama_server(
        &state,
        SpawnConfig {
            server_exe,
            model_path,
            // 16 K covers the M8.2 system prompt (~8 K tokens — 35 tool
            // descriptions plus the Hebrew worked examples) plus a full
            // 12-turn chain history. The earlier 4 K value pre-dated
            // the OS-control tranche and started rejecting prompts
            // with `exceed_context_size_error` once the catalogue
            // grew. Qwen3-1.7B natively supports 32 K, so this stays
            // well inside the model's window; the KV-cache cost on a
            // Q8 build is roughly an extra ~450 MB of GPU memory.
            ctx_size: 16384,
            n_gpu_layers: 999,
        },
    )
    .await
    .map_err(|e| format!("{e:#}"))?;
    Ok(server.base_url())
}
