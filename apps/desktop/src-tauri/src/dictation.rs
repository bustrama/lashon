//! Dictation: hotkey → capture → transcribe → inject.
//!
//! A dedicated worker thread owns the (`!Send`) audio stream, the STT
//! provider, and the Silero VAD model, so capture, voice-activity detection,
//! transcription, and injection all stay on one thread. The Tauri commands
//! only forward hotkey edges to it.
//!
//! Two activation modes (docs/roadmap.md §1.1):
//! - **Hands-free** (default) — press the hotkey once; capture runs until
//!   Silero VAD's endpoint detector reports the utterance has ended, a second
//!   press, or a hard cap.
//! - **Hold** — capture runs only while the hotkey is held.
//!
//! The mode becomes a user setting in a later milestone.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use lashon_core::audio::{AudioCapture, TARGET_RATE};
use lashon_core::inject::inject_text;
use lashon_core::stt::{FasterWhisperProvider, SttProvider};
use lashon_core::vad::{self, EndpointSignal, Endpointer, SileroVad, FRAME_SAMPLES};

/// Active activation mode. A settings panel will make this user-selectable;
/// for now hands-free is the default and hold mode waits behind it.
const DICTATION_MODE: DictationMode = DictationMode::HandsFree;

/// Hands-free: how often the capture loop wakes — fast enough to feed the
/// tongue's live waveform a fresh loudness reading.
const LEVEL_INTERVAL: Duration = Duration::from_millis(50);

/// Keep this much audio past the last detected speech and drop the rest, so
/// trailing silence never reaches the transcriber.
const TAIL_MARGIN: Duration = Duration::from_millis(500);

/// Hands-free: a hard cap on a take's length. A backstop against a capture
/// that never ends — a wedged detector, or VAD unavailable so only a second
/// press would otherwise stop it.
const MAX_TAKE: Duration = Duration::from_secs(30);

/// How often to poll the STT sidecar while it prepares the model on first run.
const MODEL_POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// End the model wait after this many consecutive polls with the sidecar
/// unreachable. `spawn()` carries an 8 s timeout, so this is ~30 s of a sidecar
/// that will not start — past which retrying only churns spawn-then-kill
/// (ADR-0010) and the tongue would sit on "preparing" forever.
const MAX_SIDECAR_FAILURES: u32 = 3;

/// How long the tongue holds the Error state before falling back to Idle.
const ERROR_DWELL: Duration = Duration::from_millis(1600);

/// How a dictation take is started and stopped.
#[derive(Clone, Copy)]
enum DictationMode {
    /// Capture only while the hotkey is held. Reachable once the settings
    /// panel can select it; `DICTATION_MODE` is the sole selector for now.
    #[allow(dead_code)]
    Hold,
    /// Press once to start; stop on the VAD endpoint, a second press, or the
    /// hard cap.
    HandsFree,
}

/// What the worker does with the transcript once the take finishes. M7
/// shipped only the `Inject` route (dictation); M8 adds `Command` which
/// hands the transcript to the LLM tool-call dispatcher
/// (`docs/adr/0024`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TakeMode {
    /// Inject the transcript at the user's cursor — original dictation.
    Inject,
    /// Hand the transcript to the Command-mode dispatcher. Compiled out of the
    /// free dictation build (ADR-0034).
    #[cfg(feature = "command-mode")]
    Command,
}

/// A hotkey edge forwarded from the frontend.
enum DictationCommand {
    HotkeyPressed(TakeMode),
    HotkeyReleased,
}

/// Tauri-managed handle for posting hotkey edges to the worker.
pub struct DictationChannel(Mutex<Sender<DictationCommand>>);

impl DictationChannel {
    fn send(&self, command: DictationCommand) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(command);
        }
    }

    /// Open a dictation take — the wake-word worker's equivalent of a hotkey
    /// press. Inject mode: transcript is typed into the focused field.
    pub fn trigger(&self) {
        self.send(DictationCommand::HotkeyPressed(TakeMode::Inject));
    }

    /// Open a Command-mode take — the Command wake slot's equivalent of
    /// the Command hotkey. Transcript is handed to the M8 dispatcher
    /// (`command_mode::dispatch_transcript`), not injected.
    #[cfg(feature = "command-mode")]
    pub fn trigger_command(&self) {
        self.send(DictationCommand::HotkeyPressed(TakeMode::Command));
    }
}

/// Spawn the dictation worker thread and return its Tauri-managed channel.
///
/// The worker emits `dictation:state` events, so it needs an `AppHandle`.
pub fn spawn_worker(app: AppHandle, gates: crate::Gates) -> DictationChannel {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("lashon-dictation".into())
        .spawn(move || run_worker(rx, app, gates))
        .expect("spawn the dictation worker thread");
    DictationChannel(Mutex::new(tx))
}

