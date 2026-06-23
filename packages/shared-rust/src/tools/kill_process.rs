//! `kill_process` — destructive. Sends `TerminateProcess` on Windows,
//! `SIGKILL` on Unix. The PID is the model's responsibility; it should
//! have run `list_processes` first.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct KillProcess;

impl KillProcess {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KillProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for KillProcess {
    fn name(&self) -> &str {
        "kill_process"
    }

    fn description(&self) -> &str {
        "Terminate a process by PID. On Windows this is \
         `TerminateProcess`; on Unix it is `SIGKILL`. The process gets \
         no chance to save state — pair with `list_processes` first so \
         the user is killing the right thing. Requires user \
         confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pid": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Process id from `list_processes`."
                }
            },
            "required": ["pid"]
        })
    }

    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let pid = args
                .get("pid")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow!("kill_process: missing required `pid` argument"))?;
            if pid == 0 {
                return Ok(ToolResult::error(
                    "kill_process: pid must be > 0".to_string(),
                ));
            }
            match terminate(pid as u32) {
                Ok(()) => Ok(ToolResult {
                    content: format!("killed pid {pid}"),
                    display_summary: Some(format!("הרגתי תהליך {pid}")),
                }),
                Err(e) => Ok(ToolResult::error(format!("kill_process: pid {pid}: {e}"))),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn terminate(pid: u32) -> Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| anyhow!("OpenProcess failed: {e}"))?;
        let res = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        res.map_err(|e| anyhow!("TerminateProcess failed: {e}"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn terminate(pid: u32) -> Result<()> {
    use std::process::Command;
    let status = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .map_err(|e| anyhow!("spawn kill: {e}"))?;
    if !status.success() {
        return Err(anyhow!(
            "kill exited with code {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
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
        assert_eq!(KillProcess.name(), "kill_process");
        assert!(KillProcess.requires_confirmation(&json!({"pid": 1})));
    }

    #[test]
    fn parameters_describe_pid() {
        let p = KillProcess.parameters();
        assert_eq!(p["properties"]["pid"]["type"], "integer");
        assert_eq!(p["required"][0], "pid");
    }

    #[test]
    fn missing_pid_argument_errors() {
        let err = rt()
            .block_on(KillProcess.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("pid"));
    }

    #[test]
    fn pid_zero_rejected() {
        let result = rt()
            .block_on(KillProcess.execute(&json!({"pid": 0})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
    }
}
