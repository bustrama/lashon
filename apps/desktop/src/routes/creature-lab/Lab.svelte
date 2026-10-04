<script lang="ts">
	// Every state side by side, each on its own stage and driven the way the
	// overlay drives it, with what its frames cost. Dev only (see +page.svelte).
	import { onMount } from 'svelte';
	import Creature from '$lib/creature/Creature.svelte';
	import { CREATURE_STATES } from '$lib/creature/data';
	import type { FrameStats } from '$lib/creature/engine/engine';
	import { PLACEMENTS, STAGE, type CreatureState, type Gaze, type Placement } from '$lib/creature/types';

	// `?placement=ceiling&states=dictation,error` frames a few states for a close look.
	const params = new URLSearchParams(location.search);
	const only = params.get('states')?.split(',') ?? [];
	const states = CREATURE_STATES.filter((s) => only.length === 0 || only.includes(s));

	let placement: Placement = $state(
		PLACEMENTS.find((p) => p === params.get('placement')) ?? 'taskbar'
	);
	let voice = $state(true);
	let fixedLevel = $state(0.4);
	let speech = $state(0);
	let wakeArmed = $state(false);
	let dragging = $state(false);
	let mode: 'dictation' | 'command' = $state('dictation');
	let followCursor = $state(true);
	let showHits = $state(false);
	let replaying = $state(false);
	let generation = $state(0);
	let hovered: CreatureState | null = $state(null);
	let pokes: Record<string, number> = $state({});
	let gazes: Record<string, Gaze | null> = $state({});
	let stats: Record<string, FrameStats> = $state({});
	let cells: Record<string, HTMLElement> = $state({});

	const level = $derived(voice ? speech : fixedLevel);
	const shown = (state: CreatureState): CreatureState => (replaying ? 'idle' : state);

	const total = $derived.by(() => {
		const all = Object.values(stats);
		if (!all.length) return null;
		return {
			renderers: [...new Set(all.map((s) => s.renderer))].join(', '),
			fps: all.reduce((a, s) => a + s.fps, 0) / all.length,
			meanMs: all.reduce((a, s) => a + s.meanMs, 0) / all.length,
			maxMs: Math.max(...all.map((s) => s.maxMs))
		};
	});

	/** Leave and re-enter every state, so the one-shot events play again. */
	function replay(): void {
		replaying = true;
		setTimeout(() => (replaying = false), 80);
	}

	function onPointerMove(event: PointerEvent): void {
		const next: Record<string, Gaze | null> = {};
		for (const [state, el] of Object.entries(cells)) {
			if (!followCursor) {
				next[state] = null;
				continue;
			}
			const r = el.getBoundingClientRect();
			const clamp = (v: number) => Math.max(-1, Math.min(1, v));
			next[state] = {
				x: clamp((event.clientX - (r.left + r.width / 2)) / 240),
				y: clamp((event.clientY - (r.top + r.height / 2)) / 160)
			};
		}
		gazes = next;
	}

	function onCellOver(state: CreatureState, event: PointerEvent): void {
		const onBody = (event.target as Element).closest('[data-interactive="creature"]');
		hovered = onBody ? state : hovered === state ? null : hovered;
	}

	function onCellClick(state: CreatureState, event: MouseEvent): void {
		if ((event.target as Element).closest('[data-interactive="creature"]')) {
			pokes[state] = (pokes[state] ?? 0) + 1;
		}
	}

	onMount(() => {
		// A made-up voice: syllables riding on a slower phrase envelope.
		let raf = 0;
		const loop = (now: number) => {
			const t = now / 1000;
			const phrase = 0.5 + 0.5 * Math.sin(t * 0.9);
			const syllable = Math.max(0, Math.sin(t * 7.3) * Math.sin(t * 3.1 + 1));
			speech = Math.min(1, phrase * (0.25 + 0.9 * syllable));
			raf = requestAnimationFrame(loop);
		};
		raf = requestAnimationFrame(loop);
		return () => cancelAnimationFrame(raf);
	});
</script>

<svelte:window onpointermove={onPointerMove} />

