//! `run_command` — shell-escape hatch. PowerShell on Windows
//! (`powershell.exe -NoProfile -Command <cmd>`), `/bin/sh -c <cmd>`
//! elsewhere. The output is capped, the timeout is mandatory, and the
//! confirmation modal renders the full literal command before
//! anything runs.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::process::Command;
use tokio::time::timeout;

use super::path_safety::resolve_safe_path;
use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Default timeout — 30 seconds. Most shell commands the user wants
/// (`npm install` aside) finish well under this; longer-running tools
/// should be invoked through `open_app` instead.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// Hard cap — 5 minutes. No `--no-timeout` escape; the LLM cannot bypass
/// this.
const MAX_TIMEOUT_MS: u64 = 5 * 60 * 1000;
/// Output cap. Anything longer is tail-truncated with a marker. Keeps a
/// `cargo build` log from blowing the LLM's context window.
const MAX_OUTPUT_BYTES: usize = 4096;

pub struct RunCommand;

impl RunCommand {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RunCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command and return its combined stdout/stderr output. \
         Uses PowerShell on Windows (`powershell.exe -NoProfile -Command`) \
         and `/bin/sh -c` on macOS/Linux. The default timeout is 30 s \
         (capped at 5 min) — the child process is killed if it overruns. \
         Output is capped at 4 KB (tail truncated). A non-zero exit code \
         is returned as an error result. The optional `cwd` must resolve \
         under the user's home directory or the OS temp folder. Requires \
         user confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command line. Quoting follows the host shell's rules."
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory. Defaults to the user's home directory; must resolve under home or temp if provided."
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 100,
                    "maximum": MAX_TIMEOUT_MS,
                    "description": "Override the default 30 000 ms timeout. Capped at 300 000 ms (5 min)."
                }
            },
            "required": ["command"]
        })
    }

    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let command = args
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("run_command: missing required `command` argument"))?;
            if command.trim().is_empty() {
                return Err(anyhow!("run_command: `command` must not be empty"));
            }
            let cwd = args.get("cwd").and_then(|v| v.as_str());
            let resolved_cwd = match cwd {
                Some(c) if !c.trim().is_empty() => match resolve_safe_path(c) {
                    Ok(p) => {
                        if !p.is_dir() {
                            return Ok(ToolResult::error(format!(
                                "run_command: cwd `{}` is not a directory",
                                p.display()
                            )));
                        }
                        Some(p)
                    }
                    Err(e) => {
                        return Ok(ToolResult::error(format!("run_command: cwd: {e}")));
                    }
                },
                _ => None,
            };
            let timeout_ms = args
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .min(MAX_TIMEOUT_MS);

            let mut cmd = build_command(command);
            if let Some(cwd) = resolved_cwd.as_ref() {
                cmd.current_dir(cwd);
            }
            cmd.stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null());

            let child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    return Ok(ToolResult::error(format!("run_command: spawn failed: {e}")));
                }
            };
            let wait = child.wait_with_output();
            match timeout(Duration::from_millis(timeout_ms), wait).await {
                Ok(Ok(output)) => {
                    let combined = combine_output(&output.stdout, &output.stderr);
                    let trimmed = truncate(&combined);
                    if !output.status.success() {
                        let code = output.status.code().unwrap_or(-1);
                        return Ok(ToolResult::error(format!(
                            "run_command: exit code {code}\n{trimmed}"
                        )));
                    }
                    Ok(ToolResult {
                        content: if trimmed.is_empty() {
                            "(no output)".to_string()
                        } else {
                            trimmed
                        },
                        display_summary: Some("הרצתי פקודה".into()),
                    })
                }
                Ok(Err(e)) => Ok(ToolResult::error(format!("run_command: wait failed: {e}"))),
                Err(_) => {
                    // Tokio's wait_with_output consumed the child; the
                    // `Elapsed` branch here means the OS process is no
                    // longer ours to kill via the handle. tokio's spawn
                    // however sets `kill_on_drop(true)` by default for
                    // Command::new built via tokio::process — dropping
                    // the JoinSet drops the future, which sends SIGKILL.
                    // The error message preserves the timeout signal so
                    // the LLM can pick a different approach.
                    Ok(ToolResult::error(format!(
                        "run_command: timed out after {timeout_ms} ms"
                    )))
                }
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn build_command(line: &str) -> Command {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", line]);
    cmd.kill_on_drop(true);
    // CREATE_NO_WINDOW — same focus-steal fix as the recipe runtime
    // (`recipes::runtime::run_powershell`) and the llama-server +
    // STT sidecar spawn paths. Without this, every Command-mode
    // `run_command` invocation flashes a powershell console window
    // that steals focus from the user's foreground app — the exact
    // app the LLM is usually trying to act on.
    cmd.creation_flags(0x0800_0000);
    cmd
}

#[cfg(not(target_os = "windows"))]
fn build_command(line: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", line]);
    cmd.kill_on_drop(true);
    cmd
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let mut s = String::new();
    s.push_str(&String::from_utf8_lossy(stdout));
    if !stderr.is_empty() {
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(&String::from_utf8_lossy(stderr));
    }
    s
}

fn truncate(text: &str) -> String {
    let bytes = text.as_bytes();
    if bytes.len() <= MAX_OUTPUT_BYTES {
        return text.to_string();
    }
    // UTF-8-safe tail: walk forward from the cut point until we land on
    // a byte that isn't a continuation byte. This way we never split a
    // codepoint and produce invalid UTF-8.
    let start = bytes.len() - MAX_OUTPUT_BYTES;
    let mut start = start;
    while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
        start += 1;
    }
    let tail = String::from_utf8_lossy(&bytes[start..]).into_owned();
    format!(
        "(truncated; showing the last {} of {} bytes)\n{tail}",
        bytes.len() - start,
        bytes.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(RunCommand.name(), "run_command");
        assert!(RunCommand.requires_confirmation(&json!({"command": "ls"})));
    }

    #[test]
    fn parameters_describe_command_and_timeout() {
        let p = RunCommand.parameters();
        assert_eq!(p["properties"]["command"]["type"], "string");
        assert_eq!(p["properties"]["timeout_ms"]["type"], "integer");
        assert_eq!(p["required"][0], "command");
    }

    #[test]
    fn missing_command_argument_errors() {
        let err = rt()
            .block_on(RunCommand.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("command"));
    }

    #[test]
    fn empty_command_errors() {
        let err = rt()
            .block_on(RunCommand.execute(&json!({"command": "   "})))
            .err()
            .expect("blank command must error");
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn refuses_cwd_outside_allowed_roots() {
        let bad = if cfg!(target_os = "windows") {
            r"C:\Windows\System32"
        } else {
            "/etc"
        };
        let result = rt()
            .block_on(RunCommand.execute(&json!({"command": "echo hi", "cwd": bad})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("outside the allowed roots"));
    }

    #[test]
    fn runs_echo_and_captures_output() {
        // The literal command varies by shell — on Windows we want a
        // PowerShell-shaped one, on Unix a sh-shaped one. Both `echo`
        // commands print "lashon-run-command-test" to stdout.
        let cmd = if cfg!(target_os = "windows") {
            "Write-Output lashon-run-command-test"
        } else {
            "echo lashon-run-command-test"
        };
        let result = rt()
            .block_on(RunCommand.execute(&json!({"command": cmd})))
            .unwrap();
        assert!(
            result.content.contains("lashon-run-command-test"),
            "{}",
            result.content
        );
    }

    #[test]
    fn surface_non_zero_exit_as_error() {
        let cmd = if cfg!(target_os = "windows") {
            "exit 7"
        } else {
            "exit 7"
        };
        let result = rt()
            .block_on(RunCommand.execute(&json!({"command": cmd})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        assert!(result.content.contains("exit code"));
    }

    #[test]
    fn truncate_keeps_utf8_boundary() {
        // Hebrew chars are 2 bytes in UTF-8; build a payload bigger than
        // the cap and verify the tail is still valid UTF-8 (no panic on
        // String::from_utf8_lossy turning it into replacement chars).
        let payload: String = "ש".repeat(MAX_OUTPUT_BYTES);
        let truncated = truncate(&payload);
        assert!(truncated.starts_with("(truncated"));
        // The tail must be valid UTF-8 — round-trip through String.
        let tail = truncated.split_once('\n').unwrap().1;
        assert!(tail.chars().next().is_some());
    }
}
