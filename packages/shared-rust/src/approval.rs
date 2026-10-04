//! The approval queue behind Ottid's approval card
//! ([ADR-0048](../../../docs/adr/0048-the-approval-card.md)).
//!
//! Anything that needs the user's yes or no before it acts asks through one
//! queue: a command-mode tool, a recipe's shell step, and a coding agent. The
//! overlay shows the oldest request as a card with the full text of what
//! would run. This module is the decision logic, with the clock passed in.
//! The Tauri shell (`apps/desktop/src-tauri/src/approval.rs`) owns the
//! channels back to the askers, the events, the timer and the hotkeys.
//!
//! The rules:
//!
//! - One request is on screen at a time, oldest first. Its clock starts when
//!   it is shown, not when it was asked.
//! - Allow is taken only for the request on screen, once the card has
//!   reported that all of its text has been in view for [`ARM_DELAY`], and
//!   never sooner than [`ARM_DELAY`] after it was shown.
//! - Deny is always taken. A request nobody answers is denied [`TIMEOUT`]
//!   after it was shown.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How long the whole request has to be in view before Allow works, so a
/// stray click or keypress can't approve what the user hasn't seen.
pub const ARM_DELAY: Duration = Duration::from_millis(700);

/// How long a request stays on screen before it is denied unanswered.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// How long the Allow hotkey has to be held down to allow. Other apps bind
/// the same chord (VS Code's debug console, Firefox's downloads), and a
/// habit presses it as a tap: a tap never allows.
pub const HOLD: Duration = Duration::from_secs(1);

/// How often a hold looks at the keys, so letting go of any of them ends it
/// with the fill on the card.
pub const HOLD_LOOK: Duration = Duration::from_millis(50);

/// The global chord that allows the request on screen when held for
/// [`HOLD`], registered only while one is pending. The key is a physical key
/// code (`KeyY`, the virtual key `VK_Y` on Windows), not a character, so the
/// chord works under any keyboard layout, Hebrew included.
pub const ALLOW_ACCELERATOR: &str = "Control+Shift+KeyY";

/// The global chord that denies the request on screen. See
/// [`ALLOW_ACCELERATOR`].
pub const DENY_ACCELERATOR: &str = "Control+Shift+KeyN";

/// The keycaps the card shows for [`ALLOW_ACCELERATOR`].
pub const ALLOW_KEYS: [&str; 3] = ["Ctrl", "Shift", "Y"];

/// The keycaps the card shows for [`DENY_ACCELERATOR`].
pub const DENY_KEYS: [&str; 3] = ["Ctrl", "Shift", "N"];

/// The command-mode tool that runs a shell command line.
const RUN_COMMAND: &str = "run_command";

/// The recipe step that runs a shell command line.
const RUN_SHELL: &str = "run_shell";

/// A coding agent's tools that run a shell command line (Claude Code's).
const AGENT_SHELLS: [&str; 2] = ["Bash", "PowerShell"];

/// A request's id, unique for the life of the queue.
pub type Id = u64;

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

impl Decision {
    pub fn code(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
        }
    }
}

/// Who is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A command-mode tool call the planner chose.
    Command,
    /// A step of a recipe.
    Recipe,
    /// A coding agent's tool call, such as Claude Code's through the hooks
    /// bridge (ADR-0049).
    Agent,
}

impl Source {
    pub fn code(self) -> &'static str {
        match self {
            Source::Command => "command",
            Source::Recipe => "recipe",
            Source::Agent => "agent",
        }
    }
}