<div class="lab" class:show-hits={showHits} dir="ltr">
	<header>
		<h1>Creature lab</h1>
		<fieldset>
			<label>
				Placement
				<select bind:value={placement}>
					{#each PLACEMENTS as p (p)}<option value={p}>{p}</option>{/each}
				</select>
			</label>
			<label><input type="checkbox" bind:checked={voice} /> Voice</label>
			<label>
				Level
				<input type="range" min="0" max="1" step="0.01" bind:value={fixedLevel} disabled={voice} />
			</label>
			<label>
				Wake mode
				<select bind:value={mode}>
					<option value="dictation">dictation</option>
					<option value="command">command</option>
				</select>
			</label>
			<label><input type="checkbox" bind:checked={wakeArmed} /> Wake armed</label>
			<label><input type="checkbox" bind:checked={dragging} /> Dragging</label>
			<label><input type="checkbox" bind:checked={followCursor} /> Gaze</label>
			<label><input type="checkbox" bind:checked={showHits} /> Hit areas</label>
			<button type="button" onclick={replay}>Replay events</button>
			<button type="button" onclick={() => generation++}>Remount</button>
		</fieldset>
		<p class="total" data-testid="total">
			{#if total}
				{total.renderers} · {total.fps.toFixed(0)} fps · mean {total.meanMs.toFixed(2)} ms · worst
				{total.maxMs.toFixed(2)} ms (CPU per frame, averaged over {Object.keys(stats).length} creatures)
			{:else}
				measuring…
			{/if}
		</p>
	</header>

	{#key generation}
		<ul class="grid">
			{#each states as state (state)}
				<li>
					<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
					<div
						class="stage {placement}"
						style="width: {STAGE.width}px; height: {STAGE.height}px;"
						bind:this={cells[state]}
						onpointerover={(e) => onCellOver(state, e)}
						onpointerleave={() => hovered === state && (hovered = null)}
						onclick={(e) => onCellClick(state, e)}
					>
						<Creature
							state={shown(state)}
							{placement}
							gaze={gazes[state] ?? null}
							level={state === 'dictation' || state === 'command' ? level : 0}
							{wakeArmed}
							{dragging}
							hovered={hovered === state}
							pokes={pokes[state] ?? 0}
							mode={state === 'wake' ? mode : null}
							onStats={(s) => {
								stats[state] = s;
							}}
						/>
					</div>
					<p class="label">
						<b>{state}</b>
						{#if stats[state]}
							<span>{stats[state].fps} fps · {stats[state].meanMs.toFixed(2)} / {stats[state].maxMs.toFixed(2)} ms</span>
						{/if}
					</p>
				</li>
			{/each}
		</ul>
	{/key}
</div>

<style>
	:global(html),
	:global(body) {
		overflow: auto;
	}
	.lab {
		min-height: 100vh;
		padding: 16px;
		box-sizing: border-box;
		background: var(--vellum);
		color: var(--vellum-text);
		font-family: var(--font-lat-sans);
		user-select: text;
	}
	@media (prefers-color-scheme: dark) {
		.lab {
			background: var(--ink);
			color: var(--ink-text);
		}
	}
	h1 {
		margin: 0 0 8px;
		font-size: 18px;
	}
	fieldset {
		display: flex;
		flex-wrap: wrap;
		gap: 8px 16px;
		align-items: center;
		border: 0;
		padding: 0;
		margin: 0;
		font-size: 13px;
	}
	.total {
		font: 12px var(--font-mono);
		opacity: 0.8;
	}
	.grid {
		list-style: none;
		padding: 0;
		margin: 0;
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(236px, 1fr));
		gap: 16px;
	}
	.stage {
		position: relative;
		outline: 1px dashed var(--vellum-line);
	}
	@media (prefers-color-scheme: dark) {
		.stage {
			outline-color: var(--ink-line);
		}
	}
	/* The surface it stands on or hangs from. */
	.stage.taskbar {
		border-block-end: 2px solid var(--state-cloud);
	}
	.stage.ceiling {
		border-block-start: 2px solid var(--state-cloud);
	}
	.show-hits :global([data-interactive='creature']) {
		outline: 1px solid var(--aqua);
	}
	.label {
		margin: 4px 0 0;
		font-size: 12px;
		display: flex;
		justify-content: space-between;
		gap: 8px;
	}
	.label span {
		font-family: var(--font-mono);
		opacity: 0.7;
	}
</style>
