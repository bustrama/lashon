//! Lashon desktop core — the Tauri 2 application.
//!
//! This crate is a thin GUI shell. The testable provider and sidecar logic
//! lives in the `lashon-core` crate (`packages/shared-rust`); see
//! `docs/adr/0003-core-logic-in-a-tauri-independent-crate.md`.
//!
//! It owns the tongue window, the tray, and the global hotkeys, and delegates
//! capture, transcription, and text injection to the lashon-core crate.

#[cfg(feature = "command-mode")]
mod command_mode;
mod dictation;
#[cfg(feature = "command-mode")]
mod llm;
#[cfg(feature = "command-mode")]
mod recipes;
mod wakeword;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Listener, Manager, PhysicalPosition};
use tauri_plugin_store::StoreExt;

#[cfg(feature = "command-mode")]
use lashon_core::llama_server::LlamaServerState;
use lashon_core::sidecar::{self, HealthReport, SidecarState};

/// Drag-loop state shared by `start_window_drag` / `stop_window_drag`.
/// A single atomic flag is enough — only one drag at a time per app
/// (the tongue is the only draggable window).
#[derive(Default)]
pub struct DragState {
    active: Arc<AtomicBool>,
}

/// Suspend gates shared by the dictation and wake-word workers.
///
/// The wake-word detector pauses while dictation is capturing — and, from M10,
/// while TTS is speaking — so it never self-triggers on Lashon's own
/// microphone audio (`.claude/rules/architecture.md`).
#[derive(Clone, Default)]
pub struct Gates {
    pub is_capturing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub is_speaking: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Probe the STT sidecar and report whether it is reachable and serving.
///
/// Invoked from the frontend debug surface (Ctrl+Shift+D).
#[tauri::command]
async fn lashon_healthcheck(state: tauri::State<'_, SidecarState>) -> Result<HealthReport, String> {
    Ok(sidecar::healthcheck(&state).await)
}

/// Validate a candidate dictation hotkey for the Settings Hub.
///
/// Returns the `HotkeyError` reason code on rejection (`reserved`,
/// `no-modifier`, …) so the Hub can render a localized explanation. The rule
/// itself lives in `lashon-core` and is unit-tested there.
#[tauri::command]
fn validate_hotkey(accelerator: String) -> Result<(), String> {
    lashon_core::hotkey::validate_accelerator(&accelerator).map_err(|err| err.code().to_string())
}

/// Reveal the Settings Hub — invoked by a double-click on the tongue, the same
/// window the tray "Settings" entry opens.
#[tauri::command]
fn open_hub(app: tauri::AppHandle) {
    show_hub(&app);
}

/// Relaunch the app — invoked by the Hub's restart control so a hardware-tier
/// change takes effect at once (the STT sidecar reads `LASHON_STT_DEVICE` only
/// at startup — see `configure_stt_device_env`).
#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// Detect the host's hardware tier for the onboarding hardware step and the
/// Hub's Hardware section (docs/tech-stack.md, docs/adr/0013).
///
/// The probing (NVML, Vulkan, sysinfo) runs on a blocking thread so the
/// detection latency never stalls the webview's IPC.
#[tauri::command]
async fn detect_hardware() -> Result<lashon_core::hardware::HardwareReport, String> {
    tauri::async_runtime::spawn_blocking(lashon_core::hardware::detect)
        .await
        .map_err(|err| err.to_string())
}

/// Probe the microphone for the onboarding mic step. On macOS the first call
/// also raises the OS microphone-permission prompt (docs/adr/0013).
///
/// Run on a blocking thread: opening the capture stream — and, on a first-run
/// macOS prompt, waiting for the user — must not block the main thread.
#[tauri::command]
async fn probe_microphone() -> Result<lashon_core::audio::MicProbe, String> {
    tauri::async_runtime::spawn_blocking(lashon_core::audio::probe_input)
        .await
        .map_err(|err| err.to_string())
}

/// The wake-word classifier models installed on disk — the filenames without
/// the `.onnx` suffix, sorted. Drives the Hub's wake-word picker.
#[tauri::command]
fn list_wake_models() -> Vec<String> {
    lashon_core::model::list_wake_models()
}

/// The opt-in wake-word classifiers the Hub can offer to download — each is
/// CC-BY-NC and is never bundled (see models/manifests/wake-classifiers.json).
#[tauri::command]
fn available_wake_models() -> Vec<lashon_core::model::AvailableWakeModel> {
    lashon_core::model::available_wake_models()
}

/// Download and verify one of the opt-in wake-word classifiers, placing it in
/// the wake-words directory. The Hub shows the licence badge in a confirmation
/// dialog before invoking this command.
#[tauri::command]
async fn install_wake_model(id: String) -> Result<String, String> {
    lashon_core::model::install_wake_classifier(&id)
        .await
        .map_err(|err| format!("{err:#}"))
}

/// Show the tongue's right-click context menu — the same items as the tray.
/// Invoked on a `contextmenu` event from the tongue window.
#[tauri::command]
fn show_tongue_menu(window: tauri::Window, menu: tauri::State<'_, Menu<tauri::Wry>>) {
    use tauri::menu::ContextMenu;
    if let Err(err) = menu.popup(window) {
        tracing::warn!("could not show the tongue context menu: {err:#}");
    }
}

/// Start a server-side window drag for the tongue.
///
/// `offset_x` / `offset_y` are the cursor's position relative to the window's
/// top-left at drag start (physical px). Rust then spins a background task
/// that polls the cursor every ~8 ms and re-positions the window so the
/// cursor stays at the same relative offset — i.e. the window follows
/// the cursor 1:1. Calling `stop_window_drag` (or losing the JS hand on
/// mouseup) flips the active flag and the loop exits next tick.
///
/// Why: the JS-side equivalent (mousemove → setPosition per frame) is
/// rate-limited by IPC latency; even coalesced down to one in-flight
/// call at a time, the window visibly lags behind the cursor. Running
/// the loop in Rust eliminates the per-frame IPC entirely — set_position
/// is a direct Tauri/Wry call (no IPC roundtrip), so the drag runs at
/// 120 Hz with no perceptible lag.
#[tauri::command]
async fn start_window_drag(
    window: tauri::WebviewWindow,
    drag: tauri::State<'_, DragState>,
    offset_x: f64,
    offset_y: f64,
) -> Result<(), String> {
    // If a previous drag is still running (race), tell it to stop. The new
    // drag picks up cleanly from the current cursor.
    drag.active.store(false, Ordering::SeqCst);
    // Tiny yield so any in-flight tick observes the stop.
    tokio::task::yield_now().await;
    drag.active.store(true, Ordering::SeqCst);
    let active = drag.active.clone();
    let win = window.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(8));
        // The first tick fires immediately — without `Burst` mode the
        // delay-then-tick behaviour skips the initial cursor sample.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while active.load(Ordering::SeqCst) {
            interval.tick().await;
            let cursor = match win.cursor_position() {
                Ok(p) => p,
                Err(_) => break,
            };
            let new_x = (cursor.x - offset_x).round() as i32;
            let new_y = (cursor.y - offset_y).round() as i32;
            if win
                .set_position(PhysicalPosition::new(new_x, new_y))
                .is_err()
            {
                break;
            }
        }
    });
    Ok(())
}

