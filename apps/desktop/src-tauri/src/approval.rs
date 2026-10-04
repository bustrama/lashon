//! The approval broker (docs/adr/0048): every request for the user's yes or
//! no goes through here, and the overlay's approval card answers it.
//!
//! The queue and its rules are `ottid_core::approval`, unit-tested there.
//! This module adds what needs Tauri:
//!
//! - the reply channel back to each asker, async (command mode) or blocking
//!   (the recipe runtime's synchronous gate);
//! - the `approval:changed` and `approval:nudge` events the card listens to,
//!   and the commands it answers with;
//! - the timer that denies a request nobody answers;
//! - two global hotkeys, registered only while a request is pending, since
//!   the overlay never has keyboard focus (ADR-0044);
//! - showing a hidden overlay while a request waits, so the card can be seen.
//!
//! Locks: `queue` is held only for quick queue operations. `side` orders the
//! slow changes that follow the queue (hotkeys, the window), which round-trip
//! to the main thread, so nothing on the main thread may wait on it: the
//! commands here are async, and the hotkey and menu handlers hand their work
//! to the async runtime. `side` is always taken before `queue`, never after.

use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ottid_core::approval::{
    Decision, Id, Queue, Refusal, Request, Shown, ALLOW_ACCELERATOR, ALLOW_KEYS, DENY_ACCELERATOR,
    DENY_KEYS,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::oneshot;

use crate::overlay;

const LOG: &str = "ottid::approval";

/// The card to show, or `null` when nothing is pending.
const EVENT_CHANGED: &str = "approval:changed";
/// Allow was pressed before the card was armed.
const EVENT_NUDGE: &str = "approval:nudge";

/// How long a blocking asker waits before giving up on its own. The broker
/// answers every request within `approval::TIMEOUT` of showing it, but one
/// may first wait behind others; this only frees the thread if that
/// promise is ever broken.
const BLOCKING_BACKSTOP: Duration = Duration::from_secs(600);

/// Where an answer goes.
enum Reply {
    /// An async asker awaits it.
    Async(oneshot::Sender<Decision>),
    /// A blocking asker parks its thread on it.
    Blocking(mpsc::SyncSender<Decision>),
}

impl Reply {
    fn send(self, decision: Decision) {
        match self {
            Reply::Async(tx) => {
                let _ = tx.send(decision);
            }
            Reply::Blocking(tx) => {
                let _ = tx.try_send(decision);
            }
        }
    }
}

/// The keycaps of the hotkeys that are registered; `None` for one that
/// couldn't be (another app holds it), so the card doesn't offer it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Keys {
    allow: Option<[&'static str; 3]>,
    deny: Option<[&'static str; 3]>,
}

/// What the card gets: the request on screen and how to answer it.
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    #[serde(flatten)]
    shown: Shown,
    keys: Keys,
}

#[derive(Debug, Clone, Serialize)]
struct Nudge {
    id: Id,
}

#[derive(Default)]
struct Side {
    /// Registered while anything is pending.
    keys: Option<Keys>,
    /// We showed the overlay for the card; hide it again when done.
    revealed: bool,
    /// The request a timeout is running for.
    timer_for: Option<Id>,
}

/// Tauri-managed state: the pending requests.
#[derive(Default)]
pub struct Approvals {
    queue: Mutex<Queue<Reply>>,
    side: Mutex<Side>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not leave every later request unanswerable.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Ask the user and wait for the answer. If the caller is dropped first (a
/// new take aborted the command-mode task), the card is taken down.
pub async fn ask(app: &AppHandle, request: Request) -> Decision {
    let (tx, rx) = oneshot::channel();
    let id = submit(app, request, Reply::Async(tx));
    let mut pending = Pending {
        app: app.clone(),
        id: Some(id),
    };
    let decision = rx.await.unwrap_or(Decision::Deny);
    pending.id = None;
    decision
}

/// Ask the user and block this thread until the answer, for the recipe
/// runtime's synchronous confirmation gate.
pub fn ask_blocking(app: &AppHandle, request: Request) -> Decision {
    let (tx, rx) = mpsc::sync_channel(1);
    let id = submit(app, request, Reply::Blocking(tx));
    match rx.recv_timeout(BLOCKING_BACKSTOP) {
        Ok(decision) => decision,
        Err(_) => {
            withdraw(app, id);
            Decision::Deny
        }
    }
}

/// Withdraws a request whose asker went away before it was answered.
struct Pending {
    app: AppHandle,
    id: Option<Id>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let app = self.app.clone();
            // Drop can run on any thread, and `changed` round-trips to the
            // main thread: do it on the async runtime.
            tauri::async_runtime::spawn(async move { withdraw(&app, id) });
        }
    }
}

fn submit(app: &AppHandle, request: Request, reply: Reply) -> Id {
    let source = request.source;
    let tool = request.tool.clone();
    let id = lock(&app.state::<Approvals>().queue).push(request, reply, Instant::now());
    // The tool name comes from a fixed set. What it would run is never
    // logged (.claude/rules/security.md).
    tracing::info!(target: LOG, id, source = source.code(), tool = %tool, "approval requested");
    changed(app);
    id
}

fn answer(app: &AppHandle, id: Id, decision: Decision, by: &'static str) -> Result<(), Refusal> {
    let answered = lock(&app.state::<Approvals>().queue).answer(id, decision, Instant::now())?;
    tracing::info!(target: LOG, id, decision = decision.code(), by, "approval answered");
    answered.reply.send(answered.decision);
    changed(app);
    Ok(())
}

fn withdraw(app: &AppHandle, id: Id) {
    let gone = lock(&app.state::<Approvals>().queue).withdraw(id, Instant::now());
    if gone.is_some() {
        tracing::info!(target: LOG, id, "approval withdrawn");
        changed(app);
    }
}

/// Bring the hotkeys, the window, the timer and the card in line with the
/// queue. Call after every change to it, never on the main thread.
fn changed(app: &AppHandle) {
    let state = app.state::<Approvals>();
    let mut side = lock(&state.side);
    let pending = !lock(&state.queue).is_empty();
    if pending && side.keys.is_none() {
        side.keys = Some(register_keys(app));
        side.revealed = reveal(app);
    } else if !pending {
        if let Some(keys) = side.keys.take() {
            unregister_keys(app, &keys);
        }
        if std::mem::take(&mut side.revealed) {
            conceal(app);
        }
        side.timer_for = None;
    }

    let shown = lock(&state.queue).shown(Instant::now());
    if let Some(shown) = &shown {
        if side.timer_for != Some(shown.id) {
            side.timer_for = Some(shown.id);
            spawn_timeout(app.clone(), shown.id);
        }
    }
    let card = shown.map(|shown| Card {
        shown,
        keys: side.keys.clone().unwrap_or_default(),
    });
    let _ = app.emit_to(overlay::WINDOW, EVENT_CHANGED, card);
}

/// Deny `id` when its time on screen is up, unless it is answered first.
fn spawn_timeout(app: AppHandle, id: Id) {
    tauri::async_runtime::spawn(async move {
        loop {
            let deadline = match lock(&app.state::<Approvals>().queue).deadline() {
                Some((shown, at)) if shown == id => at,
                _ => return,
            };
            tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
            let expired = {
                let state = app.state::<Approvals>();
                let mut queue = lock(&state.queue);
                match queue.deadline() {
                    Some((shown, _)) if shown == id => queue.expire(Instant::now()),
                    _ => return,
                }
            };
            if let Some(answered) = expired {
                tracing::warn!(target: LOG, id, "approval timed out; denied");
                answered.reply.send(answered.decision);
                changed(&app);
                return;
            }
            // Woke a hair early: wait out the rest.
        }
    });
}

// ---- Hotkeys ----

fn register_keys(app: &AppHandle) -> Keys {
    Keys {
        allow: register_key(app, ALLOW_ACCELERATOR, Decision::Allow).then_some(ALLOW_KEYS),
        deny: register_key(app, DENY_ACCELERATOR, Decision::Deny).then_some(DENY_KEYS),
    }
}

fn register_key(app: &AppHandle, accelerator: &'static str, decision: Decision) -> bool {
    let shortcut = match accelerator.parse::<Shortcut>() {
        Ok(shortcut) => shortcut,
        Err(err) => {
            tracing::warn!(target: LOG, chord = accelerator, "bad approval chord: {err}");
            return false;
        }
    };
    let registered = app
        .global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                // The plugin calls this on the main thread with its own
                // shortcut map locked, and answering unregisters hotkeys:
                // do it on the async runtime.
                let app = app.clone();
                tauri::async_runtime::spawn(async move { on_key(&app, decision) });
            }
        });
    match registered {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(target: LOG, chord = accelerator, "could not register an approval hotkey: {err}");
            false
        }
    }
}