fn run_worker(rx: Receiver<DictationCommand>, app: AppHandle, gates: crate::Gates) {
    let mut capture = AudioCapture::new();
    let provider = FasterWhisperProvider::new();

    // The Silero VAD model drives hands-free auto-stop. It is small and loaded
    // once for the worker's lifetime. If it is missing — a fresh checkout
    // without the model, or a packaged build before its first-run download —
    // hands-free falls back to a second press (or the MAX_TAKE cap) to stop.
    let mut vad = match SileroVad::load() {
        Ok(silero) => Some(silero),
        Err(err) => {
            tracing::error!(
                "dictation: Silero VAD unavailable — hands-free auto-stop is off: {err:#}"
            );
            None
        }
    };

    // First-run readiness: the sidecar may be downloading the ~1.6 GB Hebrew
    // model. Hold the tongue in "preparing" until the model is ready.
    if !wait_for_model(&provider, &app, &rx) {
        return; // the channel closed — the app is shutting down
    }

    loop {
        // Idle between takes — the wake-word worker may detect now.
        gates.is_capturing.store(false, Ordering::Relaxed);

        // Idle — wait for the hotkey press that opens a take.
        let take_mode = match rx.recv() {
            Err(_) => return, // channel closed: the app is shutting down
            Ok(DictationCommand::HotkeyReleased) => continue, // stray release
            Ok(DictationCommand::HotkeyPressed(mode)) => mode,
        };

        if let Err(err) = capture.start() {
            tracing::error!("dictation: capture failed to start: {err:#}");
            emit_state(&app, "idle");
            continue;
        }
        // Suspend the wake-word detector for the whole take, so it never
        // self-triggers on the audio being dictated.
        gates.is_capturing.store(true, Ordering::Relaxed);
        emit_state(&app, "capturing");

        let outcome = listen(&rx, &capture, &mut vad, &app);
        let pcm = finish_take(&mut capture, &outcome);

        // Skip takes under ~0.25 s — a stray tap, not speech.
        let Some(pcm) = pcm.filter(|p| p.len() >= TARGET_RATE as usize / 4) else {
            emit_state(&app, "idle");
            continue;
        };

        emit_state(&app, "transcribing");
        // "" lets the sidecar auto-detect the language — Hebrew, English, or mixed.
        match tauri::async_runtime::block_on(provider.transcribe(&pcm, "")) {
            Ok(transcript) => {
                tracing::info!(
                    chars = transcript.text.chars().count(),
                    ms = transcript.inference_ms,
                    mode = ?take_mode,
                    "dictation transcribed"
                );
                // Broadcast the text so in-app surfaces — the first-run
                // tutorial's practice step — can echo back what was heard.
                // This is an in-process Tauri event; the text is never logged
                // (see .claude/rules/security.md).
                if let Err(err) = app.emit("dictation:transcript", &transcript.text) {
                    tracing::warn!("dictation: failed to emit transcript: {err}");
                }
                match take_mode {
                    TakeMode::Inject => match inject_text(&transcript.text) {
                        Ok(()) => emit_state(&app, "idle"),
                        Err(err) => {
                            tracing::error!("dictation: injection failed: {err:#}");
                            signal_error(&app);
                        }
                    },
                    #[cfg(feature = "command-mode")]
                    TakeMode::Command => {
                        // Hand the transcript off to the Command-mode
                        // dispatcher running as a Tauri async task. The
                        // worker returns to idle right away; the result
                        // comes back as a `command:result` event the
                        // tongue listens for (M8 docs/adr/0024).
                        emit_state(&app, "idle");
                        crate::command_mode::dispatch_transcript(app.clone(), transcript.text);
                    }
                }
            }
            Err(err) => {
                tracing::error!("dictation: transcription failed: {err:#}");
                signal_error(&app);
            }
        }
    }
}

