//! M8 Phase-1 tool catalogue (`docs/roadmap.md §2.2`).
//!
//! Each submodule implements one `LashonTool`. The
//! `register_phase_one_tools` helper wires the whole catalogue into a
//! `ToolRegistry` so the Tauri shell can construct one with a single
//! call.
//!
//! Tools land in alphabetical order in the registry; the LLM sees
//! the same order in its `tools` array on every chat call.

use std::sync::Arc;

use crate::tool::{LashonTool, ToolRegistry};

pub mod click_element;
pub mod clipboard;
pub mod double_click;
pub mod drag;
pub mod file_delete;
pub mod file_move;
pub mod file_read;
pub mod file_write;
pub mod focus_window;
pub mod kill_process;
pub mod list_files;
pub mod list_open_windows;
pub mod list_processes;
pub mod lock_screen;
pub mod new_browser_tab;
pub mod open_app;
pub mod open_url;
pub mod path_safety;
pub mod press_keys;
pub mod read_active_window_text;
// `uia_focus` — focused-element runtime-id snapshot. Not a `LashonTool`
// itself; the M9 recipe runtime's `wait_for_focus_change` step type
// consumes it to wait for keyboard focus to move to a different
// control (e.g. Ctrl+K opening a modal whose input gets focus). The
// module exposes a Windows impl + a non-Windows error-stub so the
// recipe-runtime call site doesn't need cfg-gating.
pub mod uia_focus;
pub mod read_browser_url;
pub mod read_screen;
pub mod right_click;
pub mod run_command;
pub mod scroll;
pub mod set_volume;
pub mod show_notification;
pub mod type_text;
pub mod wait_for_element;
pub mod wait_for_window;
pub mod wait_ms;
pub mod window_state;

/// Populate `registry` with the full M8 tool catalogue — Phase 1
/// (M8.1, the safe interactive set) plus Phase 2 (M8.2, the OS-control
/// tranche: file_*, run_command, window/process management,
/// notification, browser introspection). The 8 destructive tools in
/// Phase 2 (file_write, file_delete, file_move, close_window,
/// run_command, kill_process, lock_screen) override
/// `requires_confirmation` so the dispatcher's modal gates them.
///
/// Order matters only as far as test snapshot diffs — actual call
/// order is decided by the LLM, not the registry.
pub fn register_phase_one_tools(registry: &mut ToolRegistry) {
    let tools: Vec<Arc<dyn LashonTool>> = vec![
        // Phase 1 — M8.1 safe interactive set.
        Arc::new(click_element::ClickElement::new()),
        Arc::new(clipboard::ClipboardGet::new()),
        Arc::new(clipboard::ClipboardSet::new()),
        Arc::new(focus_window::FocusWindow::new()),
        Arc::new(open_app::OpenApp::new()),
        Arc::new(open_url::OpenUrl::new()),
        Arc::new(press_keys::PressKeys::new()),
        Arc::new(read_active_window_text::ReadActiveWindowText::new()),
        Arc::new(type_text::TypeText::new()),
        Arc::new(wait_for_element::WaitForElement::new()),
        Arc::new(wait_for_window::WaitForWindow::new()),
        Arc::new(wait_ms::WaitMs::new()),
        Arc::new(open_url::WebSearch::new()),
        // Phase 2 — M8.2 OS-control tranche.
        Arc::new(double_click::DoubleClick::new()),
        Arc::new(drag::Drag::new()),
        Arc::new(file_delete::FileDelete::new()),
        Arc::new(file_move::FileMove::new()),
        Arc::new(file_read::FileRead::new()),
        Arc::new(file_write::FileWrite::new()),
        Arc::new(kill_process::KillProcess::new()),
        Arc::new(list_files::ListFiles::new()),
        Arc::new(list_open_windows::ListOpenWindows::new()),
        Arc::new(list_processes::ListProcesses::new()),
        Arc::new(lock_screen::LockScreen::new()),
        Arc::new(new_browser_tab::NewBrowserTab::new()),
        Arc::new(read_browser_url::ReadBrowserUrl::new()),
        Arc::new(read_screen::ReadScreen::new()),
        Arc::new(right_click::RightClick::new()),
        Arc::new(run_command::RunCommand::new()),
        Arc::new(scroll::Scroll::new()),
        Arc::new(set_volume::SetVolume::new()),
        Arc::new(show_notification::ShowNotification::new()),
        Arc::new(window_state::CloseWindow::new()),
        Arc::new(window_state::MaximizeWindow::new()),
        Arc::new(window_state::MinimizeWindow::new()),
    ];
    for tool in tools {
        registry.register(tool);
    }
}

