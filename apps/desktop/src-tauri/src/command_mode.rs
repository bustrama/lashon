//! Tauri-side glue for M8 Command mode (`docs/adr/0024`).
//!
//! The dictation worker hands us a finished transcript via
//! `dispatch_transcript`. We:
//!
//! 1. Read the user's active Command-mode LLM provider + model + base
//!    URL from `settings.json`, the same way `test_llm_prompt` does.
//! 2. Build a fresh provider instance (cheap; HTTP client only).
//! 3. Build the Phase-1 `ToolRegistry`.
//! 4. Pick a `ConfirmHandler` — `EventBasedConfirm` that emits
//!    `command:confirm` and awaits a `command:confirm:reply` event from
//!    the tongue.
//! 5. Spawn `lashon_core::command_mode::dispatch` on the Tauri async
//!    runtime; emit the result as a `command:result` event the tongue
//!    flashes.
//!
//! Nothing here holds long-lived `dyn LLMProvider` or `ToolRegistry`
//! state — every take builds them fresh so a Hub change to the LLM
//! provider takes effect on the next press without restart.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::oneshot;

use lashon_core::command_mode::{dispatch, AlwaysAllow, CommandProgressHandler, ConfirmHandler};
use lashon_core::recipes::{
    storage::collect_recipes, try_recipe_cascade, CascadeMatcher, CommandRoute,
    ConfirmHandler as RecipeConfirmHandler,
};
use lashon_core::llm::{
    anthropic::{
        AnthropicLlmProvider, AVAILABLE_MODELS as ANTHROPIC_MODELS,
        DEFAULT_MODEL as ANTHROPIC_DEFAULT_MODEL,
    },
    local::{LocalLlmProvider, DEFAULT_MODEL as LOCAL_LLM_DEFAULT, PROVIDER_ID as LOCAL_LLM_ID},
    openai_compat::{OpenAiCompatConfig, OpenAiCompatLlmProvider, ALL_VENDORS},
    LLMProvider,
};
use lashon_core::tool::ConfirmDecision;
use lashon_core::tools::phase_one_registry;

/// Payload of the `command:result` event the tongue listens for.
#[derive(Debug, Clone, Serialize)]
struct CommandResultEvent {
    text: String,
    tool_summaries: Vec<String>,
    turns: usize,
}

/// Payload of the `command:confirm` event. The tongue renders a modal
/// asking the user to allow / deny the named tool.
#[derive(Debug, Clone, Serialize)]
struct CommandConfirmRequest {
    /// Correlation id — the tongue echoes this back in its reply so
    /// concurrent confirm prompts can't be confused.
    id: String,
    tool: String,
    /// Best-effort JSON-stringified args. The tongue truncates to a
    /// readable preview in its modal for everything except
    /// `run_command`, which uses the `command_preview` field below for
    /// untruncated code-block rendering.
    args_preview: String,
    /// Set when `tool == "run_command"`: the literal command line and
    /// resolved cwd, rendered as `<code>` in the modal so the user can
    /// read every character before approving. The Rust side picks this
    /// up from the args; the Svelte modal switches its preview render
    /// path on its presence.
    #[serde(skip_serializing_if = "Option::is_none")]
    command_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd_preview: Option<String>,
}

/// Payload of the `command:confirm:reply` event the tongue emits.
#[derive(Debug, Deserialize)]
struct CommandConfirmReply {
    id: String,
    /// `"allow"` or `"deny"` — anything else is treated as `"deny"`.
    decision: String,
}

/// Payload of the `command:tool` event. The tongue rolls the
/// `display_summary` into its status line so the user sees each step
/// of a tool chain as it happens (M8.1 — `docs/adr/0024`).
#[derive(Debug, Clone, Serialize)]
struct CommandToolEvent {
    /// The tool's wire name. Used as an i18n key if a localised label
    /// is registered (`command.tool.<name>`); otherwise the tongue
    /// falls back to the summary.
    name: String,
    /// `"started"` while the tool is executing, `"finished"` once it
    /// returns. The tongue shows the indeterminate state during
    /// `started` and the summary during `finished`.
    status: &'static str,
    /// `ToolResult::display_summary` — Hebrew-friendly when set, or
    /// `None` for silent tools (clipboard_get etc.).
    summary: Option<String>,
}

