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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use lashon_core::audio::{AudioCapture, TARGET_RATE};
use lashon_core::inject::inject_text;
use lashon_core::local_agreement::{LocalAgreement, Preview};
use lashon_core::streaming::{DecodeScheduler, LanguageLatch, WindowAnchor};
use lashon_core::stt::{FasterWhisperProvider, Segment, SttProvider, TranscribeOptions};
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

/// Hands-free: a safety backstop on a take's length, **not** a normal cap. The
/// utterance is meant to end on the VAD endpoint or a second press; this only
/// catches a capture that would otherwise never end — a wedged detector, or VAD
/// unavailable so nothing but a second press stops it. It must sit far above any
/// real continuous utterance (the old 30 s fired mid-sentence during ordinary
/// long-form dictation), while still bounding a forgotten session — at 16 kHz
/// mono f32 the buffer grows ~62 KB/s, so five minutes is ~19 MB.
const MAX_TAKE: Duration = Duration::from_secs(5 * 60);

/// How often to poll the STT sidecar while it prepares the model on first run.
const MODEL_POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// End the model wait after this many consecutive polls with the sidecar
/// unreachable. `spawn()` carries an 8 s timeout, so this is ~30 s of a sidecar
/// that will not start — past which retrying only churns spawn-then-kill
/// (ADR-0010) and the tongue would sit on "preparing" forever.
const MAX_SIDECAR_FAILURES: u32 = 3;

/// How long the tongue holds the Error state before falling back to Idle.
const ERROR_DWELL: Duration = Duration::from_millis(1600);

/// Live streaming: minimum buffered audio before the first re-decode (~1 s).
/// Sub-second decodes are mostly noise — the benchmark's min-decode gate.
const MIN_DECODE_SAMPLES: usize = TARGET_RATE as usize; // 16_000 = 1.0 s

/// Live streaming: re-decode once this much new audio has arrived (~500 ms).
/// On a Tier-A GPU a re-decode is ~130 ms, comfortably inside this hop; the
/// number is the cadence `scripts/stream-test.py` validated (docs/adr/0035).
const DECODE_HOP_SAMPLES: usize = TARGET_RATE as usize / 2; // 8_000 = 0.5 s

/// Live streaming: per-decode budget. A machine whose re-decode overruns this
/// cannot sustain live partials (a CPU decode of the turbo model is ~12 s, far
/// past it), so streaming self-disables and the take keeps only its final
/// decode — today's one-shot behaviour. The threshold sits well above a slow
/// GPU and the one-time ~1 s CUDA warm-up, so only a genuinely non-viable
/// machine trips it (docs/adr/0035).
const MAX_DECODE_LATENCY: Duration = Duration::from_millis(2500);

/// Live streaming: how many recent committed words to feed a windowed re-decode
/// as Whisper decoding context (`initial_prompt`). Enough to restore sentence
/// context at the window's start, while staying well inside Whisper's prompt
/// budget (~half of the 448-token context); Hebrew words can be several tokens
/// each, so this stays modest. See [`WindowAnchor`] and docs/adr/0037.
const STREAM_PROMPT_WORDS: usize = 40;

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

/// The result of one off-thread streaming re-decode, sent back to the worker.
struct DecodeOutcome {
    /// The window's hypothesis text, the language the decode reported, and its
    /// segments (for advancing the window anchor) — or `None` when the decode
    /// errored. Latency still comes back either way so a slow, erroring machine
    /// can still self-disable.
    hypothesis: Option<(String, String, Vec<Segment>)>,
    /// Wall-clock the decode took — feeds the scheduler's self-disable.
    latency: Duration,
}

/// Drives live partial transcripts for one hands-free take.
///
/// Every capture tick the worker calls [`pump`](Self::pump). When the
/// [`DecodeScheduler`] says so, it snapshots the uncommitted tail of the buffer
/// — the audio from the [`WindowAnchor`] forward, not the whole take — and spawns
/// a single-flight off-thread re-decode (so the capture thread never stalls on
/// inference). Each finished decode comes back through an mpsc channel; its
/// window text is reassembled with the committed prefix, folded through
/// LocalAgreement-2 into a flicker-free `(committed, provisional)` preview,
/// emitted as `dictation:partial`, and its segments advance the anchor past
/// whatever just committed. The architecture is unary windowed re-decode +
/// client-side commit, not bidi streaming — see docs/adr/0035 and docs/adr/0037.
struct Streamer {
    provider: Arc<FasterWhisperProvider>,
    app: AppHandle,
    scheduler: DecodeScheduler,
    latch: LanguageLatch,
    committer: LocalAgreement,
    /// Tracks the re-decode window so each decode covers only the uncommitted
    /// tail, keeping cost bounded however long the take runs (docs/adr/0037).
    anchor: WindowAnchor,
    /// Set while an off-thread decode is running — the single-flight guard.
    in_flight: Arc<AtomicBool>,
    tx: Sender<DecodeOutcome>,
    rx: Receiver<DecodeOutcome>,
}

