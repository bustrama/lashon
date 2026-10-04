<script lang="ts">
	// The approval card (docs/adr/0048): the request the Rust broker shows,
	// whole, with Allow and Deny.
	//
	// - Nothing is cut off. A long request scrolls inside the card, and
	//   characters that would not show for what they are (bidi controls,
	//   zero-width and look-alike spaces) appear by code point.
	// - Allow arms once the whole text has been on screen, with the card
	//   fully open, for ARM_DELAY_MS. Until then a click on Allow, or the
	//   Allow hotkey, says what is missing; the hotkey also scrolls on
	//   through a long request. Deny works at once.
	// - From the keyboard, Allow takes holding its hotkey for HOLD_MS, so a
	//   tap meant for another app never allows. The button fills while it
	//   is held; let go too soon, and the card says to hold it.
	// - The broker denies the request when its time runs out.
	//
	// The overlay never has keyboard focus (docs/adr/0044): the keyboard path
	// is the hotkeys the broker registers while a card is pending, and the
	// screen reader hears the island's live regions.
	//
	// The island keys this component by the request's id, so its state is
	// per request.
	import { onMount, untrack } from 'svelte';
	import { t } from '$lib/i18n';
	import {
		ARM_DELAY_MS,
		ArmClock,
		HOLD_MS,
		atEnd,
		fill,
		nextPage,
		pieces,
		seenNudges,
		type ApprovalCard,
		type ApprovalDecision,
		type ApprovalNudge,
		type Scroll
	} from './approval';

	let {
		card,
		settled,
		nudge,
		onArmed,
		onAnswer,
		onSay
	}: {
		card: ApprovalCard;
		/** The card has finished opening. */
		settled: boolean;
		/** The latest early Allow from the hotkey. */
		nudge: ApprovalNudge | null;
		onArmed: (id: number) => void;
		onAnswer: (id: number, decision: ApprovalDecision) => void;
		/** Say something through the polite live region. */
		onSay: (text: string) => void;
	} = $props();

	let body: HTMLDivElement | undefined = $state();
	let content: HTMLDivElement | undefined = $state();
	/** Fonts loaded and laid out, on a visible page. */
	let rendered = $state(false);
	/** The end of the text has been in view. It stays seen. */
	let seenAll = $state(false);
	let overflowing = $state(false);
	let visible = $state(true);
	let charging = $state(false);
	let armed = $state(false);
	/** The Allow hotkey is held down. */
	let holding = $state(false);
	/** Briefly highlight why Allow isn't taken yet. */
	let hint = $state(false);
	/** Briefly shown in place of the countdown. */
	let keyHint = $state<string | null>(null);
	let secondsLeft = $state(0);

	const clock = new ArmClock();
	let armTimer: ReturnType<typeof setTimeout> | undefined;
	let hintTimer: ReturnType<typeof setTimeout> | undefined;
	let keyHintTimer: ReturnType<typeof setTimeout> | undefined;
	// Set before the nudge effect first runs, so a nudge already there when
	// the card mounts isn't taken for this card's.
	let lastNudge = untrack(() => seenNudges(nudge, card.id));

	const toolQuestion = $derived($t('approval.question.tool').split('{tool}'));

	function metrics(el: HTMLElement): Scroll {
		return {
			top: el.scrollTop,
			left: el.scrollLeft,
			clientHeight: el.clientHeight,
			clientWidth: el.clientWidth,
			scrollHeight: el.scrollHeight,
			scrollWidth: el.scrollWidth
		};
	}

	function measure(): void {
		if (!body || !rendered) return;
		const m = metrics(body);
		overflowing = m.scrollHeight > m.clientHeight + 1 || m.scrollWidth > m.clientWidth + 1;
		if (atEnd(m)) seenAll = true;
		advance();
	}

	// Run the arm clock: it counts while the whole text has been seen, the
	// card is open and the page is visible.
	function advance(): void {
		if (armed) return;
		const now = performance.now();
		clock.update(rendered && seenAll && settled && visible, now);
		const left = clock.remaining(now);
		clearTimeout(armTimer);
		charging = left !== null && left > 0;
		if (left === 0) {
			armed = true;
			onArmed(card.id);
		} else if (left !== null) {
			armTimer = setTimeout(advance, left);
		}
	}

	$effect(() => {
		void [settled, visible, seenAll, rendered];
		untrack(advance);
	});

	// Say why Allow isn't taken yet. From the hotkey, also scroll on: the
	// keyboard has no other way through a long request.
	function explain(page: boolean): void {
		hint = true;
		clearTimeout(hintTimer);
		hintTimer = setTimeout(() => (hint = false), 1600);
		if (overflowing && !seenAll && body) {
			if (page) {
				const lineHeight = parseFloat(getComputedStyle(body).lineHeight) || 19;
				body.scrollTop = nextPage(metrics(body), lineHeight);
				measure();
				onSay($t('approval.nudge.more'));
			} else {
				onSay($t('approval.scrollToEnd'));
			}
		} else {
			onSay($t('approval.nudge.wait'));
		}
	}

	// Say, and show, how the keyboard allows.
	function sayHold(): void {
		if (!card.keys.allow) return;
		const text = fill($t('approval.announce.allowKey'), { keys: card.keys.allow.join('+') });
		hint = true;
		clearTimeout(hintTimer);
		hintTimer = setTimeout(() => (hint = false), 1600);
		keyHint = text;
		clearTimeout(keyHintTimer);
		keyHintTimer = setTimeout(() => (keyHint = null), 2400);
		onSay(text);
	}

	$effect(() => {
		const n = nudge;
		if (!n || n.id !== card.id || n.n === lastNudge) return;
		lastNudge = n.n;
		holding = n.kind === 'hold';
		if (n.kind === 'early') untrack(() => explain(true));
		else if (n.kind === 'short') untrack(sayHold);
	});

	// Only a pointer answers. Enter or Space on a button, or an accessibility
	// tool's invoke, arrives as a click with no presses (`detail` 0); the
	// keyboard path is the hotkeys, which the broker gates the same way.
	function allow(event: MouseEvent): void {
		if (event.detail === 0) {
			sayHold();
		} else if (armed) {
			onAnswer(card.id, 'allow');
		} else {
			explain(false);
		}
	}

	// A press must not move focus to a button: if the overlay ever has the
	// keyboard (after its menu), Enter or Space would press it.
	function keepFocus(event: MouseEvent): void {
		event.preventDefault();
	}

	onMount(() => {
		const expiresAt = performance.now() + card.expires_in_ms;
		const countDown = () =>
			(secondsLeft = Math.max(0, Math.ceil((expiresAt - performance.now()) / 1000)));
		countDown();
		const countdown = setInterval(countDown, 250);

		const onVisibility = () => {
			visible = document.visibilityState === 'visible';
			measure();
		};
		visible = document.visibilityState === 'visible';
		document.addEventListener('visibilitychange', onVisibility);

		const sizes = new ResizeObserver(measure);
		if (body) sizes.observe(body);
		if (content) sizes.observe(content);

		// Rendered: the fonts are in and two frames have laid the text out.
		// Frames don't run while the page is hidden, so neither does this.
		let frame = 0;
		let stopped = false;
		void document.fonts.ready.then(() => {
			if (stopped) return;
			frame = requestAnimationFrame(() => {
				frame = requestAnimationFrame(() => {
					rendered = true;
					measure();
				});
			});
		});

		return () => {
			stopped = true;
			cancelAnimationFrame(frame);
			clearInterval(countdown);
			clearTimeout(armTimer);
			clearTimeout(hintTimer);
			clearTimeout(keyHintTimer);
			sizes.disconnect();
			document.removeEventListener('visibilitychange', onVisibility);
		};
	});
