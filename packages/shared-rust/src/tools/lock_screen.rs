//! `lock_screen` — Win32 `LockWorkStation`. Recoverable (the user can
//! unlock with their password) but disruptive enough to gate on a
//! confirmation modal. Out-of-scope: actual sign-out / shutdown — those
//! land in a future PR.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct LockScreen;

impl LockScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LockScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for LockScreen {
    fn name(&self) -> &str {
        "lock_screen"
    }

    fn description(&self) -> &str {
        "Lock the workstation so the user must re-enter their password. \
         Equivalent to Win+L on Windows. Recoverable but disruptive — \
         the user will be locked out of every running app until they \
         unlock. Requires user confirmation."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn requires_confirmation(&self, _args: &Value) -> bool {
        true
    }

    fn execute<'a>(&'a self, _args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            match lock() {
                Ok(()) => Ok(ToolResult {
                    content: "locked the workstation".to_string(),
                    display_summary: Some("נעלתי את המסך".into()),
                }),
                Err(e) => Ok(ToolResult::error(format!("lock_screen: {e}"))),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn lock() -> Result<()> {
    use windows::Win32::System::Shutdown::LockWorkStation;
    unsafe {
        LockWorkStation().map_err(|e| anyhow!("LockWorkStation: {e}"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn lock() -> Result<()> {
    Err(anyhow!(
        "lock_screen: not yet implemented on this OS — Windows-only in M8.2"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(LockScreen.name(), "lock_screen");
        assert!(LockScreen.requires_confirmation(&json!({})));
    }

    #[test]
    fn parameters_are_object_with_no_required_fields() {
        let p = LockScreen.parameters();
        assert_eq!(p["type"], "object");
        assert!(p["properties"].as_object().unwrap().is_empty());
    }

    // We don't actually invoke `lock` in unit tests — it would lock the
    // CI runner's screen and the test would never complete. The trait
    // shape is what we verify.
}
