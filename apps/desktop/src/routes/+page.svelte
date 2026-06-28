<script lang="ts">
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { register, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
	import { invoke } from '@tauri-apps/api/core';
	import { emit, listen } from '@tauri-apps/api/event';
	import type { DictationState, DictationPartial } from '$lib/dictation';
	import { getSetting, DEFAULTS } from '$lib/settings';
	import Tongue from '$lib/components/Tongue.svelte';
	import DebugSurface from '$lib/components/DebugSurface.svelte';
	import { restorePosition, scheduleSnap } from '$lib/snap';
	import { startClickThrough } from '$lib/clickThrough';
	import { FULL_EDITION } from '$lib/edition';

	// Ctrl+Shift+D toggles the debug surface (an M0 deliverable; docs/roadmap.md).
	const DEBUG_SHORTCUT = 'CommandOrControl+Shift+D';
	// The dictation chord is user-configurable from the Settings Hub. It is
	// loaded from settings on mount and re-registered live when the Hub
	// broadcasts a `settings:changed` event (docs/adr/0011 sibling — M4).
	let dictationShortcut = $state('Control+Space');
	// M8 Command-mode chord — independently configurable. Defaults to
	// `CommandOrControl+Backquote` (Ctrl+`) so it doesn't clash with the
	// dictation Ctrl+Space and is reachable with the left pinky.
	let commandShortcut = $state(DEFAULTS['hotkeys.command']);

	let debugVisible = $state(false);
	let dictationState = $state<DictationState>('idle');

	// Live streaming dictation (docs/adr/0035). The worker re-decodes the
	// growing take and emits `dictation:partial` ~twice a second; we hold the
	// latest LocalAgreement-2 split here and pass it to the Tongue, which grows
	// a text panel to show committed (solid) + provisional (muted) words. It is
	// cleared when the take ends (state → idle / error), collapsing the panel.
	// The frontend holds no dictation state of its own — this is a render cache
	// of the last event, nothing more (.claude/rules/frontend.md).
	let partial = $state<DictationPartial | null>(null);

	// REDESIGN — the redesigned Tongue draws the two armed listening states
	// (dictation = saffron / gold halo, command = cobalt blue halo, the
	// `--garnet` token) in DIFFERENT hues so the user can tell at a glance
	// which take is in flight. The dictation worker doesn't emit the take
	// mode in `dictation:state`, so we track it here from the local
	// hotkey-edge events instead.
	//
	// Reset rules:
	//   - Dictation flow ends when `dictation:state` returns to 'idle'
	//     AND no command-mode dispatch is still running.
	//   - Command flow ends when `command:result` fires (the dispatcher's
	//     terminal event — see onCommandResult).
	type TakeMode = 'idle' | 'dictation' | 'command';
	let takeMode = $state<TakeMode>('idle');

	// Wake-listening — true when the always-on wake-word detector is armed
	// and no take is in progress. Plumbed from settings via the existing
	// `wakeword.enabled` flag, refreshed live on `settings:changed`. Pure
	// visual signal — the actual detector runs in the Rust shell.
	let wakeActive = $state(false);

	// The bare tool name (e.g. "open_app", "type_text") — used by the
	// redesigned tool bubble's mono header row, distinct from the
	// (Hebrew) summary line below it.
	let commandToolName = $state<string | null>(null);

	// M8 command-mode flash. `commandFlash` holds a short Hebrew (or English)
	// status the tongue shows for ~3.5s after a Command-mode take completes
	// (docs/adr/0024). `commandFlashTimer` lets a follow-up flash replace the
	// current one without leaving the previous one half-displayed.
	let commandFlash = $state<string | null>(null);
	let commandFlashTimer: ReturnType<typeof setTimeout> | undefined;
	const FLASH_MS = 3500;

	// M8.1 command-mode progress. The dispatcher emits `command:state`
	// (`thinking` / `idle`) around each LLM round-trip and `command:tool`
	// at the start and end of every tool call. The tongue renders:
	//   - a "thinking" indicator while `commandState === 'thinking'`
	//   - a status line carrying the latest tool's `display_summary`
	//     while `commandState === 'tool'`, replaced on each new tool
	//     so the user sees the chain step by step.
	type CommandState = 'idle' | 'thinking' | 'tool';
	let commandState = $state<CommandState>('idle');
	let commandToolLabel = $state<string | null>(null);
	const TOOL_FLASH_MS = 1200;

	// M8.2 — the STT transcript fired off to the LLM. The dispatcher
	// emits `command:transcript` immediately after STT so the user can
	// see what was heard and abort a misheard take. Stays visible until
	// the first tool actually executes (irreversible point) or the
	// result flash takes over.
	let commandTranscript = $state<string | null>(null);
	let commandCancellable = $state(false);

	// M8 confirmation modal. When the Rust dispatcher needs the user's yes/no
	// before executing a destructive tool, it emits `command:confirm`; we
	// expand the tongue window and render Allow/Deny. For `run_command`
	// the Rust side also fills `command_preview` / `cwd_preview` so the
	// modal can render the literal shell command as an untruncated code
	// block instead of the JSON args preview the other destructive tools
	// use (`docs/stories/m8-os-tools.md` — Confirmation modal section).
	type ConfirmRequest = {
		id: string;
		tool: string;
		args_preview: string;
		command_preview?: string;
		cwd_preview?: string;
	};
	let confirmRequest = $state<ConfirmRequest | null>(null);

	// M8.3 — the tongue window is dynamically sized by a ResizeObserver
	// inside `Tongue.svelte` that watches the rendered `.tongue` root
	// and calls `setSize` to match. There are no fixed FLASH / PROGRESS
	// / CONFIRM size constants — the window grows when the transcript
	// appears, shrinks when it dismisses, and stays exactly as tall as
	// it needs to be at every step. Idle = just the mark; transcript
	// + spinner + cancel = whatever those add up to; flash = its own
	// height. Zero dead space.
	//
	// At mount we run `setResizable(true)` + `setSizeConstraints({})`
	// once so the observer's setSize calls aren't capped by an
	// inherited min/max or by Windows' WS_THICKFRAME being absent.

	function onKeydown(event: KeyboardEvent): void {
		if (event.key === 'Escape') {
			if (confirmRequest) {
				// Esc denies an active confirmation prompt — never just hides
				// the tongue underneath it.
				void denyConfirm();
				return;
			}
			if (commandCancellable) {
				// Esc during a still-cancellable Command-mode take aborts
				// it (M8.2). The user gets immediate feedback rather than
				// having to mouse over to the Cancel button.
				void cancelCommand();
				return;
			}
			void getCurrentWindow().hide();
		}
	}

	// A double-click anywhere on the tongue opens the Settings Hub. Also
	// call `unmaximizeIfNeeded` as a safety net — see capture-phase
	// listener below that's the PRIMARY guard against the maximize.
	function onDblClick(): void {
		void invoke('open_hub');
		void unmaximizeIfNeeded();
	}

	// ── The dblclick → maximize fix ──────────────────────────────────────
	// Tauri's drag.js (injected by tauri::generate_context!()) listens for
	// mousedown on every page. When `e.detail === 2` (a dblclick) lands
	// on a `data-tauri-drag-region` element, it invokes
	// `plugin:window|internal_toggle_maximize` — i.e. maximises the
	// window. It does this in the BUBBLE phase.
	//
	// We intercept in the CAPTURE phase (runs first). When we detect the
	// same condition, we `stopImmediatePropagation` (Tauri's handler
	// never sees the event) and invoke `open_hub` ourselves instead.
	// Result: dblclick on the mark opens the Hub with zero maximize side
	// effect — no flash, no recovery — because the maximize never fires.
	function interceptDblClick(e: MouseEvent): void {
		if (e.button !== 0 || e.detail !== 2) return;
		const path = (e.composedPath?.() ?? []) as EventTarget[];
		for (const el of path) {
			if (el instanceof HTMLElement && el.hasAttribute('data-tauri-drag-region')) {
				e.stopImmediatePropagation();
				e.preventDefault();
				void invoke('open_hub');
				return;
			}
		}
	}

	// A right-click on the tongue opens its context menu — the same items as
	// the tray. The native menu is shown by the Rust side (see show_tongue_menu).
	function onContextMenu(event: MouseEvent): void {
		event.preventDefault();
		void invoke('show_tongue_menu');
	}

	// Wake-word acknowledgment chime. Synthesised on the fly with Web Audio
	// so we don't ship an asset (no licensing question, no install bloat).
	// A short, soft sine "ping" — pleasant, unobtrusive, identical for both
	// dictation and command takes; the user just needs to know the wake
	// phrase landed. Audio context is created lazily and reused; we
	// `resume()` defensively in case the webview suspended it (Chromium
	// autoplay policy). Failures are swallowed — a missing chime should
	// never break the take.
	let wakeAudioCtx: AudioContext | null = null;
	function playWakeChime(): void {
		try {
			if (!wakeAudioCtx) {
				const Ctor =
					(window.AudioContext as typeof AudioContext | undefined) ??
					(window as unknown as { webkitAudioContext?: typeof AudioContext })
						.webkitAudioContext;
				if (!Ctor) return;
				wakeAudioCtx = new Ctor();
			}
			const ctx = wakeAudioCtx;
			if (ctx.state === 'suspended') void ctx.resume();
			const now = ctx.currentTime;
			const osc = ctx.createOscillator();
			const gain = ctx.createGain();
			osc.type = 'sine';
			osc.frequency.setValueAtTime(880, now);
			gain.gain.setValueAtTime(0.0001, now);
			gain.gain.exponentialRampToValueAtTime(0.18, now + 0.01);
			gain.gain.exponentialRampToValueAtTime(0.0001, now + 0.22);
			osc.connect(gain).connect(ctx.destination);
			osc.start(now);
			osc.stop(now + 0.24);
		} catch (err) {
			console.warn('wake chime failed:', err);
		}
	}

	// Forward each dictation-chord edge; the worker interprets it per the
	// active mode (hands-free toggle by default, or hold).
	// REDESIGN — on press we tag the local `takeMode` so the tongue
	// halo picks the right hue (saffron for dictation, cobalt blue for
	// command — the `--garnet` token).
	function onDictationEdge(event: { state: string }): void {
		if (event.state === 'Pressed') takeMode = 'dictation';
		void invoke(
			event.state === 'Pressed' ? 'dictation_hotkey_pressed' : 'dictation_hotkey_released'
		);
	}

	// M8 — Command-mode chord. Same edge protocol as dictation, just a
	// different worker entry point so the take ends up routed to the LLM
	// dispatcher instead of the injector.
	function onCommandEdge(event: { state: string }): void {
		if (event.state === 'Pressed') takeMode = 'command';
		void invoke(
			event.state === 'Pressed' ? 'command_hotkey_pressed' : 'command_hotkey_released'
		);
	}

	function registerShortcuts(): void {
		void register(DEBUG_SHORTCUT, (event) => {
			if (event.state === 'Pressed') {
				debugVisible = !debugVisible;
			}
		});
		// `validate_hotkey` is only a policy gate — a chord can still fail to
		// register (most often an OS-level conflict with another app). If it
		// does, fall back to the default so dictation is never left without a
		// working hotkey.
		void register(dictationShortcut, onDictationEdge).catch((err) => {
			console.warn(`dictation hotkey "${dictationShortcut}" could not be registered:`, err);
			if (dictationShortcut !== DEFAULTS['hotkeys.dictation']) {
				dictationShortcut = DEFAULTS['hotkeys.dictation'];
				void register(dictationShortcut, onDictationEdge).catch(() => {});
			}
		});
		// Command mode is full-edition only — the free dictation build has no
		// command backend, so don't register the chord (it would shadow Ctrl+`
		// globally and error on press). See docs/adr/0034.
		if (FULL_EDITION) void register(commandShortcut, onCommandEdge).catch((err) => {
			console.warn(`command hotkey "${commandShortcut}" could not be registered:`, err);
			if (commandShortcut !== DEFAULTS['hotkeys.command']) {
				commandShortcut = DEFAULTS['hotkeys.command'];
				void register(commandShortcut, onCommandEdge).catch(() => {});
			}
		});
	}

	// Clear every registration, then register the current chords fresh — used
	// on mount and whenever the dictation hotkey is rebound. Re-registering
	// over a live shortcut fails, so the slate is always cleared first.
	function refreshShortcuts(): void {
		void unregisterAll().finally(registerShortcuts);
	}

	// Called once at mount. Clears any inherited size constraints and
	// puts the window in "resizable" mode so the ResizeObserver inside
	// `Tongue.svelte` can grow/shrink it freely. On Windows, borderless
	// transparent windows that were declared `resizable: false` in
	// tauri.conf.json get WS_THICKFRAME stripped — without that style,
	// Win32 refuses any subsequent SetWindowPos size change. Forcing
	// `setResizable(true)` here re-adds the style at runtime so
	// programmatic resizes stick.
	async function prepareForDynamicResize(): Promise<void> {
		const win = getCurrentWindow();
		try {
			await win.setResizable(true);
		} catch (err) {
			console.warn('tongue: setResizable failed', err);
		}
		try {
			await win.setSizeConstraints({});
		} catch (err) {
			console.warn('tongue: setSizeConstraints failed', err);
		}
		// Belt-and-braces: the `maximizable: false` config entry should
		// strip WS_MAXIMIZEBOX so Win32 won't maximise on caption-dblclick,
		// but Tauri 2's config doesn't always apply that reliably on
		// Windows. Calling `setMaximizable(false)` at runtime forces it.
		try {
			await win.setMaximizable(false);
		} catch (err) {
			console.warn('tongue: setMaximizable failed', err);
		}
	}

	// Safety net: if Win32 manages to maximise the tongue despite
	// `maximizable: false` + runtime `setMaximizable(false)` (some Tauri 2
	// builds on Windows still leak the caption-dblclick → SC_MAXIMIZE
	// path), immediately unmaximize. The user sees a brief flash at most
	// instead of a permanently fullscreen overlay.
	async function unmaximizeIfNeeded(): Promise<void> {
		const win = getCurrentWindow();
		try {
			if (await win.isMaximized()) {
				await win.unmaximize();
			}
		} catch {
			/* not reachable / not maximized — ignore */
		}
	}

	function onCommandResult(payload: {
		text: string;
		tool_summaries: string[];
		turns: number;
	}): void {
		clearTimeout(commandFlashTimer);
		// The result is the terminal state of the take — clear the live
		// thinking/tool indicator and the transcript preview. The flash
		// takes over the same UI slot. The window shrinks / grows
		// automatically via the Tongue's ResizeObserver.
		commandState = 'idle';
		commandToolLabel = null;
		commandToolName = null;
		commandTranscript = null;
		commandCancellable = false;
		// Command flow done — return the tongue's listening hue to neutral
		// so the next dictation take draws saffron, not the command blue.
		takeMode = 'idle';
		// Prefer the assistant's text; fall back to the tool summary so the
		// user sees *something* even when the LLM returned no prose.
		const message =
			payload.text && payload.text.trim().length > 0
				? payload.text
				: payload.tool_summaries[payload.tool_summaries.length - 1] ?? '';
		commandFlash = message;
		commandFlashTimer = setTimeout(() => {
			commandFlash = null;
		}, FLASH_MS);
	}

	// `command:state` events — the dispatcher emits `thinking` before
	// each LLM call and `idle` at the end of a take. We just flip the
	// state flag; the ResizeObserver inside Tongue.svelte handles the
	// window growing or shrinking around the new content.
	function onCommandStateEvent(value: string): void {
		if (value === 'thinking') {
			commandState = 'thinking';
			commandToolLabel = null;
			commandToolName = null;
			// Cancel is available throughout the thinking phase — the
			// user can still abort before any tool fires.
			commandCancellable = true;
		} else if (value === 'idle') {
			commandState = 'idle';
			commandToolLabel = null;
			commandToolName = null;
			commandTranscript = null;
			commandCancellable = false;
		}
	}

	// `command:tool` events — the dispatcher emits these around every
	// tool execution. `started` flips the indicator to "executing X"
	// (the bare tool name); `finished` swaps in the tool's
	// display_summary if it set one, and a timer rolls back to the
	// thinking state for the next LLM round.
	//
	// REDESIGN — the tool bubble now shows the bare tool name on its top
	// row (mono, garnet) and the Hebrew summary on a second row. We track
	// both: `commandToolName` for the top row, `commandToolLabel` for the
	// summary line.
	let commandToolTimer: ReturnType<typeof setTimeout> | undefined;
	function onCommandToolEvent(payload: {
		name: string;
		status: 'started' | 'finished';
		summary: string | null;
	}): void {
		clearTimeout(commandToolTimer);
		commandState = 'tool';
		commandToolName = payload.name;
		if (payload.status === 'started') {
			// While the tool runs, surface only the bare name (no summary yet).
			commandToolLabel = null;
			// First tool execution = past the point of cheap cancel.
			// We don't promise rollback of side effects (open_app, file
			// writes, …) so the Cancel button disappears once a tool
			// has actually started.
			commandCancellable = false;
		} else {
			// `finished` carries the display_summary (Hebrew or English).
			commandToolLabel = payload.summary ?? payload.name;
			// Hold the finished label briefly so the user actually reads
			// it before the next `thinking` event redraws.
			commandToolTimer = setTimeout(() => {
				if (commandState === 'tool' && commandToolLabel === (payload.summary ?? payload.name)) {
					commandState = 'thinking';
					commandToolLabel = null;
					commandToolName = null;
				}
			}, TOOL_FLASH_MS);
		}
	}

	// M8.2 — `command:transcript` event payload. The dispatcher fires
	// this right after STT, before the LLM round-trip lands. We hold
	// the transcript visible until either the first tool execution
	// (commandCancellable flips to false there) or the result flash.
	function onCommandTranscriptEvent(payload: { text: string }): void {
		commandTranscript = payload.text;
		commandCancellable = true;
	}

	// M8.2 — Cancel button / Escape handler. Calls `cancel_command` on
	// the Rust side which aborts the in-flight dispatch task and emits
	// `command:result` with a "cancelled" message. The Rust side owns
	// the cleanup; we just trigger it.
	async function cancelCommand(): Promise<void> {
		commandCancellable = false;
		try {
			await invoke('cancel_command');
		} catch (err) {
			console.error('hub: cancel_command failed', err);
		}
	}

	async function allowConfirm(): Promise<void> {
		if (!confirmRequest) return;
		await emit('command:confirm:reply', { id: confirmRequest.id, decision: 'allow' });
		confirmRequest = null;
	}

	async function denyConfirm(): Promise<void> {
		if (!confirmRequest) return;
		await emit('command:confirm:reply', { id: confirmRequest.id, decision: 'deny' });
		confirmRequest = null;
	}

	onMount(() => {
		// Return the tongue to wherever the user last snapped it. Run
		// the dynamic-resize preparation in parallel so the ResizeObserver
		// inside Tongue.svelte can grow / shrink the window without being
		// capped by inherited min/max constraints or by Win32's
		// WS_THICKFRAME being missing on a borderless window.
		void restorePosition();
		void prepareForDynamicResize();
		// REDESIGN — start the click-through poll so transparent regions of
		// the tongue window pass clicks through to whatever app is underneath.
		// `data-interactive` markers on the mark / bubbles / confirm card
		// define the click-capturing zones; everything else is transparent
		// to both eyes and mouse.
		const stopClickThrough = startClickThrough();

		// `onMoved` fires continuously during a user drag; the shared
		// `scheduleSnap` debounce in snap.ts coalesces it with the
		// resize-driven snap from Tongue.svelte so the two can't fight
		// over window position.
		const movedUnlisten = getCurrentWindow().onMoved(() => scheduleSnap({ persist: true }));

		// Maximize watcher — `data-tauri-drag-region` sends HTCAPTION
		// mousedowns to Win32, and two of them within the system dblclick
		// window trigger SC_MAXIMIZE despite our `maximizable: false` +
		// `setMaximizable(false)`. Whenever the window resizes, check if
		// it's gotten maximized; if so, undo it immediately. The user sees
		// at most one frame of fullscreen before the snap-back.
		const resizedUnlisten = getCurrentWindow().onResized(() => {
			void unmaximizeIfNeeded();
		});
		// The Rust dictation worker drives the tongue's listening animation.
		const stateUnlisten = listen<DictationState>('dictation:state', (event) => {
			dictationState = event.payload;
			// A take ending collapses the live-partial panel: the final text was
			// already injected, and a lingering preview would outlast the take.
			if (event.payload === 'idle' || event.payload === 'error') {
				partial = null;
			}
			// REDESIGN — reset `takeMode` when the dictation lifecycle ends
			// (state returns to idle) UNLESS a command-mode dispatch is still
			// running. For command takes the dispatcher's `command:result`
			// handles the reset; for dictation takes this is the right hook.
			if (
				event.payload === 'idle' &&
				takeMode === 'dictation' &&
				commandState === 'idle' &&
				!commandTranscript &&
				!confirmRequest
			) {
				takeMode = 'idle';
			}
		});
		// Live streaming partials (docs/adr/0035). Additive to the existing
		// dictation events — the worker emits the running LocalAgreement-2 split
		// as it re-decodes the take; the Tongue renders it. Never logged here
		// (it is transcript content — .claude/rules/security.md).
		const partialUnlisten = listen<DictationPartial>('dictation:partial', (event) => {
			partial = event.payload;
		});
		// M8 — command-mode result + confirmation events from Rust.
		const commandResultUnlisten = listen<{
			text: string;
			tool_summaries: string[];
			turns: number;
		}>('command:result', (event) => onCommandResult(event.payload));
		const commandConfirmUnlisten = listen<ConfirmRequest>('command:confirm', (event) => {
			confirmRequest = event.payload;
		});
		// M8.1 — live progress feedback. `command:state` flips the
		// indicator on/off; `command:tool` rolls the per-tool flash.
		const commandStateUnlisten = listen<string>('command:state', (event) =>
			onCommandStateEvent(event.payload)
		);
		const commandToolUnlisten = listen<{
			name: string;
			status: 'started' | 'finished';
			summary: string | null;
		}>('command:tool', (event) => onCommandToolEvent(event.payload));
		// M8.2 — STT transcript preview. Fires before the LLM round-trip
		// so the user can read what was heard and cancel a misheard take.
		const commandTranscriptUnlisten = listen<{ text: string }>('command:transcript', (event) =>
			onCommandTranscriptEvent(event.payload)
		);
		// Wake-word acknowledgment chime + halo-hue sync. Rust emits
		// `wake:detected` the instant either slot's classifier passes its
		// threshold (see `wakeword.rs::SlotEngine::observe`) — both
		// dictation and command modes fire the same chime so the user
		// knows the phrase landed before the STT worker spins up.
		//
		// The wake path also has to set `takeMode` here: hotkey edges set
		// it from `onDictationEdge`/`onCommandEdge`, but the wake worker
		// goes straight to `channel.trigger*()` in Rust and never touches
		// the frontend. Without this assignment, a wake-fired command
		// take has `takeMode` stuck at `'idle'`, falls through to the
		// `'dict'` branch in `tongueState`, and lights up saffron exactly
		// like dictation — the two listening hues collapse to one. The
		// existing resets (`onCommandResult` for command, the
		// `dictation:state === 'idle'` watcher for dictation) clear
		// `takeMode` at the end of the take, so we don't need cleanup
		// here.
		const wakeUnlisten = listen<{ mode: string }>('wake:detected', (event) => {
			playWakeChime();
			if (event.payload.mode === 'command') {
				takeMode = 'command';
			} else if (event.payload.mode === 'dictation') {
				takeMode = 'dictation';
			}
		});
		// Load the configured chords, then register the shortcuts.
		void Promise.all([
			getSetting('hotkeys.dictation').then((chord) => (dictationShortcut = chord)),
			getSetting('hotkeys.command').then((chord) => (commandShortcut = chord))
		]).then(refreshShortcuts);
		// REDESIGN — read wake-word enablement so the tongue's idle state
		// can render `wake` (faint pulsing ring + antenna glyph) instead of
		// `idle` when the user has wake detection turned on. Live-refreshed
		// on the existing `settings:changed` broadcast below.
		void getSetting('wakeword.enabled').then((on) => (wakeActive = !!on));
		// The Hub broadcasts `settings:changed` when a hotkey is rebound —
		// reload it and re-register so the new chord takes effect at once.
		const settingsUnlisten = listen<{ key: string }>('settings:changed', (event) => {
			if (event.payload.key === 'hotkeys.dictation') {
				void getSetting('hotkeys.dictation').then((chord) => {
					dictationShortcut = chord;
					refreshShortcuts();
				});
			} else if (event.payload.key === 'hotkeys.command') {
				void getSetting('hotkeys.command').then((chord) => {
					commandShortcut = chord;
					refreshShortcuts();
				});
			} else if (event.payload.key === 'wakeword.enabled') {
				void getSetting('wakeword.enabled').then((on) => (wakeActive = !!on));
			}
		});
		window.addEventListener('keydown', onKeydown);
		window.addEventListener('dblclick', onDblClick);
		window.addEventListener('contextmenu', onContextMenu);
		// Capture phase — must register BEFORE Tauri's drag.js mousedown
		// listener (which runs at bubble phase). Capture phase fires
		// first; our handler can stopImmediatePropagation to skip Tauri's
		// dblclick → maximize.
		document.addEventListener('mousedown', interceptDblClick, true);

		return () => {
			window.removeEventListener('keydown', onKeydown);
			window.removeEventListener('dblclick', onDblClick);
			window.removeEventListener('contextmenu', onContextMenu);
			document.removeEventListener('mousedown', interceptDblClick, true);
			// `scheduleSnap`'s timer lives at module scope in snap.ts; it
			// outlives the component intentionally (a dev remount must not
			// strand a pending snap).
			clearTimeout(commandFlashTimer);
			clearTimeout(commandToolTimer);
			stopClickThrough();
			void movedUnlisten.then((unlisten) => unlisten());
			void resizedUnlisten.then((unlisten) => unlisten());
			void stateUnlisten.then((unlisten) => unlisten());
			void partialUnlisten.then((unlisten) => unlisten());
			void commandResultUnlisten.then((unlisten) => unlisten());
			void commandConfirmUnlisten.then((unlisten) => unlisten());
			void commandStateUnlisten.then((unlisten) => unlisten());
			void commandToolUnlisten.then((unlisten) => unlisten());
			void commandTranscriptUnlisten.then((unlisten) => unlisten());
			void wakeUnlisten.then((unlisten) => unlisten());
			void settingsUnlisten.then((unlisten) => unlisten());
			// The shortcuts are process-global and intentionally outlive this
			// component: a dev remount re-registers them, and unregistering
			// here would clobber that fresh registration on the old unmount.
		};
	});
</script>

<main>
	{#if debugVisible}
		<DebugSurface />
	{:else}
		<Tongue
			state={dictationState}
			{takeMode}
			{wakeActive}
			{partial}
			{commandFlash}
			{commandState}
			{commandToolLabel}
			{commandToolName}
			{commandTranscript}
			{commandCancellable}
			{confirmRequest}
			onAllow={() => void allowConfirm()}
			onDeny={() => void denyConfirm()}
			onCancel={() => void cancelCommand()}
		/>
	{/if}
</main>

<style>
	/* M8.3: `main` no longer forces the viewport size. The tongue is
	   content-sized and a ResizeObserver inside it grows the OS window
	   to match; if `main` were `100vw / 100vh` here we'd pin the
	   layout to whatever the OS HWND currently is and the inner
	   content couldn't grow past it on its own. */
	main {
		display: inline-flex;
	}
</style>