/// Hold the tongue in "preparing" until the STT sidecar reports its model is
/// ready. On first run the sidecar downloads the model (~1.6 GB), which can
/// take minutes. Returns `false` when the channel closes (app shutting down).
fn wait_for_model(
    provider: &FasterWhisperProvider,
    app: &AppHandle,
    rx: &Receiver<DictationCommand>,
) -> bool {
    emit_state(app, "preparing");
    let mut unreachable_polls = 0u32;
    loop {
        // Hotkey edges pressed before the model is ready: a press earns a
        // "hold on" nudge in the tutorial; the matching release is dropped.
        // A closed channel means the app is shutting down.
        match rx.try_recv() {
            Ok(DictationCommand::HotkeyPressed(_)) => {
                if let Err(err) = app.emit("dictation:not-ready", ()) {
                    tracing::warn!("dictation: failed to emit not-ready: {err}");
                }
                continue;
            }
            Ok(DictationCommand::HotkeyReleased) => continue,
            Err(mpsc::TryRecvError::Disconnected) => return false,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let report = tauri::async_runtime::block_on(provider.health());
        if report.model_ready {
            break;
        }
        if report.detail.contains("failed") {
            tracing::error!(detail = %report.detail, "STT model preparation failed");
            break;
        }
        // `ok` is false when the sidecar cannot be reached at all — a failed
        // spawn, not a slow model download. Each such poll re-spawns it
        // (spawn-then-kill since ADR-0010); retrying forever only churns and
        // strands the tongue on "preparing", so give up after a few.
        if report.ok {
            unreachable_polls = 0;
        } else {
            unreachable_polls += 1;
            if unreachable_polls >= MAX_SIDECAR_FAILURES {
                tracing::error!(detail = %report.detail, "STT sidecar did not start");
                break;
            }
        }
        tracing::debug!(detail = %report.detail, "STT model not ready yet");
        // Surface the warm-up status so the tutorial can show live progress
        // instead of a frozen-looking screen. `detail` is a status line
        // (e.g. "downloading … 45%"), never transcript content
        // (.claude/rules/security.md).
        if let Err(err) = app.emit("dictation:preparing", &report.detail) {
            tracing::warn!("dictation: failed to emit preparing status: {err}");
        }
        thread::sleep(MODEL_POLL_INTERVAL);
    }
    emit_state(app, "idle");
    true
}

/// What `listen` learned about a take — enough for `finish_take` to trim it.
///
/// `Some` once Silero VAD ran start to end: the endpoint detector knows where
/// speech ended. `None` in hold mode, or when VAD was unavailable — then the
/// whole take is transcribed.
struct TakeOutcome {
    endpointer: Option<Endpointer>,
}

/// The result of feeding the buffered audio frames to Silero VAD.
enum FrameVerdict {
    /// The utterance is still going.
    Continue,
    /// The endpoint detector reported the utterance has ended.
    Ended,
    /// VAD inference errored — stop feeding it for the rest of the take.
    VadFailed,
}

/// Pull every complete 512-sample frame out of `frame_buf`, score it with
/// Silero VAD, and feed the probability to the endpoint detector.
fn drain_frames(
    silero: &mut SileroVad,
    endpointer: &mut Endpointer,
    frame_buf: &mut Vec<f32>,
) -> FrameVerdict {
    while frame_buf.len() >= FRAME_SAMPLES {
        let frame: [f32; FRAME_SAMPLES] = frame_buf[..FRAME_SAMPLES]
            .try_into()
            .expect("a FRAME_SAMPLES-long slice");
        frame_buf.drain(..FRAME_SAMPLES);
        match silero.observe(&frame) {
            Ok(prob) => {
                if endpointer.observe(prob) == EndpointSignal::Ended {
                    return FrameVerdict::Ended;
                }
            }
            Err(err) => {
                tracing::error!("dictation: VAD inference failed: {err:#}");
                return FrameVerdict::VadFailed;
            }
        }
    }
    FrameVerdict::Continue
}

/// Capture until the active mode says to stop.
fn listen(
    rx: &Receiver<DictationCommand>,
    capture: &AudioCapture,
    vad: &mut Option<SileroVad>,
    app: &AppHandle,
) -> TakeOutcome {
    match DICTATION_MODE {
        // Hold — capture until the hotkey is released.
        DictationMode::Hold => {
            loop {
                match rx.recv() {
                    Ok(DictationCommand::HotkeyReleased) | Err(_) => break,
                    Ok(DictationCommand::HotkeyPressed(_)) => {}
                }
            }
            TakeOutcome { endpointer: None }
        }
        // Hands-free — feed Silero VAD 32 ms frames and stop when its endpoint
        // detector reports the utterance ended. A second press or the MAX_TAKE
        // cap also stops the take. The loop wakes every `LEVEL_INTERVAL` to
        // feed the tongue's waveform; VAD runs on whole 512-sample frames as
        // they accumulate, independent of that cadence.
        DictationMode::HandsFree => {
            let mut endpointer = Endpointer::default();
            if let Some(silero) = vad.as_mut() {
                silero.reset();
            }
            let started = Instant::now();
            let mut cursor = 0usize;
            let mut frame_buf: Vec<f32> = Vec::new();
            let mut vad_failed = false;

            loop {
                match rx.recv_timeout(LEVEL_INTERVAL) {
                    Ok(DictationCommand::HotkeyPressed(_)) => break, // press again
                    Ok(DictationCommand::HotkeyReleased) => {}       // the tap's key-up
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        let chunk = capture.samples_since(cursor);
                        cursor += chunk.len();
                        // Feed the tongue's live waveform every wake.
                        emit_level(app, vad::rms(&chunk));

                        if let Some(silero) = vad.as_mut().filter(|_| !vad_failed) {
                            frame_buf.extend_from_slice(&chunk);
                            match drain_frames(silero, &mut endpointer, &mut frame_buf) {
                                FrameVerdict::Continue => {}
                                FrameVerdict::Ended => break,
                                FrameVerdict::VadFailed => vad_failed = true,
                            }
                        }

                        if started.elapsed() >= MAX_TAKE {
                            break;
                        }
                    }
                }
            }

            // The endpoint verdict is usable only if Silero ran start to end.
            let vad_ran = vad.is_some() && !vad_failed;
            TakeOutcome {
                endpointer: vad_ran.then_some(endpointer),
            }
        }
    }
}

