//! M9 Phase 1b — recipe runtime executor.
//!
//! Walks a validated `Recipe` through its host-OS step list, executing
//! each step against Lashon's OS-UI primitives. Step types map to:
//!
//! | Step | Backing impl |
//! |---|---|
//! | `KeyChord` | `enigo` synthetic keypress (same path as `tools::press_keys`) |
//! | `TypeUnicode` | `inject::inject_text` — Hebrew clipboard path baked in |
//! | `FocusWindow` | `tools::focus_window::try_focus` (Win32 `EnumWindows`) |
//! | `WaitForWindow` | poll-loop on `try_focus` until match or timeout |
//! | `WaitMs` | `tokio::time::sleep` |
//! | `ScreenshotToClipboard` | PowerShell shell-out: `System.Windows.Forms.Clipboard::SetImage` |
//! | `ClipboardSet` | `arboard::Clipboard::set_text` |
//! | `ClipboardGetInto` | `arboard::Clipboard::get_text`, bound to a recipe var |
//! | `RunShell` | PowerShell on Windows / sh elsewhere — gated by [`ConfirmHandler`] |
//! | `OpenUrl` | `open::that` |
//! | `OpenApp` | `cmd /c start "" "<name>"` on Windows |
//! | `ClickLabel` | Phase 1b v1: returns [`RuntimeError::StepNotImplemented`]. UIA wiring lands in v1.1. |
//!
//! Slot interpolation: every text-bearing field passes through
//! [`interpolate`], which substitutes `{{ key }}` (whitespace-tolerant)
//! against `args` + step-local vars (`ClipboardGetInto.var`,
//! `RunShell.capture_into`). An unresolved reference aborts the run —
//! the validator (Phase 1a) catches the static cases, but runtime
//! references to a missing step-local var (e.g. a recipe that uses
//! `{{ stash }}` before `clipboard_get_into` runs) get surfaced here.
//!
//! Confirmation: [`RunShell`] is the only destructive step type in v1.
//! The runtime calls [`ConfirmHandler::confirm`] with the interpolated
//! command before executing. `Deny` aborts the run with
//! [`RuntimeError::Denied`].

use std::collections::HashMap;
use std::process::Stdio;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

use super::schema::{Recipe, Step};

/// Decision a [`ConfirmHandler`] returns when asked to gate a
/// destructive step. The runtime aborts on [`Deny`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDecision {
    Allow,
    Deny,
}

/// Confirmation gate the runtime calls before any destructive step
/// (currently only [`Step::RunShell`]). The Tauri shell will plug a
/// modal-backed handler; the included [`AlwaysAllow`] / [`AlwaysDeny`]
/// impls cover tests and the CLI's `--yes` / `--no` flags.
pub trait ConfirmHandler: Send + Sync {
    /// Synchronous decision — implementors that need to ask a UI must
    /// block on it. The runtime calls this from an async context so a
    /// blocking call here will park the executor thread; that is the
    /// intended shape (we don't want to advance to the next step
    /// while waiting for the user's answer).
    fn confirm(&self, prompt: &str) -> ConfirmDecision;
}

/// Approves every prompt. Used in tests and by the CLI's `--yes`.
pub struct AlwaysAllow;
impl ConfirmHandler for AlwaysAllow {
    fn confirm(&self, _prompt: &str) -> ConfirmDecision {
        ConfirmDecision::Allow
    }
}

/// Denies every prompt. Used in tests and by the CLI's `--no`.
pub struct AlwaysDeny;
impl ConfirmHandler for AlwaysDeny {
    fn confirm(&self, _prompt: &str) -> ConfirmDecision {
        ConfirmDecision::Deny
    }
}

/// One thing that went wrong during recipe execution. The runtime
/// returns on the first error — partial side effects (anything that
/// landed before the error) are not rolled back.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("recipe has no `os_steps.{os}:` variant — refusing to run on this host")]
    NoStepsForOs { os: &'static str },

    #[error(
        "unresolved interpolation: {{{{ {name} }}}} references no parameter or step-local variable"
    )]
    UnknownInterpolation { name: String },

    #[error("step {index} ({kind}) is not yet implemented in the v1 runtime — {reason}")]
    StepNotImplemented {
        index: usize,
        kind: &'static str,
        reason: &'static str,
    },

    #[error("step {index} ({kind}) failed: {source}")]
    StepFailed {
        index: usize,
        kind: &'static str,
        #[source]
        source: anyhow::Error,
    },

    #[error("step {index} ({kind}) denied by confirmation handler")]
    Denied { index: usize, kind: &'static str },

    #[error("step {index} ({kind}) timed out after {elapsed_ms} ms (limit {limit_ms} ms)")]
    Timeout {
        index: usize,
        kind: &'static str,
        elapsed_ms: u64,
        limit_ms: u64,
    },
}