</script>

<!-- Each non-ASCII word is its own isolate, so the line reads in the order
     it runs (`runs` in approval.ts). -->
{#snippet text(value: string)}{#each pieces(value) as piece, i (i)}{#if piece.kind === 'text'}{piece.text}{:else if piece.kind === 'isolate'}<bdi>{piece.text}</bdi>{:else}<span class="hidden-char" title={fill($t('approval.hiddenCharTitle'), { code: piece.code })}>{piece.code}</span>{/if}{/each}{/snippet}

<div class="approval" role="group" aria-labelledby="approval-question-{card.id}">
	<div class="head">
		<span class="dot" aria-hidden="true"></span>
		<span class="eyebrow">{$t('approval.eyebrow')}</span>
		{#if card.waiting > 0}
			<span class="waiting">{fill($t('approval.waiting'), { count: card.waiting })}</span>
		{/if}
	</div>

	<p class="question" id="approval-question-{card.id}">
		{#if card.command !== undefined}
			{$t('approval.question.command')}
		{:else}
			{toolQuestion[0]}<bdi class="tool">{card.tool}</bdi>{toolQuestion[1] ?? ''}
		{/if}
	</p>
	<p class="meta">
		{#if card.source === 'recipe'}<span>{$t('approval.source.recipe')}</span>
			<span aria-hidden="true">·</span>{/if}
		<bdi class="tool">{card.tool}</bdi>
	</p>

	<!-- Left to right, isolated: a command line reads in its own order. -->
	<div class="body" class:more={overflowing && !seenAll} bind:this={body} onscroll={measure} dir="ltr">
		<div class="content" bind:this={content}>
			{#if card.command !== undefined}
				<div class="command"><span class="prompt" aria-hidden="true">$</span>{@render text(card.command)}</div>
			{/if}
			{#if card.cwd !== undefined}
				<div class="cwd"><bdi class="cwd-label">{$t('approval.cwd')}</bdi> <span class="cwd-path">{@render text(card.cwd)}</span></div>
			{/if}
			{#if card.details !== undefined}
				<div class="details">{@render text(card.details)}</div>
			{/if}
		</div>
	</div>

	{#if overflowing && !seenAll}
		<p class="scroll-hint" class:flash={hint}>{$t('approval.scrollToEnd')}</p>
	{/if}

	<div class="actions">
		<button
			type="button"
			class="allow"
			class:armed
			class:charging
			class:holding
			class:flash={hint && !overflowing}
			aria-disabled={!armed}
			tabindex="-1"
			onmousedown={keepFocus}
			onclick={allow}
		>
			<span class="charge" style="--arm-ms: {ARM_DELAY_MS}ms" aria-hidden="true"></span>
			<span class="hold" style="--hold-ms: {HOLD_MS}ms" aria-hidden="true"></span>
			<span class="label">{$t('approval.allow')}</span>
			{#if card.keys.allow}<kbd class="keys">{card.keys.allow.join('+')}</kbd>{/if}
		</button>
		<button
			type="button"
			class="deny"
			tabindex="-1"
			onmousedown={keepFocus}
			onclick={() => onAnswer(card.id, 'deny')}
		>
			<span class="label">{$t('approval.deny')}</span>
			{#if card.keys.deny}<kbd class="keys">{card.keys.deny.join('+')}</kbd>{/if}
		</button>
	</div>

	<!-- A ticking value is information, not decoration: reduced motion keeps
	     it. The live region announced the time limit once. -->
	{#if keyHint}
		<p class="expires key-hint" aria-hidden="true">{keyHint}</p>
	{:else}
		<p class="expires" aria-hidden="true">{fill($t('approval.expires'), { seconds: secondsLeft })}</p>
	{/if}
</div>

<style>
	.approval {
		--tint: var(--aqua);
		box-sizing: border-box;
		width: min(380px, calc(100vw - 16px));
		max-height: var(--island-max-h, 100vh);
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 14px 14px 10px;
		border-radius: 16px;
		background: color-mix(in srgb, var(--ink) 88%, transparent);
		backdrop-filter: blur(28px) saturate(120%);
		-webkit-backdrop-filter: blur(28px) saturate(120%);
		color: var(--ink-text);
		font-family: var(--font-he-sans);
		text-align: start;
		box-shadow:
			0 12px 40px color-mix(in srgb, var(--ink) 60%, transparent),
			0 0 0 1px var(--ink-line-2),
			0 0 0 1.5px color-mix(in srgb, var(--tint) 40%, transparent),
			0 0 28px color-mix(in srgb, var(--tint) 22%, transparent);
	}

	.head {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: 0 0 auto;
	}
	.dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		background: var(--tint);
		box-shadow: 0 0 8px var(--aqua-glow);
		flex: 0 0 auto;
	}
	.eyebrow {
		font-size: 12px;
		font-weight: 700;
		color: var(--tint);
		letter-spacing: 0.2px;
	}
	.waiting {
		margin-inline-start: auto;
		font-size: 11px;
		color: var(--ink-mute);
	}

	.question {
		margin: 0;
		flex: 0 0 auto;
		font-family: var(--font-he-display);
		font-size: 17px;
		font-weight: 500;
		line-height: 1.35;
	}
	.meta {
		margin: -4px 0 0;
		flex: 0 0 auto;
		font-size: 11.5px;
		color: var(--ink-faint);
		display: flex;
		gap: 6px;
		align-items: baseline;
	}
	.tool {
		font-family: var(--font-mono);
		color: var(--garnet);
	}
	.meta .tool {
		color: var(--ink-mute);
	}

	/* The request itself: the one part that grows and scrolls. */
	.body {
		flex: 1 1 auto;
		min-height: 3.2em;
		overflow: auto;
		overscroll-behavior: contain;
		unicode-bidi: isolate;
		text-align: left;
		padding: 10px 12px;
		border-radius: 9px;
		background: color-mix(in srgb, var(--ink) 70%, black);
		box-shadow: inset 0 0 0 1px var(--ink-line);
		font-family: var(--font-mono);
		font-size: 13px;
		line-height: 1.45;
		color: var(--ink-text);
		user-select: text;
		scrollbar-width: thin;
		scrollbar-color: var(--ink-line-2) transparent;
	}
	/* More below: fade the bottom edge so the cut reads as "keep going". */
	.body.more {
		-webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - 1.6em), transparent);
		mask-image: linear-gradient(to bottom, #000 calc(100% - 1.6em), transparent);
	}
	.command,
	.details {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	/* Svelte trims a trailing space inside an element: the gap is CSS. */
	.prompt {
		margin-inline-end: 1ch;
		color: var(--tint);
		font-weight: 700;
	}
	.cwd {
		margin-top: 6px;
		font-size: 12px;
		color: var(--ink-mute);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
	.cwd-label {
		font-family: var(--font-he-sans);
		color: var(--ink-faint);
	}
	.cwd-path {
		color: var(--ink-text);
	}
	.details {
		margin-top: 6px;
		font-size: 12px;
		color: var(--ink-mute);
	}
	.command + .details,
	.cwd + .details {
		padding-top: 6px;
		border-top: 1px solid var(--ink-line);
	}
	/* A character that would not show for what it is. */
	.hidden-char {
		unicode-bidi: isolate;
		display: inline-block;
		margin: 0 1px;
		padding: 0 3px;
		border-radius: 4px;
		font-size: 10.5px;
		line-height: 1.4;
		color: var(--ink);
		background: var(--saffron);
		vertical-align: baseline;
	}

	.scroll-hint {
		margin: 0;
		flex: 0 0 auto;
		font-size: 12px;
		color: var(--ink-mute);
		text-align: center;
		transition: color 0.2s ease;
	}
	.scroll-hint.flash {
		color: var(--tint);
	}

	.actions {
		display: flex;
		gap: 8px;
		flex: 0 0 auto;
	}
	.allow,
	.deny {
		position: relative;
		overflow: hidden;
		flex: 1;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		padding: 9px 10px;
		border-radius: 9px;
		font-family: var(--font-he-sans);
		font-size: 14px;
		font-weight: 700;
		cursor: pointer;
	}
	.label,
	.keys {
		position: relative;
	}
	.keys {
		font-family: var(--font-mono);
		font-size: 10.5px;
		font-weight: 600;
		padding: 1px 5px;
		border-radius: 4px;
		direction: ltr;
		unicode-bidi: isolate;
		border: 1px solid currentColor;
		opacity: 0.75;
	}

	/* Allow runs something that can't be taken back: rose, and dim until it
	   arms. The charge fills it from the inline start while the arm delay
	   runs; once armed, holding the hotkey fills it again, darker. */
	.allow {
		border: 1px solid color-mix(in srgb, var(--state-error) 60%, transparent);
		background: color-mix(in srgb, var(--state-error) 16%, transparent);
		color: var(--ink-mute);
		cursor: not-allowed;
	}
	.allow .charge {
		position: absolute;
		inset: 0;
		background: color-mix(in srgb, var(--state-error) 38%, transparent);
		transform: scaleX(0);
		transform-origin: var(--charge-from, right);
	}
	:global([dir='ltr']) .allow {
		--charge-from: left;
	}
	.allow.charging .charge {
		transform: scaleX(1);
		transition: transform var(--arm-ms) linear;
	}
	.allow.armed {
		background: var(--state-error);
		border-color: var(--state-error);
		color: var(--ink);
		cursor: pointer;
	}
	.allow.armed .charge {
		display: none;
	}
	.allow .hold {
		position: absolute;
		inset: 0;
		background: color-mix(in srgb, var(--ink) 30%, transparent);
		transform: scaleX(0);
		transform-origin: var(--charge-from, right);
	}
	.allow.holding .hold {
		transform: scaleX(1);
		transition: transform var(--hold-ms) linear;
	}
	.allow.armed:hover {
		filter: brightness(1.08);
	}
	.allow.flash {
		border-color: var(--tint);
	}

	.deny {
		background: transparent;
		color: var(--ink-text);
		border: 1px solid var(--ink-line-2);
		font-weight: 600;
	}
	.deny:hover {
		background: color-mix(in srgb, var(--ink-text) 6%, transparent);
		border-color: color-mix(in srgb, var(--ink-text) 30%, transparent);
	}

	.expires {
		margin: 0;
		flex: 0 0 auto;
		font-size: 11px;
		color: var(--ink-faint);
		text-align: center;
		font-variant-numeric: tabular-nums;
	}
	.key-hint {
		color: var(--tint);
	}

	@media (prefers-reduced-motion: reduce) {
		.allow.charging .charge,
		.allow.holding .hold {
			transition: none;
			transform: scaleX(0);
		}
		/* No fill to watch: mark the held button instead. */
		.allow.holding {
			outline: 2px solid var(--tint);
			outline-offset: 2px;
		}
		.scroll-hint {
			transition: none;
		}
	}
</style>
