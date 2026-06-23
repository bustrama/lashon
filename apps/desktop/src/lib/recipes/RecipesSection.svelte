<!--
	Hub Recipes section. The whole tab — toolbar, filters, tag scrubber,
	row list, footer counter — lives in this single component so the
	Hub +page.svelte stays manageable. Spawned from the Hub's section
	switcher when `section === 'recipes'`.

	State:
	- rows: the listing from the Tauri backend
	- search / filter / activeTag: client-side filtering (no IPC)
	- selectedForRun: the recipe the slot-fill modal is currently
	  rendering (null when no modal)
	- runtimeError + running: drive the modal's inline error + loading
	- deleteConfirm: the recipe id awaiting Trash confirmation

	Concurrency: only one Tauri write (delete/duplicate/run) is in
	flight at a time. The Run / Trash / Duplicate buttons disable while
	one is running so the IPC race doesn't leave the user in a
	half-loaded state.
-->
<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import EmptyState from '$lib/recipes/EmptyState.svelte';
	import FilterChip from '$lib/recipes/FilterChip.svelte';
	import RecipeRow from '$lib/recipes/RecipeRow.svelte';
	import SlotFillModal from '$lib/recipes/SlotFillModal.svelte';
	import StepsPanel from '$lib/recipes/StepsPanel.svelte';
	import Banner from '$lib/design/Banner.svelte';
	import { t } from '$lib/i18n';
	import { onMount } from 'svelte';
	import type {
		HubRecipeListing,
		Parameter,
		Recipe,
		RecipeSource,
		RunOutcome
	} from '$lib/recipes/types';

	type FilterKey = 'all' | 'bundled' | 'user' | 'destructive';

	let rows = $state<HubRecipeListing[]>([]);
	let loading = $state(true);
	let listError = $state<string | null>(null);

	let search = $state('');
	let activeFilter = $state<FilterKey>('all');
	let activeTag = $state<string | null>(null);

	type ModalRecipe = {
		id: string;
		name: string;
		description: string;
		parameters: Parameter[];
		permissions: string[];
		has_shell_step: boolean;
		source: RecipeSource;
	};
	let modalRecipe = $state<ModalRecipe | null>(null);
	let runtimeError = $state<string | null>(null);
	let running = $state(false);

	// Toast surfaced after a successful run (zero-param recipes
	// skip the modal entirely; this toast is their only feedback)
	// or after duplicate / delete. Auto-clears after 4 seconds.
	let toast = $state<{ kind: 'success' | 'info' | 'warn'; text: string } | null>(null);
	let toastTimer: ReturnType<typeof setTimeout> | null = null;
	function flashToast(kind: 'success' | 'info' | 'warn', text: string) {
		toast = { kind, text };
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => (toast = null), 4_000);
	}

	// Delete confirmation. `null` → no prompt; otherwise → showing
	// the "really delete <id>?" inline modal.
	let deleteConfirm = $state<{ id: string; name: string } | null>(null);

	// Steps panel — opened by Eye on a recipe row. Holds the listing
	// row (for chrome metadata) + the loaded Recipe (for the step
	// list) + the parse error message when loading fails.
	let stepsListing = $state<HubRecipeListing | null>(null);
	let stepsRecipe = $state<Recipe | null>(null);
	let stepsParseError = $state<string | null>(null);

	let { onopenmcp = () => {} }: { onopenmcp?: () => void } = $props();

	onMount(() => {
		void refresh();
		return () => {
			if (toastTimer) clearTimeout(toastTimer);
		};
	});

	async function refresh() {
		loading = true;
		listError = null;
		try {
			rows = await invoke<HubRecipeListing[]>('list_recipes_for_hub');
		} catch (err) {
			listError = String(err);
			console.error('hub: list_recipes_for_hub failed', err);
		} finally {
			loading = false;
		}
	}

	// All tags that appear in the listing — drives the tag scrubber.
	// Unique-then-sorted; we use the listing rather than a hardcoded
	// set so user-added tags surface automatically.
	const allTags = $derived.by(() => {
		const set = new Set<string>();
		for (const r of rows) for (const tag of r.tags) set.add(tag);
		return Array.from(set).sort();
	});

	const filtered = $derived.by(() => {
		const q = search.trim().toLowerCase();
		return rows.filter((r) => {
			if (activeFilter === 'bundled' && r.source !== 'bundled') return false;
			if (activeFilter === 'user' && r.source !== 'user') return false;
			if (activeFilter === 'destructive' && !r.permissions.includes('destructive'))
				return false;
			if (activeTag && !r.tags.includes(activeTag)) return false;
			if (q) {
				const hay =
					`${r.id} ${r.name} ${r.description} ${r.tags.join(' ')}`.toLowerCase();
				if (!hay.includes(q)) return false;
			}
			return true;
		});
	});

	// ── Action handlers (Run / Edit / Trash / Eye / Duplicate / Open file) ──

	async function onRun(id: string) {
		const row = rows.find((r) => r.id === id);
		if (!row) return;
		// Zero-param recipes skip the modal and fire immediately,
		// flashing a toast on completion. Destructive zero-param
		// recipes still hit the M8 confirmation modal at run_shell
		// time — the runtime gate is independent of the slot-fill UI.
		if (row.parameter_count === 0) {
			await actuallyRunRecipe(id, {});
			return;
		}
		// Load the full recipe (parameters + os_steps) so the modal
		// can render typed inputs + compute has_shell_step.
		try {
			const recipe = await invoke<Recipe>('get_recipe', { id });
			const hostSteps =
				recipe.os_steps.windows ?? recipe.os_steps.macos ?? recipe.os_steps.linux ?? [];
			modalRecipe = {
				id: recipe.id,
				name: recipe.name,
				description: recipe.description,
				parameters: recipe.parameters,
				permissions: recipe.permissions,
				has_shell_step: hostSteps.some((s) => s.type === 'run_shell'),
				source: row.source as RecipeSource
			};
			runtimeError = null;
		} catch (err) {
			flashToast('warn', `${$t('hub.recipes.errorLoad')}: ${err}`);
		}
	}

	async function actuallyRunRecipe(id: string, args: Record<string, string>) {
		running = true;
		runtimeError = null;
		try {
			const outcome = await invoke<RunOutcome>('run_recipe', { id, args });
			flashToast('success', outcome.summary);
			modalRecipe = null;
		} catch (err) {
			runtimeError = String(err);
			if (modalRecipe === null) {
				// Zero-param path — no modal to attach the error to;
				// surface as a toast.
				flashToast('warn', `${$t('hub.recipes.errorRun')}: ${err}`);
			}
		} finally {
			running = false;
		}
	}

	function onEdit(id: string) {
		void invoke('open_recipe_file', { id }).catch((err) => {
			flashToast('warn', `${$t('hub.recipes.errorOpen')}: ${err}`);
		});
	}

	function onTrash(id: string) {
		const row = rows.find((r) => r.id === id);
		if (!row) return;
		deleteConfirm = { id: row.id, name: row.name };
	}

	async function confirmDelete() {
		if (!deleteConfirm) return;
		const { id, name } = deleteConfirm;
		deleteConfirm = null;
		try {
			await invoke('delete_user_recipe', { id });
			flashToast('success', $t('hub.recipes.deletedToast').replace('{name}', name));
			await refresh();
		} catch (err) {
			flashToast('warn', `${$t('hub.recipes.errorDelete')}: ${err}`);
		}
	}

	async function onEye(id: string) {
		// Open the Steps panel — the visual recipe viewer. Loads the
		// full Recipe (so the panel can render step cards) plus the
		// listing row (for header chrome + permissions). A row that
		// failed to parse at list time still gets the panel open — it
		// renders the parse-error banner with an "open file" button.
		const row = rows.find((r) => r.id === id);
		if (!row) return;
		stepsListing = row;
		stepsParseError = row.parse_error;
		if (row.parse_error) {
			stepsRecipe = null;
			return;
		}
		try {
			stepsRecipe = await invoke<Recipe>('get_recipe', { id });
		} catch (err) {
			stepsRecipe = null;
			stepsParseError = String(err);
		}
	}

	function closeStepsPanel() {
		stepsListing = null;
		stepsRecipe = null;
		stepsParseError = null;
	}

	async function onCommentSave(stepIndex: number, next: string | null) {
		if (!stepsListing) return;
		const id = stepsListing.id;
		try {
			await invoke('update_recipe_comment', { id, stepIndex, comment: next });
			// Re-fetch the recipe so the panel reflects the persisted
			// state. Cheap — get_recipe is a single file read.
			stepsRecipe = await invoke<Recipe>('get_recipe', { id });
		} catch (err) {
			flashToast('warn', `${$t('hub.recipes.steps.errorComment')}: ${err}`);
		}
	}

	async function onDuplicate(id: string) {
		try {
			const newId = await invoke<string>('duplicate_recipe_to_user', { id });
			flashToast('success', $t('hub.recipes.duplicatedToast').replace('{id}', newId));
			await refresh();
		} catch (err) {
			flashToast('warn', `${$t('hub.recipes.errorDuplicate')}: ${err}`);
		}
	}

	function onOpenFile(id: string) {
		void invoke('open_recipe_file', { id }).catch((err) => {
			flashToast('warn', `${$t('hub.recipes.errorOpen')}: ${err}`);
		});
	}

	function toggleTag(tag: string) {
		activeTag = activeTag === tag ? null : tag;
	}