fn unregister_keys(app: &AppHandle, keys: &Keys) {
    let chords = [
        (keys.allow.is_some(), ALLOW_ACCELERATOR),
        (keys.deny.is_some(), DENY_ACCELERATOR),
    ];
    for (registered, accelerator) in chords {
        if !registered {
            continue;
        }
        let Ok(shortcut) = accelerator.parse::<Shortcut>() else {
            continue;
        };
        if let Err(err) = app.global_shortcut().unregister(shortcut) {
            tracing::warn!(target: LOG, chord = accelerator, "could not unregister an approval hotkey: {err}");
        }
    }
}

/// A hotkey answers the request on screen. Allow before the card is armed
/// is refused, and the card is told, so it can scroll on through the text
/// and say why.
fn on_key(app: &AppHandle, decision: Decision) {
    let Some(id) = lock(&app.state::<Approvals>().queue).shown_id() else {
        return;
    };
    if let Err(Refusal::NotArmed) = answer(app, id, decision, "hotkey") {
        let _ = app.emit_to(overlay::WINDOW, EVENT_NUDGE, Nudge { id });
    }
}

// ---- The window ----

/// Show the overlay if the user hid it, so the card can be seen. Returns
/// whether it did.
fn reveal(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(overlay::WINDOW) else {
        return false;
    };
    if window.is_visible().unwrap_or(true) {
        return false;
    }
    tracing::info!(target: LOG, "showing the hidden overlay for an approval");
    window.show().is_ok()
}

