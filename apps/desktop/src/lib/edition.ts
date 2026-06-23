// Build-time edition flag — the frontend half of command-mode editioning
// (docs/adr/0034-command-mode-editioning.md).
//
// The free, dictation-only build is produced with VITE_LASHON_EDITION=free,
// which gates out the command-mode UI so the frontend surface matches the
// dictation-only Rust binary (built with --no-default-features, where the
// command-mode Tauri commands simply do not exist). Any other value —
// including the unset default of a plain `npm run dev` / `tauri dev` — is the
// full edition, so a developer build is always the complete app.
//
// This is a UI-surface gate, NOT the security boundary. Command mode is
// genuinely absent from the free binary because the Rust `command-mode`
// Cargo feature is compiled out; even if the hidden UI were forced to render,
// the Tauri commands it invokes do not exist in the free build.
export const FULL_EDITION: boolean = import.meta.env.VITE_LASHON_EDITION !== 'free';
