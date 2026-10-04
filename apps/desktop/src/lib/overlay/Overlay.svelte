<script lang="ts">
	// The overlay window's content: the creature's stage and the island of
	// cards beside it, laid out from the frame the Rust shell computed.
	//
	// The window is far larger than what it draws and lets clicks through
	// everywhere but the elements marked `data-interactive`; this component
	// reports those to the Rust cursor poll. It also turns pointer input on
	// the stage into a drag (the Rust shell moves the window), a poke, the
	// Hub (double-click) or the menu (right-click).
	import { onMount, type ComponentProps, type Snippet } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { Creature, listens, type CreatureState, type Gaze } from '$lib/creature';
	import { prefersReducedMotion } from '$lib/motion/spring';
	import Island from './Island.svelte';
	import { PREVIEW_FRAME, watchFrame, type Frame } from './frame';
	import { reportRegions, type RegionReporter } from './regions';

	type IslandProps = Omit<ComponentProps<typeof Island>, 'side' | 'extra'>;

	let {
		// `state` would shadow the `$state` rune.
		state: current,
		wakeArmed,
		mode,
		announcement,
		island,
		extra,
		onIslandHover
	}: {
		state: CreatureState;
		wakeArmed: boolean;
		mode: 'dictation' | 'command' | null;
		/** What the polite live region says about the current state. */
		announcement: string;
		island: IslandProps;
		extra?: Snippet;
		/** The cursor entered (true) or left (false) the island's cards. */
		onIslandHover: (on: boolean) => void;
	} = $props();

	let frame = $state<Frame>(PREVIEW_FRAME);
	let gaze = $state<Gaze | null>(null);
	let hoverRegion = $state<string | null>(null);
	let pokes = $state(0);
	let root: HTMLDivElement | undefined = $state();
	let reporter: RegionReporter | null = null;

	const GAP = 6;
	const MARGIN = 8;
	// A press has to travel this far (CSS px) before it becomes a drag, so a
	// slightly shaky click still counts as a click.
	const DRAG_THRESHOLD = 4;

	// ---- Layout ----
	let islandWidth = $state(0);
	const stageCenter = $derived(frame.stage.x + frame.stage.width / 2);
	const islandLeft = $derived(
		Math.max(MARGIN, Math.min(frame.width - islandWidth - MARGIN, stageCenter - islandWidth / 2))
	);
	const islandMaxHeight = $derived(
		frame.island === 'above'
			? frame.stage.y - GAP - MARGIN
			: frame.height - (frame.stage.y + frame.stage.height) - GAP - MARGIN
	);
	const islandEdge = $derived(
		frame.island === 'above'
			? `bottom: ${frame.height - frame.stage.y + GAP}px`
			: `top: ${frame.stage.y + frame.stage.height + GAP}px`
	);
	// Moving the stage or the island inside the window changes no element's
	// size, so the observers can't see it: measure the regions again.
	$effect(() => {
		void [frame, islandLeft, islandEdge];
		reporter?.refresh();
	});

	// ---- Voice level ----
	// The capture worker streams one RMS scalar per ~50 ms on
	// `dictation:level` (no audio content — security.md). Peak-normalised so
	// quiet and hot mics both fill 0..1, eased per frame between readings.
	const LEVEL_PEAK_FLOOR = 0.012;
	const LEVEL_PEAK_DECAY = 0.975;
	const LEVEL_EASE = 0.2;
	let level = $state(0);
	const listening = $derived(listens(current));
	$effect(() => {
		if (!listening || prefersReducedMotion()) {
			level = 0;
			return;
		}
		let target = 0;
		let smoothed = 0;
		let peak = LEVEL_PEAK_FLOOR;
		let raf = 0;
		const unlisten = listen<number>('dictation:level', (event) => {
			const raw = Math.max(0, event.payload);
			peak = Math.max(raw, peak * LEVEL_PEAK_DECAY, LEVEL_PEAK_FLOOR);
			target = Math.min(1, raw / peak);
		});
		const tick = () => {
			smoothed += (target - smoothed) * LEVEL_EASE;
			level = smoothed;
			raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
		return () => {
			cancelAnimationFrame(raf);
			void unlisten.then((stop) => stop()).catch(() => {});
			level = 0;
		};
	});

	// ---- Pointer on the stage ----
	// `drag` resolves to the token of the drag the press started, or null if none did.
	let press: { id: number; x: number; y: number; drag: Promise<number | null> | null } | null = null;
	let swallowClick = false;

	function onPointerDown(event: PointerEvent): void {
		if (event.button !== 0) return;
		// Capture on the hit element, so the drag keeps its events wherever
		// the cursor goes while the window follows it.
		(event.target as Element).setPointerCapture(event.pointerId);
		press = { id: event.pointerId, x: event.screenX, y: event.screenY, drag: null };
	}

	function onPointerMove(event: PointerEvent): void {
		if (!press || press.drag || event.pointerId !== press.id) return;
		const moved = Math.hypot(event.screenX - press.x, event.screenY - press.y);
		if (moved < DRAG_THRESHOLD) return;
		const current = press;
		const drag: Promise<number | null> = invoke<number>('overlay_drag_start').catch(() => {
			// No drag started (no cursor or no layout yet): the next move tries again.
			if (current.drag === drag) current.drag = null;
			return null;
		});
		current.drag = drag;
	}

	function onPointerEnd(event: PointerEvent): void {
		if (!press || event.pointerId !== press.id) return;
		const drag = press.drag;
		press = null;
		if (drag) {
			swallowClick = true;
			// End the drag this press started, once it has started: the shell may
			// handle the two commands in either order, and an end that overtook
			// its start would leave the window following the cursor.
			void drag
				.then((token) => (token === null ? undefined : invoke('overlay_drag_end', { token })))
				.catch(() => {});
		}
	}

	function onClick(): void {
		if (swallowClick) {
			swallowClick = false;
			return;
		}
		pokes += 1;
	}

	function onDblClick(): void {
		void invoke('open_hub').catch(() => {});
	}

	// Right-click anywhere Ottid draws opens its menu (the tray's items); the
	// webview's own context menu never shows.
	function onContextMenu(event: MouseEvent): void {
		event.preventDefault();
		void invoke('show_tongue_menu').catch(() => {});
	}

	onMount(() => {
		const stopFrame = watchFrame((next) => (frame = next));
		const gazeUnlisten = listen<Gaze>('overlay:gaze', (event) => (gaze = event.payload));
		const hoverUnlisten = listen<{ region: string | null }>('overlay:hover', (event) => {
			const was = hoverRegion === 'island';
			hoverRegion = event.payload.region;
			const now = hoverRegion === 'island';
			if (was !== now) onIslandHover(now);
		});
		reporter = root ? reportRegions(root) : null;
		window.addEventListener('contextmenu', onContextMenu);
		return () => {
			window.removeEventListener('contextmenu', onContextMenu);
			reporter?.stop();
			void stopFrame.then((stop) => stop());
			void gazeUnlisten.then((stop) => stop()).catch(() => {});
			void hoverUnlisten.then((stop) => stop()).catch(() => {});
		};
	});
</script>

<div class="overlay" bind:this={root}>
	<!-- The overlay never takes keyboard focus (it is non-activating), so
	     Ottid's keyboard paths are the hotkeys, the tray menu and the Hub. -->
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div
		class="stage"
		style="left: {frame.stage.x}px; top: {frame.stage.y}px; width: {frame.stage.width}px; height: {frame.stage.height}px;"
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={onPointerEnd}
		onpointercancel={onPointerEnd}
		onlostpointercapture={onPointerEnd}
		onclick={onClick}
		ondblclick={onDblClick}
	>
		<Creature
			state={current}
			placement={frame.placement}
			{gaze}
			{level}
			{wakeArmed}
			dragging={frame.dragging}
			hovered={hoverRegion === 'creature'}
			{pokes}
			{mode}
		/>
	</div>

	<div
		class="island-slot"
		bind:clientWidth={islandWidth}
		style="left: {islandLeft}px; {islandEdge}; --island-max-h: {Math.max(0, islandMaxHeight)}px;"
	>
		<Island side={frame.island} {...island} {extra} />
	</div>

	<!-- Every lifecycle change is announced; the creature is decoration on
	     top of this, never the only signal. -->
	<span class="sr-only" aria-live="polite" aria-atomic="true">{announcement}</span>
</div>

<style>
	.overlay {
		position: fixed;
		inset: 0;
		overflow: hidden;
		pointer-events: none;
	}
	.stage {
		position: absolute;
		pointer-events: none;
		touch-action: none;
	}
	/* The creature's hit element opts back in. */
	.stage :global([data-interactive]) {
		pointer-events: auto;
	}
	.island-slot {
		position: absolute;
		pointer-events: none;
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
