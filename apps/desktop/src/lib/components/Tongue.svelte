<script lang="ts">
	// The Tongue is a transparent, frameless, always-on-top overlay — Ottid,
	// the creature (docs/adr/0040), floating and draggable, with its bubbles
	// stacked underneath.
	//
	// The parent feeds the raw lifecycle events from the Rust FSM; they
	// collapse to one of the creature's twelve states (lib/creature/state.ts).
	// The creature changes pose with the state and its lamp always shows the
	// state's colour, so a single still frame reads under reduced motion too.
	// The live region below announces every state; the creature is never
	// the only signal.
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { t } from '$lib/i18n';
	import type { DictationState, DictationPartial } from '$lib/dictation';
	import StateGlyph from '$lib/components/StateGlyph.svelte';
	import { Creature, creatureState, listens, STAGE } from '$lib/creature';

	// Drag + click handling. The trade-off space here:
	//
	//  - `data-tauri-drag-region` makes the drag native + reliable but
	//    sends WM_NCLBUTTONDOWN HTCAPTION on each mousedown. Two of those
	//    inside the system dblclick window = Win32 MAXIMIZE the window
	//    (`WS_MAXIMIZEBOX` strip doesn't always take effect on Tauri
	//    2.11.2 builds, so the user keeps getting maximized windows).
	//
	//  - Custom `dragOnMove` with setPosition avoids HTCAPTION entirely:
	//    no caption-mousedown, no maximize, no system menu. The cost is
	//    a per-frame IPC during drag (~60/s) and we have to wire dblclick
	//    + right-click manually.
	//
	// Going with the custom path — the maximize bug is unacceptable, and
	// per-frame IPC is fine for a tiny overlay.

	// Drag via Tauri's native `data-tauri-drag-region` on the outer
	// `.tongue` div. This is what the pre-redesign code used and
	// what the user described as "perfect" — Win32's caption-modal-drag
	// is native-smooth, no per-frame work needed.
	//
	// The side effect — caption-dblclick triggers SC_MAXIMIZE — is
	// caught by a resize watcher in +page.svelte that immediately
	// `unmaximize`s if the window ever ends up maximized.
	//
	// Click / dblclick / right-click are handled in two places:
	//   - Window-level listeners in +page.svelte catch the bubbled
	//     dblclick + contextmenu events (the pre-redesign approach).
	//   - The handler functions below catch single-clicks for the
	//     manual dblclick detector as a fallback when the browser's
	//     native dblclick synthesis misses fast taps at the user's
	//     ~600 ms cadence.

	type ConfirmRequest = {
		id: string;
		tool: string;
		args_preview: string;
		command_preview?: string;
		cwd_preview?: string;
	};
	type CommandState = 'idle' | 'thinking' | 'tool';
	type TakeMode = 'idle' | 'dictation' | 'command';

	let {
		state: dictationState = 'idle',
		takeMode = 'idle',
		wakeActive = false,
		partial = null,
		commandFlash = null,
		commandState = 'idle',
		commandToolLabel = null,
		commandToolName = null,
		commandToolStep = null,
		commandTranscript = null,
		commandCancellable = false,
		confirmRequest = null,
		onAllow = () => {},
		onDeny = () => {},
		onCancel = () => {},
		onDblClick = () => {},
		onContextMenu = () => {}
	}: {
		state?: DictationState;
		takeMode?: TakeMode;
		wakeActive?: boolean;
		partial?: DictationPartial | null;
		commandFlash?: string | null;
		commandState?: CommandState;
		commandToolLabel?: string | null;
		commandToolName?: string | null;
		commandToolStep?: { current: number; total: number } | null;
		commandTranscript?: string | null;
		commandCancellable?: boolean;
		confirmRequest?: ConfirmRequest | null;
		onAllow?: () => void;
		onDeny?: () => void;
		onCancel?: () => void;
		// REDESIGN — element-level handlers so dblclick / right-click fire
		// reliably on the mark itself, bypassing the click-through poll's
		// 50 ms lag (which could otherwise eat the event if the user
		// clicked within that window of moving onto the mark).
		onDblClick?: () => void;
		onContextMenu?: () => void;
	} = $props();

	// The wake word was just heard: the creature's startled "wake" event,
	// shown briefly over the start of the take it triggers. A render cache of
	// the `wake:detected` event; the detector itself runs in Rust.
	const WAKE_MS = 900;
	let woke = $state(false);

	$effect(() => {
		let timer: ReturnType<typeof setTimeout> | undefined;
		const unlistenPromise = listen('wake:detected', () => {
			woke = true;
			clearTimeout(timer);
			timer = setTimeout(() => (woke = false), WAKE_MS);
		});
		return () => {
			clearTimeout(timer);
			void unlistenPromise.then((u) => u()).catch(() => {});
		};
	});

	const current = $derived(
		creatureState({
			dictation: dictationState,
			takeMode,
			commandState,
			confirming: !!confirmRequest,
			woke
		})
	);
	// The take's mode, for the wake flash's colour.
	const mode = $derived(takeMode === 'idle' ? null : takeMode);
	// Listening: the voice level drives the creature.
	const armed = $derived(listens(current));

	// The cursor over the creature's body, for its attentive look. The body's
	// hit element lets clicks fall through to the drag region (see the
	// styles), so the hover test is geometric.
	let stageEl: HTMLDivElement | undefined = $state();
	let hovered = $state(false);

	function onPointerMove(event: PointerEvent): void {
		const hit = stageEl?.querySelector('[data-interactive="creature"]');
		if (!hit) return;
		const r = hit.getBoundingClientRect();
		const x = (event.clientX - (r.left + r.width / 2)) / (r.width / 2);
		const y = (event.clientY - (r.top + r.height / 2)) / (r.height / 2);
		hovered = x * x + y * y <= 1;
	}

	// ---- Live mic-volume reactivity (armed listening) ----
	// The Rust capture worker streams `dictation:level` at ~20 Hz: a single
	// raw RMS scalar per event (no audio content — see security.md). We
	// peak-normalise so quiet mics and hot mics both fill the 0..1 range,
	// and ease at 60 fps for smooth motion between readings. The creature
	// takes it as its `level`: the lamp, the wobble and the pencil follow
	// the voice.
	//
	// Tuning lifted from `Waveform.svelte` (proved across M3/M4 hardware).
	// `LEVEL_PEAK_FLOOR` keeps room hiss from registering as speech;
	// `LEVEL_PEAK_DECAY` lets the calibration relax once the voice stops.
	const LEVEL_PEAK_FLOOR = 0.012;
	const LEVEL_PEAK_DECAY = 0.975;
	const LEVEL_EASE = 0.2;
	let liveLevel = $state(0);

	$effect(() => {
		// Re-runs whenever `armed` flips. Cleanup tears the subscription +
		// rAF loop down so we don't keep paying for them after the take ends.
		if (!armed) {
			liveLevel = 0;
			return;
		}
		// Reduced-motion: don't subscribe, leave level pinned at 0. The
		// creature holds its listening pose with a steady lamp, and the
		// ARIA-live region carries "listening".
		if (
			typeof window !== 'undefined' &&
			window.matchMedia('(prefers-reduced-motion: reduce)').matches
		) {
			return;
		}

		let target = 0;
		let smoothed = 0;
		let peak = LEVEL_PEAK_FLOOR;
		let stopped = false;
		let frame = 0;

		const unlistenPromise = listen<number>('dictation:level', (event) => {
			const raw = Math.max(0, event.payload);
			peak = Math.max(raw, peak * LEVEL_PEAK_DECAY, LEVEL_PEAK_FLOOR);
			target = Math.min(1, raw / peak);
		});

		const tick = () => {
			if (stopped) return;
			smoothed += (target - smoothed) * LEVEL_EASE;
			liveLevel = smoothed;
			frame = requestAnimationFrame(tick);
		};
		frame = requestAnimationFrame(tick);

		return () => {
			stopped = true;
			cancelAnimationFrame(frame);
			void unlistenPromise.then((u) => u()).catch(() => {});
			liveLevel = 0;
		};
	});

	// ---- Take elapsed-time counter (streaming UI) ----
	// A dictation take can now run up to the 5-minute backstop (docs/adr/0037),
	// so the streaming bubble shows how long the current take has been running.
	// Presentation only — the authoritative take length and its cap live in the
	// Rust FSM; this just counts from the `capturing` state and clears when the
	// take ends. A ticking value is information, not decorative motion, so it is
	// deliberately NOT gated by prefers-reduced-motion.
	const streamingTake = $derived(dictationState === 'capturing' && takeMode !== 'command');
	let elapsedLabel = $state('');

	function formatElapsed(ms: number): string {
		const total = Math.floor(ms / 1000);
		const mins = Math.floor(total / 60);
		const secs = total % 60;
		return `${mins}:${secs.toString().padStart(2, '0')}`;
	}

	$effect(() => {
		// Re-runs whenever the take starts/stops. Cleanup clears the interval so
		// it never ticks past the take.
		if (!streamingTake) {
			elapsedLabel = '';
			return;
		}
		const start = performance.now();
		elapsedLabel = formatElapsed(0);
		// 250 ms keeps the seconds readout snappy without a per-frame loop.
		const id = setInterval(() => {
			elapsedLabel = formatElapsed(performance.now() - start);
		}, 250);
		return () => clearInterval(id);
	});

	// ---- Prompter scroll (streaming UI) ----
	// On a long take the partial text would tower and grow the window unbounded.
	// Instead the text area is height-capped (CSS) and the latest line is pinned
	// to the bottom — old text scrolls up out of view, teleprompter-style.
	let partialScrollEl: HTMLParagraphElement | null = $state(null);
	let partialOverflowing = $state(false);

	$effect(() => {
		// Reference `partial` so this re-runs on every update, after the DOM has
		// patched in the new text.
		partial;
		const el = partialScrollEl;
		if (!el) return;
		el.scrollTop = el.scrollHeight; // stick to the newest text
		partialOverflowing = el.scrollHeight > el.clientHeight + 1;
	});

	// The stage is CONSTANT-SIZED across all states: per-state sizing resized
	// the window on every transition and the creature visibly jumped. State
	// changes only change what the creature paints inside it; the window
	// grows only when bubbles appear underneath. Its transparent margins are
	// functionally invisible thanks to click-through (`clickThrough.ts`).

	// Surface visibility — same rules as before, slightly tightened so we
	// never show two competing slabs at once. The confirm modal pre-empts
	// everything; the transcript pre-empts the tool/think indicator
	// because it carries the Cancel control the user might still need.
	const showTranscript = $derived(
		!!commandTranscript && !commandFlash && !confirmRequest
	);

	// Live dictation partials (docs/adr/0035). Shown during a dictation take
	// while there is committed or provisional text, and never alongside a
	// command surface (command takes have their own transcript bubble) or once
	// the take has ended. The panel grows the window via the ResizeObserver and
	// collapses when the parent clears `partial` on idle.
	const hasPartialText = $derived(
		!!partial && (partial.committed.length > 0 || partial.provisional.length > 0)
	);
	const showPartial = $derived(
		hasPartialText &&
			takeMode !== 'command' &&
			commandState === 'idle' &&
			!commandFlash &&
			!confirmRequest
	);
	const showCommandProgress = $derived(
		commandState !== 'idle' && !commandFlash && !confirmRequest && !showTranscript
	);

	const commandProgressLabel = $derived(
		commandState === 'tool' && commandToolLabel
			? commandToolLabel
			: $t('command.progress.thinking')
	);

	// ---- ResizeObserver-driven window sizing ----
	// `.tongue` is `display: inline-flex` so its `offsetSize` collapses
	// to its content. With the constant STAGE_SIZE, the WIDTH of `.tongue`
	// is stable across state changes (only stage width + padding) — the
	// only thing that changes is the HEIGHT, when bubbles appear or
	// disappear under the mark. Bubbles grow the window downward (Tauri
	// setSize keeps top-left fixed), and the mark sits at the top of the
	// flex column, so the MARK's screen position never changes from a
	// state transition. No recenter logic needed.
	function autoResize(node: HTMLElement): { destroy(): void } {
		let pendingFrame: number | undefined;
		let lastW = 0;
		let lastH = 0;
		const apply = async () => {
			pendingFrame = undefined;
			const w = Math.ceil(node.offsetWidth);
			const h = Math.ceil(node.offsetHeight);
			if (w === lastW && h === lastH) return;
			lastW = w;
			lastH = h;
			try {
				const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window');
				await getCurrentWindow().setSize(new LogicalSize(w, h));
			} catch (err) {
				void invoke('log_tongue_diag', {
					message: `setSize(${w}, ${h}) failed: ${String(err)}`
				}).catch(() => {});
			}
		};
		const observer = new ResizeObserver(() => {
			if (pendingFrame !== undefined) return;
			pendingFrame = requestAnimationFrame(() => void apply());
		});
		observer.observe(node);
		return {
			destroy() {
				observer.disconnect();
				if (pendingFrame !== undefined) cancelAnimationFrame(pendingFrame);
			}
		};
	}