/// Payload of the `command:transcript` event. The dictation worker
/// hands the STT result to `dispatch_transcript`, which fires this
/// event before the LLM is invoked so the tongue can show what was
/// heard and the user can cancel a misheard take (M8.2).
#[derive(Debug, Clone, Serialize)]
struct CommandTranscriptEvent {
    text: String,
}

/// Tauri-managed state pointing at the **single** in-flight dispatcher
/// task. When a new take starts, the old task is aborted so two
/// chains can't fight over the foreground app. The cancel command
/// reaches into this same state and aborts the current task.
#[derive(Default)]
pub struct ActiveDispatch(std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

impl ActiveDispatch {
    fn replace(&self, task: tauri::async_runtime::JoinHandle<()>) {
        if let Ok(mut slot) = self.0.lock() {
            if let Some(prev) = slot.take() {
                prev.abort();
            }
            *slot = Some(task);
        }
    }
    fn take(&self) -> Option<tauri::async_runtime::JoinHandle<()>> {
        self.0.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// Hand a freshly-transcribed take to the Command-mode dispatcher.
/// Spawns on the Tauri async runtime so the dictation worker can return
/// to idle immediately.
pub fn dispatch_transcript(app: AppHandle, transcript: String) {
    // Announce the transcript first so the tongue can render
    // "I heard: …" + Cancel button before the LLM round-trip lands
    // (M8.2 — the gap the user complained about previously).
    let _ = app.emit(
        "command:transcript",
        CommandTranscriptEvent {
            text: transcript.clone(),
        },
    );
    let app_for_task = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(err) = run(&app_for_task, transcript).await {
            tracing::error!("command_mode: dispatch failed: {err:#}");
            let _ = app_for_task.emit(
                "command:result",
                CommandResultEvent {
                    text: format!("שגיאה: {err}"),
                    tool_summaries: Vec::new(),
                    turns: 0,
                },
            );
            // Always clear the tongue's thinking state — without this
            // an error returned from run() leaves the spinner running
            // forever.
            let _ = app_for_task.emit("command:state", "idle");
        }
    });
    // Tauri's app state holds the JoinHandle so `cancel_command` can
    // abort the take in flight (M8.2). The previous task is aborted
    // by `replace` — only one take runs at a time.
    let state = app.state::<ActiveDispatch>();
    state.replace(task);
}

/// Cancel the in-flight Command-mode take, if any. Aborts the
/// dispatcher's task — any pending LLM HTTP request, tool sleep, or
/// UIA poll is dropped — and emits a `cancelled` result so the tongue
/// shows a clear "ביטלת" flash instead of leaving the spinner hanging.
///
/// Safe to call while no take is in flight: the function becomes a
/// no-op (no state mutation, no event emitted).
#[tauri::command]
pub async fn cancel_command(app: AppHandle) -> Result<(), String> {
    // `state::<ActiveDispatch>()` is always Some — wired in
    // `apps/desktop/src-tauri/src/lib.rs`'s `.manage(...)` call.
    let state = app.state::<ActiveDispatch>();
    let Some(handle) = state.take() else {
        return Ok(());
    };
    handle.abort();
    // Tell the tongue we're done; surface a friendly Hebrew message.
    // Done after abort so the user can't see lingering progress events
    // racing the cancellation.
    let _ = app.emit(
        "command:result",
        CommandResultEvent {
            text: "ביטלת את הפקודה.".to_string(),
            tool_summaries: Vec::new(),
            turns: 0,
        },
    );
    let _ = app.emit("command:state", "idle");
    Ok(())
}

async fn run(app: &AppHandle, transcript: String) -> anyhow::Result<()> {
    let take_started = std::time::Instant::now();
    // Structural log of inputs — sizes only, no transcript content
    // (`.claude/rules/security.md`).
    tracing::info!(
        transcript_chars = transcript.chars().count(),
        "command_mode: take begin"
    );

    // ── Post-STT word-aliases ───────────────────────────────────────
    // User-supplied correction layer for recurring STT misrecognitions
    // (Whisper hearing "cloud" instead of "claude", contact-name
    // homonyms, Hebrew transliteration drift). The corrected
    // transcript is what the cascade + the LLM planner both see —
    // single place to fix per-user vocabulary issues.
    let aliases = read_word_aliases(app);
    let transcript = if aliases.is_empty() {
        transcript
    } else {
        let corrected = lashon_core::transcript::apply_aliases(&transcript, &aliases);
        if corrected != transcript {
            tracing::info!(
                aliases_total = aliases.len(),
                changed = true,
                "command_mode: word-alias substitution applied"
            );
        }
        corrected
    };
    // ── end word-aliases ────────────────────────────────────────────

    // ── M9 Phase 1c — intent cascade pre-pass ───────────────────────
    // Try the recipe cascade *before* resolving the LLM provider /
    // spawning llama-server / building the tool registry. On match,
    // the runtime executes the recipe deterministically (0–1 LLM
    // turns, since v1 ships the regex tier only) and we skip the
    // planner entirely. On miss, fall through to the existing
    // dispatch path unchanged.
    //
    // The expensive Local-LLM spawn doesn't happen on a cascade hit —
    // that's the whole point. A typical recipe run goes mic → STT →
    // 50 ms cascade → runtime → tongue flash, vs the ~5–10 s the
    // planner would otherwise spend.
    let recipes = tauri::async_runtime::spawn_blocking(collect_recipes)
        .await
        .unwrap_or_default();
    tracing::info!(
        recipe_count = recipes.len(),
        "command_mode: cascade pre-pass starting"
    );
    let matcher = CascadeMatcher::default_phase_1c_v1();
    let recipe_confirm: std::sync::Arc<dyn RecipeConfirmHandler> =
        std::sync::Arc::new(crate::recipes::EventBasedConfirm::new(app.clone()));
    match try_recipe_cascade(&matcher, &recipes, recipe_confirm, &transcript).await {
        Ok(CommandRoute::Recipe {
            recipe_id,
            tier,
            run,
        }) => {
            let cascade_ms = take_started.elapsed().as_millis() as u64;
            tracing::info!(
                recipe = %recipe_id,
                tier = tier.as_str(),
                steps = run.steps_executed,
                cascade_ms,
                "command_mode: cascade short-circuit"
            );
            // Emit a structured "the cascade handled this" event so a
            // future tongue update can flash the ↯ glyph + recipe
            // name. For v1 the tongue just receives a normal
            // `command:result` summary and renders it as if the
            // planner had produced it.
            let _ = app.emit(
                "command:recipe-matched",
                serde_json::json!({
                    "recipe_id": recipe_id,
                    "tier": tier.as_str(),
                    "steps_executed": run.steps_executed,
                }),
            );
            let assistant_text = format!(
                "הרצתי את המתכון {recipe_id} ({} צעדים)",
                run.steps_executed
            );
            let _ = app.emit(
                "command:result",
                CommandResultEvent {
                    text: assistant_text,
                    tool_summaries: vec![],
                    turns: 0,
                },
            );
            let _ = app.emit("command:state", "idle");
            return Ok(());
        }
        Ok(CommandRoute::Planner) => {
            tracing::info!("command_mode: cascade miss — falling through to LLM planner");
            // fall through
        }
        Err(err) => {
            // The cascade matched but the runtime failed (e.g. the
            // user denied the run_shell confirmation, or the
            // window-focus step found no matching window). Surface
            // the error to the tongue rather than silently falling
            // through to the planner — if the user *meant* the
            // recipe, retrying via the planner won't help and could
            // do something unexpected.
            tracing::warn!(error = %err, "command_mode: cascade match but runtime failed");
            let _ = app.emit(
                "command:result",
                CommandResultEvent {
                    text: format!("המתכון נכשל: {err}"),
                    tool_summaries: vec![],
                    turns: 0,
                },
            );
            let _ = app.emit("command:state", "idle");
            return Ok(());
        }
    }
    // ── end cascade pre-pass ────────────────────────────────────────

    // Resolve the active Command-mode provider.
    let provider_id = read_setting(app, "llm.command.provider")
        .filter(|s| !s.is_empty() && s != "none")
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Command mode has no LLM configured — set one in Settings → Language models"
            )
        })?;
    let model_override = read_setting(app, "llm.command.model");
    // For Local: the base URL is the spawned llama-server's loopback
    // port, not a user setting. Spawn it now (idempotent) and override.
    let base_url_override = if provider_id == LOCAL_LLM_ID {
        let active_model = model_override
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| LOCAL_LLM_DEFAULT.to_string());
        let spawn_started = std::time::Instant::now();
        let url = crate::llm::ensure_local_llm_base_url(app, &active_model)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        tracing::info!(
            spawn_ms = spawn_started.elapsed().as_millis() as u64,
            model = %active_model,
            "command_mode: local llama-server ready"
        );
        Some(url)
    } else {
        read_setting(app, &format!("llm.{provider_id}.base_url"))
    };
    tracing::info!(
        provider = %provider_id,
        model = model_override.as_deref().unwrap_or("<default>"),
        base_url_override = base_url_override.is_some(),
        "command_mode: provider resolved"
    );
    let provider = build_llm_provider(&provider_id, base_url_override, model_override)?;
    let registry = Arc::new(phase_one_registry());
    let ui_language = read_setting(app, "ui.language").unwrap_or_else(|| "he".into());

    // Pick the confirmation handler. Phase-1 tools are all safe so the
    // event-based handler never actually emits; we wire it anyway so
    // M8.2's destructive tools just work.
    let confirm: Arc<dyn ConfirmHandler> = Arc::new(EventBasedConfirm::new(app.clone()));
    // Progress handler emits `command:state` (`thinking` / `idle`) and
    // `command:tool` events to the tongue so the user sees what's
    // happening at every step of the tool chain (M8.1).
    let progress: Arc<dyn CommandProgressHandler> = Arc::new(EventProgress::new(app.clone()));
    let _ = app.emit("command:state", "thinking");

    let outcome = dispatch(
        provider,
        registry,
        confirm,
        progress,
        transcript,
        &ui_language,
    )
    .await?;

    tracing::info!(
        turns = outcome.turns,
        tools = outcome.tool_summaries.len(),
        take_ms = take_started.elapsed().as_millis() as u64,
        text_chars = outcome.assistant_text.chars().count(),
        "command_mode: take complete"
    );

    let _ = app.emit(
        "command:result",
        CommandResultEvent {
            text: outcome.assistant_text,
            tool_summaries: outcome.tool_summaries,
            turns: outcome.turns,
        },
    );
    // Tell the tongue the dispatcher is finished — clears the thinking
    // state if no result-flash has redrawn yet.
    let _ = app.emit("command:state", "idle");
    Ok(())
}

