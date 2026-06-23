// The single typed gateway to the `tauri-plugin-store` `settings.json`. Every
// persisted preference is declared in `Settings`; callers go through
// `getSetting` / `setSetting` and never touch the store directly. Outside a
// Tauri webview (e.g. a browser preview) the store is unavailable, so reads
// fall back to `DEFAULTS` and writes are no-ops.
import { load, type Store } from '@tauri-apps/plugin-store';
import type { Lang } from '$lib/i18n';
import type { Tier } from '$lib/hardware';

export interface Settings {
	'ui.language': Lang;
	'hotkeys.dictation': string;
	// M8 Command-mode hotkey (docs/adr/0024). Press, speak a command, the
	// LLM picks tools to fulfil it. Defaults to a chord that does not
	// clash with Ctrl+Space (the dictation hotkey).
	'hotkeys.command': string;
	'tutorial.completed': boolean;
	'tongue.position': { x: number; y: number } | null;
	// The hardware tier picks the default STT/LLM/TTS models. Detected during
	// onboarding and overridable there and in the Hub; `null` until onboarding
	// has run (docs/adr/0013).
	'hardware.tier': Tier | null;
	// Wake-word detection — off by default. Two independent slots:
	// `dictation` fires the dictation worker (`TakeMode::Inject`),
	// `command` fires the M8 Command-mode dispatcher (`TakeMode::Command`).
	// Each slot has its own enable/sensitivity/model; the model picker
	// in the Hub forces them to be different (one classifier ⇒ one
	// intent). Sensitivities are 0..1 (higher = lower threshold).
	'wakeword.dictation.enabled': boolean;
	'wakeword.dictation.sensitivity': number;
	'wakeword.dictation.model': string;
	'wakeword.command.enabled': boolean;
	'wakeword.command.sensitivity': number;
	'wakeword.command.model': string;
	// Legacy flat schema — kept in the type so the one-shot Tauri-side
	// migration (wakeword.rs::read_settings) can still read them out
	// of users' existing settings.json without a TS error. New code
	// must use the `.dictation.*` / `.command.*` keys above.
	'wakeword.enabled': boolean;
	'wakeword.sensitivity': number;
	'wakeword.model': string;
	// M7 LLM provider mux (docs/adr/0019, docs/adr/0021). Two modes, each with
	// its own active provider + model. `"none"` is the explicit, valid default
	// — cloud is never the silent default (docs/adr/0022 Invariant 1). API
	// keys live in the OS keychain, never in this store (docs/adr/0020).
	'llm.command.provider': string;
	'llm.command.model': string;
	'llm.chat.provider': string;
	'llm.chat.model': string;
	// Per-provider base-URL override — empty string means "use the vendor
	// default" (docs/adr/0021 Persistence Schema).
	'llm.anthropic.base_url': string;
	'llm.openai.base_url': string;
	'llm.groq.base_url': string;
	'llm.deepseek.base_url': string;
	'llm.mistral.base_url': string;
	'llm.together.base_url': string;
	'llm.openrouter.base_url': string;
	'llm.minimax.base_url': string;
	'llm.opencode-go.base_url': string;
	'llm.ollama-local.base_url': string;
	'llm.ollama-remote.base_url': string;
}

export const DEFAULTS: Settings = {
	'ui.language': 'he',
	'hotkeys.dictation': 'Control+Space',
	// Cross-platform default — `CommandOrControl+Backquote` resolves to
	// Ctrl+` on Win/Linux and Cmd+` on macOS. Left-pinky reachable, doesn't
	// collide with the dictation Ctrl+Space, and `Backquote` is the W3C
	// keyboard-event `code` the Tauri accelerator parser accepts for the
	// key in the top-left position next to `1` (engraved ` on US layouts,
	// ; on Israeli Hebrew layout). The user can rebind it from the Hub.
	'hotkeys.command': 'CommandOrControl+Backquote',
	'tutorial.completed': false,
	'tongue.position': null,
	'hardware.tier': null,
	'wakeword.dictation.enabled': false,
	'wakeword.dictation.sensitivity': 0.7,
	'wakeword.dictation.model': 'hey_lashon',
	'wakeword.command.enabled': false,
	'wakeword.command.sensitivity': 0.7,
	'wakeword.command.model': '',
	// Legacy keys retained so the Tauri-side migration sees a defined
	// value when both the new and old keys are missing. Not surfaced
	// in the UI.
	'wakeword.enabled': false,
	'wakeword.sensitivity': 0.7,
	'wakeword.model': 'hey_lashon',
	// LLM mux — both modes start at "none" so dictation continues to work
	// without any LLM configured (docs/adr/0022 Invariant 1).
	'llm.command.provider': 'none',
	'llm.command.model': '',
	'llm.chat.provider': 'none',
	'llm.chat.model': '',
	'llm.anthropic.base_url': '',
	'llm.openai.base_url': '',
	'llm.groq.base_url': '',
	'llm.deepseek.base_url': '',
	'llm.mistral.base_url': '',
	'llm.together.base_url': '',
	'llm.openrouter.base_url': '',
	'llm.minimax.base_url': '',
	'llm.opencode-go.base_url': '',
	'llm.ollama-local.base_url': 'http://127.0.0.1:11434/v1',
	'llm.ollama-remote.base_url': ''
};

let storePromise: Promise<Store | null> | undefined;

function settingsStore(): Promise<Store | null> {
	// Memoised: one handle to settings.json for the window's lifetime. `load`
	// rejects outside a Tauri webview — resolve to null so callers degrade.
	storePromise ??= load('settings.json').catch(() => null);
	return storePromise;
}

export async function getSetting<K extends keyof Settings>(key: K): Promise<Settings[K]> {
	const store = await settingsStore();
	if (!store) return DEFAULTS[key];
	const value = await store.get<Settings[K]>(key);
	return value ?? DEFAULTS[key];
}

export async function setSetting<K extends keyof Settings>(
	key: K,
	value: Settings[K]
): Promise<void> {
	const store = await settingsStore();
	if (!store) return;
	await store.set(key, value);
	await store.save();
}