/// Stop the server-side window drag — sets the flag the loop polls.
#[tauri::command]
fn stop_window_drag(drag: tauri::State<'_, DragState>) {
    drag.active.store(false, Ordering::SeqCst);
}

/// Diagnostic — re-emit a frontend message to the Rust `tracing` stream so it
/// appears in the same terminal as the dictation / command-mode `INFO` logs.
/// Used by the M8.3 tongue ResizeObserver while we triangulate the cropping
/// bug; it's the cheapest way to surface frontend state when devtools aren't
/// open (and on this borderless WebView2 window, F12 is unreliable).
///
/// Safe to leave wired up — frontend code only calls it from the autoResize
/// action, which is dormant outside Command mode.
#[tauri::command]
fn log_tongue_diag(message: String) {
    tracing::info!(target: "lashon::tongue_diag", "tongue: {}", message);
}

/// Check GitHub Releases for a newer version of Lashon.
///
/// The updater plugin verifies the minisign signature on the manifest before
/// returning a hit. On a hit this command downloads and installs the update,
/// emitting `updater:progress` events to the Hub. The actual relaunch is left
/// to the user — the Hub drives it via the existing `restart_app` command so
/// the user can choose when to restart.
///
/// Returns `"up-to-date"` when no update is available, or `"installed"` when
/// the update has been downloaded and applied — the next launch runs the new
/// version.
#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app
        .updater()
        .map_err(|err| format!("updater unavailable: {err:#}"))?;

    let update = updater
        .check()
        .await
        .map_err(|err| format!("update check failed: {err:#}"))?;

    let Some(update) = update else {
        tracing::info!("Lashon is up to date");
        return Ok("up-to-date".to_string());
    };

    tracing::info!(
        version = %update.version,
        current = %update.current_version,
        "update available — downloading"
    );

    let _ = app.emit(
        "updater:progress",
        serde_json::json!({
            "status": "downloading",
            "version": update.version,
            "current_version": update.current_version
        }),
    );

    // The plugin invokes the chunk callback on every ~8 KB read. Emitting
    // one IPC event per chunk saturates the Tauri bridge (each emit serialises
    // JSON and crosses into the JS event loop), which on Windows can throttle
    // an otherwise gigabit-network download to ~1 Mbps. Cap emissions to one
    // per integer-percent crossing — ~100 events per download.
    let mut downloaded: u64 = 0;
    let mut last_emit_percent: i32 = -1;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let percent: f64 = total
                    .map(|t| {
                        if t == 0 {
                            0.0
                        } else {
                            (downloaded as f64 / t as f64) * 100.0
                        }
                    })
                    .unwrap_or(0.0);
                let percent_int = percent as i32;
                if percent_int == last_emit_percent {
                    return;
                }
                last_emit_percent = percent_int;
                let _ = app.emit(
                    "updater:progress",
                    serde_json::json!({
                        "status": "downloading",
                        "downloaded": downloaded,
                        "total": total,
                        "percent": percent
                    }),
                );
            },
            || {
                let _ = app.emit(
                    "updater:progress",
                    serde_json::json!({
                        "status": "installing"
                    }),
                );
            },
        )
        .await
        .map_err(|err| format!("update install failed: {err:#}"))?;

    tracing::info!("update installed — waiting for user to relaunch");
    let _ = app.emit(
        "updater:progress",
        serde_json::json!({
            "status": "installed"
        }),
    );

    Ok("installed".to_string())
}