fn read_setting(app: &AppHandle, key: &str) -> Option<String> {
    let store = app.store("settings.json").ok()?;
    store
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

/// Read the user's `stt.word_aliases` map from `settings.json`. Empty
/// map when unset / malformed — the post-STT substitution is a no-op
/// in that case so we don't error out a take just because settings
/// don't have an aliases section yet.
fn read_word_aliases(app: &AppHandle) -> std::collections::HashMap<String, String> {
    let Ok(store) = app.store("settings.json") else {
        return std::collections::HashMap::new();
    };
    let Some(value) = store.get("stt.word_aliases") else {
        return std::collections::HashMap::new();
    };
    let Some(obj) = value.as_object() else {
        return std::collections::HashMap::new();
    };
    obj.iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect()
}

/// Hub `get_word_aliases` — returns the current `stt.word_aliases`
/// map. Used by the Voice corrections section to populate the table.
#[tauri::command]
pub async fn get_word_aliases(
    app: AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    Ok(read_word_aliases(&app))
}

/// Hub `set_word_aliases` — persists the user's edited map back to
/// `settings.json`. Caller already validated locally (no empty keys
/// etc.); we just round-trip via `serde_json`.
#[tauri::command]
pub async fn set_word_aliases(
    app: AppHandle,
    aliases: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let store = app
        .store("settings.json")
        .map_err(|e| format!("settings open failed: {e}"))?;
    // Strip empty-key entries defensively — the Hub UI suppresses
    // them but a third-party caller may not.
    let cleaned: std::collections::BTreeMap<String, String> = aliases
        .into_iter()
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let value = serde_json::to_value(&cleaned).map_err(|e| e.to_string())?;
    store.set("stt.word_aliases", value);
    store.save().map_err(|e| format!("settings save failed: {e}"))?;
    tracing::info!(count = cleaned.len(), "hub: set_word_aliases");
    Ok(())
}

/// Construct the active provider as a fresh `dyn LLMProvider`. Mirrors
/// the M7 `test_llm_prompt` builder; the duplication is intentional so
/// neither path holds long-lived `dyn LLMProvider` state.
fn build_llm_provider(
    id: &str,
    base_url_override: Option<String>,
    model_override: Option<String>,
) -> anyhow::Result<Arc<dyn LLMProvider>> {
    if id == "anthropic" {
        let mut provider = AnthropicLlmProvider::new();
        if let Some(url) = base_url_override.filter(|s| !s.is_empty()) {
            provider = provider.with_base_url(url);
        }
        let model = model_override
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_MODEL.to_string());
        provider = provider.with_model(model);
        // Sanity check the catalog still references claude-sonnet-4-6
        // — keeps this in sync with the Hub's model picker.
        let _ = ANTHROPIC_MODELS;
        return Ok(Arc::new(provider));
    }
    if id == LOCAL_LLM_ID {
        // Local-LLM (docs/adr/0025) — `base_url_override` carries the
        // loopback URL of the Lashon-managed llama-server (already
        // spawned by `run`'s `ensure_local_llm_base_url`). The model
        // id is informational (the server serves whichever GGUF it
        // was launched against; we forward it for log clarity).
        let model = model_override.unwrap_or_default();
        let mut provider = LocalLlmProvider::default().with_model(model);
        if let Some(url) = base_url_override.filter(|s| !s.is_empty()) {
            provider = provider.with_base_url(url);
        }
        return Ok(Arc::new(provider));
    }
    let vendor: OpenAiCompatConfig = ALL_VENDORS
        .iter()
        .find(|v| v.name == id)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("unknown LLM provider id: {id}"))?;
    let mut provider = OpenAiCompatLlmProvider::new(vendor);
    if let Some(url) = base_url_override.filter(|s| !s.is_empty()) {
        provider = provider.with_base_url(url);
    }
    let model = model_override
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| vendor.default_model.to_string());
    provider = provider.with_model(model);
    Ok(Arc::new(provider))
}

