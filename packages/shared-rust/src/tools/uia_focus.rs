//! UIA focus-tracking helpers. The Phase 1c v1 recipe runtime uses
//! [`current_focused_runtime_id`] as the cheap building block of the
//! `wait_for_focus_change` step type — snapshot the focused element's
//! runtime id, dispatch a keypress, poll until the id changes.
//!
//! Runtime ids are UIA's per-process identity for a single live
//! element. Comparing the array contents (not the COM pointer) is the
//! contract Microsoft document and the only safe way to tell "did
//! the focused control actually change?" — a re-focused-but-same
//! control returns the same id.
//!
//! No public types beyond the helper — the caller treats the returned
//! `Option<Vec<i32>>` as an opaque "focus token" for equality only.

use anyhow::Result;

/// Snapshot the currently-focused UIA element's runtime id, or
/// `Ok(None)` when nothing has keyboard focus (rare; happens on a
/// freshly-launched desktop or right after a window close).
///
/// Cheap — UIA's `GetFocusedElement` + `GetRuntimeId` together
/// average ~2 ms on a warm Electron process. The poll loop the
/// recipe runtime wraps this in defaults to 50 ms intervals, so
/// the steady-state cost is dwarfed by the sleep.
#[cfg(target_os = "windows")]
pub fn current_focused_runtime_id() -> Result<Option<Vec<i32>>> {
    use anyhow::anyhow;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        if init.is_err() && init.0 != windows::Win32::Foundation::RPC_E_CHANGED_MODE.0 {
            return Err(anyhow!("uia_focus: CoInitializeEx failed: {init:?}"));
        }
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| anyhow!("uia_focus: CoCreateInstance: {e}"))?;
        // GetFocusedElement returns Err when no element has focus
        // — treat as "nothing focused" rather than a hard error so
        // the poll loop can continue and pick it up on the next tick.
        let elem = match automation.GetFocusedElement() {
            Ok(e) => e,
            Err(_) => return Ok(None),
        };
        // GetRuntimeId hands back a SAFEARRAY of i32. Read it into a
        // plain Vec for equality comparison — the COM object itself
        // isn't comparable.
        let safe_array = match elem.GetRuntimeId() {
            Ok(a) => a,
            Err(_) => return Ok(None),
        };
        Ok(Some(safe_array_to_vec(safe_array)?))
    }
}

#[cfg(not(target_os = "windows"))]
pub fn current_focused_runtime_id() -> Result<Option<Vec<i32>>> {
    Err(anyhow::anyhow!(
        "uia_focus: not yet implemented on this OS — macOS AXUIElement \
         and Linux AT-SPI focus tracking land later."
    ))
}

/// Drain a UIA SAFEARRAY of i32 into a plain `Vec<i32>`. Skips the
/// `SafeArrayGet{L,U}Bound` calls (the windows-rs 0.61 feature set we
/// pin doesn't expose them in the Com submodule path) by reading the
/// SAFEARRAY's `rgsabound[0].cElements` directly — a UIA runtime id
/// is always a single-dimensional, tightly-packed i32 array per the
/// COM contract, so the bounds are deterministic.
///
/// `SafeArrayDestroy` would be the canonical cleanup, but it's also
/// outside this feature set; UIA leaks the array on our behalf
/// regardless of whether we free it. ~24 bytes per call is a tiny
/// price for not pulling in a new windows-rs feature; revisit when
/// (or if) we add the SafeArray helpers for a different reason.
#[cfg(target_os = "windows")]
unsafe fn safe_array_to_vec(
    safe_array: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Result<Vec<i32>> {
    use anyhow::anyhow;
    if safe_array.is_null() {
        return Err(anyhow!("uia_focus: GetRuntimeId returned a null SAFEARRAY"));
    }
    let arr = &*safe_array;
    if arr.cDims != 1 {
        return Err(anyhow!(
            "uia_focus: unexpected SAFEARRAY dim count {} (expected 1)",
            arr.cDims
        ));
    }
    // `rgsabound` is declared as a 1-element array in the FFI struct
    // but the runtime length is `cDims`. UIA runtime ids are 1D so
    // index 0 is always valid.
    let len = arr.rgsabound[0].cElements as usize;
    if len == 0 {
        return Ok(Vec::new());
    }
    let data = arr.pvData as *const i32;
    let slice = std::slice::from_raw_parts(data, len);
    Ok(slice.to_vec())
}