/// What the card shows. Every field is shown whole: the card scrolls rather
/// than cut anything off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    pub source: Source,
    /// The agent asking, for [`Source::Agent`] ("Claude Code").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The tool or step name: from a fixed set, or an agent's tool name the
    /// bridge has checked is plain ASCII (safe to log either way).
    pub tool: String,
    /// The literal shell command line, for a tool or step that runs one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The working directory the command line was given, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Every other argument, as pretty-printed JSON. JSON keeps a value that
    /// holds a line break or a quote from passing for another argument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl Request {
    /// A command-mode tool call. `run_command` shows its command line and
    /// working directory on their own; any argument left over, for any tool,
    /// goes into `details`, so nothing that changes what runs is left off.
    pub fn for_tool(tool: &str, args: &Value) -> Self {
        let mut rest = args.clone();
        let mut command = None;
        let mut cwd = None;
        if tool == RUN_COMMAND {
            if let Value::Object(map) = &mut rest {
                if let Some(Value::String(line)) = map.get("command") {
                    command = Some(line.clone());
                    map.remove("command");
                }
                if let Some(Value::String(dir)) = map.get("cwd") {
                    // A blank cwd is ignored by the tool, which runs in its
                    // default directory; there is nothing to show.
                    if !dir.trim().is_empty() {
                        cwd = Some(dir.clone());
                    }
                    map.remove("cwd");
                }
            }
        }
        Self {
            source: Source::Command,
            agent: None,
            tool: tool.to_string(),
            command,
            cwd,
            details: details_of(&rest),
        }
    }

    /// A recipe's `run_shell` step, with its command line already
    /// interpolated: what the card shows is exactly what would run.
    pub fn for_recipe_step(command: &str) -> Self {
        Self {
            source: Source::Recipe,
            agent: None,
            tool: RUN_SHELL.to_string(),
            command: Some(command.to_string()),
            cwd: None,
            details: None,
        }
    }

    /// A coding agent's tool call (ADR-0049). A shell tool's command line
    /// shows on its own, as `run_command`'s does; every other input, the
    /// agent's own description of the command included, goes into
    /// `details`. `cwd` is the folder the agent works in, shown for every
    /// tool: it tells two sessions apart, and a relative path in the input
    /// points into it.
    pub fn for_agent(agent: &str, tool: &str, input: &Value, cwd: Option<&str>) -> Self {
        let mut rest = input.clone();
        let mut command = None;
        if AGENT_SHELLS.contains(&tool) {
            if let Value::Object(map) = &mut rest {
                if let Some(Value::String(line)) = map.get("command") {
                    command = Some(line.clone());
                    map.remove("command");
                }
            }
        }
        Self {
            source: Source::Agent,
            agent: Some(agent.to_string()),
            tool: tool.to_string(),
            command,
            cwd: cwd.filter(|dir| !dir.trim().is_empty()).map(str::to_string),
            details: details_of(&rest),
        }
    }
}

/// Arguments left over after the command line, as pretty-printed JSON.
/// JSON keeps a value that holds a line break or a quote from passing for
/// another argument.
fn details_of(rest: &Value) -> Option<String> {
    match rest {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        other => Some(serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string())),
    }
}

/// The request on screen, as the card gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub id: Id,
    #[serde(flatten)]
    pub request: Request,
    /// How many more requests wait behind this one.
    pub waiting: usize,
    /// Milliseconds until the request is denied unanswered.
    pub expires_in_ms: u64,
}

/// Why an answer or an arm was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No pending request has this id: it was answered, timed out or
    /// withdrawn.
    Unknown,
    /// The request waits behind another one and isn't on screen yet.
    NotShown,
    /// Allow came before the card was armed.
    NotArmed,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Refusal::Unknown => "unknown",
            Refusal::NotShown => "not-shown",
            Refusal::NotArmed => "not-armed",
        }
    }
}

/// A request that left the queue with its answer, and the asker's reply
/// channel to send it on.
#[derive(Debug)]
pub struct Answered<T> {
    pub id: Id,
    pub decision: Decision,
    pub reply: T,
}

#[derive(Debug)]
struct Entry<T> {
    id: Id,
    request: Request,
    reply: T,
    /// When the request reached the front of the queue. Always set for the
    /// front entry, never for the others.
    shown_at: Option<Instant>,
    /// The card reported the request's whole text in view for `ARM_DELAY`.
    armed: bool,
}

/// The pending requests, oldest first. `T` is the asker's reply channel.
#[derive(Debug)]
pub struct Queue<T> {
    entries: VecDeque<Entry<T>>,
    next_id: Id,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            next_id: 1,
        }
    }
}