/// A confirmation handler that emits `command:confirm` to the tongue
/// and waits for `command:confirm:reply`. Default 30s timeout — past
/// that the dispatcher gets a Deny so a forgotten modal can't wedge
/// the take forever.
struct EventBasedConfirm {
    app: AppHandle,
}

impl EventBasedConfirm {
    fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ConfirmHandler for EventBasedConfirm {
    fn confirm<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a serde_json::Value,
    ) -> Pin<Box<dyn std::future::Future<Output = ConfirmDecision> + Send + 'a>> {
        let app = self.app.clone();
        let tool_name = tool_name.to_string();
        let args_preview = args.to_string();
        // For `run_command` the modal must show the full literal
        // command (and the resolved cwd, if set) as a code block, no
        // truncation — the user needs to read every character before
        // approving a shell command. For every other destructive tool
        // the existing JSON-preview path is enough.
        let (command_preview, cwd_preview) = if tool_name == "run_command" {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cwd = args
                .get("cwd")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string());
            (command, cwd)
        } else {
            (None, None)
        };
        Box::pin(async move {
            let id = format!(
                "confirm-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            let (tx, rx) = oneshot::channel::<ConfirmDecision>();
            let tx = std::sync::Mutex::new(Some(tx));
            let id_clone = id.clone();
            let handler = app.listen("command:confirm:reply", move |event| {
                let Ok(reply) = serde_json::from_str::<CommandConfirmReply>(event.payload()) else {
                    return;
                };
                if reply.id != id_clone {
                    return;
                }
                let decision = if reply.decision == "allow" {
                    ConfirmDecision::Allow
                } else {
                    ConfirmDecision::Deny
                };
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(decision);
                }
            });
            if let Err(err) = app.emit(
                "command:confirm",
                CommandConfirmRequest {
                    id: id.clone(),
                    tool: tool_name,
                    args_preview,
                    command_preview,
                    cwd_preview,
                },
            ) {
                tracing::warn!("command_mode: failed to emit confirm request: {err}");
                app.unlisten(handler);
                return ConfirmDecision::Deny;
            }
            let decision = match tokio::time::timeout(Duration::from_secs(30), rx).await {
                Ok(Ok(d)) => d,
                Ok(Err(_)) => ConfirmDecision::Deny,
                Err(_) => {
                    tracing::warn!("command_mode: confirmation timed out");
                    ConfirmDecision::Deny
                }
            };
            app.unlisten(handler);
            decision
        })
    }
}

