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

/**
 * A live partial transcript, broadcast on the `dictation:partial` Tauri event
 * as the worker re-decodes the growing take (docs/adr/0035). The split is the
 * LocalAgreement-2 committer's: `committed` words are final (two consecutive
 * hypotheses agreed) and render solid; `provisional` is the unconfirmed tail
 * and renders muted. Both can be empty (silence at the start of a take).
 */
export type DictationPartial = {
	committed: string;
	provisional: string;
};