impl<T> Queue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Add a request. It is shown at once if nothing else is pending.
    pub fn push(&mut self, request: Request, reply: T, now: Instant) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        let shown_at = self.entries.is_empty().then_some(now);
        self.entries.push_back(Entry {
            id,
            request,
            reply,
            shown_at,
            armed: false,
        });
        id
    }

    /// The id of the request on screen.
    pub fn shown_id(&self) -> Option<Id> {
        self.entries.front().map(|entry| entry.id)
    }

    /// The request on screen, with the time it has left.
    pub fn shown(&self, now: Instant) -> Option<Shown> {
        let head = self.entries.front()?;
        let shown_at = head.shown_at?;
        let left = (shown_at + TIMEOUT).saturating_duration_since(now);
        Some(Shown {
            id: head.id,
            request: head.request.clone(),
            waiting: self.entries.len() - 1,
            expires_in_ms: u64::try_from(left.as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// The card reports that the request's whole text has been in view for
    /// [`ARM_DELAY`]. Only the request on screen can be armed.
    pub fn arm(&mut self, id: Id) -> Result<(), Refusal> {
        match self.position(id) {
            None => Err(Refusal::Unknown),
            Some(0) => {
                if let Some(head) = self.entries.front_mut() {
                    head.armed = true;
                }
                Ok(())
            }
            Some(_) => Err(Refusal::NotShown),
        }
    }

    /// Whether Allow would be taken for `id` now.
    pub fn can_allow(&self, id: Id, now: Instant) -> Result<(), Refusal> {
        match self.position(id) {
            None => Err(Refusal::Unknown),
            Some(0) => {
                let head = &self.entries[0];
                let settled = head
                    .shown_at
                    .is_some_and(|shown_at| now.saturating_duration_since(shown_at) >= ARM_DELAY);
                if head.armed && settled {
                    Ok(())
                } else {
                    Err(Refusal::NotArmed)
                }
            }
            Some(_) => Err(Refusal::NotShown),
        }
    }

    /// Answer a request. Deny is taken for any pending request, Allow only
    /// for the armed one on screen. The next request is shown at `now`.
    pub fn answer(
        &mut self,
        id: Id,
        decision: Decision,
        now: Instant,
    ) -> Result<Answered<T>, Refusal> {
        let index = self.position(id).ok_or(Refusal::Unknown)?;
        if decision == Decision::Allow {
            self.can_allow(id, now)?;
        }
        self.take(index, now)
            .map(|entry| Answered {
                id: entry.id,
                decision,
                reply: entry.reply,
            })
            .ok_or(Refusal::Unknown)
    }

    /// When the request on screen times out.
    pub fn deadline(&self) -> Option<(Id, Instant)> {
        let head = self.entries.front()?;
        Some((head.id, head.shown_at? + TIMEOUT))
    }

    /// Deny the request on screen if its time is up.
    pub fn expire(&mut self, now: Instant) -> Option<Answered<T>> {
        let (_, deadline) = self.deadline()?;
        if now < deadline {
            return None;
        }
        self.take(0, now).map(|entry| Answered {
            id: entry.id,
            decision: Decision::Deny,
            reply: entry.reply,
        })
    }

    /// The asker went away (its task was cancelled): drop its request
    /// without an answer.
    pub fn withdraw(&mut self, id: Id, now: Instant) -> Option<T> {
        let index = self.position(id)?;
        self.take(index, now).map(|entry| entry.reply)
    }

    fn position(&self, id: Id) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    /// Remove an entry and show the next one, if the front changed.
    fn take(&mut self, index: usize, now: Instant) -> Option<Entry<T>> {
        let entry = self.entries.remove(index)?;
        if let Some(head) = self.entries.front_mut() {
            if head.shown_at.is_none() {
                head.shown_at = Some(now);
            }
        }
        Some(entry)
    }
}

/// The Allow hotkey held down for a request, until it has been held for
/// [`HOLD`]. Each press is numbered, so the timer of an earlier press can't
/// finish a later one.
///
/// Only the timer allows, and only when the OS says the whole chord is
/// still down. The hotkey's own release can't vouch for that: on Windows it
/// watches only the main key, so Ctrl or Shift let go goes unseen, and a
/// release that never comes would turn a tap into a hold. Anything short of
/// "all down" ends the hold without allowing.
#[derive(Debug, Default)]
pub struct Hold {
    presses: u64,
    held: Option<Held>,
}

#[derive(Debug, Clone, Copy)]
struct Held {
    press: u64,
    id: Id,
    since: Instant,
}

/// Where a hold stands when its timer looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldCheck {
    /// The whole chord held for [`HOLD`]: allow this request.
    Allow(Id),
    /// Still held: look again after this long.
    Wait(Duration),
    /// A key of the chord is up, or the OS can't tell: the hold is over
    /// without allowing. Tell the user to hold it.
    Short(Id),
    /// Released, or replaced by a later press.
    Gone,
}

/// What letting go of the Allow hotkey means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldRelease {
    /// Let go before the timer allowed: tell the user to hold it.
    Short(Id),
    /// Nothing was held, or the hold was already done.
    Nothing,
}

impl Hold {
    pub fn new() -> Self {
        Self::default()
    }

    /// The Allow hotkey went down for request `id`. Returns the press, for
    /// [`check`](Self::check).
    pub fn press(&mut self, id: Id, now: Instant) -> u64 {
        self.presses += 1;
        self.held = Some(Held {
            press: self.presses,
            id,
            since: now,
        });
        self.presses
    }

    /// The timer of `press` looks at `now`, with whether the OS reports the
    /// whole chord down (`None`: it can't tell). An answer it returns is
    /// given once.
    pub fn check(&mut self, press: u64, now: Instant, chord_down: Option<bool>) -> HoldCheck {
        match self.held {
            Some(held) if held.press == press => {
                if chord_down != Some(true) {
                    self.held = None;
                    return HoldCheck::Short(held.id);
                }
                let held_for = now.saturating_duration_since(held.since);
                if held_for >= HOLD {
                    self.held = None;
                    HoldCheck::Allow(held.id)
                } else {
                    HoldCheck::Wait((HOLD - held_for).min(HOLD_LOOK))
                }
            }
            _ => HoldCheck::Gone,
        }
    }

    /// The Allow hotkey came up. Only the timer allows, so a hold the
    /// timer hasn't finished is short, however long it lasted.
    pub fn release(&mut self) -> HoldRelease {
        match self.held.take() {
            Some(held) => HoldRelease::Short(held.id),
            None => HoldRelease::Nothing,
        }
    }
}

/// Calls off the requests a task asked for, when the task can't be stopped
/// where it waits: the recipe runtime blocks a thread on each answer, so
/// aborting a voice-triggered take leaves that thread waiting, and a later
/// Allow would still run the step.
///
/// Cancelling hands back the request being waited on, for the broker to
/// withdraw, and denies whatever the task asks after that. An answer that
/// races the cancel doesn't count either.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<Mutex<CancelState>>);

#[derive(Debug, Default)]
struct CancelState {
    cancelled: bool,
    waiting: Option<Id>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, CancelState> {
        // Two plain fields: a panic elsewhere leaves them consistent.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Call the task's requests off. Returns the one it is waiting on, if
    /// any, to withdraw.
    pub fn cancel(&self) -> Option<Id> {
        let mut state = self.state();
        state.cancelled = true;
        state.waiting.take()
    }

    /// Whether the task was called off.
    pub fn is_cancelled(&self) -> bool {
        self.state().cancelled
    }

    /// The task now waits on `id`. False if it was called off already: then
    /// withdraw `id` at once and deny it.
    pub fn wait_on(&self, id: Id) -> bool {
        let mut state = self.state();
        if state.cancelled {
            return false;
        }
        state.waiting = Some(id);
        true
    }

    /// The wait on `id` is over. Whether the task was called off meanwhile,
    /// in which case its answer must be taken as Deny.
    pub fn done(&self, id: Id) -> bool {
        let mut state = self.state();
        if state.waiting == Some(id) {
            state.waiting = None;
        }
        state.cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MS: Duration = Duration::from_millis(1);

    fn shell(line: &str) -> Request {
        Request::for_recipe_step(line)
    }

    #[test]
    fn run_command_shows_its_command_line_and_cwd_verbatim() {
        let line = "Get-ChildItem -Path 'C:\\Users\\דנה\\מסמכים' | Remove-Item -WhatIf";
        let request = Request::for_tool(
            "run_command",
            &json!({ "command": line, "cwd": "C:\\Users\\דנה" }),
        );
        assert_eq!(request.source, Source::Command);
        assert_eq!(request.tool, "run_command");
        assert_eq!(request.command.as_deref(), Some(line));
        assert_eq!(request.cwd.as_deref(), Some("C:\\Users\\דנה"));
        assert_eq!(request.details, None);
    }

    #[test]
    fn run_command_keeps_every_other_argument_in_details() {
        let request = Request::for_tool(
            "run_command",
            &json!({ "command": "npm install", "timeout_ms": 120000 }),
        );
        assert_eq!(request.command.as_deref(), Some("npm install"));
        let details = request.details.expect("timeout_ms is shown");
        assert!(details.contains("\"timeout_ms\": 120000"), "{details}");
        assert!(!details.contains("npm install"), "{details}");
    }

    #[test]
    fn a_blank_cwd_is_not_shown() {
        let request = Request::for_tool("run_command", &json!({ "command": "dir", "cwd": "   " }));
        assert_eq!(request.cwd, None);
        assert_eq!(request.details, None);
    }

    #[test]
    fn a_multi_line_hebrew_command_survives_whole() {
        // Niqqud (combining marks) and line breaks must come through as is.
        let line = "echo \"שָׁלוֹם\"\nWrite-Output 'עולם hello'\n";
        let request = Request::for_tool("run_command", &json!({ "command": line }));
        assert_eq!(request.command.as_deref(), Some(line));
    }

    #[test]
    fn a_malformed_command_argument_is_shown_as_json() {
        let request = Request::for_tool("run_command", &json!({ "command": ["rm", "-rf"] }));
        assert_eq!(request.command, None);
        let details = request.details.expect("the malformed argument is shown");
        assert!(details.contains("\"rm\""), "{details}");
    }

    #[test]
    fn other_tools_show_all_their_arguments_with_hebrew_unescaped() {
        let request = Request::for_tool(
            "file_delete",
            &json!({ "path": "C:\\Users\\דנה\\קובץ ישן.txt" }),
        );
        assert_eq!(request.command, None);
        assert_eq!(request.cwd, None);
        let details = request.details.expect("the path is shown");
        assert!(details.contains("דנה"), "{details}");
        assert!(details.contains("קובץ ישן.txt"), "{details}");
    }

    #[test]
    fn a_tool_without_arguments_has_no_details() {
        assert_eq!(Request::for_tool("lock_screen", &json!({})).details, None);
        assert_eq!(Request::for_tool("lock_screen", &Value::Null).details, None);
    }

    #[test]
    fn a_recipe_step_shows_its_interpolated_command() {
        let request = shell("Rename-Item 'דוח.txt' 'דוח 2026.txt'");
        assert_eq!(request.source, Source::Recipe);
        assert_eq!(request.tool, "run_shell");
        assert_eq!(
            request.command.as_deref(),
            Some("Rename-Item 'דוח.txt' 'דוח 2026.txt'")
        );
        assert_eq!(request.agent, None);
    }

    #[test]
    fn an_agents_shell_call_shows_its_command_line_and_folder() {
        let request = Request::for_agent(
            "Claude Code",
            "Bash",
            &json!({
                "command": "git push --force origin main",
                "description": "Force-push the rebased branch",
                "timeout": 120000
            }),
            Some("C:\\Users\\דנה\\פרויקט"),
        );
        assert_eq!(request.source, Source::Agent);
        assert_eq!(request.agent.as_deref(), Some("Claude Code"));
        assert_eq!(request.tool, "Bash");
        assert_eq!(
            request.command.as_deref(),
            Some("git push --force origin main")
        );
        assert_eq!(request.cwd.as_deref(), Some("C:\\Users\\דנה\\פרויקט"));
        // The agent's own description is shown too, as what it is: an
        // argument, not the command.
        let details = request.details.expect("the rest of the input");
        assert!(
            details.contains("Force-push the rebased branch"),
            "{details}"
        );
        assert!(details.contains("\"timeout\": 120000"), "{details}");
        assert!(!details.contains("git push"), "{details}");

        let ps = Request::for_agent(
            "Claude Code",
            "PowerShell",
            &json!({ "command": "Remove-Item x" }),
            None,
        );
        assert_eq!(ps.command.as_deref(), Some("Remove-Item x"));
        assert_eq!(ps.cwd, None);
    }

    #[test]
    fn an_agents_other_tools_show_every_input_whole() {
        let input = json!({
            "file_path": "C:\\repo\\README.he.md",
            "old_string": "שלום\nעולם",
            "new_string": "שָׁלוֹם \u{202E}עולם"
        });
        let request = Request::for_agent("Claude Code", "Edit", &input, Some("C:\\repo"));
        assert_eq!(request.command, None);
        let details = request.details.expect("the input");
        let back: Value = serde_json::from_str(&details).unwrap();
        assert_eq!(back, input, "nothing dropped or changed");
        // A `command` field of another tool is just input.
        let other = Request::for_agent(
            "Claude Code",
            "mcp__x__run",
            &json!({ "command": "y" }),
            None,
        );
        assert_eq!(other.command, None);
        assert!(other.details.unwrap().contains("\"command\": \"y\""));
    }

    #[test]
    fn an_agents_blank_folder_or_empty_input_shows_nothing_extra() {
        let request = Request::for_agent(
            "Claude Code",
            "Bash",
            &json!({ "command": "ls" }),
            Some("  "),
        );
        assert_eq!(request.cwd, None);
        assert_eq!(request.details, None);
        // A non-string command isn't a command line: it stays in details.
        let odd = Request::for_agent(
            "Claude Code",
            "Bash",
            &json!({ "command": ["rm", "-rf"] }),
            None,
        );
        assert_eq!(odd.command, None);
        assert!(odd.details.unwrap().contains("rm"));
    }

    #[test]
    fn the_card_gets_the_agent_and_source() {
        let request = Request::for_agent("Claude Code", "Bash", &json!({ "command": "ls" }), None);
        let mut queue = Queue::new();
        queue.push(request, (), Instant::now());
        let card = serde_json::to_value(queue.shown(Instant::now()).unwrap()).unwrap();
        assert_eq!(card["source"], "agent");
        assert_eq!(card["agent"], "Claude Code");
        // Other sources carry no agent field at all.
        let mut queue = Queue::new();
        queue.push(shell("dir"), (), Instant::now());
        let card = serde_json::to_value(queue.shown(Instant::now()).unwrap()).unwrap();
        assert!(card.get("agent").is_none());
    }

    #[test]
    fn the_card_payload_leaves_out_empty_fields() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        queue.push(shell("dir"), (), t0);
        let value = serde_json::to_value(queue.shown(t0).unwrap()).unwrap();
        assert_eq!(
            value,
            json!({
                "id": 1,
                "source": "recipe",
                "tool": "run_shell",
                "command": "dir",
                "waiting": 0,
                "expires_in_ms": 30000
            })
        );
    }

    #[test]
    fn the_first_request_is_shown_and_later_ones_wait() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), (), t0);
        let second = queue.push(shell("b"), (), t0 + MS);
        assert_ne!(first, second);
        let shown = queue.shown(t0 + MS).unwrap();
        assert_eq!(shown.id, first);
        assert_eq!(shown.waiting, 1);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn allow_needs_the_card_armed_and_the_delay_passed() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let id = queue.push(shell("a"), (), t0);

        // Not armed: refused however long it has been on screen.
        assert_eq!(
            queue
                .answer(id, Decision::Allow, t0 + TIMEOUT / 2)
                .unwrap_err(),
            Refusal::NotArmed
        );
        // Armed, but sooner than the delay after it was shown: refused.
        queue.arm(id).unwrap();
        assert_eq!(
            queue
                .answer(id, Decision::Allow, t0 + ARM_DELAY - MS)
                .unwrap_err(),
            Refusal::NotArmed
        );
        // Both: taken.
        let answered = queue.answer(id, Decision::Allow, t0 + ARM_DELAY).unwrap();
        assert_eq!(answered.id, id);
        assert_eq!(answered.decision, Decision::Allow);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_refused_allow_leaves_the_request_pending() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let id = queue.push(shell("a"), (), t0);
        assert!(queue.answer(id, Decision::Allow, t0).is_err());
        assert_eq!(queue.shown_id(), Some(id));
    }

    #[test]
    fn deny_is_always_taken() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let id = queue.push(shell("a"), (), t0);
        let answered = queue.answer(id, Decision::Deny, t0).unwrap();
        assert_eq!(answered.decision, Decision::Deny);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_waiting_request_can_be_denied_but_not_armed_or_allowed() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), (), t0);
        let second = queue.push(shell("b"), (), t0);
        assert_eq!(queue.arm(second).unwrap_err(), Refusal::NotShown);
        assert_eq!(
            queue
                .answer(second, Decision::Allow, t0 + TIMEOUT)
                .unwrap_err(),
            Refusal::NotShown
        );
        queue.answer(second, Decision::Deny, t0).unwrap();
        assert_eq!(queue.shown_id(), Some(first));
        assert_eq!(queue.shown(t0).unwrap().waiting, 0);
    }

    #[test]
    fn the_next_request_starts_its_own_clock_when_shown() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), (), t0);
        let second = queue.push(shell("b"), (), t0);

        let t1 = t0 + Duration::from_secs(10);
        queue.answer(first, Decision::Deny, t1).unwrap();
        assert_eq!(queue.shown_id(), Some(second));
        // Armed by the card, but the delay counts from t1, not from t0.
        queue.arm(second).unwrap();
        assert_eq!(
            queue
                .answer(second, Decision::Allow, t1 + ARM_DELAY - MS)
                .unwrap_err(),
            Refusal::NotArmed
        );
        // And so does the timeout.
        assert_eq!(queue.deadline(), Some((second, t1 + TIMEOUT)));
        assert_eq!(queue.shown(t1).unwrap().expires_in_ms, 30_000);
        assert!(queue
            .answer(second, Decision::Allow, t1 + ARM_DELAY)
            .is_ok());
    }

    #[test]
    fn arming_does_not_carry_over_to_the_next_request() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), (), t0);
        let second = queue.push(shell("b"), (), t0);
        queue.arm(first).unwrap();
        queue.answer(first, Decision::Deny, t0).unwrap();
        // A second press of the same key must not approve what came next.
        assert_eq!(
            queue.can_allow(second, t0 + TIMEOUT).unwrap_err(),
            Refusal::NotArmed
        );
    }

    #[test]
    fn an_unanswered_request_is_denied_when_its_time_is_up() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let id = queue.push(shell("a"), (), t0);
        assert!(queue.expire(t0 + TIMEOUT - MS).is_none());
        assert_eq!(queue.shown(t0 + TIMEOUT - MS).unwrap().expires_in_ms, 1);
        let answered = queue.expire(t0 + TIMEOUT).unwrap();
        assert_eq!(answered.id, id);
        assert_eq!(answered.decision, Decision::Deny);
        assert!(queue.is_empty());
        assert!(queue.expire(t0 + TIMEOUT * 2).is_none());
    }

    #[test]
    fn a_withdrawn_request_leaves_without_an_answer() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), 1, t0);
        let second = queue.push(shell("b"), 2, t0);
        assert_eq!(queue.withdraw(first, t0 + MS), Some(1));
        assert_eq!(queue.shown_id(), Some(second));
        assert_eq!(queue.deadline(), Some((second, t0 + MS + TIMEOUT)));
        assert_eq!(queue.withdraw(first, t0 + MS), None);
    }

    #[test]
    fn an_answered_request_cannot_be_answered_again() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let id = queue.push(shell("a"), (), t0);
        queue.answer(id, Decision::Deny, t0).unwrap();
        assert_eq!(
            queue.answer(id, Decision::Deny, t0).unwrap_err(),
            Refusal::Unknown
        );
        assert_eq!(queue.arm(id).unwrap_err(), Refusal::Unknown);
    }

    #[test]
    fn ids_are_never_reused() {
        let t0 = Instant::now();
        let mut queue = Queue::new();
        let first = queue.push(shell("a"), (), t0);
        queue.answer(first, Decision::Deny, t0).unwrap();
        let second = queue.push(shell("b"), (), t0);
        assert!(second > first);
    }

    #[test]
    fn decisions_parse_from_the_card() {
        assert_eq!(
            serde_json::from_value::<Decision>(json!("allow")).unwrap(),
            Decision::Allow
        );
        assert_eq!(
            serde_json::from_value::<Decision>(json!("deny")).unwrap(),
            Decision::Deny
        );
        assert!(serde_json::from_value::<Decision>(json!("yes")).is_err());
    }

    const DOWN: Option<bool> = Some(true);
    const UP: Option<bool> = Some(false);

    #[test]
    fn a_hold_allows_once_held_for_the_full_time() {
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(4, t0);
        // It looks every HOLD_LOOK, and lands on the full time.
        assert_eq!(hold.check(press, t0, DOWN), HoldCheck::Wait(HOLD_LOOK));
        assert_eq!(
            hold.check(press, t0 + HOLD - 20 * MS, DOWN),
            HoldCheck::Wait(20 * MS)
        );
        assert_eq!(hold.check(press, t0 + HOLD, DOWN), HoldCheck::Allow(4));
        // Given once: the timer looking again, or the key coming up, finds
        // nothing more.
        assert_eq!(hold.check(press, t0 + HOLD + MS, DOWN), HoldCheck::Gone);
        assert_eq!(hold.release(), HoldRelease::Nothing);
    }

    #[test]
    fn a_tap_never_allows() {
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(4, t0);
        assert_eq!(hold.release(), HoldRelease::Short(4));
        assert_eq!(hold.check(press, t0 + HOLD, DOWN), HoldCheck::Gone);
    }

    #[test]
    fn a_tap_whose_release_is_lost_never_allows() {
        // The hotkey's release never comes, but the keys are up: the timer
        // must not take that for a hold.
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(4, t0);
        assert_eq!(hold.check(press, t0 + HOLD, UP), HoldCheck::Short(4));
        assert_eq!(hold.check(press, t0 + HOLD + MS, DOWN), HoldCheck::Gone);
        assert_eq!(hold.release(), HoldRelease::Nothing);
    }

    #[test]
    fn letting_go_of_ctrl_or_shift_ends_the_hold() {
        // Y stays down, so the hotkey sends no release; the OS reports the
        // chord broken at the next look.
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(2, t0);
        assert_eq!(hold.check(press, t0, DOWN), HoldCheck::Wait(HOLD_LOOK));
        assert_eq!(hold.check(press, t0 + 400 * MS, UP), HoldCheck::Short(2));
        // Pressing Ctrl again doesn't bring the hold back.
        assert_eq!(hold.check(press, t0 + HOLD, DOWN), HoldCheck::Gone);
    }

    #[test]
    fn a_hold_cannot_allow_where_the_keys_cannot_be_read() {
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(3, t0);
        assert_eq!(hold.check(press, t0 + HOLD, None), HoldCheck::Short(3));
    }

    #[test]
    fn letting_go_before_the_timer_allows_is_short_even_after_the_full_time() {
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let press = hold.press(9, t0);
        assert_eq!(hold.release(), HoldRelease::Short(9));
        assert_eq!(hold.check(press, t0 + HOLD + 6 * MS, DOWN), HoldCheck::Gone);
    }

    #[test]
    fn a_new_press_replaces_the_old_one() {
        let t0 = Instant::now();
        let mut hold = Hold::new();
        let first = hold.press(1, t0);
        assert_eq!(hold.release(), HoldRelease::Short(1));
        let second = hold.press(1, t0 + 200 * MS);
        // The first press's timer must not finish the second hold early.
        assert_eq!(hold.check(first, t0 + HOLD, DOWN), HoldCheck::Gone);
        assert_eq!(
            hold.check(second, t0 + HOLD - 20 * MS, DOWN),
            HoldCheck::Wait(HOLD_LOOK)
        );
        assert_eq!(
            hold.check(second, t0 + 200 * MS + HOLD, DOWN),
            HoldCheck::Allow(1)
        );
    }

    #[test]
    fn letting_go_with_nothing_held_does_nothing() {
        let mut hold = Hold::new();
        assert_eq!(hold.release(), HoldRelease::Nothing);
    }

    #[test]
    fn cancel_hands_back_the_request_being_waited_on() {
        let cancel = Cancel::new();
        assert!(cancel.wait_on(7));
        assert_eq!(cancel.cancel(), Some(7));
        // Its answer arrives anyway (the withdrawal denied it, or it raced
        // the cancel): it doesn't count.
        assert!(cancel.done(7));
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn cancel_refuses_what_the_task_asks_next() {
        let cancel = Cancel::new();
        assert_eq!(cancel.cancel(), None);
        assert!(!cancel.wait_on(8));
        assert!(cancel.done(8));
        // Cancelling again has nothing more to withdraw.
        assert_eq!(cancel.cancel(), None);
    }

    #[test]
    fn an_answer_before_any_cancel_counts() {
        let cancel = Cancel::new();
        assert!(cancel.wait_on(1));
        assert!(!cancel.done(1));
        assert!(cancel.wait_on(2));
        assert!(!cancel.done(2));
        // Cancelling after the last answer withdraws nothing.
        assert_eq!(cancel.cancel(), None);
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn clones_share_one_cancel() {
        let task = Cancel::new();
        let canceller = task.clone();
        assert!(task.wait_on(3));
        assert_eq!(canceller.cancel(), Some(3));
        assert!(task.done(3));
    }
}