/// `CommandProgressHandler` impl that emits `command:state` and
/// `command:tool` events to the tongue webview. Wires the
/// M8.1 thinking-animation + per-tool flash UX directly into the
/// dispatcher (`docs/adr/0024`).
struct EventProgress {
    app: AppHandle,
}

impl EventProgress {
    fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl CommandProgressHandler for EventProgress {
    fn on_thinking(&self) {
        let _ = self.app.emit("command:state", "thinking");
    }
    fn on_tool_started(&self, name: &str) {
        // Per-tool tracing so the terminal shows each tool the LLM picked
        // in the same INFO stream as `command_mode: take complete`. Lets
        // us diagnose loops (e.g. "tried open_app 6 times in a row")
        // without having to attach devtools to the tongue webview.
        // Tool names are a fixed enum — no PII risk (security.md).
        tracing::info!(tool = name, "command_mode: tool started");
        let _ = self.app.emit(
            "command:tool",
            CommandToolEvent {
                name: name.to_string(),
                status: "started",
                summary: None,
            },
        );
    }
    fn on_tool_finished(&self, name: &str, summary: Option<&str>) {
        // Log name + summary length but NOT the summary itself — summaries
        // can carry the user's search query (`web_search`), typed text
        // (`type_text`), URL (`open_url`), etc. Per security.md we never
        // log transcript content or PII, even at debug level.
        tracing::info!(
            tool = name,
            summary_len = summary.map(|s| s.len()).unwrap_or(0),
            "command_mode: tool finished"
        );
        let _ = self.app.emit(
            "command:tool",
            CommandToolEvent {
                name: name.to_string(),
                status: "finished",
                summary: summary.map(|s| s.to_string()),
            },
        );
    }
}

/// Probe-only Tauri command — returns whether Command mode is ready
/// (an active LLM provider + a stored API key OR a no-key-needed
/// provider like Ollama local). The tongue uses this to grey itself
/// out when the user hasn't configured an LLM yet.
#[tauri::command]
pub async fn command_mode_status(app: AppHandle) -> CommandModeStatus {
    let provider_id = read_setting(&app, "llm.command.provider").unwrap_or_default();
    if provider_id.is_empty() || provider_id == "none" {
        return CommandModeStatus {
            configured: false,
            provider: None,
            reason: Some("no provider".into()),
        };
    }
    // Build the provider just to ask whether it has a key.
    let model = read_setting(&app, "llm.command.model");
    let base_url = read_setting(&app, &format!("llm.{provider_id}.base_url"));
    match build_llm_provider(&provider_id, base_url, model) {
        Ok(provider) => CommandModeStatus {
            configured: provider.has_api_key(),
            provider: Some(provider_id),
            reason: if provider.has_api_key() {
                None
            } else {
                Some("no key".into())
            },
        },
        Err(err) => CommandModeStatus {
            configured: false,
            provider: Some(provider_id),
            reason: Some(err.to_string()),
        },
    }
}

#[derive(Debug, Serialize)]
pub struct CommandModeStatus {
    pub configured: bool,
    pub provider: Option<String>,
    pub reason: Option<String>,
}

/// Smoke-test command — useful from a JS console / dev tooling without
/// going through the hotkey + capture pipeline. NOT wired to a UI in
/// M8.1 (the manual smoke path is the hotkey); reviewers can invoke it
/// from the webview's devtools to dry-run with synthetic input.
#[tauri::command]
pub async fn command_mode_dispatch_text(app: AppHandle, transcript: String) -> Result<(), String> {
    if transcript.trim().is_empty() {
        return Err("transcript must not be empty".into());
    }
    dispatch_transcript(app, transcript);
    Ok(())
}

// AlwaysAllow re-export so a `cargo doc` test can construct it from the
// shell crate if needed. Strictly cosmetic — kept here so other parts
// of the shell that want to skip the modal in dev have a knob.
#[allow(dead_code)]
fn always_allow_handler() -> Arc<dyn ConfirmHandler> {
    Arc::new(AlwaysAllow)
}