fn conceal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(overlay::WINDOW) {
        let _ = window.hide();
    }
}

/// The user showed or hid Ottid from the menu: that choice stands after the
/// card closes. Safe to call on the main thread.
pub fn forget_reveal(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        lock(&app.state::<Approvals>().side).revealed = false;
    });
}

// ---- Commands ----

/// The card on screen, for an overlay that (re)loaded after it was sent.
#[tauri::command]
pub async fn approval_current(app: AppHandle) -> Option<Card> {
    let state = app.state::<Approvals>();
    let side = lock(&state.side);
    let shown = lock(&state.queue).shown(Instant::now())?;
    Some(Card {
        shown,
        keys: side.keys.clone().unwrap_or_default(),
    })
}

/// The card has had the request's whole text in view for the arm delay.
#[tauri::command]
pub async fn approval_armed(app: AppHandle, id: Id) -> Result<(), String> {
    lock(&app.state::<Approvals>().queue)
        .arm(id)
        .map_err(|refusal| refusal.code().to_string())
}

/// A click on the card's Allow or Deny.
#[tauri::command]
pub async fn approval_answer(app: AppHandle, id: Id, decision: Decision) -> Result<(), String> {
    answer(&app, id, decision, "click").map_err(|refusal| refusal.code().to_string())
}

/// Development only: show a sample request on the card, to try the card,
/// its hotkeys and its focus behaviour without a model or a recipe.
/// Answering it runs nothing; the answer is returned.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn approval_preview(app: AppHandle, sample: String) -> String {
    let command = match sample.as_str() {
        "long" => (1..=40)
            .map(|n| format!("Write-Output \"שורה {n} של {}\"", 40))
            .collect::<Vec<_>>()
            .join("\n"),
        // A right-to-left override and a no-break space, which the card
        // must show rather than obey.
        "hidden" => "Rename-Item report\u{202E}fdp.exe -NewName\u{00A0}x.txt".to_string(),
        _ => "Get-ChildItem -Path \"$HOME\\מסמכים\" -Filter *.txt | Select-Object -First 5"
            .to_string(),
    };
    let request = Request::for_tool(
        "run_command",
        &serde_json::json!({ "command": command, "cwd": "~\\מסמכים" }),
    );
    ask(&app, request).await.code().to_string()
}
