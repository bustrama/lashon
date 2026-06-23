<!--
	StepsPanel — the side drawer that opens when the user clicks the
	Eye affordance on a recipe row. Renders the recipe's steps as a
	vertical card stack, with header (recipe name + permissions +
	Run / Edit YAML / Duplicate buttons + slot legend) and footer
	(step count + duration estimate + Esc hint).

	Side drawer (not modal) — keeps the recipe list visible behind so
	the user can switch recipes without dismissing. Hearth top-edge
	glow keeps the "recipe accent" identity solid (no pulse — pulse
	is reserved for LLM-driven states).

	Open + close are owned by the parent. Esc + the close button both
	fire `onclose`. `onComment` bubbles up per-step comment-save
	requests so the parent owns the Tauri call to
	`update_recipe_comment` + the toast on failure.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import Banner from '../design/Banner.svelte';
	import PermissionBadge from '../design/PermissionBadge.svelte';
	import RecipeGlyph from '../design/RecipeGlyph.svelte';
	import SlotPill from '../design/SlotPill.svelte';
	import SourceBadge from '../design/SourceBadge.svelte';
	import StepCard from './StepCard.svelte';
	import { t } from '$lib/i18n';
	import type { HubRecipeListing, Recipe, RecipeStep, RecipeSource } from './types';

	let {
		listing,
		recipe,
		parseError = null,
		onclose,
		onrun,
		oneditYaml,
		onduplicate,
		oncomment
	}: {
		/** The row data from the listing — carries name + source +
		 *  permissions. Decoupled from `recipe` because the parse-error
		 *  state has a listing but no recipe. */
		listing: HubRecipeListing;
		/** Full recipe + steps. `null` when `parseError` is set. */
		recipe: Recipe | null;
		/** Parse failure message; non-null suppresses the steps area. */
		parseError?: string | null;
		onclose?: () => void;
		onrun?: () => void;
		oneditYaml?: () => void;
		onduplicate?: () => void;
		oncomment?: (stepIndex: number, next: string | null) => void;
	} = $props();

	/** Pick the host-OS step list. The Hub runs on the user's box, so
	 *  we mirror the runtime's `host_os()` choice — Windows in the
	 *  M9 v1 target, but we keep the switch so a future macOS / Linux
	 *  build renders the right variant. */
	function hostSteps(r: Recipe): RecipeStep[] {
		// `navigator.platform` is the cheapest cross-browser host hint
		// reachable from a WebView. Tauri exposes `os.platform()` but
		// that's async + costs an IPC roundtrip; the platform string is
		// enough here.
		const platform = (navigator.platform || '').toLowerCase();
		if (platform.includes('mac')) return (r.os_steps.macos ?? []) as RecipeStep[];
		if (platform.includes('linux')) return (r.os_steps.linux ?? []) as RecipeStep[];
		return (r.os_steps.windows ?? []) as RecipeStep[];
	}

	const steps = $derived(recipe ? hostSteps(recipe) : []);

	/** Total wall-clock estimate (ms) for the recipe. Matches the
	 *  per-variant numbers in the design's `estimateMs` helper so the
	 *  footer line agrees with what the designer mocked. */
	function estimateMs(step: RecipeStep): number {
		switch (step.type) {
			case 'wait_ms':
				return step.ms;
			case 'wait_for_window':
				return Math.min(800, step.timeout_ms);
			case 'run_shell':
				return 400;
			case 'screenshot_to_clipboard':
				return 200;
			case 'open_app':
				return 800;
			case 'open_url':
				return 400;
			case 'focus_window':
				return 150;
			case 'key_chord':
				return 80;
			case 'type_unicode':
				return 120 + step.text.length * 10;
			default:
				return 150;
		}
	}

	const totalSeconds = $derived(
		(steps.reduce((acc, s) => acc + estimateMs(s), 0) / 1000).toFixed(1)
	);

	const isUser = $derived(listing.source === 'user');
	const isBundled = $derived(listing.source === 'bundled');
	const hasSlots = $derived((recipe?.parameters?.length ?? 0) > 0);

	onMount(() => {
		const handler = (e: KeyboardEvent) => {
			if (e.key === 'Escape') {
				e.preventDefault();
				onclose?.();
			}
		};
		window.addEventListener('keydown', handler);
		return () => window.removeEventListener('keydown', handler);
	});
