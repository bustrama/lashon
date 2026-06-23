/**
 * The dictation lifecycle states the Rust worker broadcasts on the
 * `dictation:state` Tauri event. The frontend renders these — it holds no
 * dictation state of its own (see .claude/rules/architecture.md, frontend.md).
 *
 * - `idle`         — waiting for the hotkey
 * - `preparing`    — first-run STT model download / warm-up
 * - `capturing`    — recording the microphone
 * - `transcribing` — STT is running on the take
 * - `error`        — a take failed; the tongue flickers red, then returns to idle
 */
export type DictationState = 'idle' | 'preparing' | 'capturing' | 'transcribing' | 'error';