/// Build and run the Lashon desktop application.
pub fn run() {
    init_tracing();
    tracing::info!("Lashon starting");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(SidecarState::default())
        .manage(DragState::default());

    // Command-mode-only managed state — compiled out of the free build (ADR-0034).
    #[cfg(feature = "command-mode")]
    let builder = builder
        .manage(LlamaServerState::default())
        .manage(command_mode::ActiveDispatch::default());

    builder
        // Menu selections from the tongue's right-click context menu arrive
        // here; the tray menu keeps its own handler (both call the same
        // `handle_menu_event`).
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .invoke_handler(tauri::generate_handler![
            lashon_healthcheck,
            validate_hotkey,
            detect_hardware,
            probe_microphone,
            list_wake_models,
            available_wake_models,
            install_wake_model,
            open_hub,
            restart_app,
            check_for_updates,
            show_tongue_menu,
            log_tongue_diag,
            start_window_drag,
            stop_window_drag,
            dictation::dictation_hotkey_pressed,
            dictation::dictation_hotkey_released,
            // --- command-mode-only commands; compiled out of the free build (ADR-0034) ---
            #[cfg(feature = "command-mode")]
            dictation::command_hotkey_pressed,
            #[cfg(feature = "command-mode")]
            dictation::command_hotkey_released,
            #[cfg(feature = "command-mode")]
            command_mode::command_mode_status,
            #[cfg(feature = "command-mode")]
            command_mode::command_mode_dispatch_text,
            #[cfg(feature = "command-mode")]
            command_mode::cancel_command,
            #[cfg(feature = "command-mode")]
            llm::get_llm_providers,
            #[cfg(feature = "command-mode")]
            llm::set_llm_provider,
            #[cfg(feature = "command-mode")]
            llm::save_api_key,
            #[cfg(feature = "command-mode")]
            llm::has_api_key,
            #[cfg(feature = "command-mode")]
            llm::delete_api_key,
            #[cfg(feature = "command-mode")]
            llm::detect_ollama,
            #[cfg(feature = "command-mode")]
            llm::test_llm_prompt,
            #[cfg(feature = "command-mode")]
            llm::fetch_provider_models,
            #[cfg(feature = "command-mode")]
            llm::local_llm_status,
            #[cfg(feature = "command-mode")]
            llm::install_local_llm,
            #[cfg(feature = "command-mode")]
            llm::delete_local_llm,
            #[cfg(feature = "command-mode")]
            recipes::list_recipes_for_hub,
            #[cfg(feature = "command-mode")]
            recipes::get_recipe,
            #[cfg(feature = "command-mode")]
            recipes::run_recipe,
            #[cfg(feature = "command-mode")]
            recipes::open_recipe_file,
            #[cfg(feature = "command-mode")]
            recipes::duplicate_recipe_to_user,
            #[cfg(feature = "command-mode")]
            recipes::delete_user_recipe,
            #[cfg(feature = "command-mode")]
            recipes::update_recipe_comment,
            #[cfg(feature = "command-mode")]
            command_mode::get_word_aliases,
            #[cfg(feature = "command-mode")]
            command_mode::set_word_aliases
        ])
        .setup(|app| {
            // Point lashon-core at the bundled, frozen STT sidecar and a
            // per-user model directory before the dictation worker can spawn
            // it. In `tauri dev` the resources are absent and this is a no-op.
            configure_sidecar_env(app);
            configure_stt_device_env(app);
            stage_bundled_audio_models(app);

            // The dictation worker is spawned here, after the `AppHandle`
            // exists. The wake-word worker runs through a controller so the
            // Hub can toggle it, change sensitivity, or switch models without
            // an app restart — `settings:changed` events trigger a reload.
            let gates = Gates::default();
            app.manage(dictation::spawn_worker(app.handle().clone(), gates.clone()));

            let controller = Arc::new(Mutex::new(wakeword::WakeController::new()));
            if let Ok(mut ctrl) = controller.lock() {
                ctrl.reload(app.handle().clone(), gates.clone());
            }
            app.manage(controller.clone());

            // Live-reload the wake worker on wake-word settings changes.
            let listen_app = app.handle().clone();
            let listen_gates = gates;
            let listen_controller = controller;
            app.handle().listen("settings:changed", move |event| {
                let payload = event.payload();
                let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                    return;
                };
                let key = value.get("key").and_then(|value| value.as_str());
                let Some(key) = key else { return };
                if !key.starts_with("wakeword.") {
                    return;
                }
                if let Ok(mut ctrl) = listen_controller.lock() {
                    ctrl.reload(listen_app.clone(), listen_gates.clone());
                }
            });

            // One bilingual menu, shared by the tray and the tongue's
            // right-click context menu (see `show_tongue_menu`). The labels
            // are built once and are not re-localized when the language
            // changes — show the tongue, open the Settings Hub, replay the
            // tutorial, or quit.
            let menu = build_app_menu(app.handle())?;
            // The tray uses the background-free mark so it sits cleanly on the
            // taskbar; the window and installer keep the framed icon.
            TrayIconBuilder::with_id("lashon-tray")
                .icon(tauri::include_image!("icons/tray.png"))
                .tooltip("Lashon · לשון")
                .menu(&menu)
                .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
                .build(app)?;
            // Keep the menu alive and reachable for the right-click context menu.
            app.manage(menu);

            // First run reveals the interactive tutorial (issue #9) over the
            // tongue — both windows are on screen, so the practice step's
            // live feedback is visible. The tutorial window ships hidden in
            // tauri.conf.json; show it until the user has finished or skipped
            // it once. The frontend records `tutorial.completed` in the
            // `tauri-plugin-store` settings file.
            if !tutorial_completed(app.handle()) {
                show_tutorial(app.handle(), false);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Lashon application");
}

/// Whether the user has finished or skipped the first-run tutorial.
///
/// The frontend persists `tutorial.completed` in the `tauri-plugin-store`
/// settings file when the tutorial window is dismissed. A missing flag, or a
/// store that cannot be opened, is treated as "not yet done" — the tutorial
/// should err towards showing on a genuine first run.
fn tutorial_completed(app: &tauri::AppHandle) -> bool {
    match app.store("settings.json") {
        Ok(store) => store
            .get("tutorial.completed")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        Err(err) => {
            tracing::warn!("could not open the settings store: {err:#}");
            false
        }
    }
}

/// Reveal and focus the tutorial window. When `restart` is set (the tray
/// "Tutorial" entry), emit `tutorial:open` so the page rewinds to step one —
/// the window is only hidden, never destroyed, so its state would otherwise
/// persist from the previous viewing.
fn show_tutorial(app: &tauri::AppHandle, restart: bool) {
    let Some(window) = app.get_webview_window("tutorial") else {
        tracing::warn!("tutorial window is not registered");
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
    if restart {
        let _ = window.emit("tutorial:open", ());
    }
}

/// Reveal and focus the Settings Hub window. Like the tutorial it is only
/// hidden, never destroyed; reopening keeps the last-viewed section, which is
/// the right behaviour for a settings surface.
fn show_hub(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("hub") else {
        tracing::warn!("hub window is not registered");
        return;
    };
    let _ = window.show();
    let _ = window.set_focus();
}

/// Build Lashon's bilingual menu — shared by the tray and the tongue's
/// right-click context menu. Labels are `Hebrew · English`; the menu is built
/// once and is not re-localized when the in-app language changes.
fn build_app_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, "show", "הצג את לשון · Show Lashon", true, None::<&str>)?;
    let tutorial = MenuItem::with_id(app, "tutorial", "מדריך · Tutorial", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "הגדרות · Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "יציאה · Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&show, &tutorial, &settings, &quit])
}