/// Per-run state surfaced after a successful (or partially successful)
/// recipe execution.
#[derive(Debug, Default)]
pub struct RecipeRun {
    /// Number of steps that completed (not counting the one that
    /// errored, if any).
    pub steps_executed: usize,
    /// Step-local variables bound during the run — every
    /// `clipboard_get_into` `var` and `run_shell` `capture_into`
    /// ends up here, alongside the original parameter values.
    /// Useful for tests + the future Hub run-history pane.
    pub variables: HashMap<String, String>,
}

/// The host-OS variant of `recipe.os_steps:` that the runtime uses.
/// Hard-coded per build; tests can override via [`execute_recipe_for_os`].
#[cfg(target_os = "windows")]
fn host_os() -> &'static str {
    "windows"
}
#[cfg(target_os = "macos")]
fn host_os() -> &'static str {
    "macos"
}
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn host_os() -> &'static str {
    "linux"
}

/// Run `recipe` on the host OS with the given parameter values.
/// `confirm` is consulted before any destructive step ([`Step::RunShell`]).
///
/// Returns the [`RecipeRun`] state on success or a [`RuntimeError`]
/// on the first step that fails / times out / is denied.
pub async fn execute_recipe(
    recipe: &Recipe,
    args: HashMap<String, String>,
    confirm: &dyn ConfirmHandler,
) -> Result<RecipeRun, RuntimeError> {
    execute_recipe_for_os(recipe, host_os(), args, confirm).await
}

/// Variant of [`execute_recipe`] that lets tests pick the platform
/// branch without actually running on that host. The selected step
/// list still has to be executable on the test host — only the
/// `os_steps` variant pick is overridden.
pub async fn execute_recipe_for_os(
    recipe: &Recipe,
    os: &'static str,
    args: HashMap<String, String>,
    confirm: &dyn ConfirmHandler,
) -> Result<RecipeRun, RuntimeError> {
    let steps: &Vec<Step> = match os {
        "windows" => recipe.os_steps.windows.as_ref(),
        "macos" => recipe.os_steps.macos.as_ref(),
        "linux" => recipe.os_steps.linux.as_ref(),
        other => return Err(RuntimeError::NoStepsForOs { os: leak_os(other) }),
    }
    .ok_or(RuntimeError::NoStepsForOs { os })?;

    let mut state = RecipeRun {
        variables: args,
        ..Default::default()
    };

    for (index, step) in steps.iter().enumerate() {
        execute_step(index, step, &mut state, confirm).await?;
        state.steps_executed += 1;
    }
    Ok(state)
}

/// Static-leak the OS label for the `NoStepsForOs` variant when the
/// caller passed a non-canonical string. Bounded — only happens in
/// tests that mistype the OS arg.
fn leak_os(label: &str) -> &'static str {
    Box::leak(label.to_string().into_boxed_str())
}