/// Stop the capture and return the PCM to transcribe. `None` when hands-free
/// VAD heard no speech at all; otherwise trailing silence is trimmed off.
fn finish_take(capture: &mut AudioCapture, outcome: &TakeOutcome) -> Option<Vec<f32>> {
    let mut pcm = capture.stop();

    match &outcome.endpointer {
        // Hold mode, or VAD unavailable — transcribe the whole take.
        None => Some(pcm),
        Some(endpointer) => {
            if !endpointer.heard_speech() {
                return None; // never really spoke — nothing to transcribe
            }
            // Keep audio up to the last speech plus a margin, dropping the
            // trailing silence so it never reaches the transcriber.
            let speech_end = endpointer.speech_end().unwrap_or_default();
            let keep_secs = (speech_end + TAIL_MARGIN).as_secs_f64();
            let keep = (keep_secs * f64::from(TARGET_RATE)) as usize;
            pcm.truncate(keep.min(pcm.len()));
            Some(pcm)
        }
    }
}

/// Dictation hotkey pressed — forwarded to the worker as an Inject take.
#[tauri::command]
pub fn dictation_hotkey_pressed(channel: tauri::State<'_, DictationChannel>) {
    channel.send(DictationCommand::HotkeyPressed(TakeMode::Inject));
}

/// Dictation hotkey released — forwarded to the worker.
#[tauri::command]
pub fn dictation_hotkey_released(channel: tauri::State<'_, DictationChannel>) {
    channel.send(DictationCommand::HotkeyReleased);
}

/// Command-mode hotkey pressed — forwarded to the worker as a Command take.
/// On the next transcript, the worker hands the text to
/// `command_mode::dispatch_transcript` instead of injecting it (M8).
#[cfg(feature = "command-mode")]
#[tauri::command]
pub fn command_hotkey_pressed(channel: tauri::State<'_, DictationChannel>) {
    channel.send(DictationCommand::HotkeyPressed(TakeMode::Command));
}

/// Command-mode hotkey released — forwarded to the worker.
#[cfg(feature = "command-mode")]
#[tauri::command]
pub fn command_hotkey_released(channel: tauri::State<'_, DictationChannel>) {
    channel.send(DictationCommand::HotkeyReleased);
}

/// Notify the tongue of a dictation lifecycle change (docs/soul.md).
///
/// `state` is one of `idle`, `preparing`, `capturing`, `transcribing`, or
/// `error` — the states the tongue UI renders (see
/// `apps/desktop/src/lib/dictation.ts`).
fn emit_state(app: &AppHandle, state: &str) {
    if let Err(err) = app.emit("dictation:state", state) {
        tracing::warn!("dictation: failed to emit {state}: {err}");
    }
}

/// Stream the current capture loudness so the tongue's waveform can lean into
/// the voice. The payload is a single RMS scalar — a loudness meter, never
/// audio content (see .claude/rules/security.md).
fn emit_level(app: &AppHandle, level: f32) {
    // A cosmetic ~20 Hz stream: a dropped frame is invisible, so — unlike the
    // lifecycle events — a failed emit is swallowed rather than logged.
    let _ = app.emit("dictation:level", level);
}

/// Flash the tongue's Error state, hold it briefly so the failure registers,
/// then return to Idle. Runs on the worker thread between takes; a hotkey
/// press during the dwell is queued and serviced as soon as it returns.
fn signal_error(app: &AppHandle) {
    emit_state(app, "error");
    thread::sleep(ERROR_DWELL);
    emit_state(app, "idle");
}