/// Build a `ToolRegistry` pre-populated with the Phase-1 tools.
/// Convenience for the Tauri shell's startup path.
pub fn phase_one_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_phase_one_tools(&mut registry);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tools that must gate on the confirmation modal — kept in one
    /// place so the registry test and any future audits stay in sync
    /// with the in-code overrides of `LashonTool::requires_confirmation`.
    /// 8 tools per `docs/stories/m8-os-tools.md`.
    pub(crate) const DESTRUCTIVE_TOOLS: &[&str] = &[
        "close_window",
        "file_delete",
        "file_move",
        "file_write",
        "kill_process",
        "lock_screen",
        "run_command",
    ];

    #[test]
    fn phase_one_registry_has_all_expected_tools() {
        let registry = phase_one_registry();
        let names = registry.names();
        let expected = vec![
            // Phase 1 (M8.1) — safe interactive set.
            "click_element",
            "clipboard_get",
            "clipboard_set",
            "focus_window",
            "open_app",
            "open_url",
            "press_keys",
            "read_active_window_text",
            "type_text",
            "wait_for_element",
            "wait_for_window",
            "wait_ms",
            "web_search",
            // Phase 2 (M8.2) — OS-control tranche.
            "close_window",
            "double_click",
            "drag",
            "file_delete",
            "file_move",
            "file_read",
            "file_write",
            "kill_process",
            "list_files",
            "list_open_windows",
            "list_processes",
            "lock_screen",
            "maximize_window",
            "minimize_window",
            "new_browser_tab",
            "read_browser_url",
            "read_screen",
            "right_click",
            "run_command",
            "scroll",
            "set_volume",
            "show_notification",
        ];
        for name in &expected {
            assert!(
                names.iter().any(|n| n == name),
                "missing tool: {name} (have: {names:?})"
            );
        }
        assert_eq!(names.len(), expected.len());
    }

    #[test]
    fn phase_one_registry_serialises_to_llm_tools() {
        let registry = phase_one_registry();
        let llm_tools = registry.to_llm_tools();
        assert_eq!(llm_tools.len(), 35);
        // Every tool exposes a non-empty description and an object schema.
        for tool in &llm_tools {
            assert!(!tool.description.is_empty(), "{}", tool.name);
            assert_eq!(tool.parameters["type"], "object", "{}", tool.name);
        }
    }

    #[test]
    fn destructive_tools_require_confirmation_and_others_do_not() {
        // The dispatcher's modal gate keys off `requires_confirmation`.
        // The set of destructive tools is documented in
        // `docs/stories/m8-os-tools.md`; this test pins it in code so a
        // future PR adding a destructive tool can't accidentally skip
        // the modal.
        let registry = phase_one_registry();
        let destructive: std::collections::HashSet<&str> =
            DESTRUCTIVE_TOOLS.iter().copied().collect();
        for tool in registry.all() {
            let requires = tool.requires_confirmation(&serde_json::json!({}));
            let expected = destructive.contains(tool.name());
            assert_eq!(
                requires,
                expected,
                "tool `{}`: requires_confirmation = {}, expected = {}",
                tool.name(),
                requires,
                expected
            );
        }
        // Conversely, every name in DESTRUCTIVE_TOOLS must exist in the
        // registry (catches typos when the list is edited).
        let names = registry.names();
        for name in DESTRUCTIVE_TOOLS {
            assert!(
                names.iter().any(|n| n == name),
                "DESTRUCTIVE_TOOLS lists `{name}` but it is not registered"
            );
        }
    }
}
