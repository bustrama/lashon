//! `set_volume` — system master output level, 0–100. Windows uses the
//! `IAudioEndpointVolume` COM interface; macOS / Linux are deferred to
//! the same follow-up that lands their UIA peers.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

pub struct SetVolume;

impl SetVolume {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SetVolume {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for SetVolume {
    fn name(&self) -> &str {
        "set_volume"
    }

    fn description(&self) -> &str {
        "Set the system master output volume to a percentage (0–100). \
         Acts on the default audio render endpoint (the same one the \
         volume mixer slider controls)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "percent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 100,
                    "description": "Target volume from 0 (mute) to 100 (max)."
                }
            },
            "required": ["percent"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let percent = args
                .get("percent")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow!("set_volume: missing required `percent` argument"))?;
            if !(0..=100).contains(&percent) {
                return Ok(ToolResult::error(format!(
                    "set_volume: percent must be 0–100, got {percent}"
                )));
            }
            match set(percent as u8) {
                Ok(()) => Ok(ToolResult {
                    content: format!("set master volume to {percent}%"),
                    display_summary: Some(format!("ווליום ל-{percent}%")),
                }),
                Err(e) => Ok(ToolResult::error(format!("set_volume: {e}"))),
            }
        })
    }
}

#[cfg(target_os = "windows")]
fn set(percent: u8) -> Result<()> {
    use windows::core::GUID;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };

    let scalar = percent as f32 / 100.0;
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("CoInitializeEx failed: {init:?}"));
        }
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| anyhow!("GetDefaultAudioEndpoint: {e}"))?;
        let endpoint_volume: IAudioEndpointVolume = device
            .Activate::<IAudioEndpointVolume>(CLSCTX_INPROC_SERVER, None)
            .map_err(|e| anyhow!("Activate(IAudioEndpointVolume): {e}"))?;
        endpoint_volume
            .SetMasterVolumeLevelScalar(scalar, &GUID::zeroed())
            .map_err(|e| anyhow!("SetMasterVolumeLevelScalar: {e}"))?;
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set(_percent: u8) -> Result<()> {
    Err(anyhow!(
        "set_volume: not yet implemented on this OS — Windows-only in M8.2"
    ))
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
        assert_eq!(SetVolume.name(), "set_volume");
        assert!(!SetVolume.requires_confirmation(&json!({"percent": 50})));
    }

    #[test]
    fn missing_percent_argument_errors() {
        let err = rt()
            .block_on(SetVolume.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("percent"));
    }

    #[test]
    fn out_of_range_percent_rejected() {
        let result = rt()
            .block_on(SetVolume.execute(&json!({"percent": 200})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
        let result = rt()
            .block_on(SetVolume.execute(&json!({"percent": -10})))
            .unwrap();
        assert!(result.content.starts_with("error:"));
    }
}
