//! Wake-word detection worker with two independent slots.
//!
//! Lashon hosts up to **two** wake-word engines simultaneously:
//!
//! - **Dictation slot** — when its classifier fires, the dictation
//!   worker starts an `Inject` take (transcript types into the focused
//!   field).
//! - **Command slot** — when its classifier fires, the M8 Command-mode
//!   dispatcher gets the transcript (transcript runs as a tool chain).
//!
//! Each slot has its own settings (`wakeword.<slot>.enabled`,
//! `wakeword.<slot>.sensitivity`, `wakeword.<slot>.model`) and its own
//! [`WakeWord`] engine. The two slots **must** name different
//! classifiers — a single utterance can't be classified as both a
//! Dictation and a Command intent. The Hub picker enforces this; the
//! backend defensively skips the Command slot if both pick the same
//! model (Dictation wins).
//!
//! On `settings:changed` events under `wakeword.*` the controller
//! restarts the worker, picking up new slot configs without an app
//! restart.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use lashon_core::wake::{Trigger, WakeWord, CHUNK_SAMPLES};
use lashon_core::{audio, model};

use crate::dictation::DictationChannel;
use crate::Gates;

/// How often the worker wakes up when no audio chunk has arrived, so it can
/// notice the shutdown signal from the controller.
const POLL: Duration = Duration::from_millis(100);

/// The wake-word model used when `wakeword.dictation.model` is unset on a
/// fresh install. The default is aspirational — `hey_lashon.onnx` is the
/// offline-trained classifier (docs/wake-word-training.md). Until trained,
/// fresh installs will idle the wake word with "classifier model is absent"
/// and the Hub picker lets the user select a different installed model.
const DEFAULT_MODEL: &str = "hey_lashon";

/// How long to wait after firing the wake event before opening the
/// dictation take. The frontend plays a short acknowledgment chime on
/// `wake:detected` (~220 ms of audio + a few tens of ms of frontend
/// round-trip), and the laptop's open microphone picks that chime up
/// straight through the speakers if capture starts immediately — the
/// chime bleed drowns the user's voice and STT returns nothing. Holding
/// the trigger until the chime is over keeps the dictation mic clean.
const CHIME_BLEED_GUARD: Duration = Duration::from_millis(300);

/// Which take mode a wake slot fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotMode {
    /// Inject the transcript at the cursor (legacy "dictation" mode).
    Dictation,
    /// Hand the transcript to the M8 Command-mode dispatcher. Compiled out of
    /// the free dictation build (ADR-0034).
    #[cfg(feature = "command-mode")]
    Command,
}

impl SlotMode {
    /// The string the `wake:detected` event payload carries — same wording
    /// the frontend's `takeMode` state machine uses, so the chime handler
    /// can switch on it without a translation step.
    fn as_event_str(self) -> &'static str {
        match self {
            SlotMode::Dictation => "dictation",
            #[cfg(feature = "command-mode")]
            SlotMode::Command => "command",
        }
    }
}

/// Payload of the `wake:detected` event. Fired by the wake-word worker
/// the instant a slot's classifier passes its threshold — the frontend
/// uses it to play a short acknowledgment chime so the user knows the
/// wake phrase landed. Hotkey-triggered takes do NOT emit this event;
/// a manual press doesn't need confirming.
#[derive(Debug, Clone, Serialize)]
struct WakeDetectedEvent {
    mode: &'static str,
}

/// Settings for one wake slot. The slot is **off** when `enabled` is
/// false OR when the classifier file is absent (see `SlotEngine::load`).
#[derive(Debug, Clone)]
struct SlotSettings {
    enabled: bool,
    sensitivity: f32,
    model: String,
}

impl SlotSettings {
    fn threshold(&self) -> f32 {
        (1.0 - self.sensitivity).clamp(0.1, 0.9)
    }
}

