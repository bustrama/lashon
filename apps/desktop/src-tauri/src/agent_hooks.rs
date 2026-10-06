//! The Claude Code hooks bridge in the shell (docs/adr/0050).
//!
//! The protocol, the listener and the settings editor are
//! `ottid_core::agent_bridge`, tested there. This module adds what needs
//! Tauri:
//!
//! - **The listener runs only while the hook is installed** in Claude Code's
//!   user settings: checked at start-up, started by the Hub's install and
//!   stopped by its uninstall. Without the hook, Ottid has no listener.
//! - Each request goes to the approval card through `approval::ask_agent`.
//!   A request nobody answers goes back to Claude Code's own prompt.
//! - The Hub's commands: the status, a preview of the change to Claude
//!   Code's settings, and applying it once the user confirms. Only the Hub
//!   window may apply.
//! - Stopping on exit deletes the bridge file, and the token with it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use ottid_core::agent_bridge::{
    claude_settings, codex_settings, endpoint, Agent, AgentAsk, AskFn, Bridge, Verdict,
};
use ottid_core::approval::{Decision, Request};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

use crate::approval;

const LOG: &str = "ottid::agent_hooks";

/// The Hub's window label: the only one that may change Claude Code's
/// settings.
const HUB: &str = "hub";

/// Tauri-managed state: the running listener, if any.
#[derive(Default)]
pub struct AgentBridge(
    Mutex<Option<Bridge>>,
    Mutex<ottid_core::agent_bridge::activity::Tracker>,
);

#[tauri::command]
pub fn agent_activity_current(
    app: AppHandle,
) -> Option<ottid_core::agent_bridge::activity::Summary> {
    app.state::<AgentBridge>()
        .1
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .summary(std::time::Instant::now())
}

fn lock(state: &AgentBridge) -> MutexGuard<'_, Option<Bridge>> {
    state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start the listener if the hook is installed. Run on the async runtime:
/// the listener's tasks are spawned on it.
fn selected(value: Option<&str>) -> Result<Agent, String> {
    match value {
        None | Some("claude") => Ok(Agent::Claude),
        Some("codex") => Ok(Agent::Codex),
        _ => Err("bad-agent".into()),
    }
}
fn settings_path_for(agent: Agent) -> Result<PathBuf, String> {
    match agent {
        Agent::Claude => claude_settings::user_settings_path(),
        Agent::Codex => codex_settings::user_settings_path(),
    }
    .ok_or_else(|| "no-home".into())
}
fn find_for(
    agent: Agent,
    current: Option<&str>,
) -> Result<claude_settings::Found, claude_settings::SettingsError> {
    match agent {
        Agent::Claude => claude_settings::find(current),
        Agent::Codex => codex_settings::find(current),
    }
}
fn installed(agent: Agent) -> bool {
    settings_path_for(agent)
        .ok()
        .and_then(|path| claude_settings::read(&path).ok())
        .and_then(|current| find_for(agent, current.as_deref()).ok())
        .is_some_and(|found| !found.commands.is_empty())
}
pub fn start_if_installed(app: &AppHandle) {
    if installed(Agent::Claude) || installed(Agent::Codex) {
        start(app);
    } else {
        stop(app);
    }
}

fn start(app: &AppHandle) {
    let state = app.state::<AgentBridge>();
    let mut bridge = lock(&state);
    if bridge.is_some() {
        return;
    }
    let Some(dir) = endpoint::default_dir() else {
        tracing::warn!(target: LOG, "no per-user data folder for the agent bridge");
        return;
    };
    match Bridge::start(&dir, asker(app.clone())) {
        Ok(started) => *bridge = Some(started),
        // The hook falls back to Claude Code's prompt while this is so.
        Err(err) => tracing::error!(target: LOG, "could not start the agent bridge: {err}"),
    }
}

/// Stop the listener and delete the bridge file. Every request still open
/// falls back to Claude Code's prompt.
pub fn stop(app: &AppHandle) {
    if let Some(state) = app.try_state::<AgentBridge>() {
        lock(&state).take();
    }
}

