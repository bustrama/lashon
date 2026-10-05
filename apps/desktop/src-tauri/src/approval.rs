//! The approval broker (docs/adr/0048): every request for the user's yes or
//! no goes through here, and the overlay's approval card answers it.
//!
//! The queue and its rules are `ottid_core::approval`, unit-tested there.
//! This module adds what needs Tauri:
//!
//! - the reply channel back to each asker, async (command mode, and a coding
//!   agent through the hooks bridge of ADR-0050) or blocking (the recipe
//!   runtime's synchronous gate);
//! - the `approval:changed` and `approval:nudge` events the card listens to,
//!   and the commands it answers with;
//! - the timer that denies a request nobody answers, or hands an agent's
//!   back to the agent's own prompt;
//! - two global hotkeys, registered only while a request is pending, since
//!   the overlay never has keyboard focus (ADR-0044);
//! - showing a hidden overlay while a request waits, so the card can be seen;
//! - calling off a recipe's blocking requests when its take is cancelled.
//!
//! Locks: `queue` is held only for quick queue operations. `side` orders the
//! slow changes that follow the queue (hotkeys, the window), which round-trip
//! to the main thread, so nothing on the main thread may wait on it: the
//! commands here are async, and the hotkey and menu handlers hand their
//! answers to the async runtime. `side` is always taken before `queue`,
//! never after. `hold` is taken alone, after `queue` is let go.

use std::sync::{mpsc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ottid_core::approval::{
    Cancel, Decision, Hold, HoldCheck, HoldRelease, Id, Queue, Refusal, Request, Shown,
    ALLOW_ACCELERATOR, ALLOW_KEYS, DENY_ACCELERATOR, DENY_KEYS,
};
use ottid_core::overlay::{keyboard, Foreground};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::oneshot;

use crate::overlay;

const LOG: &str = "ottid::approval";

/// The card to show, or `null` when nothing is pending.
const EVENT_CHANGED: &str = "approval:changed";
/// The Allow hotkey did something the card shows.
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
    /// A coding agent awaits it through the hooks bridge (ADR-0050). `None`
    /// means nobody answered: the agent asks in its own prompt instead.
    #[cfg(feature = "agent-hooks")]
    Agent(oneshot::Sender<Option<Decision>>),
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
            #[cfg(feature = "agent-hooks")]
            Reply::Agent(tx) => {
                let _ = tx.send(Some(decision));
            }
        }
    }

    /// Nobody answered in time. Ottid's own askers take that as Deny. An
    /// agent has a prompt of its own to fall back on, so its request goes
    /// back there undecided rather than denied.
    fn lapse(self) -> &'static str {
        match self {
            #[cfg(feature = "agent-hooks")]
            Reply::Agent(tx) => {
                let _ = tx.send(None);
                "left to the agent's own prompt"
            }
            other => {
                other.send(Decision::Deny);
                "denied"
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
    /// `early`: pressed before the card armed. `hold`: held down, counting
    /// to `approval::HOLD`. `short`: let go too soon.
    kind: &'static str,
}

#[derive(Default)]
struct Side {
    /// Registered while anything is pending.
    keys: Option<Keys>,
    /// We showed the overlay for the card; hide it again when done.
    revealed: bool,
    /// The request a timeout is running for.
    timer_for: Option<Id>,
    /// The window in front when that request came on screen.
    before: Option<Foreground>,
}

/// Tauri-managed state: the pending requests.
#[derive(Default)]
pub struct Approvals {
    queue: Mutex<Queue<Reply>>,
    side: Mutex<Side>,
    /// The Allow hotkey held down.
    hold: Mutex<Hold>,
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

/// Ask the user for a coding agent (ADR-0050) and wait for the answer:
/// `None` if nobody answered in time. If the caller is dropped first (the
/// agent's hook went away), the card is taken down.
#[cfg(feature = "agent-hooks")]
pub async fn ask_agent(app: &AppHandle, request: Request) -> Option<Decision> {
    let (tx, rx) = oneshot::channel();
    let id = submit(app, request, Reply::Agent(tx));
    let mut pending = Pending {
        app: app.clone(),
        id: Some(id),
    };
    let decision = rx.await.unwrap_or(None);
    pending.id = None;
    decision
}

/// Ask the user and block this thread until the answer, for the recipe
/// runtime's synchronous confirmation gate. [`cancel`] with the same
/// `Cancel` calls it off: the card is taken down and the answer is Deny.
pub fn ask_blocking(app: &AppHandle, request: Request, cancelled: &Cancel) -> Decision {
    if cancelled.is_cancelled() {
        return Decision::Deny;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let id = submit(app, request, Reply::Blocking(tx));
    if !cancelled.wait_on(id) {
        withdraw(app, id);
        return Decision::Deny;
    }
    let decision = match rx.recv_timeout(BLOCKING_BACKSTOP) {
        Ok(decision) => decision,
        // Withdrawn, which drops the sender, or the backstop ran out.
        Err(_) => {
            withdraw(app, id);
            Decision::Deny
        }
    };
    if cancelled.done(id) {
        Decision::Deny
    } else {
        decision
    }
}

/// Call off the blocking requests asked with `cancelled`: withdraw the one
/// being waited on, which answers it Deny, and deny any asked after. Safe
/// to call on any thread.
pub fn cancel(app: &AppHandle, cancelled: &Cancel) {
    if let Some(id) = cancelled.cancel() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { withdraw(&app, id) });
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
        side.before = None;
    }

    let shown = lock(&state.queue).shown(Instant::now());
    if let Some(shown) = &shown {
        if side.timer_for != Some(shown.id) {
            side.timer_for = Some(shown.id);
            side.before = Foreground::remember();
            spawn_timeout(app.clone(), shown.id);
        }
    }
    let card = shown.map(|shown| Card {
        shown,
        keys: side.keys.clone().unwrap_or_default(),
    });
    let _ = app.emit_to(overlay::WINDOW, EVENT_CHANGED, card);
}

/// Lapse `id` when its time on screen is up, unless it is answered first:
/// Ottid's own request is denied, an agent's goes back to the agent.
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
                let outcome = answered.reply.lapse();
                tracing::warn!(target: LOG, id, outcome, "approval timed out");
                changed(&app);
                return;
            }
            // Woke a hair early: wait out the rest.
        }
    });
}