</script>

<div
	class="panel"
	role="dialog"
	aria-modal="false"
	aria-label={`${listing.name} — ${listing.id}`}
>
	<div class="hearth-edge" aria-hidden="true"></div>

	<header class="header">
		<div class="title-row">
			<span class="glyph-tile">
				<RecipeGlyph size={14} />
			</span>
			<div class="title-text">
				<div class="name he">{listing.name}</div>
				<div class="meta">
					<span class="id mono">{listing.id}</span>
					<span class="dot">·</span>
					<SourceBadge kind={listing.source as RecipeSource} />
				</div>
			</div>
			<button class="close" type="button" onclick={() => onclose?.()} aria-label={$t('hub.recipes.steps.closeAriaEsc')}>
				✕
			</button>
		</div>

		{#if listing.permissions.length > 0}
			<div class="perms">
				{#each listing.permissions as p (p)}
					<PermissionBadge kind={p} size="sm" />
				{/each}
			</div>
		{/if}

		<div class="actions">
			<button class="btn primary he-sans" type="button" onclick={() => onrun?.()}>
				<svg width="9" height="10" viewBox="0 0 9 10" aria-hidden="true">
					<path d="M1 1l7 4-7 4z" fill="currentColor" />
				</svg>
				{$t('hub.recipes.steps.run')}
			</button>
			<button class="btn ghost he-sans" type="button" onclick={() => oneditYaml?.()}>
				<svg width="11" height="11" viewBox="0 0 16 16" fill="none" aria-hidden="true">
					<path
						d="M2 14L4 12L11 5L13 7L6 14L2 14Z M11 5L12.5 3.5L13.5 4.5L12 6"
						stroke="currentColor"
						stroke-width="1.4"
						stroke-linejoin="round"
					/>
				</svg>
				{$t('hub.recipes.steps.editYaml')}
			</button>
			{#if isBundled}
				<button class="btn ghost he-sans" type="button" onclick={() => onduplicate?.()}>
					<svg width="11" height="11" viewBox="0 0 16 16" fill="none" aria-hidden="true">
						<rect x="2" y="2" width="9" height="9" rx="1" stroke="currentColor" stroke-width="1.4" />
						<path d="M5 14h8a1 1 0 0 0 1-1V6" stroke="currentColor" stroke-width="1.4" />
					</svg>
					{$t('hub.recipes.steps.duplicate')}
				</button>
			{/if}
		</div>

		{#if hasSlots}
			<div class="slot-legend he-sans">
				<SlotPill name="example" />
				<span>{$t('hub.recipes.steps.slotLegend')}</span>
			</div>
		{/if}
	</header>

	<div class="body">
		{#if parseError}
			<Banner kind="warn" title={$t('hub.recipes.steps.errorParseTitle')}>
				<span class="he-sans">{$t('hub.recipes.steps.errorParseBody')}</span>
			</Banner>
			<div class="path-line mono">{listing.path}</div>
			<button class="btn warn he-sans" type="button" onclick={() => oneditYaml?.()}>
				{$t('hub.recipes.steps.openFile')}
			</button>
		{:else if steps.length === 0}
			<Banner kind="info" title={$t('hub.recipes.steps.errorEmptyTitle')}>
				<span class="he-sans">{$t('hub.recipes.steps.errorEmptyBody')}</span>
			</Banner>
		{:else}
			<div class="stack">
				{#each steps as step, i (i)}
					<StepCard
						{step}
						index={i}
						total={steps.length}
						editable={isUser}
						notImplemented={step.type === 'click_label'}
						onCommentSave={(next) => oncomment?.(i, next)}
					/>
				{/each}
			</div>
		{/if}
	</div>

	<footer class="footer">
		<span class="count he-sans">
			{#if steps.length > 0}
				<b>{$t('hub.recipes.steps.stepCount').replace('{n}', String(steps.length))}</b>
				<span class="dot">·</span>
				<span class="mono"
					>{$t('hub.recipes.steps.durationApprox').replace('{seconds}', totalSeconds)}</span
				>
			{/if}
		</span>
		<span class="esc mono">{$t('hub.recipes.steps.escClose')}</span>
	</footer>
</div>

<style>
	.panel {
		--panel-width: 520px;
		width: var(--panel-width);
		max-width: 100vw;
		height: 100%;
		display: flex;
		flex-direction: column;
		background: var(--ink);
		color: var(--ink-text);
		direction: rtl;
		box-shadow:
			-24px 0 64px rgba(0, 0, 0, 0.45),
			inset 1px 0 0 var(--ink-line);
		font-family: var(--font-he-sans);
		position: relative;
	}
	.hearth-edge {
		position: absolute;
		top: 0;
		inset-inline: 0;
		height: 2px;
		background: var(--hearth);
		box-shadow: 0 0 18px rgba(217, 122, 74, 0.66);
	}
	.header {
		padding: 18px 22px 14px;
		border-bottom: 1px solid var(--ink-line);
		flex: 0 0 auto;
	}
	.title-row {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		margin-bottom: 10px;
	}
	.glyph-tile {
		width: 30px;
		height: 30px;
		border-radius: 7px;
		background: var(--hearth-soft);
		display: inline-flex;
		align-items: center;
		justify-content: center;
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.33);
		margin-top: 2px;
		flex: 0 0 auto;
	}
	.title-text {
		flex: 1;
		min-width: 0;
	}
	.name {
		font-size: 19px;
		font-weight: 500;
		line-height: 1.25;
		margin-bottom: 2px;
	}
	.meta {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.id {
		font-size: 10.5px;
		color: var(--ink-faint);
		direction: ltr;
	}
	.dot {
		color: var(--ink-faint);
	}
	.close {
		background: transparent;
		border: none;
		color: var(--ink-mute);
		cursor: pointer;
		font-size: 14px;
		padding: 4px;
		line-height: 1;
	}
	.close:hover,
	.close:focus-visible {
		color: var(--ink-text);
		outline: none;
	}
	.perms {
		display: flex;
		gap: 5px;
		flex-wrap: wrap;
		margin-bottom: 12px;
	}
	.actions {
		display: flex;
		gap: 8px;
	}
	.btn {
		border-radius: 7px;
		font-weight: 600;
		font-size: 12.5px;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-family: var(--font-he-sans);
	}
	.btn.primary {
		padding: 8px 16px;
		border: none;
		background: var(--saffron);
		color: var(--ink);
		font-weight: 700;
	}
	.btn.ghost {
		padding: 8px 12px;
		border: 1px solid var(--ink-line-2);
		background: transparent;
		color: var(--ink-text);
	}
	.btn.warn {
		padding: 8px 14px;
		border: none;
		background: var(--state-error);
		color: #fff;
		font-weight: 700;
	}
	.btn:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}
	.slot-legend {
		margin-top: 12px;
		font-size: 11px;
		color: var(--ink-mute);
		display: inline-flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}
	.body {
		flex: 1;
		overflow: auto;
		padding: 16px 18px 18px;
	}
	.stack {
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.path-line {
		font-size: 11px;
		color: var(--ink-faint);
		direction: ltr;
		margin: 12px 0 10px;
	}
	.footer {
		padding: 10px 22px;
		border-top: 1px solid var(--ink-line);
		background: var(--ink-2);
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 11px;
		flex: 0 0 auto;
	}
	.count {
		color: var(--ink-mute);
	}
	.count b {
		color: var(--ink-text);
	}
	.esc {
		font-size: 9.5px;
		color: var(--ink-faint);
		direction: ltr;
		letter-spacing: 0.5px;
	}
	.mono {
		font-family: var(--font-mono);
	}
</style>
