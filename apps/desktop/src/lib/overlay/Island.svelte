<script lang="ts">
	// The island: the cards that open next to Ottid — the live dictation
	// text, the command transcript with its Cancel, the tool progress, the
	// result flash and the approval card. It opens on the side of the stage
	// the window has room on (`side`), with the newest card nearest Ottid.
	//
	// Only cards with something to click or hover take the mouse
	// (`data-interactive="island"`). The live text and the progress line are
	// read-only, so clicks pass through them to the app underneath.
	import type { Snippet } from 'svelte';
	import { t } from '$lib/i18n';
	import type { DictationPartial } from '$lib/dictation';
	import type { CommandState, TakeMode } from '$lib/creature';
	import StateGlyph from '$lib/components/StateGlyph.svelte';
	import { CLOSE_EASING, CLOSE_MS, prefersReducedMotion, springEasing } from '$lib/motion/spring';
	import ApprovalCardView from './ApprovalCard.svelte';
	import {
		announcement,
		type ApprovalCard,
		type ApprovalDecision,
		type ApprovalNudge
	} from './approval';
	import type { IslandSide } from './frame';
	import type { AgentActivity } from '$lib/agent/activity';

	let {
		side,
		listening,
		takeMode,
		partial,
		commandState,
		commandToolName,
		commandToolLabel,
		commandTranscript,
		commandCancellable,
		commandFlash,
		agentActivity = null,
		agentSessions = [],
		approval,
		approvalNudge,
		onApprovalArmed,
		onApprovalAnswer,
		onCancel,
		extra
	}: {
		side: IslandSide;
		/** A dictation take is capturing (drives the elapsed timer). */
		listening: boolean;
		takeMode: TakeMode;
		partial: DictationPartial | null;
		commandState: CommandState;
		commandToolName: string | null;
		commandToolLabel: string | null;
		commandTranscript: string | null;
		commandCancellable: boolean;
		commandFlash: string | null;
		agentActivity?: string | null;
		agentSessions?: AgentActivity[];
		/** The request the approval broker shows (docs/adr/0048). */
		approval: ApprovalCard | null;
		/** The latest early press of the Allow hotkey. */
		approvalNudge: ApprovalNudge | null;
		onApprovalArmed: (id: number) => void;
		onApprovalAnswer: (id: number, decision: ApprovalDecision) => void;
		onCancel: () => void;
		/** Another card to show (the debug surface). */
		extra?: Snippet;
	} = $props();

	// One card at a time per kind, in a fixed precedence: the approval card
	// pre-empts everything, the transcript (it carries Cancel) pre-empts the
	// progress line, and the live text only shows during a dictation take.
	const showFlash = $derived(!!commandFlash && !approval);
	const showTranscript = $derived(!!commandTranscript && !commandFlash && !approval);
	const showProgress = $derived(
		commandState !== 'idle' && !commandFlash && !approval && !showTranscript
	);
	const hasPartialText = $derived(
		!!partial && (partial.committed.length > 0 || partial.provisional.length > 0)
	);
	const showPartial = $derived(
		hasPartialText &&
			takeMode !== 'command' &&
			commandState === 'idle' &&
			!commandFlash &&
			!approval
	);

	const progressLabel = $derived(
		commandState === 'tool' && commandToolLabel ? commandToolLabel : $t('command.progress.thinking')
	);

	// ---- Take elapsed time ----
	// A take can run to the 5-minute backstop (docs/adr/0037), so the live
	// text shows how long it has been running. A ticking value is
	// information, not decoration, so reduced motion doesn't stop it.
	let elapsedLabel = $state('');
	function formatElapsed(ms: number): string {
		const total = Math.floor(ms / 1000);
		return `${Math.floor(total / 60)}:${(total % 60).toString().padStart(2, '0')}`;
	}
	$effect(() => {
		if (!listening || takeMode === 'command') {
			elapsedLabel = '';
			return;
		}
		const start = performance.now();
		elapsedLabel = formatElapsed(0);
		const id = setInterval(() => (elapsedLabel = formatElapsed(performance.now() - start)), 250);
		return () => clearInterval(id);
	});

	// ---- Prompter scroll ----
	// The live text is height-capped and pinned to its newest line, so a long
	// take scrolls up like a teleprompter instead of towering.
	let partialScrollEl: HTMLParagraphElement | null = $state(null);
	let partialOverflowing = $state(false);
	$effect(() => {
		void partial;
		const el = partialScrollEl;
		if (!el) return;
		el.scrollTop = el.scrollHeight;
		partialOverflowing = el.scrollHeight > el.clientHeight + 1;
	});

	// ---- Approval ----
	// The overlay never has focus, so a screen reader meets the card through
	// live regions: the assertive one reads the whole request once, when it
	// is shown; the polite one says why Allow isn't taken yet. Each is
	// emptied first, so the same words twice are still announced.
	let approvalAnnounced = $state('');
	let approvalSaid = $state('');
	let announcedId: number | null = null;
	let announceTimer: ReturnType<typeof setTimeout> | undefined;
	let sayTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		const card = approval;
		const id = card?.id ?? null;
		if (id === announcedId) return;
		announcedId = id;
		clearTimeout(announceTimer);
		approvalAnnounced = '';
		if (card) {
			const text = announcement(card, $t);
			announceTimer = setTimeout(() => (approvalAnnounced = text), 60);
		}
	});
	function say(text: string): void {
		clearTimeout(sayTimer);
		approvalSaid = '';
		sayTimer = setTimeout(() => (approvalSaid = text), 60);
	}
	$effect(() => () => {
		clearTimeout(announceTimer);
		clearTimeout(sayTimer);
	});

	// The card has finished opening; Allow's arm delay counts from here.
	let approvalSettled = $state(false);
	$effect(() => {
		if (!approval) approvalSettled = false;
		else if (prefersReducedMotion()) approvalSettled = true;
	});

	// ---- Open and close ----
	// Cards spring open toward the user and close on an eased curve with no
	// overshoot. Reduced motion shows and hides them at once.
	const OPEN = springEasing(0.5, 0.72);
	const toward = $derived(side === 'above' ? 1 : -1);

	function cardIn(_node: Element) {
		if (prefersReducedMotion()) return { duration: 0 };
		const dy = 10 * toward;
		return {
			duration: OPEN.duration,
			easing: OPEN.easing,
			css: (t: number, u: number) =>
				`opacity: ${Math.min(1, t * 1.6)}; transform: translateY(${u * dy}px) scale(${0.96 + 0.04 * t});`
		};
	}
	function cardOut(_node: Element) {
		if (prefersReducedMotion()) return { duration: 0 };
		const dy = 6 * toward;
		return {
			duration: CLOSE_MS,
			easing: CLOSE_EASING,
			css: (t: number, u: number) =>
				`opacity: ${t}; transform: translateY(${u * dy}px) scale(${0.97 + 0.03 * t});`
		};
	}