// ---- Hotkeys ----

fn register_keys(app: &AppHandle) -> Keys {
    // A hold allows only when the OS says the chord is down. Where it can't
    // say, the hotkey could never allow, so it isn't offered.
    let can_hold = keyboard::allow_chord_down().is_some();
    Keys {
        allow: (can_hold && register_key(app, ALLOW_ACCELERATOR, Decision::Allow))
            .then_some(ALLOW_KEYS),
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
            on_key(app, decision, event.state() == ShortcutState::Pressed);
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

/// A hotkey went down or up, for the request on screen.
///
/// - Deny answers when pressed.
/// - Allow answers once held for `approval::HOLD`; a tap never allows.
///   While it is held the card fills its Allow button, and if it is let go
///   too soon the card says to hold it.
/// - Allow pressed before the card armed is refused, and the card is told,
///   so it can scroll on through the text and say why.
///
/// The plugin calls this synchronously on whichever thread global-hotkey
/// sends the event from, with its own shortcut map locked. On Windows a
/// press comes from the main thread's window procedure, and only after it
/// is handled does global-hotkey start a thread that polls the main key
/// (Y or N alone) and sends the release when it comes up. So a press is
/// always noted before its release, but the release says nothing about
/// Ctrl or Shift, and may never come. That's why only the hold's timer
/// allows, after asking the OS whether the whole chord is still down.
/// Answering unregisters hotkeys, so the answers go to the async runtime.
fn on_key(app: &AppHandle, decision: Decision, pressed: bool) {
    let state = app.state::<Approvals>();
    let now = Instant::now();
    match (decision, pressed) {
        (Decision::Deny, true) => {
            if let Some(id) = lock(&state.queue).shown_id() {
                spawn_answer(app, id, Decision::Deny);
            }
        }
        (Decision::Deny, false) => {}
        (Decision::Allow, true) => {
            let ready = {
                let queue = lock(&state.queue);
                queue.shown_id().map(|id| (id, queue.can_allow(id, now)))
            };
            match ready {
                Some((id, Ok(()))) => {
                    let press = lock(&state.hold).press(id, now);
                    nudge(app, id, "hold");
                    spawn_hold(app.clone(), press);
                }
                Some((id, Err(Refusal::NotArmed))) => nudge(app, id, "early"),
                _ => {}
            }
        }
        (Decision::Allow, false) => match lock(&state.hold).release() {
            HoldRelease::Short(id) => nudge(app, id, "short"),
            HoldRelease::Nothing => {}
        },
    }
}

fn nudge(app: &AppHandle, id: Id, kind: &'static str) {
    let _ = app.emit_to(overlay::WINDOW, EVENT_NUDGE, Nudge { id, kind });
}

/// Answer from a hotkey, off the main thread. The broker checks the rules
/// again: an Allow it refuses as not armed is shown as an early press.
fn spawn_answer(app: &AppHandle, id: Id, decision: Decision) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(Refusal::NotArmed) = answer(&app, id, decision, "hotkey") {
            nudge(&app, id, "early");
        }
    });
}