fn asker(app: AppHandle) -> AskFn {
    Arc::new(move |ask: AgentAsk| {
        let app = app.clone();
        Box::pin(async move {
            if !installed(ask.agent) {
                return Verdict::Ask;
            }
            if ask.tool == ottid_core::agent_bridge::activity::TOOL {
                if let Ok(activity) = serde_json::from_value::<
                    ottid_core::agent_bridge::activity::Activity,
                >(ask.input)
                {
                    if activity.valid() {
                        app.state::<AgentBridge>()
                            .1
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .update(ask.agent, activity, std::time::Instant::now());
                        let _ = app.emit("agent:activity", agent_activity_current(app.clone()));
                    }
                }
                return Verdict::Ask;
            }
            // Codex fires PermissionRequest before automatic review too, but
            // the hook payload does not identify the reviewer. Opt in to cards
            // explicitly; otherwise leave the native review flow untouched.
            if ask.agent == Agent::Codex
                && !app
                    .store("settings.json")
                    .ok()
                    .and_then(|store| store.get("agents.codexApprovalCards"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            {
                return Verdict::Ask;
            }
            let request =
                Request::for_agent(ask.agent.name(), &ask.tool, &ask.input, ask.cwd.as_deref());
            match approval::ask_agent(&app, request).await {
                Some(Decision::Allow) => Verdict::Allow,
                Some(Decision::Deny) => Verdict::Deny,
                None => Verdict::Ask,
            }
        })
    })
}

/// The `ottid-hook` binary: bundled next to the other binaries, or, in a
/// development build, next to Ottid's own executable in Cargo's target
/// folder (`cargo build -p ottid-core --bin ottid-hook`).
fn hook_exe(app: &AppHandle, agent: Agent) -> Option<PathBuf> {
    let name = match (agent, cfg!(windows)) {
        (Agent::Claude, true) => "ottid-hook.exe",
        (Agent::Claude, false) => "ottid-hook",
        (Agent::Codex, true) => "ottid-codex-hook.exe",
        (Agent::Codex, false) => "ottid-codex-hook",
    };
    let bundled = app
        .path()
        .resolve(
            format!(
                "binaries/{}/{name}",
                if agent == Agent::Codex {
                    "ottid-codex-hook"
                } else {
                    "ottid-hook"
                }
            ),
            tauri::path::BaseDirectory::Resource,
        )
        .ok();
    let beside = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name(name));
    [bundled, beside]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .map(|path| plain_path(&path))
}

/// Claude Code spawns the command as written: give it a plain absolute
/// path, without Windows' `\\?\` prefix.
fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

fn from_hub(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == HUB {
        Ok(())
    } else {
        Err("not-the-hub".to_string())
    }
}

fn install_plan(
    agent: Agent,
    current: Option<&str>,
    exe: &str,
) -> Result<claude_settings::Plan, claude_settings::SettingsError> {
    let plan = match agent {
        Agent::Claude => claude_settings::plan_install(current, exe),
        Agent::Codex => codex_settings::plan_install(current, exe),
    }?;
    ottid_core::agent_bridge::activity::plan_hooks(plan, exe, agent, true)
}
fn uninstall_plan(
    agent: Agent,
    current: Option<&str>,
) -> Result<claude_settings::Plan, claude_settings::SettingsError> {
    let plan = match agent {
        Agent::Claude => claude_settings::plan_uninstall(current),
        Agent::Codex => codex_settings::plan_uninstall(current),
    }?;
    ottid_core::agent_bridge::activity::plan_hooks(plan, "", agent, false)
}

/// What the Hub shows.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// Claude Code's user settings file.
    settings_path: String,
    /// The hook binary, if this build has one.
    hook_exe: Option<String>,
    /// Ottid's hook is in the settings file.
    installed: bool,
    /// ...and runs this build's hook binary.
    current: bool,
    /// `disableAllHooks` is on in the settings file.
    hooks_disabled: bool,
    /// The listener is running.
    listening: bool,
    /// Why the settings file can't be read, if it can't.
    error: Option<String>,
}

