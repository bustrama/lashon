<script lang="ts">
	// The Tongue is a transparent, frameless, always-on-top overlay — just
	// the Lashon mark, floating and draggable.
	//
	// REDESIGN — "המנורה / The Lamp" direction:
	// - The mark's resting color is the locked peach. State is communicated
	//   by a soft radial halo + a small supporting glyph in a circle at the
	//   bottom-left of the mark.
	// - Armed states (dictation / command / chat) tint the MARK itself to
	//   the mode hue (saffron / garnet / indigo) for max glance-signal.
	//   Active "doing" states keep the mark peach and let the halo carry
	//   the state.
	// - The glyph is mandatory: reduced-motion users still need to read
	//   state from a single frame, so motion alone is not a signal.
	//
	// State derivation lives here — the parent feeds raw dictation +
	// command-mode flags and we collapse them to one of 13 tongue states.
	// Wake-listening and chat are wired but unreachable from current code
	// (no chat mode yet, wake-active flag not yet plumbed; both light up
	// the moment the parent passes them).
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { t } from '$lib/i18n';
	import type { DictationState, DictationPartial } from '$lib/dictation';
	import Mark from '$lib/components/Mark.svelte';
	import StateGlyph from '$lib/components/StateGlyph.svelte';

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

	// `chat` is reserved for M9 (chat mode + TTS). Re-add it to this union
	// and re-add the matching branches in `halo` / `glyph` / `motionClass`
	// when chat mode ships.
	type TongueState =
		| 'idle'
		| 'prep'
		| 'dict'
		| 'cmd'
		| 'transcribe'
		| 'think'
		| 'tool'
		| 'confirm'
		| 'wake'
		| 'error';

	// State precedence: most urgent → most quiescent. The confirm modal
	// outranks everything; the tool / thinking phases outrank the listening
	// phases that produced them; wake-listening only shows when nothing
	// else is happening.
	const tongueState: TongueState = $derived.by(() => {
		if (confirmRequest) return 'confirm';
		if (commandState === 'tool') return 'tool';
		if (commandState === 'thinking') return 'think';
		if (dictationState === 'transcribing') return 'transcribe';
		if (dictationState === 'capturing') {
			return takeMode === 'command' ? 'cmd' : 'dict';
		}
		if (dictationState === 'preparing') return 'prep';
		if (dictationState === 'error') return 'error';
		if (wakeActive) return 'wake';
		return 'idle';
	});

	// Halo color, intensity, blur, and the radial-gradient fade stop per
	// state. Idle / wake use the locked peach so the brand identity holds
	// when nothing's happening; armed listening states use their distinct
	// mode hue.
	//
	// `fade` is the % where the radial gradient hits transparent. SMALLER
	// values = tighter, more focused glow that fades to clear earlier and
	// doesn't fill the visible window like a solid color slab. The first
	// pass had fade=62 across the board which made the saffron halo look
	// like a solid yellow rectangle when listening — the halo's most-opaque
	// CENTER was bigger than the visible window.
	const halo = $derived.by(() => {
		switch (tongueState) {
			case 'idle':
				return { color: 'var(--peach)', intensity: 0, blur: 22, fade: 50 };
			case 'wake':
				return { color: 'var(--state-cloud)', intensity: 0.4, blur: 22, fade: 45 };
			case 'prep':
				return { color: 'var(--state-cloud)', intensity: 0.32, blur: 22, fade: 50 };
			case 'dict':
				return { color: 'var(--saffron)', intensity: 0.55, blur: 28, fade: 38 };
			case 'cmd':
				return { color: 'var(--garnet)', intensity: 0.55, blur: 28, fade: 38 };
			case 'transcribe':
				return { color: 'var(--saffron)', intensity: 0.32, blur: 22, fade: 45 };
			case 'think':
				return { color: 'var(--state-cloud)', intensity: 0.4, blur: 22, fade: 48 };
			case 'tool':
				return { color: 'var(--garnet)', intensity: 0.4, blur: 22, fade: 48 };
			case 'confirm':
				return { color: 'var(--state-error)', intensity: 0.5, blur: 22, fade: 48 };
			case 'error':
				return { color: 'var(--state-error)', intensity: 0.45, blur: 22, fade: 48 };
		}
	});

	// The MARK is peach by default. Armed listening states tint it to the
	// mode hue + add a matching drop-shadow so the mark itself carries the
	// signal alongside the halo. (When M9 adds chat, include 'chat' here.)
	const armed = $derived(tongueState === 'dict' || tongueState === 'cmd');
	const markColor = $derived(armed ? halo.color : 'var(--peach)');
	const markGlow = $derived(armed ? halo.color : null);

	// ---- Live mic-volume reactivity (armed listening) ----
	// The Rust capture worker streams `dictation:level` at ~20 Hz: a single
	// raw RMS scalar per event (no audio content — see security.md). We
	// peak-normalise so quiet mics and hot mics both fill the 0..1 range,
	// ease at 60 fps for smooth motion between readings, then publish a
	// `--live-level` custom property on the mark stage. CSS reads it to
	// scale the mark + halo + sonar glow in time with the voice — the
	// "alive and reacts to volume" the old Waveform component delivered.
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
		// sonar rings are already hidden in that mode and the mark sits in
		// its armed tint — the ARIA-live region carries "listening".
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

	type GlyphKind =
		| 'pen'
		| 'gear'
		| 'gear-spin'
		| 'bubble'
		| 'dots'
		| 'orbit'
		| 'spark'
		| 'wave'
		| 'question'
		| 'antenna'
		| 'cross'
		| 'ring';

	// Glyph kind per state (the bottom-left supporting mark).
	const glyph: { kind: GlyphKind; show: boolean } = $derived.by(() => {
		switch (tongueState) {
			case 'idle':
				return { kind: 'pen', show: false };
			case 'prep':
				return { kind: 'ring', show: true };
			case 'dict':
				return { kind: 'pen', show: true };
			case 'cmd':
				return { kind: 'gear', show: true };
			case 'transcribe':
				return { kind: 'dots', show: true };
			case 'think':
				return { kind: 'orbit', show: true };
			case 'tool':
				return { kind: 'gear-spin', show: true };
			case 'confirm':
				return { kind: 'question', show: true };
			case 'wake':
				return { kind: 'antenna', show: true };
			case 'error':
				return { kind: 'cross', show: true };
		}
	});

	// Motion class per state. Reduced-motion disables them all in CSS.
	const motionClass = $derived.by(() => {
		switch (tongueState) {
			case 'idle':
			case 'wake':
				return 'tongue-anim-breath-slow';
			case 'prep':
			case 'think':
			case 'tool':
				return 'tongue-anim-breath-med';
			case 'dict':
			case 'cmd':
				return 'tongue-anim-pulse-fast';
			case 'transcribe':
				return 'tongue-anim-shimmer';
			case 'confirm':
				return 'tongue-anim-breath-fast';
			case 'error':
				return '';
		}
	});

	const MARK_SIZE = 96; // overall mark size in px

	// `.mark-stage` is CONSTANT-SIZED across all states (= the max needed
	// for any decoration the armed sonar can render). Per-state sizing
	// caused the window to resize on every state transition, and the
	// recenter logic to compensate for Win32's "keep top-left fixed"
	// behaviour was never reliable — the mark visibly jumped every
	// idle ↔ listening ↔ transcribing flip.
	//
	// With a constant stage: state changes only affect what's PAINTED
	// inside the stage (halo, sonar rings, glyph badge appear/disappear).
	// No reflow → no window resize → no mark jump. Decorations change
	// around a stationary mark, which is exactly what the user asked
	// for ("things needs to change around the lashon").
	//
	// Budget: armed sonar peaks at scale 2.2 on the 96 px mark = 211 px,
	// rounded up to 221 for breathing room. ~62 px of transparent buffer
	// around the mark on each side; click-through (`clickThrough.ts`)
	// makes the buffer functionally invisible to the user.
	const STAGE_SIZE = Math.ceil(MARK_SIZE * 2.3);

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