/// Read both slots' settings from `settings.json`, applying the
/// one-shot legacy migration described in this module's docs.
fn read_settings(app: &AppHandle) -> (SlotSettings, SlotSettings) {
    let Ok(store) = app.store("settings.json") else {
        return (
            SlotSettings {
                enabled: false,
                sensitivity: 0.7,
                model: DEFAULT_MODEL.to_string(),
            },
            SlotSettings {
                enabled: false,
                sensitivity: 0.7,
                model: String::new(),
            },
        );
    };

    // Legacy migration: if the user upgraded from a build that knew
    // only `wakeword.enabled`/`.sensitivity`/`.model`, copy those into
    // the new `.dictation.*` keys exactly once. The Command slot
    // starts off — the user opts in explicitly.
    let legacy_enabled = store.get("wakeword.enabled").and_then(|v| v.as_bool());
    let legacy_sensitivity = store.get("wakeword.sensitivity").and_then(|v| v.as_f64());
    let legacy_model = store
        .get("wakeword.model")
        .and_then(|v| v.as_str().map(String::from));

    let dictation_enabled = store
        .get("wakeword.dictation.enabled")
        .and_then(|v| v.as_bool())
        .or(legacy_enabled)
        .unwrap_or(false);
    let dictation_sensitivity = store
        .get("wakeword.dictation.sensitivity")
        .and_then(|v| v.as_f64())
        .or(legacy_sensitivity)
        .unwrap_or(0.7) as f32;
    let dictation_model = store
        .get("wakeword.dictation.model")
        .and_then(|v| v.as_str().map(String::from))
        .or(legacy_model)
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let command_enabled = store
        .get("wakeword.command.enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let command_sensitivity = store
        .get("wakeword.command.sensitivity")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7) as f32;
    // No default model for the Command slot — leaving it empty means
    // the slot is effectively off until the user picks one.
    let command_model = store
        .get("wakeword.command.model")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();

    (
        SlotSettings {
            enabled: dictation_enabled,
            sensitivity: dictation_sensitivity,
            model: dictation_model,
        },
        SlotSettings {
            enabled: command_enabled,
            sensitivity: command_sensitivity,
            model: command_model,
        },
    )
}

/// Per-slot runtime — one engine + trigger debouncer + the slot's
/// fixed threshold and target mode. Built once per worker spawn from
/// [`SlotSettings`].
struct SlotEngine {
    mode: SlotMode,
    engine: WakeWord,
    trigger: Trigger,
    threshold: f32,
    model_name: String,
}

impl SlotEngine {
    /// Build the engine if the slot is enabled AND the classifier is
    /// installed. Returns `None` (with a logged reason) otherwise.
    fn load(mode: SlotMode, settings: &SlotSettings) -> Option<Self> {
        if !settings.enabled {
            tracing::info!(?mode, "wake word slot: disabled");
            return None;
        }
        if settings.model.trim().is_empty() {
            tracing::info!(?mode, "wake word slot: no classifier picked — idling");
            return None;
        }
        let classifier = model::wake_classifier_path(&settings.model);
        if !classifier.is_file() {
            tracing::warn!(
                ?mode,
                model = %settings.model,
                path = %classifier.display(),
                "wake word slot: enabled, but classifier model is absent — idling"
            );
            return None;
        }
        let engine = match WakeWord::load(&classifier) {
            Ok(engine) => engine,
            Err(err) => {
                tracing::error!(?mode, "wake word slot: could not load engine: {err:#}");
                return None;
            }
        };
        let threshold = settings.threshold();
        tracing::info!(
            ?mode,
            model = %settings.model,
            threshold,
            "wake word slot: listening"
        );
        Some(Self {
            mode,
            engine,
            trigger: Trigger::default(),
            threshold,
            model_name: settings.model.clone(),
        })
    }

    /// Feed one 80 ms frame; on a fresh trigger event, route to the
    /// appropriate channel and log it.
    fn observe(&mut self, frame: &[f32], app: &AppHandle) {
        match self.engine.observe(frame) {
            Ok(score) => {
                if self.trigger.observe(score, self.threshold) {
                    tracing::info!(
                        mode = ?self.mode,
                        model = %self.model_name,
                        "wake word: detected"
                    );
                    // Tell the frontend the wake phrase landed so it can
                    // play an acknowledgment chime BEFORE the STT worker
                    // starts spinning — the user wants the audible signal
                    // synchronous with their utterance, not the take.
                    let _ = app.emit(
                        "wake:detected",
                        WakeDetectedEvent {
                            mode: self.mode.as_event_str(),
                        },
                    );
                    // Defer the trigger by CHIME_BLEED_GUARD so the chime
                    // finishes through the speakers before the dictation
                    // worker opens the mic — otherwise the chime bleeds
                    // back into capture and STT comes back empty (see the
                    // constant's doc-comment). Spawning keeps the wake
                    // detection loop free to keep processing audio frames.
                    let app_for_trigger = app.clone();
                    let mode = self.mode;
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(CHIME_BLEED_GUARD).await;
                        let channel = app_for_trigger.state::<DictationChannel>();
                        match mode {
                            SlotMode::Dictation => channel.trigger(),
                            #[cfg(feature = "command-mode")]
                            SlotMode::Command => channel.trigger_command(),
                        }
                    });
                }
            }
            Err(err) => tracing::error!(mode = ?self.mode, "wake word: inference failed: {err:#}"),
        }
    }

    fn reset(&mut self) {
        self.engine.reset();
    }
}