</script>

<section aria-live="polite" class="recipes-section">
	<h2 class="section-head">
		<span class="section-title he-display">{$t('hub.recipes.title')}</span>
		<span class="section-en lat">· Recipes</span>
	</h2>
	<p class="intro he-sans">{$t('hub.recipes.intro')}</p>

	{#if toast}
		<div class="toast-wrap" role="status" aria-live="polite">
			<Banner kind={toast.kind}>
				{toast.text}
			</Banner>
		</div>
	{/if}

	{#if listError}
		<div class="toast-wrap" role="alert">
			<Banner kind="warn" title={$t('hub.recipes.errorList')}>
				{listError}
			</Banner>
		</div>
	{/if}

	{#if !loading && rows.length === 0 && !listError}
		<EmptyState onopenmcp={onopenmcp} onbrowsebundled={() => void refresh()} />
	{:else}
		<!-- Toolbar: search + filter chips + future "Create" stub -->
		<div class="toolbar">
			<div class="search">
				<svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
					<circle cx="7" cy="7" r="5" stroke="currentColor" stroke-width="1.5" />
					<path d="M11 11l3 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
				</svg>
				<input
					type="search"
					class="search-input he-sans"
					placeholder={$t('hub.recipes.searchPlaceholder')}
					bind:value={search}
					dir="auto"
					aria-label={$t('hub.recipes.searchPlaceholder')}
				/>
			</div>
			<FilterChip active={activeFilter === 'all'} onclick={() => (activeFilter = 'all')}>
				{$t('hub.recipes.filterAll')}
			</FilterChip>
			<FilterChip
				active={activeFilter === 'bundled'}
				onclick={() => (activeFilter = 'bundled')}
			>
				{$t('hub.recipes.filterBundled')}
			</FilterChip>
			<FilterChip active={activeFilter === 'user'} onclick={() => (activeFilter = 'user')}>
				{$t('hub.recipes.filterUser')}
			</FilterChip>
			<FilterChip
				active={activeFilter === 'destructive'}
				danger
				onclick={() => (activeFilter = 'destructive')}
			>
				{$t('hub.recipes.filterDestructive')}
			</FilterChip>
			<button class="create-btn he-sans" type="button" onclick={onopenmcp}>
				<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
					<path
						d="M5 1v8M1 5h8"
						stroke="currentColor"
						stroke-width="1.6"
						stroke-linecap="round"
					/>
				</svg>
				{$t('hub.recipes.create')}
			</button>
		</div>

		{#if allTags.length > 0}
			<div class="tag-scrubber">
				<span class="tag-label he-sans">{$t('hub.recipes.tagsLabel')}</span>
				{#each allTags as tag (tag)}
					<button
						type="button"
						class="tag-button"
						class:active={activeTag === tag}
						onclick={() => toggleTag(tag)}
					>#{tag}</button>
				{/each}
			</div>
		{/if}

		<!-- List -->
		<div class="list" role="list">
			<div class="list-head he-sans">
				<span>{$t('hub.recipes.colName')}</span>
				<span>{$t('hub.recipes.colDesc')}</span>
				<span>{$t('hub.recipes.colSourceAction')}</span>
			</div>
			{#if loading}
				<div class="loading-row he-sans">{$t('hub.recipes.loading')}</div>
			{:else if filtered.length === 0}
				<div class="no-match he-sans">{$t('hub.recipes.noMatch')}</div>
			{:else}
				{#each filtered as recipe (recipe.id)}
					<RecipeRow
						{recipe}
						onrun={onRun}
						onedit={onEdit}
						ontrash={onTrash}
						oneye={onEye}
						onduplicate={onDuplicate}
						onopenfile={onOpenFile}
					/>
				{/each}
			{/if}
		</div>

		<!-- Footer counter — pinned to the bottom of the list. -->
		<div class="footer">
			<span class="footer-counts he-sans">
				{$t('hub.recipes.footer.count').replace('{n}', String(rows.length))}
				·
				{$t('hub.recipes.footer.user').replace(
					'{n}',
					String(rows.filter((r) => r.source === 'user').length)
				)}
				·
				{$t('hub.recipes.footer.bundled').replace(
					'{n}',
					String(rows.filter((r) => r.source === 'bundled').length)
				)}
			</span>
			<button class="refresh mono" type="button" onclick={() => void refresh()}>↻ refresh</button>
		</div>
	{/if}

	<!-- Slot-fill modal — opened by Run on a parameter-bearing recipe. -->
	{#if modalRecipe}
		<SlotFillModal
			recipe={modalRecipe}
			runtime_error={runtimeError}
			{running}
			onsubmit={(args) => void actuallyRunRecipe(modalRecipe!.id, args)}
			oncancel={() => {
				if (running) return;
				modalRecipe = null;
				runtimeError = null;
			}}
		/>
	{/if}

	<!-- Steps panel — opened by Eye on a recipe row. Side drawer that
	     slides in from the inline-end edge; recipe list stays visible
	     behind so the user can switch recipes without dismissing. -->
	{#if stepsListing}
		<div
			class="steps-overlay"
			role="presentation"
			onclick={(e) => {
				if (e.currentTarget === e.target) closeStepsPanel();
			}}
			onkeydown={() => {}}
		>
			<StepsPanel
				listing={stepsListing}
				recipe={stepsRecipe}
				parseError={stepsParseError}
				onclose={closeStepsPanel}
				onrun={() => {
					const id = stepsListing!.id;
					closeStepsPanel();
					void onRun(id);
				}}
				oneditYaml={() => stepsListing && onEdit(stepsListing.id)}
				onduplicate={() => stepsListing && void onDuplicate(stepsListing.id)}
				oncomment={onCommentSave}
			/>
		</div>
	{/if}

	<!-- Delete confirmation modal — small + opinionated. -->
	{#if deleteConfirm}
		<div class="backdrop" role="dialog" aria-modal="true" aria-labelledby="delete-confirm-title">
			<div class="delete-card">
				<div id="delete-confirm-title" class="delete-title he-display">
					{$t('hub.recipes.deleteTitle')}
				</div>
				<div class="delete-body he-sans" dir="auto">
					{@html $t('hub.recipes.deleteBody').replace(
						'{name}',
						`<strong dir="auto">${deleteConfirm.name}</strong>`
					)}
				</div>
				<div class="delete-buttons">
					<button
						type="button"
						class="delete-confirm he-sans"
						onclick={() => void confirmDelete()}
					>
						{$t('hub.recipes.deleteConfirm')}
					</button>
					<button
						type="button"
						class="delete-cancel he-sans"
						onclick={() => (deleteConfirm = null)}
					>
						{$t('hub.recipes.deleteCancel')}
					</button>
				</div>
			</div>
		</div>
	{/if}
</section>

<style>
	.recipes-section {
		display: flex;
		flex-direction: column;
		gap: 0;
	}
	.intro {
		font-size: 13px;
		line-height: 1.6;
		color: var(--ink-mute);
		margin: 0 0 14px;
	}
	.toast-wrap {
		margin-bottom: 14px;
	}

	/* Toolbar. RTL-native — search field grows; chips + create button
	   sit at the inline end. The chip row is `flex` so they wrap
	   gracefully when the column is narrow. */
	.toolbar {
		display: flex;
		gap: 10px;
		margin-bottom: 14px;
		align-items: center;
		flex-wrap: wrap;
	}
	.search {
		flex: 1 1 240px;
		min-width: 200px;
		padding: 8px 12px;
		border-radius: 8px;
		font-size: 13px;
		background: var(--ink-2);
		color: var(--ink-mute);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.search-input {
		all: unset;
		flex: 1 1 auto;
		color: var(--ink-text);
		font-size: 13px;
		font-family: inherit;
	}
	.search-input::placeholder {
		color: var(--ink-mute);
	}
	.create-btn {
		padding: 8px 14px;
		border-radius: 8px;
		border: none;
		background: var(--hearth);
		color: #fff;
		font-family: inherit;
		font-weight: 700;
		font-size: 12.5px;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.create-btn:hover {
		filter: brightness(1.08);
	}
	.create-btn:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}

	.tag-scrubber {
		display: flex;
		gap: 6px;
		margin-bottom: 16px;
		align-items: center;
		flex-wrap: wrap;
	}
	.tag-label {
		font-size: 11px;
		color: var(--ink-faint);
		margin-inline-end: 4px;
	}
	.tag-button {
		padding: 1.5px 7px;
		border-radius: 4px;
		font-size: 10.5px;
		font-weight: 500;
		color: var(--ink-faint);
		background: rgba(221, 228, 233, 0.04);
		border: none;
		font-family: inherit;
		cursor: pointer;
	}
	.tag-button:hover {
		color: var(--ink-text);
		background: rgba(221, 228, 233, 0.08);
	}
	.tag-button.active {
		background: var(--hearth-soft);
		color: var(--hearth);
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.45);
	}
	.tag-button:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	.list {
		border-radius: 10px;
		overflow: hidden;
		box-shadow: inset 0 0 0 1px var(--ink-line);
	}
	.list-head {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(0, 2.3fr) auto;
		gap: 18px;
		padding: 8px 16px;
		background: var(--ink-2);
		font-size: 10px;
		font-weight: 700;
		letter-spacing: 1px;
		text-transform: uppercase;
		color: var(--ink-faint);
		border-bottom: 1px solid var(--ink-line);
	}
	.loading-row,
	.no-match {
		padding: 24px 16px;
		font-size: 13px;
		color: var(--ink-mute);
		text-align: center;
	}

	.footer {
		margin-top: 12px;
		font-size: 11px;
		color: var(--ink-faint);
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
	}
	.footer-counts {
		direction: rtl;
	}
	.refresh {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--ink-mute);
		background: transparent;
		border: none;
		cursor: pointer;
		opacity: 0.7;
		direction: ltr;
	}
	.refresh:hover {
		opacity: 1;
		color: var(--ink-text);
	}
	.refresh:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	/* Delete-confirmation modal — same backdrop pattern as
	   SlotFillModal, but a smaller card and rose accent on the
	   primary action. */
	.backdrop {
		position: fixed;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		background: rgba(0, 0, 0, 0.55);
		backdrop-filter: blur(2px);
		padding: 24px;
		z-index: 100;
		direction: rtl;
	}
	@media (prefers-reduced-motion: reduce) {
		.backdrop {
			backdrop-filter: none;
		}
	}
	.delete-card {
		background: var(--ink-2);
		border-radius: 14px;
		padding: 22px 22px 20px;
		width: 400px;
		max-width: 100%;
		color: var(--ink-text);
		box-shadow:
			0 24px 64px rgba(0, 0, 0, 0.6),
			inset 0 0 0 1px var(--ink-line-2);
	}
	.delete-title {
		font-size: 18px;
		font-weight: 500;
		margin-bottom: 10px;
		color: var(--ink-text);
	}
	.delete-body {
		font-size: 13px;
		color: var(--ink-mute);
		line-height: 1.5;
		margin-bottom: 18px;
	}
	.delete-buttons {
		display: flex;
		gap: 8px;
	}
	.delete-confirm {
		flex: 1;
		padding: 9px 16px;
		border-radius: 8px;
		border: none;
		background: var(--state-error);
		color: #fff;
		font-family: inherit;
		font-weight: 700;
		font-size: 13px;
		cursor: pointer;
	}
	.delete-confirm:hover {
		filter: brightness(1.08);
	}
	.delete-confirm:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}
	.delete-cancel {
		padding: 9px 16px;
		border-radius: 8px;
		background: transparent;
		color: var(--ink-text);
		border: 1px solid var(--ink-line-2);
		font-family: inherit;
		font-weight: 600;
		font-size: 13px;
		cursor: pointer;
	}
	.delete-cancel:hover {
		background: rgba(221, 228, 233, 0.04);
	}
	.delete-cancel:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}

	/* Steps panel overlay — translucent scrim with the panel pinned
	   to the inline-end edge (RTL: left). Clicking outside the panel
	   closes it; inside-panel clicks bubble up to the StepsPanel's
	   own handlers. */
	.steps-overlay {
		position: fixed;
		inset: 0;
		display: flex;
		justify-content: flex-end;
		background: rgba(0, 0, 0, 0.42);
		backdrop-filter: blur(2px);
		z-index: 80;
	}
	@media (prefers-reduced-motion: reduce) {
		.steps-overlay {
			backdrop-filter: none;
		}
	}
</style>
