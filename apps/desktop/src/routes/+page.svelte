<script lang="ts">
	import { onMount } from 'svelte';
	import { register, unregisterAll } from '@tauri-apps/plugin-global-shortcut';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { t } from '$lib/i18n';
	import type { DictationState, DictationPartial } from '$lib/dictation';
	import { getSetting, DEFAULTS } from '$lib/settings';
	import { creatureState, type CommandState, type TakeMode } from '$lib/creature';
	import Overlay from '$lib/overlay/Overlay.svelte';
	import { IslandHold } from '$lib/overlay/island';
	import type { ApprovalCard, ApprovalDecision, ApprovalNudge } from '$lib/overlay/approval';
	import DebugSurface from '$lib/components/DebugSurface.svelte';
	import { FULL_EDITION } from '$lib/edition';

	// The overlay window. The Rust shell places it, moves it and decides where
	// it takes the mouse (docs/adr/0044); this page wires the lifecycle events
	// to the creature and the island of cards beside it.

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
	// latest LocalAgreement-2 split here and the island shows committed
	// (solid) + provisional (muted) words. It is cleared when the take ends
	// (state → idle / error), closing the card. The frontend holds no
	// dictation state of its own — this is a render cache of the last event,
	// nothing more (.claude/rules/frontend.md).
	let partial = $state<DictationPartial | null>(null);

	// Dictation and command takes look different (saffron vs cobalt), but the
	// dictation worker doesn't emit the take mode in `dictation:state`, so we
	// track it here from the hotkey edges and the wake event.
	//
	// Reset rules:
	//   - Dictation flow ends when `dictation:state` returns to 'idle'
	//     AND no command-mode dispatch is still running.
	//   - Command flow ends when `command:result` fires (the dispatcher's
	//     terminal event — see onCommandResult).
	let takeMode = $state<TakeMode>('idle');

	// Wake-listening — true while the wake-word detector is listening, as
	// the Rust worker reports it (`wake:armed`). Pure visual signal.
	let wakeActive = $state(false);

	// The wake word was just heard: the creature's startled "wake" event,
	// shown briefly before the take's listening pose.
	let woke = $state(false);
	let wokeTimer: ReturnType<typeof setTimeout> | undefined;
	const WAKE_MS = 900;

	// The bare tool name (e.g. "open_app", "type_text") — the tool card's
	// mono header row, distinct from the (Hebrew) summary line below it.
	let commandToolName = $state<string | null>(null);

	// M8 command-mode flash: a short status shown for a few seconds after a
	// Command-mode take completes (docs/adr/0024). It stays while the cursor
	// is on it and closes a moment after the cursor leaves.
	let commandFlash = $state<string | null>(null);
	const FLASH_MS = 3500;
	const flashHold = new IslandHold(() => (commandFlash = null));

	// M8.1 command-mode progress. The dispatcher emits `command:state`
	// (`thinking` / `idle`) around each LLM round-trip and `command:tool`
	// at the start and end of every tool call. The island renders:
	//   - a "thinking" indicator while `commandState === 'thinking'`
	//   - a status line carrying the latest tool's `display_summary`
	//     while `commandState === 'tool'`, replaced on each new tool
	//     so the user sees the chain step by step.
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

	// The approval card (docs/adr/0048). The Rust broker holds the queue of
	// requests that wait for the user's yes or no (a command-mode tool, a
	// recipe step) and sends the one to show on `approval:changed`, or null
	// when none waits. The answer goes back to the broker, which also takes
	// it from the hotkeys and denies the request when its time runs out.
	let approval = $state<ApprovalCard | null>(null);
	// An early press of the Allow hotkey, for the card to explain.
	let approvalNudge = $state<ApprovalNudge | null>(null);

	function answerApproval(id: number, decision: ApprovalDecision): void {
		void invoke('approval_answer', { id, decision }).catch(() => {});
	}
	function armApproval(id: number): void {
		void invoke('approval_armed', { id }).catch(() => {});
	}

	const current = $derived(
		creatureState({
			dictation: dictationState,
			takeMode,
			commandState,
			confirming: !!approval,
			woke
		})
	);

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
	// active mode (hands-free toggle by default, or hold). On press we tag
	// `takeMode` so the creature takes the dictation look, not the command one.
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

	function onCommandResult(payload: {
		text: string;
		tool_summaries: string[];
		turns: number;
	}): void {
		// The result is the terminal state of the take — clear the live
		// thinking/tool indicator and the transcript preview. The flash
		// takes their place in the island.
		commandState = 'idle';
		commandToolLabel = null;
		commandToolName = null;
		commandTranscript = null;
		commandCancellable = false;
		// Command flow done — return the listening hue to neutral so the next
		// dictation take draws saffron, not the command blue.
		takeMode = 'idle';
		// Prefer the assistant's text; fall back to the tool summary so the
		// user sees *something* even when the LLM returned no prose.
		const message =
			payload.text && payload.text.trim().length > 0
				? payload.text
				: (payload.tool_summaries[payload.tool_summaries.length - 1] ?? '');
		commandFlash = message;
		flashHold.show(FLASH_MS);
	}

	// `command:state` events — the dispatcher emits `thinking` before
	// each LLM call and `idle` at the end of a take.
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

	// M8.2 — the Cancel button. Calls `cancel_command` on the Rust side,
	// which aborts the in-flight dispatch task and emits `command:result`
	// with a "cancelled" message. The Rust side owns the cleanup.
	async function cancelCommand(): Promise<void> {
		commandCancellable = false;
		try {
			await invoke('cancel_command');
		} catch (err) {
			console.error('overlay: cancel_command failed', err);
		}
	}

	onMount(() => {
		// The Rust dictation worker drives the creature's listening states.
		const stateUnlisten = listen<DictationState>('dictation:state', (event) => {
			dictationState = event.payload;
			// A take ending closes the live-text card: the final text was
			// already injected, and a lingering preview would outlast the take.
			if (event.payload === 'idle' || event.payload === 'error') {
				partial = null;
			}
			// Reset `takeMode` when the dictation lifecycle ends (state returns
			// to idle) UNLESS a command-mode dispatch is still running. For
			// command takes the dispatcher's `command:result` handles the
			// reset; for dictation takes this is the right hook.
			if (
				event.payload === 'idle' &&
				takeMode === 'dictation' &&
				commandState === 'idle' &&
				!commandTranscript &&
				!approval
			) {
				takeMode = 'idle';
			}
		});
		// Live streaming partials (docs/adr/0035). Never logged here (it is
		// transcript content — .claude/rules/security.md).
		const partialUnlisten = listen<DictationPartial>('dictation:partial', (event) => {
			partial = event.payload;
		});
		// M8 — the command-mode result from Rust.
		const commandResultUnlisten = listen<{
			text: string;
			tool_summaries: string[];
			turns: number;
		}>('command:result', (event) => onCommandResult(event.payload));
		// The approval card. Ask for the current one only once subscribed,
		// and let any event that lands first win. The free edition has no
		// broker: the command fails and no card ever shows.
		let approvalHeard = false;
		const approvalUnlisten = listen<ApprovalCard | null>('approval:changed', (event) => {
			approvalHeard = true;
			approval = event.payload;
		});
		void approvalUnlisten
			.then(() => invoke<ApprovalCard | null>('approval_current'))
			.then((card) => {
				if (!approvalHeard) approval = card;
			})
			.catch(() => {});
		const nudgeUnlisten = listen<{ id: number }>('approval:nudge', (event) => {
			const n = approvalNudge?.id === event.payload.id ? approvalNudge.n + 1 : 1;
			approvalNudge = { id: event.payload.id, n };
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
		// Wake word heard. Rust emits `wake:detected` the instant either
		// slot's classifier passes its threshold (see
		// `wakeword.rs::SlotEngine::observe`): chime, show the wake event, and
		// set `takeMode` — the wake worker goes straight to
		// `channel.trigger*()` in Rust and never passes a hotkey edge here, so
		// without this a wake-fired command take would draw in the dictation
		// colour. The existing resets (`onCommandResult` for command, the
		// `dictation:state === 'idle'` watcher for dictation) clear `takeMode`
		// at the end of the take.
		const wakeUnlisten = listen<{ mode: string }>('wake:detected', (event) => {
			playWakeChime();
			if (event.payload.mode === 'command') {
				takeMode = 'command';
			} else if (event.payload.mode === 'dictation') {
				takeMode = 'dictation';
			}
			woke = true;
			clearTimeout(wokeTimer);
			wokeTimer = setTimeout(() => (woke = false), WAKE_MS);
		});
		// Load the configured chords, then register the shortcuts.
		void Promise.all([
			getSetting('hotkeys.dictation').then((chord) => (dictationShortcut = chord)),
			getSetting('hotkeys.command').then((chord) => (commandShortcut = chord))
		]).then(refreshShortcuts);
		// Whether the wake-word detector is listening, for the creature's
		// idle. Only Rust knows (a slot enabled, its classifier installed,
		// the microphone open), so render what it reports: `wake:armed` on
		// every change, and `wake_armed` for the state before this mounted.
		// Ask only once subscribed, and let any event that lands first win.
		let armedHeard = false;
		const armedUnlisten = listen<{ armed: boolean }>('wake:armed', (event) => {
			armedHeard = true;
			wakeActive = event.payload.armed;
		});
		void armedUnlisten
			.then(() => invoke<boolean>('wake_armed'))
			.then((armed) => {
				if (!armedHeard) wakeActive = armed;
			})
			.catch(() => {});
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
			}
		});

		return () => {
			clearTimeout(commandToolTimer);
			clearTimeout(wokeTimer);
			flashHold.reset();
			void stateUnlisten.then((unlisten) => unlisten());
			void partialUnlisten.then((unlisten) => unlisten());
			void commandResultUnlisten.then((unlisten) => unlisten());
			void approvalUnlisten.then((unlisten) => unlisten());
			void nudgeUnlisten.then((unlisten) => unlisten());
			void commandStateUnlisten.then((unlisten) => unlisten());
			void commandToolUnlisten.then((unlisten) => unlisten());
			void commandTranscriptUnlisten.then((unlisten) => unlisten());
			void wakeUnlisten.then((unlisten) => unlisten());
			void armedUnlisten.then((unlisten) => unlisten());
			void settingsUnlisten.then((unlisten) => unlisten());
			// The shortcuts are process-global and intentionally outlive this
			// component: a dev remount re-registers them, and unregistering
			// here would clobber that fresh registration on the old unmount.
		};
	});
</script>

{#snippet debugCard()}
	<DebugSurface />
{/snippet}

<Overlay
	state={current}
	wakeArmed={wakeActive}
	mode={takeMode === 'idle' ? null : takeMode}
	announcement={current === 'idle' && wakeActive
		? $t('creature.wakeArmed')
		: $t(`creature.states.${current}`)}
	island={{
		listening: dictationState === 'capturing',
		takeMode,
		partial,
		commandState,
		commandToolName,
		commandToolLabel,
		commandTranscript,
		commandCancellable,
		commandFlash,
		approval,
		approvalNudge,
		onApprovalArmed: armApproval,
		onApprovalAnswer: answerApproval,
		onCancel: () => void cancelCommand()
	}}
	extra={debugVisible ? debugCard : undefined}
	onIslandHover={(on) => flashHold.hover(on)}
/>