impl Streamer {
    fn new(provider: Arc<FasterWhisperProvider>, app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            provider,
            app,
            scheduler: DecodeScheduler::new(
                MIN_DECODE_SAMPLES,
                DECODE_HOP_SAMPLES,
                MAX_DECODE_LATENCY,
            ),
            latch: LanguageLatch::new(),
            committer: LocalAgreement::new(),
            anchor: WindowAnchor::new(TARGET_RATE),
            in_flight: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
        }
    }

    /// Ready the streamer for a new take: fresh committer and language latch, a
    /// reset cadence, and any stale decode from a prior take drained. The
    /// scheduler's session-wide self-disable is preserved across takes.
    fn begin(&mut self) {
        self.committer = LocalAgreement::new();
        self.latch.reset();
        self.scheduler.reset();
        self.anchor.reset();
        // A decode from the previous take may still be in flight; abandon its
        // result rather than letting it bleed into this take's preview.
        for _ in self.rx.try_iter() {}
    }

    /// One capture tick: ingest any finished decode, then maybe fire the next.
    /// `buffered` is the current capture buffer length in samples.
    fn pump(&mut self, capture: &AudioCapture, buffered: usize) {
        self.drain();
        if self
            .scheduler
            .should_decode(buffered, self.in_flight.load(Ordering::Acquire))
        {
            // Re-decode only the uncommitted tail — the audio from the window
            // anchor forward, not the whole growing buffer — so cost stays
            // bounded however long the take runs (docs/adr/0037). samples_since
            // copies without consuming.
            let snapshot = capture.samples_since(self.anchor.offset());
            // Pace by total new audio (`buffered`), not the window length, so the
            // cadence is unchanged as the anchor advances and the window shrinks.
            self.scheduler.mark_decoded(buffered);
            if snapshot.is_empty() {
                return; // window caught up to the buffer end — wait for more audio
            }
            self.in_flight.store(true, Ordering::Release);
            let provider = Arc::clone(&self.provider);
            let in_flight = Arc::clone(&self.in_flight);
            let tx = self.tx.clone();
            let language = self.latch.query().to_string();
            // Prime the windowed decode with the committed tail so it keeps the
            // sentence context it would otherwise lose by not starting at audio 0.
            let prompt = self.anchor.prompt(STREAM_PROMPT_WORDS);
            tauri::async_runtime::spawn(async move {
                let started = Instant::now();
                let result = provider
                    .transcribe(
                        &snapshot,
                        TranscribeOptions {
                            language: &language,
                            initial_prompt: &prompt,
                        },
                    )
                    .await;
                let latency = started.elapsed();
                let hypothesis = match result {
                    Ok(transcript) => {
                        Some((transcript.text, transcript.language, transcript.segments))
                    }
                    Err(err) => {
                        tracing::warn!("dictation: streaming re-decode failed: {err:#}");
                        None
                    }
                };
                let _ = tx.send(DecodeOutcome {
                    hypothesis,
                    latency,
                });
                // Clear the single-flight guard last, after the result is queued,
                // so the worker observes the latency before it can fire again.
                in_flight.store(false, Ordering::Release);
            });
        }
    }

    /// Fold every finished decode into the committer and emit its preview.
    fn drain(&mut self) {
        // Collect first so the borrow of `self.rx` ends before we touch the
        // rest of `self`.
        let outcomes: Vec<DecodeOutcome> = self.rx.try_iter().collect();
        for outcome in outcomes {
            self.scheduler.observe_latency(outcome.latency);
            if let Some((text, language, segments)) = outcome.hypothesis {
                self.latch.observe(&language);
                // The decode saw only the window; reassemble the full-utterance
                // hypothesis (committed prefix + window text) before committing,
                // so the committer's view still spans the whole take.
                let global = self.anchor.global(&text);
                let preview = self.committer.observe(&global);
                // Move the window past whatever just committed, mapping committed
                // words to audio time via this decode's segments (docs/adr/0037).
                self.anchor
                    .advance(&self.committer.committed_text(), &segments);
                self.emit(&preview);
            }
        }
    }

    /// Settle the preview on the closing transcript: everything committed,
    /// nothing provisional. The raw transcript is still what gets injected.
    fn finalize(&mut self, final_text: &str) {
        let committed = self.committer.finalize(final_text);
        self.emit(&Preview {
            committed,
            provisional: String::new(),
        });
    }

    /// Emit a preview to the tongue. A dropped partial is cosmetic — the next
    /// decode (or the final) supersedes it — so a failed emit is swallowed.
    fn emit(&self, preview: &Preview) {
        let _ = self.app.emit("dictation:partial", preview);
    }
}

