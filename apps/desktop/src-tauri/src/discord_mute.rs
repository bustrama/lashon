use ottid_core::discord_mute::Lease;
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;

#[tauri::command]
pub fn discord_mute_supported() -> bool {
    cfg!(windows)
}

pub fn before_take(app: &AppHandle) -> Result<Option<Lease>, String> {
    if !cfg!(windows) {
        return Ok(None);
    }
    let settings = app
        .store("settings.json")
        .map_err(|_| "settings-unavailable")?;
    if !settings
        .get("discordMute.enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Ok(None);
    }
    if !settings
        .get("discordMute.verified")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err("discord-not-verified".into());
    }
    Lease::acquire()
        .map(Some)
        .map_err(|_| "discord-guard-failed".into())
}
fn from_hub(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() == "hub" {
        Ok(())
    } else {
        Err("not-the-hub".into())
    }
}
#[tauri::command]
pub async fn discord_setup_key(webview: tauri::Webview) -> Result<(), String> {
    from_hub(&webview)?;
    tauri::async_runtime::spawn_blocking(|| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        ottid_core::discord_mute::setup_key().map_err(|_| "key-send-failed".into())
    })
    .await
    .map_err(|_| "setup-failed".to_string())?
}
#[tauri::command]
pub async fn discord_test_mute(webview: tauri::Webview) -> Result<(), String> {
    from_hub(&webview)?;
    tauri::async_runtime::spawn_blocking(|| {
        let _lease = Lease::acquire().map_err(|_| "discord-guard-failed".to_string())?;
        std::thread::sleep(std::time::Duration::from_secs(6));
        Ok(())
    })
    .await
    .map_err(|_| "test-failed".to_string())?
}
pub fn report_error(app: &AppHandle) {
    let _ = app.emit("discord:suppression-error", ());
}
