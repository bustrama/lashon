//! The Claude Code hooks bridge in the shell (docs/adr/0049).
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
    claude_settings, endpoint, AgentAsk, AskFn, Bridge, Verdict, AGENT,
};
use ottid_core::approval::{Decision, Request};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::approval;

const LOG: &str = "ottid::agent_hooks";

/// The Hub's window label: the only one that may change Claude Code's
/// settings.
const HUB: &str = "hub";

/// Tauri-managed state: the running listener, if any.
#[derive(Default)]
pub struct AgentBridge(Mutex<Option<Bridge>>);

fn lock(state: &AgentBridge) -> MutexGuard<'_, Option<Bridge>> {
    state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start the listener if the hook is installed. Run on the async runtime:
/// the listener's tasks are spawned on it.
pub fn start_if_installed(app: &AppHandle) {
    let Some(path) = claude_settings::user_settings_path() else {
        return;
    };
    let installed = claude_settings::read(&path)
        .ok()
        .and_then(|current| claude_settings::find(current.as_deref()).ok())
        .is_some_and(|found| !found.commands.is_empty());
    if installed {
        start(app);
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
            let request = Request::for_agent(AGENT, &ask.tool, &ask.input, ask.cwd.as_deref());
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
fn hook_exe(app: &AppHandle) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ottid-hook.exe"
    } else {
        "ottid-hook"
    };
    let bundled = app
        .path()
        .resolve(
            format!("binaries/ottid-hook/{name}"),
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

fn settings_path() -> Result<PathBuf, String> {
    claude_settings::user_settings_path().ok_or_else(|| "no-home".to_string())
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
pub async fn agent_hooks_status(app: AppHandle) -> Result<Status, String> {
    let path = settings_path()?;
    let hook = hook_exe(&app).map(|exe| exe.to_string_lossy().into_owned());
    let found = claude_settings::read(&path)
        .map_err(|err| err.code().to_string())
        .and_then(|current| {
            claude_settings::find(current.as_deref()).map_err(|err| err.code().to_string())
        });
    let listening = lock(&app.state::<AgentBridge>()).is_some();
    Ok(match found {
        Ok(found) => Status {
            settings_path: path.to_string_lossy().into_owned(),
            installed: !found.commands.is_empty(),
            current: hook
                .as_ref()
                .is_some_and(|exe| found.commands.iter().any(|command| command == exe)),
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
    action: &str,
    exe: Option<&str>,
    current: Option<&str>,
) -> Result<claude_settings::Plan, String> {
    match (action, exe) {
        ("install", Some(exe)) => {
            claude_settings::plan_install(current, exe).map_err(|err| err.code().to_string())
        }
        ("install", None) => Err("no-hook-binary".to_string()),
        ("uninstall", _) => {
            claude_settings::plan_uninstall(current).map_err(|err| err.code().to_string())
        }
        _ => Err("bad-action".to_string()),
    }
}

#[tauri::command]
pub async fn agent_hooks_preview(app: AppHandle, action: String) -> Result<Preview, String> {
    let path = settings_path()?;
    let exe = hook_exe(&app).map(|exe| exe.to_string_lossy().into_owned());
    let current = claude_settings::read(&path).map_err(|err| err.code().to_string())?;
    let plan = plan_for(&action, exe.as_deref(), current.as_deref())?;
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
) -> Result<claude_settings::Applied, String> {
    from_hub(&webview)?;
    let path = settings_path()?;
    let install = match action.as_str() {
        "install" => true,
        "uninstall" => false,
        _ => return Err("bad-action".to_string()),
    };
    let exe = hook_exe(&app).map(|exe| exe.to_string_lossy().into_owned());
    if install && exe.is_none() {
        return Err("no-hook-binary".to_string());
    }
    let applied = claude_settings::apply(&path, &fingerprint, |current| match &exe {
        Some(exe) if install => claude_settings::plan_install(current, exe),
        _ => claude_settings::plan_uninstall(current),
    })
    .map_err(|err| err.code().to_string())?;
    tracing::info!(
        target: LOG,
        action = %action,
        changed = applied.changed,
        backed_up = applied.backup.is_some(),
        "Claude Code settings updated"
    );
    if install {
        start(&app);
    } else {
        stop(&app);
    }
    Ok(applied)
}