<!-- `.tongue` is the outer layout wrapper; it is INTENTIONALLY not marked
     `data-interactive` because its transparent padding/gap regions are
     where click-through happens. Only the actually-visible children
     (mark, halo-emitting bits, bubbles) carry the marker. The
     `clickThrough.ts` poll bbox-tests against those markers. -->
<!-- Pre-redesign drag setup, restored: `data-tauri-drag-region` on
     `.tongue` and `.mark-stage`. Clicks on the visible mark fall
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
	<!-- ─── The mark + its halo / glyph ─── -->
	<div
		class="mark-stage"
		data-tauri-drag-region
		style="width: {STAGE_SIZE}px; height: {STAGE_SIZE}px; --live-level: {liveLevel};"
	>
		<!-- Outer halo — soft radial-gradient blur. Only painted when intensity > 0.
		     Sized so its FULLY-TRANSPARENT outer edge lands just outside the
		     visible window: with mark 96 px + 14 px padding, the window is
		     ~124 px wide, so a 180 px halo with a 38% fade-stop puts the
		     fade-out band inside the window edges rather than seeing only
		     the saturated center. -->
		{#if halo.intensity > 0}
			<div
				class="halo {armed
					? 'halo-live'
					: tongueState === 'idle'
						? ''
						: 'halo-anim-pulse-slow'}"
				style="
					--halo-color: {halo.color};
					--halo-intensity: {halo.intensity};
					--halo-blur: {halo.blur}px;
					--halo-size: {MARK_SIZE * (armed ? 1.85 : 1.65)}px;
					--halo-fade: {halo.fade}%;
				"
				aria-hidden="true"
			></div>
		{/if}

		<!-- Wake-listening: a faint quiet ring centered on the mark. Positioned
		     via top/left: 50% + transform translate(-50%, -50%) so the
		     halo-pulse-slow animation (which embeds the same translate to
		     keep the .halo element centered) breathes it in place without
		     offsetting. -->
		{#if tongueState === 'wake'}
			<div
				class="wake-ring halo-anim-pulse-slow"
				style="--halo-intensity: 0.4;"
				aria-hidden="true"
			></div>
		{/if}

		<!-- Armed listening (dictation / command): three concentric "sonar"
		     rings emanating outward from the mark, in the mode hue. Staggered
		     delays (0 / 0.8 / 1.6 s) read as a continuous wave rather than
		     three synced pulses. Reduced-motion hides them entirely (the
		     halo + mark scale-pulse still convey state). -->
		{#if armed}
			{#each [0, 0.8, 1.6] as delay (delay)}
				<!-- Border-color intensity + box-shadow glow track `--live-level`
				     so each sonar ping "blooms" with the voice while the ambient
				     keyframe keeps the scale/opacity sweep going (so the user can
				     still see they're armed in silence). border-color and
				     box-shadow are NOT animated by the keyframe, so they layer
				     cleanly on top — the keyframe only touches transform + opacity. -->
				<div
					class="sonar-ring"
					style="
						width: {MARK_SIZE}px;
						height: {MARK_SIZE}px;
						border: 1.5px solid color-mix(in srgb, {halo.color} calc(40% + var(--live-level, 0) * 60%), transparent);
						animation-delay: {delay}s;
						box-shadow: 0 0 calc(8px + var(--live-level, 0) * 18px) color-mix(in srgb, {halo.color} calc(30% + var(--live-level, 0) * 50%), transparent);
					"
					aria-hidden="true"
				></div>
			{/each}
		{/if}

		<!-- Inner 96×96 wrapper for the mark + its tight decorations
		     (state-ring, prep-ring, glyph-badge). The OUTER `.mark-stage`
		     may be much larger than the mark (to accommodate halo/sonar
		     overflow), but these decorations need to sit at the MARK's
		     corner, not the stage's corner. -->
		<div class="mark-and-badge">
			<!-- Preparing: a progress ring around the mark. -->
			{#if tongueState === 'prep'}
				<svg
					class="prep-ring"
					width={MARK_SIZE + 24}
					height={MARK_SIZE + 24}
					viewBox="0 0 {MARK_SIZE + 24} {MARK_SIZE + 24}"
					aria-hidden="true"
				>
					<circle
						cx={(MARK_SIZE + 24) / 2}
						cy={(MARK_SIZE + 24) / 2}
						r={MARK_SIZE / 2 + 6}
						stroke="var(--peach)"
						stroke-width="1"
						fill="none"
						opacity="0.18"
					/>
					<circle
						cx={(MARK_SIZE + 24) / 2}
						cy={(MARK_SIZE + 24) / 2}
						r={MARK_SIZE / 2 + 6}
						stroke="var(--saffron)"
						stroke-width="2"
						fill="none"
						stroke-dasharray="{(MARK_SIZE / 2 + 6) * 2 * Math.PI * 0.42} {(MARK_SIZE / 2 +
							6) *
							2 *
							Math.PI}"
						stroke-linecap="round"
						transform="rotate(-90 {(MARK_SIZE + 24) / 2} {(MARK_SIZE + 24) / 2})"
					/>
				</svg>
			{/if}
			<!-- Confirm / Error: solid ring around the mark for
			     non-motion legibility. -->
			{#if tongueState === 'confirm' || tongueState === 'error'}
				<div
					class="state-ring"
					style="--ring-color: {halo.color}"
					aria-hidden="true"
				></div>
			{/if}

			<!-- The mark itself. Tinted to mode hue when armed; peach otherwise.
			     `pointer-events: none` lets clicks fall through to the
			     ancestor `.tongue` (which carries `data-tauri-drag-region`),
			     so the native Tauri drag handler runs AND the click events
			     still bubble up to the window's dblclick / contextmenu
			     listeners in +page.svelte. -->
			<div class="mark-anim {armed ? 'mark-anim-live' : motionClass}">
				<Mark size={MARK_SIZE} color={markColor} glow={markGlow} />
			</div>

			<!-- Supporting glyph — small circle at bottom-left (RTL-friendly). -->
			{#if glyph.show}
				<div
					class="glyph-badge"
					style="--badge-ring: {halo.color}"
					aria-hidden="true"
					data-interactive
				>
					<StateGlyph kind={glyph.kind} color={halo.color} />
				</div>
			{/if}
		</div>
	</div>

	<!-- ─── Ephemeral surfaces. Stacked under the mark. ─── -->

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
			<p class="partial-text" dir="auto">
				<span class="partial-committed">{partial.committed}</span>
				{#if partial.provisional}
					<span class="partial-provisional" dir="auto">
						{partial.committed ? ' ' : ''}{partial.provisional}</span
					>
				{/if}
			</p>
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

	<!-- Accessibility — sr-only announces every lifecycle change. -->
	<span class="sr-only" aria-live="polite" aria-atomic="true"
		>{$t(`tongue.${dictationState}`)}</span
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

	/* ─── Mark stage ─── */
	/* `.mark-stage` is sized dynamically (via inline style from the
	   `stageSize` derivation) to contain every absolutely-positioned
	   decoration the current state renders — halo, sonar rings, wake
	   ring — without clipping at the window edge. The mark + badge live
	   in an inner 96×96 `.mark-and-badge` wrapper centered via flex, so
	   the badge always sits at the MARK's corner regardless of how big
	   the surrounding stage gets. */
	.mark-stage {
		position: relative;
		display: flex;
		align-items: center;
		justify-content: center;
		/* The transparent corners of the stage MUST pass clicks to whatever's
		   underneath. The +page-level click-through plumbing relies on this. */
		pointer-events: none;
	}
	.mark-and-badge {
		position: relative;
		width: 96px;
		height: 96px;
		display: flex;
		align-items: center;
		justify-content: center;
		/* The mark IS the visible content here; corners pass clicks through
		   so the click-through poll's bbox check on .mark-anim still
		   covers exactly the mark, not this wrapper. */
		pointer-events: none;
	}

	.halo {
		position: absolute;
		top: 50%;
		left: 50%;
		width: var(--halo-size);
		height: var(--halo-size);
		transform: translate(-50%, -50%);
		border-radius: 50%;
		/* `--halo-fade` is the stop at which the gradient hits transparent.
		   Smaller = tighter, more focused glow. Lower values stop the
		   halo from filling the visible window like a solid color slab. */
		background: radial-gradient(
			circle,
			var(--halo-color) 0%,
			transparent var(--halo-fade, 50%)
		);
		opacity: var(--halo-intensity);
		filter: blur(var(--halo-blur));
		pointer-events: none;
	}

	/* Wake-listening — a faint quiet cloud-toned ring centered on the
	   mark. Cloud (steel grey) reads as PASSIVE; the brand peach is
	   reserved for the mark itself. Positioned via top/left: 50% +
	   transform translate(-50%, -50%) so it matches the .halo element's
	   anchor and the halo-pulse-slow animation breathes it without
	   offsetting (the keyframes embed the same translate). */
	.wake-ring {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 116px; /* MARK_SIZE + 20 */
		height: 116px;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		border: 1px solid var(--state-cloud);
		opacity: 0.4;
		pointer-events: none;
	}

	/* Sonar rings — three concentric rings expanding outward from the
	   mark during armed listening. Each starts small + opaque and grows
	   to ~2.2× while fading to clear, like sonar pings. Staggered delays
	   on each instance produce a continuous wave.

	   `animation-fill-mode: backwards` is critical: without it, rings 2
	   and 3 (delays 0.8s / 1.6s) show their NORMAL CSS state during the
	   delay — without the keyframe's `translate(-50%, -50%)`, so they sit
	   with their top-left corner at stage center, extending into the
	   lower-right quadrant for up to 1.6 s before animating in. Adding
	   matching initial `transform` + `opacity` belt-and-braces it against
	   any frame where the animation registration hasn't taken effect. */
	.sonar-ring {
		position: absolute;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%) scale(0.6);
		opacity: 0.55;
		border-radius: 50%;
		pointer-events: none;
		animation: tongue-sonar 2.4s cubic-bezier(0.2, 0.6, 0.3, 1) infinite;
		animation-fill-mode: backwards;
	}
	@keyframes tongue-sonar {
		0% {
			transform: translate(-50%, -50%) scale(0.6);
			opacity: 0.55;
		}
		80% {
			opacity: 0;
		}
		100% {
			transform: translate(-50%, -50%) scale(2.2);
			opacity: 0;
		}
	}

	.prep-ring {
		position: absolute;
		top: -12px;
		left: -12px;
		pointer-events: none;
	}

	.state-ring {
		position: absolute;
		inset: -8px;
		border-radius: 50%;
		border: 2px solid var(--ring-color);
		box-shadow: 0 0 0 1px var(--ink);
		pointer-events: none;
	}

	.mark-anim {
		position: relative;
		display: inline-block;
		transform-origin: center;
		/* Clicks on the visible mark fall through to the ancestor .tongue
		   (which carries `data-tauri-drag-region` for drag and is where
		   the click events bubble up from for dblclick / contextmenu). */
		pointer-events: none;
	}

	.glyph-badge {
		position: absolute;
		bottom: -2px;
		inset-inline-start: -2px;
		width: 22px;
		height: 22px;
		border-radius: 50%;
		background: var(--ink);
		box-shadow: 0 0 0 1.5px var(--badge-ring);
		display: flex;
		align-items: center;
		justify-content: center;
		pointer-events: auto;
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
	.tongue-anim-breath-slow {
		animation: tongue-breath-slow 5.5s ease-in-out infinite;
	}
	.tongue-anim-breath-med {
		animation: tongue-breath-med 2.4s ease-in-out infinite;
	}
	.tongue-anim-breath-fast {
		animation: tongue-breath-fast 0.95s ease-in-out infinite;
	}
	.tongue-anim-pulse-fast {
		animation: tongue-pulse-fast 1.1s ease-in-out infinite;
	}
	.tongue-anim-shimmer {
		animation: tongue-shimmer 1.6s linear infinite;
	}

	/* Level-driven mark motion (armed listening). Replaces the time-based
	   pulse-fast keyframe for `dict`/`cmd` — the mark physically scales up
	   with voice loudness instead of breathing on a fixed cadence. Silence
	   = rest size; loud peak ≈ +14%. The brightness bump adds a glance-
	   visible "warm-up" without changing the mark's tinted color.
	   Transition matches one rAF frame (≈33 ms at 60 fps) so the motion
	   stays in sync with the JS easing without double-smoothing too hard. */
	.mark-anim-live {
		transform: scale(calc(1 + var(--live-level, 0) * 0.14));
		filter: brightness(calc(1 + var(--live-level, 0) * 0.18));
		transition:
			transform 33ms linear,
			filter 33ms linear;
	}

	/* Level-driven halo (armed listening). Replaces halo-anim-pulse for
	   `dict`/`cmd`. Opacity gets a +0.3 boost at peak voice; scale gets
	   +35%. Keeps the static `.halo`'s centering translate so the halo
	   stays anchored on the mark while it inflates with the voice. */
	.halo.halo-live {
		opacity: calc(var(--halo-intensity, 0.55) + var(--live-level, 0) * 0.3);
		transform: translate(-50%, -50%) scale(calc(1 + var(--live-level, 0) * 0.35));
		transition:
			opacity 33ms linear,
			transform 33ms linear;
	}

	.halo-anim-pulse {
		animation: halo-pulse 1.4s ease-in-out infinite;
	}
	.halo-anim-pulse-slow {
		animation: halo-pulse 3.5s ease-in-out infinite;
	}
	.orbit-spin {
		animation: tongue-orbit 6s linear infinite;
		transform-origin: center;
	}

	@keyframes tongue-breath-slow {
		0%,
		100% {
			transform: scale(1);
		}
		50% {
			transform: scale(1.04);
		}
	}
	@keyframes tongue-breath-med {
		0%,
		100% {
			transform: scale(1);
		}
		50% {
			transform: scale(1.06);
		}
	}
	@keyframes tongue-breath-fast {
		0%,
		100% {
			transform: scale(1);
		}
		50% {
			transform: scale(1.09);
		}
	}
	/* Listening pulse — combines a visible scale-up with a brightness
	   pulse. The previous opacity-only pulse (1 → 0.78) was barely
	   perceptible behind a 38px-blur halo and gave the impression that
	   nothing was moving even though the take was live. */
	@keyframes tongue-pulse-fast {
		0%,
		100% {
			transform: scale(1);
			filter: brightness(1);
		}
		50% {
			transform: scale(1.08);
			filter: brightness(1.15);
		}
	}
	@keyframes tongue-shimmer {
		0% {
			filter: brightness(1);
		}
		50% {
			filter: brightness(1.2);
		}
		100% {
			filter: brightness(1);
		}
	}
	/* Halo pulse — breathes outward + intensifies. Bigger swing than the
	   first pass (1 → 1.25 scale, +0.25 opacity) so the listening state is
	   unambiguously animated, even behind a heavy blur. */
	@keyframes halo-pulse {
		0%,
		100% {
			opacity: var(--halo-intensity, 0.55);
			transform: translate(-50%, -50%) scale(1);
		}
		50% {
			opacity: calc(var(--halo-intensity, 0.55) + 0.25);
			transform: translate(-50%, -50%) scale(1.25);
		}
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
		.tongue-anim-breath-slow,
		.tongue-anim-breath-med,
		.tongue-anim-breath-fast,
		.tongue-anim-pulse-fast,
		.tongue-anim-shimmer,
		.halo-anim-pulse,
		.halo-anim-pulse-slow,
		.orbit-spin,
		.bubble-wave > span,
		.bubble {
			animation: none;
		}
		/* Sonar rings get hidden, not just paused — the halo + mark
		   scale-pulse still convey state, and a frozen ring frame would
		   look like dead UI. */
		.sonar-ring {
			animation: none;
			opacity: 0 !important;
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
