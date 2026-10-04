<script lang="ts">
	// A stand-in for the creature until the WebGL one lands (Phase B2): the
	// Ottid mark with a lamp-coloured halo and a state glyph, standing in the
	// stage the way the creature will: on the floor (taskbar), hanging from
	// the top edge (ceiling) or in the middle (float). It honours the whole
	// `CreatureProps` contract, so the real creature can replace it without
	// touching the overlay.
	import type { ComponentProps } from 'svelte';
	import Mark from '$lib/components/Mark.svelte';
	import StateGlyph from '$lib/components/StateGlyph.svelte';
	import { Spring, prefersReducedMotion } from '$lib/motion/spring';
	import { listens } from './state';
	import type { CreatureProps, CreatureState } from './types';

	let {
		// `state` would shadow the `$state` rune.
		state: current,
		placement,
		gaze,
		level,
		wakeArmed,
		dragging,
		hovered,
		pokes,
		mode = null
	}: CreatureProps = $props();

	const MARK = 72;

	type Glyph = ComponentProps<typeof StateGlyph>['kind'];
	type Look = {
		lamp: string;
		halo: number;
		glyph: Glyph | null;
		motion: string;
	};

	// Lamp colours from docs/design-system.md, "States".
	function look(s: CreatureState): Look {
		const modeLamp = mode === 'command' ? 'var(--garnet)' : 'var(--saffron)';
		switch (s) {
			case 'idle':
				return wakeArmed
					? { lamp: 'var(--state-cloud)', halo: 0.3, glyph: 'antenna', motion: 'breath-slow' }
					: { lamp: 'var(--peach)', halo: 0, glyph: null, motion: 'breath-slow' };
			case 'preparing':
				return { lamp: 'var(--saffron)', halo: 0.3, glyph: 'ring', motion: 'breath-med' };
			case 'dictation':
				return { lamp: 'var(--saffron)', halo: 0.55, glyph: 'pen', motion: 'live' };
			case 'command':
				return { lamp: 'var(--garnet)', halo: 0.55, glyph: 'gear', motion: 'live' };
			case 'transcribing':
				return { lamp: 'var(--state-cloud)', halo: 0.35, glyph: 'dots', motion: 'shimmer' };
			case 'thinking':
				return { lamp: 'var(--state-cloud)', halo: 0.4, glyph: 'orbit', motion: 'breath-med' };
			case 'tool':
				return { lamp: 'var(--garnet)', halo: 0.4, glyph: 'gear-spin', motion: 'breath-med' };
			case 'confirm':
				return { lamp: 'var(--peach)', halo: 0.45, glyph: 'question', motion: 'breath-fast' };
			case 'wake':
				return { lamp: modeLamp, halo: 0.8, glyph: 'spark', motion: 'startle' };
			case 'error':
				return { lamp: 'var(--state-error)', halo: 0.45, glyph: 'cross', motion: '' };
			case 'agent-needs-you':
				return { lamp: 'var(--aqua)', halo: 0.6, glyph: 'bubble', motion: 'breath-fast' };
			case 'agent-done':
				return { lamp: 'var(--state-success)', halo: 0.6, glyph: 'spark', motion: 'startle' };
		}
	}

	const l = $derived(look(current));
	const live = $derived(listens(current));
	const markColor = $derived(live || current === 'wake' ? l.lamp : 'var(--peach)');

	// The eyes would follow the cursor; the mark leans toward it a little.
	// Gaze spring from the design system: response 0.22 s, damping 0.85.
	const LEAN_PX = 3;
	let leanX = $state(0);
	let leanY = $state(0);
	$effect(() => {
		const tx = (gaze?.x ?? 0) * LEAN_PX;
		const ty = (gaze?.y ?? 0) * LEAN_PX;
		if (prefersReducedMotion()) {
			leanX = tx;
			leanY = ty;
			return;
		}
		const sx = new Spring(leanX, 0.22, 0.85);
		const sy = new Spring(leanY, 0.22, 0.85);
		sx.target = tx;
		sy.target = ty;
		let raf = 0;
		let then = performance.now();
		const tick = (now: number) => {
			const dt = Math.min(0.05, (now - then) / 1000);
			then = now;
			sx.step(dt);
			sy.step(dt);
			leanX = sx.value;
			leanY = sy.value;
			if (!(sx.settled && sy.settled)) raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(raf);
	});

	// A click that wasn't a drag: a quick hop.
	let markEl: HTMLDivElement | undefined = $state();
	let seenPokes = 0;
	$effect(() => {
		if (pokes === seenPokes) return;
		seenPokes = pokes;
		if (!markEl || prefersReducedMotion()) return;
		markEl.animate(
			[
				{ transform: 'translateY(0) scale(1)' },
				{ transform: 'translateY(-6px) scale(1.06, 0.96)' },
				{ transform: 'translateY(0) scale(1)' }
			],
			{ duration: 280, easing: 'ease-out' }
		);
	});
</script>

<div
	class="body {placement}"
	class:dragging
	class:hovered
	style="--lamp: {l.lamp}; --halo: {l.halo}; --live-level: {live ? level : 0}; --lean-x: {leanX}px; --lean-y: {leanY}px;"
	aria-hidden="true"
>
	<div class="hit" data-interactive="creature">
		{#if l.halo > 0}
			<div class="halo" class:halo-live={live}></div>
		{/if}
		{#if live}
			{#each [0, 0.8, 1.6] as delay (delay)}
				<div class="sonar" style="animation-delay: {delay}s"></div>
			{/each}
		{/if}
		{#if current === 'confirm' || current === 'error'}
			<div class="ring"></div>
		{/if}
		<div class="lean">
			<div class="mark-anim motion-{live ? 'live' : l.motion}" bind:this={markEl}>
				<Mark size={MARK} color={markColor} glow={live ? l.lamp : null} />
			</div>
		</div>
		{#if l.glyph}
			<div class="glyph">
				<StateGlyph kind={l.glyph} color={l.lamp} />
			</div>
		{/if}
	</div>
</div>

<style>
	/* The body stands in the stage the way the creature will. */
	.body {
		position: absolute;
		inset-inline: 0;
		margin-inline: auto;
		width: 72px;
		height: 72px;
		pointer-events: none;
	}
	.body.taskbar {
		bottom: 0;
	}
	.body.ceiling {
		top: 0;
	}
	.body.float {
		top: 0;
		bottom: 0;
		margin-block: auto;
	}

	.hit {
		position: relative;
		width: 72px;
		height: 72px;
		pointer-events: auto;
		cursor: grab;
	}
	.dragging .hit {
		cursor: grabbing;
	}

	.lean {
		transform: translate(var(--lean-x), var(--lean-y));
	}
	.mark-anim {
		transform-origin: center;
		transition: filter 0.2s ease;
	}
	.hovered .mark-anim {
		filter: brightness(1.12);
	}
	.dragging .mark-anim {
		filter: drop-shadow(0 6px 10px rgba(0, 0, 0, 0.45));
	}

	.halo {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 130px;
		height: 130px;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		background: radial-gradient(circle, var(--lamp) 0%, transparent 45%);
		opacity: var(--halo);
		filter: blur(20px);
		pointer-events: none;
	}
	.halo.halo-live {
		opacity: calc(var(--halo) + var(--live-level) * 0.3);
		transform: translate(-50%, -50%) scale(calc(1 + var(--live-level) * 0.35));
		transition:
			opacity 33ms linear,
			transform 33ms linear;
	}

	.sonar {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 72px;
		height: 72px;
		border-radius: 50%;
		border: 1.5px solid
			color-mix(in srgb, var(--lamp) calc(40% + var(--live-level) * 60%), transparent);
		transform: translate(-50%, -50%) scale(0.6);
		opacity: 0.55;
		pointer-events: none;
		animation: sonar 2.4s cubic-bezier(0.2, 0.6, 0.3, 1) infinite backwards;
	}
	@keyframes sonar {
		0% {
			transform: translate(-50%, -50%) scale(0.6);
			opacity: 0.55;
		}
		80% {
			opacity: 0;
		}
		100% {
			transform: translate(-50%, -50%) scale(1.9);
			opacity: 0;
		}
	}

	.ring {
		position: absolute;
		inset: -6px;
		border-radius: 50%;
		border: 2px solid var(--lamp);
		box-shadow: 0 0 0 1px var(--ink);
		pointer-events: none;
	}

	.glyph {
		position: absolute;
		bottom: -2px;
		inset-inline-start: -2px;
		width: 22px;
		height: 22px;
		border-radius: 50%;
		background: var(--ink);
		box-shadow: 0 0 0 1.5px var(--lamp);
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.motion-live {
		transform: scale(calc(1 + var(--live-level) * 0.14));
		filter: brightness(calc(1 + var(--live-level) * 0.18));
		transition:
			transform 33ms linear,
			filter 33ms linear;
	}
	.motion-breath-slow {
		animation: breath 5.5s ease-in-out infinite;
		--breath: 1.04;
	}
	.motion-breath-med {
		animation: breath 2.4s ease-in-out infinite;
		--breath: 1.06;
	}
	.motion-breath-fast {
		animation: breath 0.95s ease-in-out infinite;
		--breath: 1.09;
	}
	.motion-shimmer {
		animation: shimmer 1.6s linear infinite;
	}
	.motion-startle {
		animation: startle 0.6s cubic-bezier(0.2, 0.9, 0.3, 1.3) both;
	}
	@keyframes breath {
		0%,
		100% {
			transform: scale(1);
		}
		50% {
			transform: scale(var(--breath));
		}
	}
	@keyframes shimmer {
		0%,
		100% {
			filter: brightness(1);
		}
		50% {
			filter: brightness(1.2);
		}
	}
	@keyframes startle {
		0% {
			transform: translateY(0) scale(1);
		}
		35% {
			transform: translateY(-10px) scale(1.18);
		}
		100% {
			transform: translateY(0) scale(1.08);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.motion-breath-slow,
		.motion-breath-med,
		.motion-breath-fast,
		.motion-shimmer,
		.motion-startle {
			animation: none;
		}
		.sonar {
			animation: none;
			opacity: 0;
		}
	}
</style>