/// Dispatch a menu selection — shared by the tray menu and the tongue's
/// right-click context menu, which carry the same item ids.
fn handle_menu_event(app: &tauri::AppHandle, id: &str) {
    match id {
        "show" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        "tutorial" => show_tutorial(app, true),
        "settings" => show_hub(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Point `lashon-core` at the bundled STT sidecar and the per-user model
/// directory via the `LASHON_STT_SIDECAR` / `LASHON_MODELS_ROOT` env vars.
///
/// In a packaged build the PyInstaller-frozen sidecar ships as a bundle
/// resource. In `tauri dev` that resource does not exist, so both variables
/// stay unset and `lashon-core` runs the sidecar from Python source against
/// the repository's `models/` tree (see `docs/adr/0006`, `docs/adr/0018`).
///
/// The sidecar binary name is OS-specific (see `docs/adr/0018`):
///   - Windows: `lashon-stt.exe`
///   - macOS / Linux: `lashon-stt` (no extension)
fn configure_sidecar_env(app: &tauri::App) {
    #[cfg(target_os = "windows")]
    let sidecar_rel = "binaries/lashon-stt/lashon-stt.exe";
    #[cfg(not(target_os = "windows"))]
    let sidecar_rel = "binaries/lashon-stt/lashon-stt";

    let sidecar = app
        .path()
        .resolve(sidecar_rel, tauri::path::BaseDirectory::Resource);
    let Ok(sidecar) = sidecar else { return };
    if !sidecar.is_file() {
        return;
    }

    std::env::set_var("LASHON_STT_SIDECAR", &sidecar);
    tracing::info!(path = %sidecar.display(), "using the bundled STT sidecar");

    match app.path().app_local_data_dir() {
        Ok(dir) => {
            // Per-user directories for the downloaded STT model (~1.6 GB) and,
            // when an NVIDIA GPU is present, the CUDA runtime (~1.2 GB).
            let models = dir.join("models");
            let cuda = dir.join("cuda");
            for path in [&models, &cuda] {
                if let Err(err) = std::fs::create_dir_all(path) {
                    tracing::warn!("could not create {}: {err:#}", path.display());
                }
            }
            std::env::set_var("LASHON_MODELS_ROOT", &models);
            std::env::set_var("LASHON_CUDA_ROOT", &cuda);
            tracing::info!(
                models = %models.display(),
                cuda = %cuda.display(),
                "STT data directories"
            );
        }
        Err(err) => tracing::error!("could not resolve the app-data directory: {err:#}"),
    }
}

/// Stage every bundled audio-pipeline ONNX into the per-user models
/// directory the engines read from. Three model sets ship in the
/// installer — the MIT wake classifier(s) (`docs/adr/0016`), the
/// Apache-2.0 openWakeWord shared melspectrogram + embedding
/// (`docs/adr/0016`), and the MIT Silero VAD v5 (`docs/adr/0015`).
/// Idempotent — a user's own replacement at any target path is left
/// untouched.
///
/// Resources listed with parent-relative paths in `tauri.conf.json` land
/// under `_up_/.../` in the resource directory; the live source-of-truth
/// for each is under `models/` three levels above `src-tauri/`.
fn stage_bundled_audio_models(app: &tauri::App) {
    let Some(models_root) = std::env::var_os("LASHON_MODELS_ROOT") else {
        // configure_sidecar_env sets this only in a packaged build — there
        // is no bundle in `tauri dev`, so there is nothing to stage.
        return;
    };
    let root = std::path::PathBuf::from(models_root);

    // (resource-side subpath, on-disk target directory under $LASHON_MODELS_ROOT).
    // Targets mirror `lashon_core::model`'s `model_dir(<local_dir>)` resolution:
    // it takes the basename of the manifest's `local_dir`, so e.g.
    // "models/wake/openwakeword" → "$LASHON_MODELS_ROOT/openwakeword".
    let layouts: [(&str, std::path::PathBuf); 3] = [
        (
            "_up_/_up_/_up_/models/wake/wakewords",
            root.join("wakewords"),
        ),
        (
            "_up_/_up_/_up_/models/wake/openwakeword",
            root.join("openwakeword"),
        ),
        (
            "_up_/_up_/_up_/models/vad/silero-vad-v5",
            root.join("silero-vad-v5"),
        ),
    ];

    for (resource_subpath, target_dir) in layouts {
        let bundled_dir = app
            .path()
            .resolve(resource_subpath, tauri::path::BaseDirectory::Resource);
        let Ok(bundled_dir) = bundled_dir else {
            tracing::debug!(
                resource = %resource_subpath,
                "no bundled directory in this build"
            );
            continue;
        };
        match lashon_core::model::install_bundled_wake_classifiers(&bundled_dir, &target_dir) {
            Ok(0) => {}
            Ok(count) => tracing::info!(
                count,
                target = %target_dir.display(),
                "staged bundled audio-pipeline models"
            ),
            Err(err) => tracing::warn!(
                source = %bundled_dir.display(),
                target = %target_dir.display(),
                "could not stage bundled audio-pipeline models: {err:#}"
            ),
        }
    }
}

/// Select the STT device mode from the detected hardware tier and hand it to
/// the sidecar via the `LASHON_STT_DEVICE` environment variable (docs/adr/0014).
///
/// Read from the `settings.json` store at startup — before the dictation
/// worker spawns the sidecar — so the sidecar inherits the choice. A tier
/// change in the Hub therefore takes effect on the next app launch. With no
/// tier saved yet (onboarding not run) the GPU-probing `auto` mode is used,
/// which is the sidecar's existing behaviour.
fn configure_stt_device_env(app: &tauri::App) {
    let device = stt_device_from_tier(app).unwrap_or(lashon_core::hardware::STT_DEVICE_AUTO);
    std::env::set_var("LASHON_STT_DEVICE", device);
    tracing::info!(device, "STT device mode selected from the hardware tier");
}

/// The STT device mode for the saved `hardware.tier`, or `None` when no valid
/// tier is stored (a fresh install, before onboarding).
fn stt_device_from_tier(app: &tauri::App) -> Option<&'static str> {
    let store = app.store("settings.json").ok()?;
    let code = store.get("hardware.tier")?;
    let tier = lashon_core::hardware::Tier::from_code(code.as_str()?)?;
    Some(tier.stt_device())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