/// A Tauri-managed handle for live-restarting the wake-word worker.
///
/// `reload` signals the active worker to exit on its next poll tick and spawns
/// a fresh worker with current settings. The new worker may briefly overlap
/// the old one's microphone stream — Windows WASAPI tolerates that — but the
/// old worker's `running` flag is `false`, so it stops producing detections.
pub struct WakeController {
    running: Arc<AtomicBool>,
}

impl WakeController {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Stop the active worker and start a fresh one from current settings.
    pub fn reload(&mut self, app: AppHandle, gates: Gates) {
        self.running.store(false, Ordering::Relaxed);
        let running = Arc::new(AtomicBool::new(true));
        self.running = running.clone();
        if let Err(err) = thread::Builder::new()
            .name("lashon-wakeword".into())
            .spawn(move || run_worker(app, gates, running))
        {
            tracing::error!("wake word: could not spawn the worker thread: {err}");
        }
    }
}

impl Default for WakeController {
    fn default() -> Self {
        Self::new()
    }
}

fn run_worker(app: AppHandle, gates: Gates, running: Arc<AtomicBool>) {
    let (dictation_settings, command_settings) = read_settings(&app);

    let mut slots: Vec<SlotEngine> = Vec::with_capacity(2);
    if let Some(slot) = SlotEngine::load(SlotMode::Dictation, &dictation_settings) {
        slots.push(slot);
    }

    // The Command wake slot is part of the paid command-mode tier (ADR-0034)
    // and is compiled out of the free dictation build.
    #[cfg(feature = "command-mode")]
    {
        let mut command_settings = command_settings;
        // Defensive: even if the Hub picker malfunctions and persists the
        // same classifier for both slots, we never run two engines on the
        // same model — the dispatch would be ambiguous. Dictation wins
        // (legacy precedent) and we log so the user can spot it in the Hub.
        if dictation_settings.enabled
            && command_settings.enabled
            && !dictation_settings.model.trim().is_empty()
            && dictation_settings.model == command_settings.model
        {
            tracing::warn!(
                model = %dictation_settings.model,
                "wake word: Command slot uses the same classifier as Dictation; \
                 disabling Command slot (pick a different model in the Hub)"
            );
            command_settings.enabled = false;
        }
        if let Some(slot) = SlotEngine::load(SlotMode::Command, &command_settings) {
            slots.push(slot);
        }
    }
    #[cfg(not(feature = "command-mode"))]
    let _ = command_settings;

    if slots.is_empty() {
        // Both slots off / unloadable. The worker exits — `WakeController`
        // re-spawns it on the next `settings:changed` event.
        return;
    }

    // `_stream` must outlive the loop — dropping it ends capture.
    let (_stream, audio_rx) = match audio::open_wake_stream() {
        Ok(pair) => pair,
        Err(err) => {
            tracing::error!("wake word: could not open the microphone: {err:#}");
            return;
        }
    };

    let mut pending: Vec<f32> = Vec::new();
    let mut suspended = false;

    while running.load(Ordering::Relaxed) {
        match audio_rx.recv_timeout(POLL) {
            Ok(chunk) => {
                // Suspend while dictation is capturing — or, from M10,
                // while TTS is speaking — so wake never fires on
                // Lashon's own audio. Same gate suspends ALL slots
                // (we don't want Command-mode-wake firing during a
                // dictation take either).
                if gates.is_capturing.load(Ordering::Relaxed)
                    || gates.is_speaking.load(Ordering::Relaxed)
                {
                    if !suspended {
                        for slot in &mut slots {
                            slot.reset();
                        }
                        pending.clear();
                        suspended = true;
                    }
                    continue;
                }
                suspended = false;

                pending.extend_from_slice(&chunk);
                while pending.len() >= CHUNK_SAMPLES {
                    let frame: Vec<f32> = pending.drain(..CHUNK_SAMPLES).collect();
                    for slot in &mut slots {
                        slot.observe(&frame, &app);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    tracing::info!(slots = slots.len(), "wake word: stopped");
}