fn run_worker(rx: Receiver<DictationCommand>, app: AppHandle, gates: crate::Gates) {
    let mut capture = AudioCapture::new();
    // Shared so an off-thread streaming re-decode can run concurrently with the
    // capture loop and the final decode. transcribe() takes &self.
    let provider = Arc::new(FasterWhisperProvider::new());
    let mut streamer = Streamer::new(Arc::clone(&provider), app.clone());

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

        streamer.begin();
        let outcome = listen(&rx, &capture, &mut vad, &app, &mut streamer);
        let pcm = finish_take(&mut capture, &outcome);

        // Skip takes under ~0.25 s — a stray tap, not speech.
        let Some(pcm) = pcm.filter(|p| p.len() >= TARGET_RATE as usize / 4) else {
            emit_state(&app, "idle");
            continue;
        };

        emit_state(&app, "transcribing");
        // The final decode is authoritative and must match the one-shot path
        // byte-for-byte: pass "" so the language is re-detected on the *full*
        // take (docs/adr/0009), not forced from the language the streaming
        // committer latched off the first ~1 s — a short, noisier sample the
        // detector can occasionally misread. The latch still spares the
        // per-chunk detector runs during streaming; only this final ignores it.
        // Its raw text is what gets injected.
        match tauri::async_runtime::block_on(
            provider.transcribe(&pcm, TranscribeOptions::default()),
        ) {
            Ok(transcript) => {
                tracing::info!(
                    chars = transcript.text.chars().count(),
                    ms = transcript.inference_ms,
                    mode = ?take_mode,
                    "dictation transcribed"
                );
                // Settle the live preview on the final transcript — committed in
                // full, nothing provisional — so the tongue shows the same text
                // about to be injected.
                streamer.finalize(&transcript.text);
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
    streamer: &mut Streamer,
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
            // Why the take stopped — assigned on every break path, logged below.
            let reason: &str;

            loop {
                match rx.recv_timeout(LEVEL_INTERVAL) {
                    Ok(DictationCommand::HotkeyPressed(_)) => {
                        reason = "second-press";
                        break;
                    }
                    Ok(DictationCommand::HotkeyReleased) => {} // the tap's key-up
                    Err(RecvTimeoutError::Disconnected) => {
                        reason = "shutdown";
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let chunk = capture.samples_since(cursor);
                        cursor += chunk.len();
                        // Feed the tongue's live waveform every wake.
                        emit_level(app, vad::rms(&chunk));

                        if let Some(silero) = vad.as_mut().filter(|_| !vad_failed) {
                            frame_buf.extend_from_slice(&chunk);
                            match drain_frames(silero, &mut endpointer, &mut frame_buf) {
                                FrameVerdict::Continue => {}
                                FrameVerdict::Ended => {
                                    reason = "vad-endpoint";
                                    break;
                                }
                                FrameVerdict::VadFailed => vad_failed = true,
                            }
                        }

                        // Drive live partials: fold any finished decode into the
                        // preview and fire the next when the cadence allows. The
                        // decode runs off-thread, so this never stalls capture.
                        streamer.pump(capture, cursor);

                        if started.elapsed() >= MAX_TAKE {
                            reason = "max-take-cap";
                            break;
                        }
                    }
                }
            }

            // Duration + stop reason only — never transcript or audio content
            // (.claude/rules/security.md). Lets a "it cut off too early" report
            // be diagnosed: VAD endpoint vs the hard cap vs a second press.
            tracing::info!(
                reason,
                secs = started.elapsed().as_secs_f32(),
                "dictation take ended"
            );

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
