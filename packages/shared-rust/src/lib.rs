//! Lashon core — provider clients and shared logic.
//!
//! This crate deliberately does **not** depend on `tauri`. The GUI lives in
//! `apps/desktop/src-tauri`; keeping the testable logic here means its tests
//! link only the networking stack and run cleanly on every OS. See
//! `docs/adr/0003-core-logic-in-a-tauri-independent-crate.md`.

pub mod audio;
#[cfg(feature = "command-mode")]
pub mod command_mode;
pub mod hardware;
pub mod hotkey;
pub mod inject;
pub mod keychain;
#[cfg(feature = "command-mode")]
pub mod llama_server;
#[cfg(feature = "command-mode")]
pub mod llm;
#[cfg(feature = "mcp-server")]
pub mod mcp;
pub mod model;
pub mod provider;
#[cfg(feature = "command-mode")]
pub mod provider_registry;
#[cfg(feature = "command-mode")]
pub mod recipes;
pub mod sidecar;
pub mod stt;
pub mod stt_proto;
#[cfg(feature = "command-mode")]
pub mod tool;
#[cfg(feature = "command-mode")]
pub mod tools;
pub mod transcript;
pub mod vad;
pub mod wake;