/// Allow once `press` has been held for `approval::HOLD` with the whole
/// chord down, unless any of its keys is let go or it is pressed again
/// first. The keys are read from the OS at every look, and anything but
/// "all down" ends the hold without allowing.
fn spawn_hold(app: AppHandle, press: u64) {
    tauri::async_runtime::spawn(async move {
        loop {
            let chord_down = keyboard::allow_chord_down();
            let check =
                lock(&app.state::<Approvals>().hold).check(press, Instant::now(), chord_down);
            match check {
                HoldCheck::Wait(left) => tokio::time::sleep(left).await,
                HoldCheck::Allow(id) => {
                    spawn_answer(&app, id, Decision::Allow);
                    return;
                }
                HoldCheck::Short(id) => {
                    nudge(&app, id, "short");
                    return;
                }
                HoldCheck::Gone => return,
            }
        }
    });
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

/// The overlay never takes focus (ADR-0044), so a click on the card leaves
/// the user's app in front. Should a click ever bring the overlay forward
/// anyway, hand the front back to the window that had it when the card
/// came on screen. Nothing happens while anything else is in front.
fn give_back(webview: &tauri::Webview, before: Option<Foreground>) {
    #[cfg(windows)]
    if let (Some(before), Ok(hwnd)) = (before, webview.window().hwnd()) {
        if before.give_back(hwnd.0 as isize) {
            tracing::info!(target: LOG, "gave the foreground back after a click on the card");
        }
    }
    #[cfg(not(windows))]
    let _ = (webview, before);
}

// ---- Commands ----

/// Only the overlay's card arms and answers requests. The Hub and the
/// tutorial load the same frontend, and must not.
fn from_card(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == overlay::WINDOW {
        Ok(())
    } else {
        Err("not-the-card".to_string())
    }
}

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
pub async fn approval_armed(app: AppHandle, webview: tauri::Webview, id: Id) -> Result<(), String> {
    from_card(&webview)?;
    lock(&app.state::<Approvals>().queue)
        .arm(id)
        .map_err(|refusal| refusal.code().to_string())
}

/// A click on the card's Allow or Deny.
#[tauri::command]
pub async fn approval_answer(
    app: AppHandle,
    webview: tauri::Webview,
    id: Id,
    decision: Decision,
) -> Result<(), String> {
    from_card(&webview)?;
    let before = lock(&app.state::<Approvals>().side).before;
    answer(&app, id, decision, "click").map_err(|refusal| refusal.code().to_string())?;
    give_back(&webview, before);
    Ok(())
}

/// Development only: show a sample request on the card, to try the card,
/// its hotkeys and its focus behaviour without a model or a recipe.
/// Answering it runs nothing; the answer is returned.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn approval_preview(app: AppHandle, sample: String) -> String {
    // A Claude Code request, as the hooks bridge shows one. The answer is
    // `ask` when the card's time runs out.
    #[cfg(feature = "agent-hooks")]
    if sample == "agent" {
        let request = Request::for_agent(
            ottid_core::agent_bridge::AGENT,
            "Bash",
            &serde_json::json!({
                "command": "git push --force-with-lease origin feat/מסלול-חדש",
                "description": "Push the rebased branch"
            }),
            Some("C:\\Users\\דנה\\ottid"),
        );
        return match ask_agent(&app, request).await {
            Some(decision) => decision.code().to_string(),
            None => "ask".to_string(),
        };
    }
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
