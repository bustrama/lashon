<script lang="ts">
	// Ottid, the creature (docs/adr/0040), drawn by the engine in ./engine:
	// a WebGL signed-distance body with its lamp, and the eyes and props on a
	// 2D canvas above it. It fills the overlay's stage and honours the
	// `CreatureProps` contract (./types.ts): the floor is the stage's bottom
	// edge on the taskbar, the ceiling its top edge, and float is free.
	//
	// It is decoration: the overlay's live region announces every state, and
	// the creature is never the only signal. It marks its hit area
	// `data-interactive="creature"`; pointer handling is the overlay's.
	import { onMount } from 'svelte';
	import { DEFAULT_CREATURE, type CreatureData } from './data';
	import { createEngine, UNIT, type Engine, type FrameStats } from './engine/engine';
	import { restingBox } from './engine/geometry';
	import type { SimInput } from './engine/sim';
	import type { CreatureProps } from './types';

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
		mode = null,
		creature = DEFAULT_CREATURE,
		onStats
	}: CreatureProps & {
		/** A validated creature; the bundled default when left out. */
		creature?: CreatureData;
		/** Called about once a second with what the frames cost. */
		onStats?: (stats: FrameStats) => void;
	} = $props();

	let host: HTMLDivElement | undefined = $state();
	let bodyCanvas: HTMLCanvasElement | undefined = $state();
	let faceCanvas: HTMLCanvasElement | undefined = $state();
	let width = $state(0);
	let height = $state(0);
	let reduced = $state(false);
	let engine: Engine | null = null;

	const input = $derived<SimInput>({
		state: current,
		placement,
		gaze,
		level,
		wakeArmed,
		dragging,
		hovered,
		pokes,
		mode,
		reduced
	});

	// The hit area: the body at rest, steady across hops and swings so the
	// overlay's region reporter isn't kept measuring every frame.
	const hit = $derived(restingBox(creature, placement, width, height, UNIT));

	onMount(() => {
		if (!host || !bodyCanvas || !faceCanvas) return;
		const motion = matchMedia('(prefers-reduced-motion: reduce)');
		reduced = motion.matches;
		const onMotion = () => (reduced = motion.matches);
		motion.addEventListener('change', onMotion);

		width = host.clientWidth;
		height = host.clientHeight;
		engine = createEngine(host, bodyCanvas, faceCanvas, creature, input, onStats);
		const observer = new ResizeObserver(() => {
			if (!host) return;
			width = host.clientWidth;
			height = host.clientHeight;
			engine?.resize(width, height);
		});
		observer.observe(host);
		return () => {
			observer.disconnect();
			motion.removeEventListener('change', onMotion);
			engine?.destroy();
			engine = null;
		};
	});

	$effect(() => {
		engine?.setInput(input);
	});

	$effect(() => {
		engine?.setCreature(creature);
	});
</script>

<div class="creature" bind:this={host} aria-hidden="true">
	<canvas class="layer" bind:this={bodyCanvas}></canvas>
	<canvas class="layer" bind:this={faceCanvas}></canvas>
	<div
		class="hit"
		class:dragging
		data-interactive="creature"
		style="top: {hit.y}px; width: {hit.width}px; height: {hit.height}px;"
	></div>
</div>

<style>
	.creature {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	/* The canvases draw in physical pixels; the creature doesn't mirror in RTL. */
	.layer {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		display: block;
		pointer-events: none;
	}
	/* The body is always centred across the stage, so logical centring is exact. */
	.hit {
		position: absolute;
		inset-inline: 0;
		margin-inline: auto;
		border-radius: 50%;
		pointer-events: auto;
		cursor: grab;
	}
	.hit.dragging {
		cursor: grabbing;
	}
</style>
