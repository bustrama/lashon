//! `list_processes` — top processes by CPU, via `sysinfo` (already a
//! Phase-1 dep for hardware tier detection). Used by the LLM as a
//! prelude to `kill_process` when the user says "kill the process
//! that's pegging my CPU".

use anyhow::Result;
use serde_json::{json, Value};
use sysinfo::System;

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

const MAX_ROWS: usize = 50;

pub struct ListProcesses;

impl ListProcesses {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ListProcesses {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for ListProcesses {
    fn name(&self) -> &str {
        "list_processes"
    }

    fn description(&self) -> &str {
        "List the top 50 running processes ordered by CPU usage. Each \
         row is `<pid> <name> <cpu%> <ram_mb>`. Use to identify a \
         runaway process before calling `kill_process`."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            // Two-stage refresh: sysinfo's CPU% is the delta between
            // consecutive reads, so a single snapshot returns 0% for
            // every process. The second `refresh_all` after a short
            // sleep populates real values.
            let mut sys = System::new_all();
            sys.refresh_all();
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            sys.refresh_all();

            let mut rows: Vec<(u32, String, f32, u64)> = sys
                .processes()
                .iter()
                .map(|(pid, proc)| {
                    let name = proc.name().to_string_lossy().into_owned();
                    let cpu = proc.cpu_usage();
                    let ram_mb = proc.memory() / 1024 / 1024;
                    (pid.as_u32(), name, cpu, ram_mb)
                })
                .collect();
            // Sort by CPU descending; stable so equal-CPU processes
            // retain pid order.
            rows.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            rows.truncate(MAX_ROWS);

            let body = rows
                .iter()
                .map(|(pid, name, cpu, ram)| format!("{pid:>6} {name:<30} {cpu:>5.1}% {ram:>5} MB"))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(ToolResult {
                content: if body.is_empty() {
                    "(no processes)".to_string()
                } else {
                    body
                },
                display_summary: Some(format!("{} תהליכים", rows.len())),
            })
        })
    }
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
        assert_eq!(ListProcesses.name(), "list_processes");
        assert!(!ListProcesses.requires_confirmation(&json!({})));
    }

    #[test]
    fn returns_a_non_empty_listing() {
        let result = rt().block_on(ListProcesses.execute(&json!({}))).unwrap();
        // Every host with a running test process has at least one — itself.
        assert!(!result.content.is_empty());
        assert!(!result.content.starts_with("error:"));
    }
}
