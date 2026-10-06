//! Codex user hooks.json. Reuses the byte-preserving JSON editor and safe file replacement.
use super::claude_settings::{self, Found, Plan, SettingsError};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const HOOK_STEM: &str = "ottid-codex-hook";

pub fn user_settings_path() -> Option<PathBuf> {
    let configured = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty());
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    configured
        .map(PathBuf::from)
        .or_else(|| home.map(|v| PathBuf::from(v).join(".codex")))
        .map(|dir| dir.join("hooks.json"))
}

pub fn command(exe: &str) -> String {
    format!("'{}'", exe.replace('\'', "'\\''"))
}

pub fn windows_command(exe: &str) -> String {
    // Codex uses a shell. EncodedCommand keeps metacharacters in the binary path
    // out of the outer shell; the single-quoted PowerShell literal never expands.
    // Forward stdin as UTF-8, including Hebrew, rather than PowerShell's legacy ASCII.
    let script = format!("$OutputEncoding = [System.Text.UTF8Encoding]::new($false); [Console]::In.ReadToEnd() | & '{}'", exe.replace('\'', "''"));
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    )
}

pub fn entry(exe: &str) -> Value {
    json!({"matcher":"*", "hooks":[{
        "type":"command", "command":command(exe), "commandWindows":windows_command(exe),
        "timeout":super::HOOK_TIMEOUT_SECS, "statusMessage":claude_settings::STATUS_MESSAGE
    }]})
}

pub(super) fn is_ours(handler: &Value) -> bool {
    if handler.get("type").and_then(Value::as_str) != Some("command") {
        return false;
    }
    let Some(cmd) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    let Some(quoted) = cmd.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) else {
        return false;
    };
    let exe = quoted.replace("'\\''", "'");
    let name = exe.rsplit(['/', '\\']).next().unwrap_or("");
    if name != HOOK_STEM && !name.eq_ignore_ascii_case("ottid-codex-hook.exe") {
        return false;
    }
    cmd == command(&exe)
        && handler.get("commandWindows").and_then(Value::as_str)
            == Some(windows_command(&exe).as_str())
}

pub fn find(current: Option<&str>) -> Result<Found, SettingsError> {
    claude_settings::find_with(current, is_ours)
}
pub fn plan_install(current: Option<&str>, exe: &str) -> Result<Plan, SettingsError> {
    let wanted = entry(exe);
    let text = serde_json::to_string_pretty(&wanted).unwrap_or_default();
    claude_settings::plan_install_with(current, wanted, &text, is_ours)
}
pub fn plan_uninstall(current: Option<&str>) -> Result<Plan, SettingsError> {
    claude_settings::plan_uninstall_with(current, is_ours)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_is_idempotent_and_uninstall_keeps_other_hooks_and_metadata() {
        let original = "{\"description\": \"עברית\", \"hooks\": {\"Stop\": [{\"hooks\": [{\"type\":\"command\",\"command\":\"echo done\"}]}]}}\n";
        let plan = plan_install(
            Some(original),
            "C:\\Program Files\\Ottid\\ottid-codex-hook.exe",
        )
        .unwrap();
        assert_eq!(find(Some(&plan.text)).unwrap().commands.len(), 1);
        assert!(
            !plan_install(
                Some(&plan.text),
                "C:\\Program Files\\Ottid\\ottid-codex-hook.exe"
            )
            .unwrap()
            .changes
        );
        let undone = plan_uninstall(Some(&plan.text)).unwrap();
        let value: Value = serde_json::from_str(&undone.text).unwrap();
        assert_eq!(value, serde_json::from_str::<Value>(original).unwrap());
        assert!(undone.text.contains("\"description\": \"עברית\""));
    }
    #[test]
    fn generated_paths_with_shell_characters_are_recognized_exactly() {
        for exe in [
            "C:\\User's $data & files\\ottid-codex-hook.exe",
            "/home/a'b/$data/ottid-codex-hook",
        ] {
            let handler = entry(exe)["hooks"][0].clone();
            assert!(is_ours(&handler));
            let mut altered = handler;
            altered["commandWindows"] = json!("echo unrelated");
            assert!(!is_ours(&altered));
        }
    }
}