</script>

<svelte:window
	onpointermove={onPointerMove}
	onpointerout={(e) => {
		// Left the window altogether.
		if (!e.relatedTarget) hovered = false;
	}}
/>

<!-- `.tongue` is the outer layout wrapper; it is INTENTIONALLY not marked
     `data-interactive` because its transparent padding/gap regions are
     where click-through happens. Only the actually-visible children
     (the creature's body, bubbles) carry the marker. The
     `clickThrough.ts` poll bbox-tests against those markers. -->
<!-- Pre-redesign drag setup, restored: `data-tauri-drag-region` on
     `.tongue` and `.mark-stage`. Clicks on the creature fall
     through `pointer-events: none` cascade to `.tongue`, where Tauri's
     native drag handler fires startDragging — native-smooth drag.
     The maximize-on-dblclick side effect is caught by the resize
     watcher in +page.svelte (`unmaximizeIfNeeded`). -->
<div
	class="tongue"
	use:autoResize
	data-tauri-drag-region
	data-interactive
>
	<!-- ─── Ottid ─── -->
	<!-- The creature fills a fixed stage and marks its body
	     `data-interactive="creature"` for the click-through poll. Pointer
	     events fall through to this drag region (see the styles), so the
	     native drag and the window's dblclick / contextmenu listeners keep
	     working. -->
	<div
		class="mark-stage"
		data-tauri-drag-region
		bind:this={stageEl}
		style="width: {STAGE.width}px; height: {STAGE.height}px;"
	>
		<Creature
			state={current}
			placement="float"
			gaze={null}
			level={liveLevel}
			wakeArmed={wakeActive}
			dragging={false}
			{hovered}
			pokes={0}
			{mode}
		/>
	</div>

	<!-- ─── Ephemeral surfaces. Stacked under the creature. ─── -->

	<!-- Live dictation partials (docs/adr/0035). Committed words render solid,
	     the provisional tail muted; `dir="auto"` keeps Hebrew RTL and isolates
	     mixed Hebrew/English runs. Visual only (aria-hidden) — the committed
	     text is announced once-settled by the polite sr-only region below, so a
	     screen reader hears stable words, not the flickering tail. -->
	{#if showPartial && partial}
		<div
			class="bubble bubble-partial"
			style="--tint: var(--saffron)"
			aria-hidden="true"
			data-interactive
		>
			<span class="bubble-wave" aria-hidden="true">
				<span></span><span></span><span></span><span></span>
			</span>
			<p
				class="partial-text"
				class:prompter-fade={partialOverflowing}
				dir="auto"
				bind:this={partialScrollEl}
			>
				<span class="partial-committed">{partial.committed}</span>
				{#if partial.provisional}
					<span class="partial-provisional" dir="auto">
						{partial.committed ? ' ' : ''}{partial.provisional}</span
					>
				{/if}
			</p>
			{#if elapsedLabel}
				<span class="partial-timer mono" aria-hidden="true">{elapsedLabel}</span>
			{/if}
		</div>
	{/if}

	<!-- Transcript preview — what STT heard, one line, with mini-wave + Cancel.
	     Same hue as the take mode (saffron for dict, garnet for cmd). -->
	{#if showTranscript}
		<div
			class="bubble bubble-transcript"
			style="--tint: {takeMode === 'command' ? 'var(--garnet)' : 'var(--saffron)'}"
			role="status"
			aria-live="polite"
			data-interactive
		>
			<span class="bubble-wave" aria-hidden="true">
				<span></span><span></span><span></span><span></span>
			</span>
			<p class="bubble-text" dir="auto">{commandTranscript}</p>
			{#if commandCancellable}
				<button
					type="button"
					class="bubble-cancel"
					onclick={onCancel}
					aria-label={$t('command.transcript.cancel')}
					title={$t('command.transcript.cancelHint')}
				>
					{$t('command.transcript.cancel')}
				</button>
			{/if}
		</div>
	{/if}

	<!-- Tool-chain status — gear-spin + tool name + Hebrew summary + step counter. -->
	{#if showCommandProgress}
		<div
			class="bubble bubble-tool"
			style="--tint: {commandState === 'tool' ? 'var(--garnet)' : 'var(--state-cloud)'}"
			dir="auto"
			role="status"
			aria-live="polite"
			data-interactive
		>
			<span class="bubble-spinner orbit-spin" aria-hidden="true">
				<StateGlyph
					kind={commandState === 'tool' ? 'gear' : 'orbit'}
					color={commandState === 'tool' ? 'var(--garnet)' : 'var(--state-cloud)'}
				/>
			</span>
			<div class="bubble-meta">
				{#if commandState === 'tool' && commandToolName}
					<div class="bubble-tool-name mono">{commandToolName}</div>
					<div class="bubble-tool-summary he-sans">{commandProgressLabel}</div>
				{:else}
					<div class="bubble-tool-summary he-sans italic">{commandProgressLabel}</div>
				{/if}
			</div>
			{#if commandToolStep}
				<span class="bubble-step mono">
					{commandToolStep.current} / {commandToolStep.total}
				</span>
			{/if}
		</div>
	{/if}

	<!-- Result flash — short success reply, fades in the parent. -->
	{#if commandFlash && !confirmRequest}
		<div
			class="bubble bubble-flash"
			dir="auto"
			role="status"
			aria-live="polite"
			data-interactive
		>
			<span class="flash-dot" aria-hidden="true"></span>
			<span class="he-sans">{commandFlash}</span>
		</div>
	{/if}

	<!-- Confirm modal — biggest surface, hugs the icon, rose-tinted. -->
	{#if confirmRequest}
		<div class="confirm-card" role="alertdialog" aria-live="assertive" data-interactive>
			<div class="confirm-header">
				<span class="confirm-dot" aria-hidden="true"></span>
				<span class="confirm-eyebrow he-sans"
					>{$t('command.confirm.requires') || 'דורש אישור'}</span
				>
			</div>
			<div class="confirm-question he">
				{$t('command.confirm.question').replace('{tool}', '')}
				<span class="confirm-tool mono">{confirmRequest.tool}</span>?
			</div>
			{#if confirmRequest.command_preview}
				<!-- `run_command`: render the literal command + cwd as
				     an untruncated code block. The user must be able to
				     read every character before approving a shell call
				     (`docs/stories/m8-os-tools.md`). -->
				<div class="confirm-command mono" dir="ltr">
					<div class="confirm-command-label">$</div>
					<pre class="confirm-command-body">{confirmRequest.command_preview}</pre>
				</div>
				{#if confirmRequest.cwd_preview}
					<div class="confirm-cwd mono" dir="ltr">
						<span class="confirm-cwd-label he-sans"
							>{$t('command.confirm.cwd') || 'cwd'}:</span
						>
						<code>{confirmRequest.cwd_preview}</code>
					</div>
				{/if}
			{:else if confirmRequest.args_preview && confirmRequest.args_preview !== '{}'}
				<div class="confirm-args mono" dir="ltr">
					{confirmRequest.args_preview.length > 240
						? confirmRequest.args_preview.slice(0, 240) + '…'
						: confirmRequest.args_preview}
				</div>
			{/if}
			<div class="confirm-actions">
				<button type="button" class="confirm-allow he-sans" onclick={onAllow}>
					{$t('command.confirm.allow')}
				</button>
				<button type="button" class="confirm-deny he-sans" onclick={onDeny}>
					{$t('command.confirm.deny')}
				</button>
			</div>
			<div class="confirm-hint mono" aria-hidden="true">↵ to confirm · Esc to deny</div>
		</div>
	{/if}

	<!-- Accessibility — sr-only announces every state change. -->
	<span class="sr-only" aria-live="polite" aria-atomic="true"
		>{current === 'idle' && wakeActive
			? $t('creature.wakeArmed')
			: $t(`creature.states.${current}`)}</span
	>
	<!-- Committed dictation text, announced politely as it settles. Only the
	     stable (committed) words are voiced — never the provisional tail — so a
	     screen reader isn't spammed by the ~2 Hz re-decode flicker. -->
	{#if showPartial && partial?.committed}
		<span class="sr-only" aria-live="polite" aria-atomic="true">{partial.committed}</span>
	{/if}
</div>

<style>
	/* The Tongue is `inline-flex` so its `offsetSize` collapses to content;
	   the autoResize action pushes that to the OS HWND so the window grows
	   exactly to fit. `min-width: 0` lets it shrink to the mark stage alone. */
	.tongue {
		display: inline-flex;
		flex-direction: column;
		align-items: center;
		padding: 14px 16px;
		box-sizing: border-box;
		gap: 8px;
		min-width: 0;
	}

	/* ─── Creature stage ─── */
	/* A fixed stage the creature fills (lib/creature/types.ts, STAGE). */
	.mark-stage {
		position: relative;
		/* The transparent corners of the stage MUST pass clicks to whatever's
		   underneath. The +page-level click-through plumbing relies on this. */
		pointer-events: none;
	}
	/* The body's hit area still marks the click-through region, but clicks
	   on it fall through to the ancestor .tongue, which carries
	   `data-tauri-drag-region` for the native drag and is where dblclick /
	   contextmenu bubble up from. */
	.mark-stage :global([data-interactive='creature']) {
		pointer-events: none;
	}

	/* ─── Bubble surfaces ─── */
	.bubble {
		max-width: 360px;
		padding: 11px 14px;
		border-radius: 14px;
		font-family: var(--font-he-sans);
		font-size: 13.5px;
		line-height: 1.45;
		color: var(--ink-text);
		background: rgba(11, 18, 22, 0.78);
		backdrop-filter: blur(24px) saturate(120%);
		-webkit-backdrop-filter: blur(24px) saturate(120%);
		box-shadow:
			0 1px 0 rgba(221, 228, 233, 0.06) inset,
			0 10px 32px rgba(0, 0, 0, 0.35),
			0 0 0 1px rgba(221, 228, 233, 0.08);
		direction: rtl;
		pointer-events: auto;
		animation: bubble-in 0.18s ease-out both;
		/* `--tint` is set by callers; lights up the border with the mode hue. */
		--tint: var(--state-cloud);
		--tint-soft: color-mix(in srgb, var(--tint) 33%, transparent);
	}
	.bubble {
		box-shadow:
			0 1px 0 rgba(221, 228, 233, 0.06) inset,
			0 10px 32px rgba(0, 0, 0, 0.35),
			0 0 0 1px rgba(221, 228, 233, 0.08),
			0 0 0 1.5px color-mix(in srgb, var(--tint) 33%, transparent),
			0 0 24px color-mix(in srgb, var(--tint) 20%, transparent);
	}

	/* ─── Transcript bubble ─── */
	.bubble-transcript {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.bubble-wave {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		flex: 0 0 auto;
	}
	.bubble-wave > span {
		width: 2px;
		height: 11px;
		border-radius: 2px;
		background: var(--tint);
		transform-origin: center;
		animation: bubble-wave-bar 0.8s ease-in-out infinite;
	}
	.bubble-wave > span:nth-child(2) {
		animation-delay: 0.12s;
	}
	.bubble-wave > span:nth-child(3) {
		animation-delay: 0.24s;
	}
	.bubble-wave > span:nth-child(4) {
		animation-delay: 0.36s;
	}
	.bubble-text {
		flex: 1;
		margin: 0;
		font-family: var(--font-he-sans);
		font-size: 14px;
		color: var(--ink-text);
		word-break: break-word;
	}

	/* ─── Live partial bubble ─── */
	.bubble-partial {
		display: flex;
		align-items: flex-start;
		gap: 10px;
	}
	.partial-text {
		flex: 1;
		margin: 0;
		font-family: var(--font-he-sans);
		font-size: 14px;
		line-height: 1.5;
		word-break: break-word;
		/* Prompter: cap the height and pin to the latest line (the effect sticks
		   scrollTop to the bottom) so a long take scrolls instead of towering the
		   window. ~5 lines tall. */
		max-height: 7.5em;
		overflow: hidden;
	}
	/* Fade old text out at the top — only while actually overflowing, so short
	   partials aren't clipped. */
	.partial-text.prompter-fade {
		-webkit-mask-image: linear-gradient(to bottom, transparent 0, #000 1.6em);
		mask-image: linear-gradient(to bottom, transparent 0, #000 1.6em);
	}
	/* Committed words are final — solid ink. */
	.partial-committed {
		color: var(--ink-text);
	}
	/* The provisional tail may still change — muted so the eye treats it as
	   not-yet-settled and committed words read as the stable transcript. */
	.partial-provisional {
		color: var(--ink-mute);
	}
	/* Elapsed-take timer — a small mono readout at the bubble's trailing edge so
	   the user sees how long the current take has run (up to the 5-min backstop,
	   docs/adr/0037). Tabular figures so the digits don't jiggle as they tick. */
	.partial-timer {
		flex: 0 0 auto;
		align-self: flex-start;
		margin-top: 1px;
		font-size: 11px;
		color: var(--ink-faint);
		font-variant-numeric: tabular-nums;
	}
	/* Cancel chip — destructive-style. Rose-tinted background + rose
	   border so it reads unambiguously as a clickable "cancel this take"
	   control inside the transcript bubble. The first pass used the
	   design source's subtle `--ink-faint` text on transparent
	   background, but at 38% opacity on the slate bubble it disappeared. */
	.bubble-cancel {
		background: color-mix(in srgb, var(--state-error) 18%, transparent);
		border: 1px solid color-mix(in srgb, var(--state-error) 55%, transparent);
		color: var(--ink-text);
		font-size: 11px;
		font-family: var(--font-he-sans);
		font-weight: 700;
		cursor: pointer;
		padding: 3px 9px;
		border-radius: 6px;
		flex: 0 0 auto;
		letter-spacing: 0.1px;
		transition:
			background 0.15s ease,
			border-color 0.15s ease;
	}
	.bubble-cancel:hover {
		background: color-mix(in srgb, var(--state-error) 32%, transparent);
		border-color: color-mix(in srgb, var(--state-error) 85%, transparent);
	}
	.bubble-cancel:focus-visible {
		outline: 2px solid var(--garnet);
		outline-offset: 2px;
	}

	/* ─── Tool bubble ─── */
	.bubble-tool {
		display: flex;
		gap: 10px;
		align-items: flex-start;
	}
	.bubble-spinner {
		flex: 0 0 auto;
		margin-top: 1px;
		display: inline-flex;
	}
	.bubble-meta {
		flex: 1;
		min-width: 0;
	}
	.bubble-tool-name {
		font-size: 13px;
		color: var(--garnet);
		font-weight: 600;
		letter-spacing: 0.2px;
	}
	.bubble-tool-summary {
		font-size: 12.5px;
		color: var(--ink-mute);
	}
	.bubble-tool-summary.italic {
		font-style: italic;
	}
	.bubble-step {
		font-size: 10px;
		color: var(--ink-faint);
		flex: 0 0 auto;
		align-self: flex-start;
		margin-top: 3px;
	}

	/* ─── Result flash ─── */
	.bubble-flash {
		display: flex;
		align-items: center;
		gap: 8px;
		max-width: 280px;
		--tint: var(--state-success);
	}
	.flash-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: var(--state-success);
		flex: 0 0 auto;
	}

	/* ─── Confirm card ─── */
	.confirm-card {
		width: 340px;
		padding: 16px;
		border-radius: 16px;
		background: rgba(11, 18, 22, 0.85);
		backdrop-filter: blur(28px) saturate(120%);
		-webkit-backdrop-filter: blur(28px) saturate(120%);
		color: var(--ink-text);
		direction: rtl;
		font-family: var(--font-he-sans);
		pointer-events: auto;
		animation: bubble-in 0.22s ease-out both;
		box-shadow:
			0 12px 40px rgba(0, 0, 0, 0.4),
			0 0 0 1px rgba(221, 228, 233, 0.10),
			0 0 0 1.5px color-mix(in srgb, var(--state-error) 33%, transparent),
			0 0 32px color-mix(in srgb, var(--state-error) 20%, transparent);
	}
	.confirm-header {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 10px;
	}
	.confirm-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: var(--state-error);
		flex: 0 0 auto;
	}
	.confirm-eyebrow {
		font-size: 11px;
		color: var(--state-error);
		font-weight: 700;
		letter-spacing: 1px;
		text-transform: uppercase;
	}
	.confirm-question {
		font-family: var(--font-he-display);
		font-size: 18px;
		font-weight: 500;
		line-height: 1.4;
		margin-bottom: 6px;
	}
	.confirm-tool {
		color: var(--garnet);
		font-family: var(--font-mono);
		font-size: 15px;
	}
	/* `run_command` preview: code block — no truncation. */
	.confirm-command {
		background: rgba(0, 0, 0, 0.45);
		padding: 10px 12px;
		border-radius: 8px;
		margin-bottom: 8px;
		display: flex;
		gap: 8px;
		align-items: flex-start;
		max-height: 180px;
		overflow: auto;
	}
	.confirm-command-label {
		color: var(--state-error);
		font-weight: 700;
		flex: 0 0 auto;
		opacity: 0.85;
	}
	.confirm-command-body {
		margin: 0;
		font-family: var(--font-mono);
		font-size: 13px;
		line-height: 1.45;
		color: var(--ink-text);
		white-space: pre-wrap;
		word-break: break-all;
	}
	.confirm-cwd {
		display: flex;
		gap: 6px;
		align-items: baseline;
		font-size: 11px;
		color: var(--ink-faint);
		margin-bottom: 8px;
	}
	.confirm-cwd-label {
		color: var(--ink-text);
		opacity: 0.65;
	}
	.confirm-args {
		background: rgba(0, 0, 0, 0.35);
		padding: 8px 10px;
		border-radius: 8px;
		font-size: 11.5px;
		color: var(--ink-mute);
		text-align: left;
		margin: 0 0 14px;
		box-shadow: inset 0 0 0 1px var(--ink-line);
		white-space: pre-wrap;
		word-break: break-all;
		max-height: 96px;
		overflow: auto;
	}
	.confirm-actions {
		display: flex;
		gap: 8px;
	}
	.confirm-allow,
	.confirm-deny {
		flex: 1;
		padding: 10px 12px;
		border-radius: 9px;
		font-family: var(--font-he-sans);
		font-weight: 700;
		font-size: 14px;
		cursor: pointer;
	}
	.confirm-allow {
		background: var(--state-error);
		color: #fff;
		border: none;
		box-shadow: 0 1px 0 rgba(255, 255, 255, 0.15) inset;
	}
	.confirm-allow:hover {
		filter: brightness(1.08);
	}
	.confirm-deny {
		background: transparent;
		color: var(--ink-text);
		border: 1px solid var(--ink-line-2);
		font-weight: 600;
	}
	.confirm-deny:hover {
		background: rgba(255, 255, 255, 0.04);
		border-color: rgba(221, 228, 233, 0.3);
	}
	.confirm-allow:focus-visible,
	.confirm-deny:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}
	.confirm-hint {
		font-size: 9.5px;
		color: var(--ink-faint);
		margin-top: 10px;
		text-align: center;
		letter-spacing: 0.5px;
		direction: ltr;
	}

	/* ─── Reusable Hebrew/Latin font classes (matches the design's `he`,
	     `he-sans`, `mono`) — only what we actually use here. ─── */
	:global(.he-sans) {
		font-family: var(--font-he-sans);
	}
	:global(.he) {
		font-family: var(--font-he-display);
		direction: rtl;
	}
	:global(.mono) {
		font-family: var(--font-mono);
		direction: ltr;
	}

	/* ─── Keyframes ─── */
	.orbit-spin {
		animation: tongue-orbit 6s linear infinite;
		transform-origin: center;
	}

	@keyframes tongue-orbit {
		0% {
			transform: rotate(0);
		}
		100% {
			transform: rotate(360deg);
		}
	}
	@keyframes bubble-in {
		from {
			opacity: 0;
			transform: translateY(-4px);
		}
		to {
			opacity: 1;
			transform: translateY(0);
		}
	}
	@keyframes bubble-wave-bar {
		0%,
		100% {
			transform: scaleY(0.35);
		}
		50% {
			transform: scaleY(1);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.orbit-spin,
		.bubble-wave > span,
		.bubble {
			animation: none;
		}
	}

	/* Visually hidden, still read aloud by screen readers. */
	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		margin: -1px;
		padding: 0;
		border: 0;
		overflow: hidden;
		clip: rect(0 0 0 0);
		clip-path: inset(50%);
		white-space: nowrap;
	}
</style>