</script>

<div class="island {side}">
	{#if extra}
		<div class="card-extra" data-interactive="island" in:cardIn out:cardOut>
			{@render extra()}
		</div>
	{/if}

	{#if approval}
		<!-- The next request in the queue swaps in place: the card opens once. -->
		<div
			class="card-approval"
			data-interactive="island"
			in:cardIn
			out:cardOut
			onintroend={() => (approvalSettled = true)}
		>
			{#key approval.id}
				<ApprovalCardView
					card={approval}
					settled={approvalSettled}
					nudge={approvalNudge}
					onArmed={onApprovalArmed}
					onAnswer={onApprovalAnswer}
					onSay={say}
				/>
			{/key}
		</div>
	{/if}

	{#if showFlash}
		<div
			class="bubble bubble-flash"
			dir="auto"
			role="status"
			aria-live="polite"
			data-interactive="island"
			in:cardIn
			out:cardOut
		>
			<span class="flash-dot" aria-hidden="true"></span>
			<span class="he-sans">{commandFlash}</span>
		</div>
	{/if}

	{#if showProgress}
		<div
			class="bubble bubble-tool"
			style="--tint: {commandState === 'tool' ? 'var(--garnet)' : 'var(--state-cloud)'}"
			dir="auto"
			role="status"
			aria-live="polite"
			in:cardIn
			out:cardOut
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
					<div class="bubble-tool-summary he-sans">{progressLabel}</div>
				{:else}
					<div class="bubble-tool-summary he-sans italic">{progressLabel}</div>
				{/if}
			</div>
		</div>
	{/if}
	{#if agentActivity && !approval && !showFlash && !showProgress && !showTranscript && !showPartial && !listening}
		<div class="bubble bubble-tool" dir="auto" role="status" aria-live="polite">
			<span class="he-sans">{agentActivity}</span>
		</div>
	{/if}
	{#if agentSessions.length && !approval && !showFlash && !showProgress && !showTranscript && !showPartial && !listening}
		<!-- Scrollable region needs keyboard focus so every session remains reachable. -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div class="session-stack" class:below={side === 'below'} data-interactive="island" role="region" aria-label={$t('hub.agents.sessions')} tabindex="0" aria-live="polite">
			{#each agentSessions as session (session.id)}
				<div class="bubble session-card" dir="auto">
					<div class="session-heading" dir="auto" title={session.title ?? undefined}>{session.title ?? `${session.agent} · #${session.id}`}</div>
					{#if session.title || session.project}
						<div class="session-context"><bdi>{session.agent}</bdi>{#if session.project} · <bdi>{session.project}</bdi>{/if}</div>
					{/if}
					<div class="session-status he-sans">{$t(`hub.agents.activity.${session.state}`)}{#if session.tool} · <bdi>{session.tool}</bdi>{/if}</div>
				</div>
			{/each}
		</div>
	{/if}

	{#if showTranscript}
		<div
			class="bubble bubble-transcript"
			style="--tint: {takeMode === 'command' ? 'var(--garnet)' : 'var(--saffron)'}"
			role="status"
			aria-live="polite"
			data-interactive="island"
			in:cardIn
			out:cardOut
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

	<!-- Live dictation text (docs/adr/0035): committed words solid, the
	     provisional tail muted. Visual only; the settled words are announced
	     by the polite region below, so a screen reader doesn't hear the
	     ~2 Hz re-decode flicker. -->
	{#if showPartial && partial}
		<div
			class="bubble bubble-partial"
			style="--tint: var(--saffron)"
			aria-hidden="true"
			in:cardIn
			out:cardOut
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
					<span class="partial-provisional" dir="auto"
						>{partial.committed ? ' ' : ''}{partial.provisional}</span
					>
				{/if}
			</p>
			{#if elapsedLabel}
				<span class="partial-timer mono" aria-hidden="true">{elapsedLabel}</span>
			{/if}
		</div>
	{/if}

	{#if showPartial && partial?.committed}
		<span class="sr-only" aria-live="polite" aria-atomic="true">{partial.committed}</span>
	{/if}

	<!-- Always mounted: a live region that appears together with its text
	     is often not read. -->
	<span class="sr-only" aria-live="assertive" aria-atomic="true">{approvalAnnounced}</span>
	<span class="sr-only" aria-live="polite" aria-atomic="true">{approvalSaid}</span>
</div>

<style>
	/* The newest card sits nearest Ottid: at the bottom of the island when
	   it opens above the stage, at the top when it opens below. */
	.island {
		display: flex;
		align-items: center;
		gap: 8px;
		max-height: var(--island-max-h, 100%);
		pointer-events: none;
	}
	.island.above {
		flex-direction: column;
	}
	.island.below {
		flex-direction: column-reverse;
	}

	.card-extra {
		pointer-events: auto;
	}
	.session-stack { display: flex; flex-direction: column; gap: 8px; width: min(360px, calc(100vw - 16px)); max-height: var(--island-max-h, 100%); box-sizing: border-box; padding: 4px; overflow-y: auto; overflow-x: hidden; pointer-events: auto; scrollbar-width: thin; }
	.session-stack.below { flex-direction: column-reverse; }
	.session-stack:focus-visible { outline: 2px solid var(--aqua); outline-offset: -2px; border-radius: 14px; }
	.session-card { flex: 0 0 auto; box-sizing: border-box; padding: 8px 12px; }
	.session-context { font-size: 11px; color: var(--ink-mute); overflow-wrap: anywhere; }
	.session-heading { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; font-weight: 600; color: var(--ink-mute); }
	.session-status { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }

	/* ─── Bubbles ─── */
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
		direction: rtl;
		/* `--tint` lights the border with the mode hue. */
		--tint: var(--state-cloud);
		box-shadow:
			0 1px 0 rgba(221, 228, 233, 0.06) inset,
			0 10px 32px rgba(0, 0, 0, 0.35),
			0 0 0 1px rgba(221, 228, 233, 0.08),
			0 0 0 1.5px color-mix(in srgb, var(--tint) 33%, transparent),
			0 0 24px color-mix(in srgb, var(--tint) 20%, transparent);
	}
	.bubble[data-interactive] {
		pointer-events: auto;
	}

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
		/* About five lines, pinned to the newest one. */
		max-height: 7.5em;
		overflow: hidden;
	}
	.partial-text.prompter-fade {
		-webkit-mask-image: linear-gradient(to bottom, transparent 0, #000 1.6em);
		mask-image: linear-gradient(to bottom, transparent 0, #000 1.6em);
	}
	.partial-committed {
		color: var(--ink-text);
	}
	.partial-provisional {
		color: var(--ink-mute);
	}
	.partial-timer {
		flex: 0 0 auto;
		align-self: flex-start;
		margin-top: 1px;
		font-size: 11px;
		color: var(--ink-faint);
		font-variant-numeric: tabular-nums;
	}

	/* Cancel: destructive, so it reads unmistakably as "stop this take". */
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
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}

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

	/* ─── Approval card ─── */
	.card-approval {
		display: flex;
		min-height: 0;
		max-height: var(--island-max-h, none);
		pointer-events: auto;
	}

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

	.orbit-spin {
		animation: orbit 6s linear infinite;
		transform-origin: center;
	}
	@keyframes orbit {
		to {
			transform: rotate(360deg);
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
		.bubble-wave > span {
			animation: none;
		}
	}

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