#[tauri::command]
pub async fn agent_hooks_status(app: AppHandle, agent: Option<String>) -> Result<Status, String> {
    let agent = selected(agent.as_deref())?;
    let path = settings_path_for(agent)?;
    let hook = hook_exe(&app, agent).map(|exe| exe.to_string_lossy().into_owned());
    let found = claude_settings::read(&path)
        .map_err(|err| err.code().to_string())
        .and_then(|current| {
            find_for(agent, current.as_deref()).map_err(|err| err.code().to_string())
        });
    let listening = lock(&app.state::<AgentBridge>()).is_some();
    Ok(match found {
        Ok(found) => Status {
            settings_path: path.to_string_lossy().into_owned(),
            installed: !found.commands.is_empty(),
            current: hook.as_ref().is_some_and(|exe| {
                claude_settings::read(&path)
                    .ok()
                    .and_then(|current| install_plan(agent, current.as_deref(), exe).ok())
                    .is_some_and(|plan| !plan.changes)
            }),
            hooks_disabled: found.hooks_disabled,
            hook_exe: hook,
            listening,
            error: None,
        },
        Err(code) => Status {
            settings_path: path.to_string_lossy().into_owned(),
            installed: false,
            current: false,
            hooks_disabled: false,
            hook_exe: hook,
            listening,
            error: Some(code),
        },
    })
}

/// What the change would be, for the user to confirm.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    settings_path: String,
    /// The file's state now; the change is applied only to this.
    fingerprint: String,
    /// The file exists, so it will be backed up first.
    exists: bool,
    #[serde(flatten)]
    plan: claude_settings::Plan,
}

fn plan_for(
    agent: Agent,
    action: &str,
    exe: Option<&str>,
    current: Option<&str>,
) -> Result<claude_settings::Plan, String> {
    match (action, exe) {
        ("install", Some(exe)) => {
            install_plan(agent, current, exe).map_err(|err| err.code().to_string())
        }
        ("install", None) => Err("no-hook-binary".to_string()),
        ("uninstall", _) => uninstall_plan(agent, current).map_err(|err| err.code().to_string()),
        _ => Err("bad-action".to_string()),
    }
}

#[tauri::command]
pub async fn agent_hooks_preview(
    app: AppHandle,
    action: String,
    agent: Option<String>,
) -> Result<Preview, String> {
    let agent = selected(agent.as_deref())?;
    let path = settings_path_for(agent)?;
    let exe = hook_exe(&app, agent).map(|exe| exe.to_string_lossy().into_owned());
    let current = claude_settings::read(&path).map_err(|err| err.code().to_string())?;
    let plan = plan_for(agent, &action, exe.as_deref(), current.as_deref())?;
    Ok(Preview {
        settings_path: path.to_string_lossy().into_owned(),
        fingerprint: claude_settings::fingerprint(current.as_deref().map(str::as_bytes)),
        exists: current.is_some(),
        plan,
    })
}

/// Make the previewed change, after the user confirmed it in the Hub.
#[tauri::command]
pub async fn agent_hooks_apply(
    app: AppHandle,
    webview: tauri::Webview,
    action: String,
    fingerprint: String,
    agent: Option<String>,
) -> Result<claude_settings::Applied, String> {
    from_hub(&webview)?;
    let agent = selected(agent.as_deref())?;
    let path = settings_path_for(agent)?;
    let install = match action.as_str() {
        "install" => true,
        "uninstall" => false,
        _ => return Err("bad-action".to_string()),
    };
    let exe = hook_exe(&app, agent).map(|exe| exe.to_string_lossy().into_owned());
    if install && exe.is_none() {
        return Err("no-hook-binary".to_string());
    }
    let applied = claude_settings::apply(&path, &fingerprint, |current| match &exe {
        Some(exe) if install => install_plan(agent, current, exe),
        _ => uninstall_plan(agent, current),
    })
    .map_err(|err| err.code().to_string())?;
    tracing::info!(
        target: LOG,
        action = %action,
        changed = applied.changed,
        backed_up = applied.backup.is_some(),
        "agent hook settings updated"
    );
    start_if_installed(&app);
    Ok(applied)
}