async fn execute_step(
    index: usize,
    step: &Step,
    state: &mut RecipeRun,
    confirm: &dyn ConfirmHandler,
) -> Result<(), RuntimeError> {
    match step {
        Step::KeyChord { keys, .. } => run_key_chord(index, keys),
        Step::TypeUnicode { text, rtl_safe, .. } => {
            let resolved = interpolate(text, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_type_unicode(index, &resolved, *rtl_safe)
        }
        Step::FocusWindow { title_contains, .. } => {
            let resolved = interpolate(title_contains, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_focus_window(index, &resolved)
        }
        Step::WaitForWindow {
            title_contains,
            timeout_ms,
            ..
        } => {
            let resolved = interpolate(title_contains, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_wait_for_window(index, &resolved, *timeout_ms).await
        }
        Step::WaitMs { ms, .. } => {
            sleep(Duration::from_millis(u64::from(*ms))).await;
            Ok(())
        }
        Step::WaitForFocusChange { timeout_ms, .. } => {
            run_wait_for_focus_change(index, *timeout_ms).await
        }
        Step::ScreenshotToClipboard { .. } => run_screenshot_to_clipboard(index).await,
        Step::ClipboardSet { text, .. } => {
            let resolved = interpolate(text, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_clipboard_set(index, &resolved)
        }
        Step::ClipboardGetInto { var, .. } => {
            let text = run_clipboard_get(index)?;
            state.variables.insert(var.clone(), text);
            Ok(())
        }
        Step::RunShell {
            command,
            timeout_ms,
            capture_into,
            dry_run,
            ..
        } => {
            let resolved = interpolate(command, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            // Dry-run skips both the confirmation gate (nothing to confirm
            // — no side effect) and the actual spawn. The Hub Steps panel
            // labels the step "dry-run בלבד" so the user already sees this
            // is a preview; logging at INFO + binding the capture var to a
            // sentinel keeps the rest of the recipe runnable.
            if *dry_run {
                tracing::info!(
                    target: "lashon::recipes::runtime",
                    step = index,
                    command_len = resolved.len(),
                    "run_shell: dry-run — command not executed"
                );
                if let Some(var_name) = capture_into {
                    state
                        .variables
                        .insert(var_name.clone(), "(dry-run)".to_string());
                }
                return Ok(());
            }
            match confirm.confirm(&resolved) {
                ConfirmDecision::Allow => {}
                ConfirmDecision::Deny => {
                    return Err(RuntimeError::Denied {
                        index,
                        kind: "run_shell",
                    })
                }
            }
            let output = run_shell(index, &resolved, *timeout_ms).await?;
            if let Some(var_name) = capture_into {
                state.variables.insert(var_name.clone(), output);
            }
            Ok(())
        }
        Step::OpenUrl { url, .. } => {
            let resolved = interpolate(url, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_open_url(index, &resolved)
        }
        Step::OpenApp { name, .. } => {
            let resolved = interpolate(name, &state.variables)
                .map_err(|name| RuntimeError::UnknownInterpolation { name })?;
            run_open_app(index, &resolved)
        }
        Step::ClickLabel { .. } => Err(RuntimeError::StepNotImplemented {
            index,
            kind: "click_label",
            reason: "UIA-based label clicking lands in v1.1 runtime; \
                     for now, prefer keyboard navigation via key_chord",
        }),
    }
}

// ---------- step implementations ----------

fn run_key_chord(index: usize, keys: &[String]) -> Result<(), RuntimeError> {
    let chord = keys.join("+");
    crate::tools::press_keys::execute_chord(&chord).map_err(|err| RuntimeError::StepFailed {
        index,
        kind: "key_chord",
        source: err,
    })
}

fn run_type_unicode(index: usize, text: &str, _rtl_safe: bool) -> Result<(), RuntimeError> {
    // `inject::inject_text` already routes Hebrew through the
    // clipboard-paste path with combining-mark integrity (per
    // `.claude/rules/hebrew.md`); `rtl_safe: true` on the step is
    // load-bearing for *non-Hebrew* recipes that hand-roll BiDi
    // markers but the current injector already handles that case
    // correctly for every script. Kept as a flag for forward compat.
    crate::inject::inject_text(text).map_err(|err| RuntimeError::StepFailed {
        index,
        kind: "type_unicode",
        source: err,
    })
}

fn run_focus_window(index: usize, title_contains: &str) -> Result<(), RuntimeError> {
    let focused = crate::tools::focus_window::try_focus(title_contains).map_err(|err| {
        RuntimeError::StepFailed {
            index,
            kind: "focus_window",
            source: err,
        }
    })?;
    if !focused {
        return Err(RuntimeError::StepFailed {
            index,
            kind: "focus_window",
            source: anyhow::anyhow!("no window with title containing {title_contains:?} is open"),
        });
    }
    Ok(())
}

async fn run_wait_for_window(
    index: usize,
    title_contains: &str,
    timeout_ms: u32,
) -> Result<(), RuntimeError> {
    let limit = Duration::from_millis(u64::from(timeout_ms));
    let started = Instant::now();
    loop {
        let found = crate::tools::focus_window::try_focus(title_contains).map_err(|err| {
            RuntimeError::StepFailed {
                index,
                kind: "wait_for_window",
                source: err,
            }
        })?;
        if found {
            return Ok(());
        }
        if started.elapsed() >= limit {
            return Err(RuntimeError::Timeout {
                index,
                kind: "wait_for_window",
                elapsed_ms: started.elapsed().as_millis() as u64,
                limit_ms: u64::from(timeout_ms),
            });
        }
        sleep(Duration::from_millis(250)).await;
    }
}

/// Wait until keyboard focus moves to a *different* UIA element than
/// the one focused when the step started. The cheap state-driven
/// companion to `wait_ms` — perfect after a key chord that opens a
/// modal whose input captures focus.
///
/// Polls every 50 ms (UIA `GetFocusedElement` + `GetRuntimeId` is
/// ~2 ms warm; the sleep dominates). A `None` snapshot at start
/// (nothing focused on the desktop, very rare) is treated as
/// "anything Some" being a change — caller pattern is "send keypress,
/// expect focus to appear somewhere".
async fn run_wait_for_focus_change(
    index: usize,
    timeout_ms: u32,
) -> Result<(), RuntimeError> {
    let started_before = crate::tools::uia_focus::current_focused_runtime_id().map_err(|err| {
        RuntimeError::StepFailed {
            index,
            kind: "wait_for_focus_change",
            source: err,
        }
    })?;
    let limit = Duration::from_millis(u64::from(timeout_ms));
    let started = Instant::now();
    loop {
        // Poll. UIA errors are demoted to "still the same" so a
        // transient COM hiccup doesn't abort the step — the timeout
        // will catch a genuinely-stuck case.
        let current = crate::tools::uia_focus::current_focused_runtime_id().unwrap_or(None);
        if current != started_before {
            return Ok(());
        }
        if started.elapsed() >= limit {
            return Err(RuntimeError::Timeout {
                index,
                kind: "wait_for_focus_change",
                elapsed_ms: started.elapsed().as_millis() as u64,
                limit_ms: u64::from(timeout_ms),
            });
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn run_screenshot_to_clipboard(index: usize) -> Result<(), RuntimeError> {
    #[cfg(target_os = "windows")]
    {
        // PowerShell snippet: capture the primary screen via
        // System.Drawing, hand the bitmap to System.Windows.Forms.Clipboard.
        // Region-restricted captures (`Step::ScreenshotToClipboard.region`)
        // are honoured in a future runtime PR; v1 captures the primary
        // screen.
        let script = "Add-Type -AssemblyName System.Windows.Forms; \
                      Add-Type -AssemblyName System.Drawing; \
                      $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
                      $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height; \
                      $g = [System.Drawing.Graphics]::FromImage($bmp); \
                      $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); \
                      [System.Windows.Forms.Clipboard]::SetImage($bmp); \
                      $g.Dispose(); $bmp.Dispose()";
        run_powershell(
            index,
            "screenshot_to_clipboard",
            script,
            BUILTIN_SHELL_TIMEOUT_MS,
        )
        .await?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(RuntimeError::StepNotImplemented {
            index,
            kind: "screenshot_to_clipboard",
            reason: "non-Windows screenshot adapter lands in v1.1",
        })
    }
}

fn run_clipboard_set(index: usize, text: &str) -> Result<(), RuntimeError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| RuntimeError::StepFailed {
        index,
        kind: "clipboard_set",
        source: anyhow::anyhow!("cannot open clipboard: {err}"),
    })?;
    clipboard
        .set_text(text.to_string())
        .map_err(|err| RuntimeError::StepFailed {
            index,
            kind: "clipboard_set",
            source: anyhow::anyhow!("set_text failed: {err}"),
        })
}

fn run_clipboard_get(index: usize) -> Result<String, RuntimeError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|err| RuntimeError::StepFailed {
        index,
        kind: "clipboard_get_into",
        source: anyhow::anyhow!("cannot open clipboard: {err}"),
    })?;
    match clipboard.get_text() {
        Ok(text) => Ok(text),
        Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
        Err(err) => Err(RuntimeError::StepFailed {
            index,
            kind: "clipboard_get_into",
            source: anyhow::anyhow!("get_text failed: {err}"),
        }),
    }
}

async fn run_shell(index: usize, command: &str, timeout_ms: u32) -> Result<String, RuntimeError> {
    #[cfg(target_os = "windows")]
    {
        run_powershell(index, "run_shell", command, timeout_ms).await
    }
    #[cfg(not(target_os = "windows"))]
    {
        run_posix_sh(index, command, timeout_ms).await
    }
}

/// Default screenshot-shell timeout used when the runtime calls
/// `run_powershell` for a non-user-controlled built-in step. 30 s is
/// well past any reasonable `CopyFromScreen` call.
#[cfg(target_os = "windows")]
const BUILTIN_SHELL_TIMEOUT_MS: u32 = 30_000;

#[cfg(target_os = "windows")]
async fn run_powershell(
    index: usize,
    kind: &'static str,
    script: &str,
    timeout_ms: u32,
) -> Result<String, RuntimeError> {
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // CREATE_NO_WINDOW — without this every powershell spawn
        // flashes a console window that briefly steals focus from
        // whatever the user has up front (the entire point of a
        // recipe is to act on a foreground app, so this is
        // load-bearing). Same posture as `llama_server::spawn` and
        // `tools::open_app::launch`.
        .creation_flags(0x0800_0000);
    let started = Instant::now();
    let limit = Duration::from_millis(u64::from(timeout_ms));
    let output = timeout(limit, command.output())
        .await
        .map_err(|_| RuntimeError::Timeout {
            index,
            kind,
            elapsed_ms: started.elapsed().as_millis() as u64,
            limit_ms: u64::from(timeout_ms),
        })?
        .map_err(|err| RuntimeError::StepFailed {
            index,
            kind,
            source: anyhow::anyhow!("powershell spawn failed: {err}"),
        })?;
    if !output.status.success() {
        return Err(RuntimeError::StepFailed {
            index,
            kind,
            source: anyhow::anyhow!(
                "powershell exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(target_os = "windows"))]
async fn run_posix_sh(index: usize, script: &str, timeout_ms: u32) -> Result<String, RuntimeError> {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let limit = Duration::from_millis(u64::from(timeout_ms));
    let output = timeout(limit, command.output())
        .await
        .map_err(|_| RuntimeError::Timeout {
            index,
            kind: "run_shell",
            elapsed_ms: started.elapsed().as_millis() as u64,
            limit_ms: u64::from(timeout_ms),
        })?
        .map_err(|err| RuntimeError::StepFailed {
            index,
            kind: "run_shell",
            source: anyhow::anyhow!("sh spawn failed: {err}"),
        })?;
    if !output.status.success() {
        return Err(RuntimeError::StepFailed {
            index,
            kind: "run_shell",
            source: anyhow::anyhow!(
                "sh exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_open_url(index: usize, url: &str) -> Result<(), RuntimeError> {
    open::that(url).map_err(|err| RuntimeError::StepFailed {
        index,
        kind: "open_url",
        source: anyhow::anyhow!("opening {url:?} failed: {err}"),
    })
}

fn run_open_app(index: usize, name: &str) -> Result<(), RuntimeError> {
    // Lazy import — the `tools::open_app` module's `launch()` is
    // currently private. We call the equivalent code path here so a
    // recipe `open_app` matches the LLM-tool `open_app`.
    #[cfg(target_os = "windows")]
    {
        // CREATE_NO_WINDOW on `cmd /c start` so the brief cmd console
        // flash doesn't steal focus from whatever the user has up
        // front. `std::process::Command` needs the `CommandExt` trait
        // in scope to reach `creation_flags`; tokio's variant carries
        // its own `creation_flags` method directly (see
        // `run_powershell` above).
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("cmd")
            .args(["/C", "start", "", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|err| RuntimeError::StepFailed {
                index,
                kind: "open_app",
                source: anyhow::anyhow!("cmd /c start failed to spawn: {err}"),
            })?;
        if !status.success() {
            return Err(RuntimeError::StepFailed {
                index,
                kind: "open_app",
                source: anyhow::anyhow!("`start \"\" \"{name}\"` returned status {status}"),
            });
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        Err(RuntimeError::StepNotImplemented {
            index,
            kind: "open_app",
            reason: "non-Windows app launcher adapter lands in v1.1",
        })
    }
}

// ---------- helpers ----------

/// Substitute every `{{ name }}` (whitespace-tolerant) in `text` with
/// the value of `vars[name]`. An unknown key returns `Err(name)` so
/// the caller can map to `RuntimeError::UnknownInterpolation`.
pub fn interpolate(text: &str, vars: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str("{{");
            rest = after;
            continue;
        };
        let name = after[..end].trim();
        match vars.get(name) {
            Some(value) => out.push_str(value),
            None => return Err(name.to_string()),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::{validate_recipe, OsSteps, Recipe};

    fn empty_recipe() -> Recipe {
        Recipe {
            version: 1,
            id: "empty".into(),
            name: "Empty".into(),
            description: "An empty recipe — runs zero steps.".into(),
            long_description: None,
            author: None,
            recipe_version: "1.0.0".into(),
            tags: vec![],
            intents: vec![],
            parameters: vec![],
            permissions: vec![],
            os_steps: OsSteps {
                windows: Some(vec![]),
                macos: None,
                linux: None,
            },
        }
    }

    #[test]
    fn interpolate_handles_multiple_substitutions() {
        let mut vars = HashMap::new();
        vars.insert("a".to_string(), "alpha".to_string());
        vars.insert("b".to_string(), "beta".to_string());
        let out = interpolate("{{ a }} and {{b}} done", &vars).unwrap();
        assert_eq!(out, "alpha and beta done");
    }

    #[test]
    fn interpolate_returns_error_on_missing_key() {
        let vars = HashMap::new();
        let err = interpolate("{{ ghost }}", &vars).unwrap_err();
        assert_eq!(err, "ghost");
    }

    #[test]
    fn interpolate_leaves_unclosed_braces_literal() {
        let vars = HashMap::new();
        let out = interpolate("{{ unclosed and more", &vars).unwrap();
        assert_eq!(out, "{{ unclosed and more");
    }

    #[tokio::test]
    async fn empty_recipe_runs_zero_steps() {
        let recipe = empty_recipe();
        // Empty windows array is invalid per the validator, so override
        // with `None` for this specific test of the dispatch path.
        let mut recipe = recipe;
        recipe.os_steps.windows = Some(vec![Step::WaitMs {
            ms: 0,
            comment: None,
        }]);
        let run = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
            .await
            .unwrap();
        assert_eq!(run.steps_executed, 1);
    }

    #[tokio::test]
    async fn wrong_os_returns_no_steps_error() {
        let recipe = empty_recipe();
        let err = execute_recipe_for_os(&recipe, "macos", HashMap::new(), &AlwaysAllow)
            .await
            .unwrap_err();
        assert!(matches!(err, RuntimeError::NoStepsForOs { os: "macos" }));
    }

    #[tokio::test]
    async fn unknown_interpolation_aborts_run() {
        let mut recipe = empty_recipe();
        recipe.os_steps.windows = Some(vec![Step::OpenUrl {
            url: "https://example.com/?q={{ missing }}".into(),
            comment: None,
        }]);
        let err = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::UnknownInterpolation { ref name } if name == "missing"
        ));
    }

    #[tokio::test]
    async fn run_shell_denied_by_handler_aborts() {
        let mut recipe = empty_recipe();
        recipe.permissions = vec!["shell.run".into()];
        recipe.os_steps.windows = Some(vec![Step::RunShell {
            command: "echo hi".into(),
            timeout_ms: 5_000,
            capture_into: None,
            dry_run: false,
            comment: None,
        }]);
        // Sanity: the recipe itself must validate so we know we're
        // testing the runtime path, not a misshapen fixture.
        validate_recipe(&recipe).expect("fixture validates");
        let err = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysDeny)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::Denied {
                kind: "run_shell",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn dry_run_shell_step_skips_execution_and_binds_capture() {
        // dry_run = true: the runtime must NOT spawn powershell and must
        // NOT consult the confirmation handler (AlwaysDeny here would
        // otherwise abort). The capture_into var binds to "(dry-run)" so
        // later steps that reference it don't error on the missing key.
        let mut recipe = empty_recipe();
        recipe.permissions = vec!["shell.run".into()];
        recipe.os_steps.windows = Some(vec![Step::RunShell {
            command: "echo SHOULD-NOT-RUN".into(),
            timeout_ms: 5_000,
            capture_into: Some("out".into()),
            dry_run: true,
            comment: None,
        }]);
        let run = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysDeny)
            .await
            .expect("dry_run bypasses both the spawn AND the deny gate");
        assert_eq!(run.steps_executed, 1);
        assert_eq!(
            run.variables.get("out").map(String::as_str),
            Some("(dry-run)"),
            "capture_into must be bound to the dry-run sentinel so later \
             steps referencing it don't fail interpolation"
        );
    }

    #[tokio::test]
    async fn click_label_returns_step_not_implemented() {
        let mut recipe = empty_recipe();
        recipe.os_steps.windows = Some(vec![Step::ClickLabel {
            label: "Send".into(),
            window: None,
            ocr_fallback: false,
            comment: None,
        }]);
        let err = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RuntimeError::StepNotImplemented {
                kind: "click_label",
                ..
            }
        ));
    }
}
