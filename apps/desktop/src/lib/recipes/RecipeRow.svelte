<!--
	Single recipe row in the Hub Recipes browser. Three-column grid:
	name (with Hebrew title + mono en id), description + perms + tags,
	source pill + action column.

	The action column has three states:
	- error (parse_error): a single rose-outline "פתח קובץ" button
	- hover (mouse over the row): Run + variant-specific icons
	  (user: Edit + Trash, bundled: Eye + Duplicate)
	- default: the saffron Run button

	The user-vs-bundled split is load-bearing: Trash is destructive
	and only ever applies to user recipes; Duplicate is the entry point
	for customising a bundled recipe.

	Hover is JS-tracked (pointerenter/pointerleave) rather than `:hover`
	so the keyboard-focused row also shows the affordances (a11y).
-->
<script lang="ts">
	import PermissionBadge from '$lib/design/PermissionBadge.svelte';
	import SourceBadge from '$lib/design/SourceBadge.svelte';
	import TagChip from '$lib/design/TagChip.svelte';
	import type { HubRecipeListing, RecipeSource } from '$lib/recipes/types';

	let {
		recipe,
		onrun,
		onedit,
		ontrash,
		oneye,
		onduplicate,
		onopenfile
	}: {
		recipe: HubRecipeListing;
		onrun?: (id: string) => void;
		onedit?: (id: string) => void;
		ontrash?: (id: string) => void;
		oneye?: (id: string) => void;
		onduplicate?: (id: string) => void;
		onopenfile?: (id: string) => void;
	} = $props();

	let hovered = $state(false);
	let focused = $state(false);
	const showActions = $derived(hovered || focused);

	const isDestructive = $derived(recipe.permissions.includes('destructive'));
	const isUser = $derived(recipe.source === 'user');
	const hasError = $derived(recipe.parse_error !== null);
</script>

<div
	class="row"
	class:hover={showActions}
	role="listitem"
	onpointerenter={() => (hovered = true)}
	onpointerleave={() => (hovered = false)}
	onfocusin={() => (focused = true)}
	onfocusout={() => (focused = false)}
>
	{#if isDestructive}
		<!-- Destructive marker — sits on the inline-end edge (right in
		     RTL) so the row reads "danger" at a glance. -->
		<span class="destructive-marker" aria-hidden="true"></span>
	{/if}

	<!-- Name column. The recipe `name` is typically bilingual already
	     (Hebrew label first); the mono `id` line sits beneath it. -->
	<div class="name-col">
		<div class="name-row">
			<span class="title he-sans" dir="auto">{recipe.name}</span>
			{#if hasError}
				<span class="error-dot" aria-hidden="true"></span>
			{/if}
		</div>
		<div class="id mono">{recipe.id}</div>
	</div>

	<!-- Description + perms + tags column. -->
	<div class="desc-col">
		<div class="desc he-sans" dir="auto">
			{#if hasError}
				<span class="error-text">❌ YAML שגוי — לא ניתן לטעון את המתכון</span>
			{:else}
				{recipe.description}
			{/if}
		</div>
		{#if !hasError}
			<div class="badges">
				{#each recipe.permissions as perm (perm)}
					<PermissionBadge kind={perm} size="sm" />
				{/each}
				{#if recipe.permissions.length > 0 && recipe.tags.length > 0}
					<span class="badge-spacer" aria-hidden="true"></span>
				{/if}
				{#each recipe.tags as tag (tag)}
					<TagChip label={tag} />
				{/each}
			</div>
		{/if}
	</div>

	<!-- Action column — source badge always shown; Run / icon set
	     swaps with hover / focus. The hidden state of the icon row
	     uses `visibility: hidden` instead of `display: none` so the
	     row's vertical rhythm doesn't jump when the icons appear. -->
	<div class="action-col">
		<SourceBadge kind={recipe.source as RecipeSource} />

		{#if hasError}
			<button
				type="button"
				class="open-file-btn he-sans"
				onclick={() => onopenfile?.(recipe.id)}
				aria-label="פתח קובץ recipe.yaml"
			>
				פתח קובץ
			</button>
		{:else if showActions}
			<div class="actions">
				<button
					type="button"
					class="icon-btn primary"
					onclick={() => onrun?.(recipe.id)}
					title="הרץ"
					aria-label={`הרץ את ${recipe.id}`}
				>
					<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"
						><path d="M1 1l7 4-7 4z" fill="currentColor" /></svg
					>
				</button>
				{#if isUser}
					<button
						type="button"
						class="icon-btn"
						onclick={() => onedit?.(recipe.id)}
						title="ערוך"
						aria-label={`ערוך את ${recipe.id}`}
					>
						<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
							<path
								d="M2 14L4 12L11 5L13 7L6 14L2 14Z M11 5L12.5 3.5L13.5 4.5L12 6"
								stroke="currentColor"
								stroke-width="1.5"
								stroke-linejoin="round"
							/>
						</svg>
					</button>
					<button
						type="button"
						class="icon-btn danger"
						onclick={() => ontrash?.(recipe.id)}
						title="מחק"
						aria-label={`מחק את ${recipe.id}`}
					>
						<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
							<path
								d="M3 4h10M6 4V2.5h4V4M5 4l1 9h4l1-9"
								stroke="currentColor"
								stroke-width="1.4"
								stroke-linejoin="round"
							/>
						</svg>
					</button>
				{:else}
					<button
						type="button"
						class="icon-btn"
						onclick={() => oneye?.(recipe.id)}
						title="הצג YAML"
						aria-label={`הצג את ה-YAML של ${recipe.id}`}
					>
						<svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
							<path
								d="M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"
								stroke="currentColor"
								stroke-width="1.4"
							/>
							<circle cx="8" cy="8" r="1.8" stroke="currentColor" stroke-width="1.4" />
						</svg>
					</button>
					<button
						type="button"
						class="icon-btn"
						onclick={() => onduplicate?.(recipe.id)}
						title="שכפל למתכונים שלי"
						aria-label={`שכפל את ${recipe.id} למתכונים שלי`}
					>
						<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
							<rect x="2" y="2" width="9" height="9" rx="1" stroke="currentColor" stroke-width="1.4" />
							<path d="M5 14h8a1 1 0 0 0 1-1V6" stroke="currentColor" stroke-width="1.4" />
						</svg>
					</button>
				{/if}
			</div>
		{:else}
			<button
				type="button"
				class="run-btn he-sans"
				onclick={() => onrun?.(recipe.id)}
				aria-label={`הרץ את ${recipe.id}`}
			>
				<svg width="9" height="10" viewBox="0 0 9 10" aria-hidden="true">
					<path d="M1 1l7 4-7 4z" fill="currentColor" />
				</svg>
				הרץ
			</button>
		{/if}
	</div>
</div>

<style>
	.row {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(0, 2.3fr) auto;
		gap: 18px;
		padding: 14px 16px;
		align-items: center;
		border-bottom: 1px solid var(--ink-line);
		background: transparent;
		position: relative;
		transition: background 0.12s ease;
	}
	.row.hover {
		background: rgba(221, 228, 233, 0.025);
	}
	.row:last-child {
		border-bottom: none;
	}
	.destructive-marker {
		position: absolute;
		inset-inline-end: 0;
		top: 12px;
		bottom: 12px;
		width: 2px;
		background: var(--state-error);
		border-radius: 2px;
	}

	.name-col {
		min-width: 0;
	}
	.name-row {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 3px;
	}
	.title {
		font-size: 14px;
		font-weight: 600;
		color: var(--ink-text);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.error-dot {
		width: 5px;
		height: 5px;
		border-radius: 999px;
		background: var(--state-error);
		flex: 0 0 auto;
	}
	.id {
		font-size: 10.5px;
		color: var(--ink-faint);
		direction: ltr;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
	}

	.desc-col {
		min-width: 0;
	}
	.desc {
		font-size: 12.5px;
		color: var(--ink-mute);
		margin-bottom: 6px;
		line-height: 1.45;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.error-text {
		color: var(--state-error);
	}
	.badges {
		display: flex;
		gap: 5px;
		flex-wrap: wrap;
		align-items: center;
	}
	.badge-spacer {
		flex: 0 0 8px;
	}

	.action-col {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 8px;
		min-width: 110px;
	}

	.run-btn {
		padding: 7px 18px;
		border-radius: 7px;
		border: none;
		background: var(--saffron);
		color: var(--ink);
		font-family: inherit;
		font-weight: 700;
		font-size: 12.5px;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.run-btn:hover {
		filter: brightness(1.08);
	}
	.run-btn:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}

	.open-file-btn {
		padding: 7px 14px;
		border-radius: 7px;
		border: none;
		background: transparent;
		color: var(--state-error);
		font-family: inherit;
		font-weight: 600;
		font-size: 12px;
		cursor: pointer;
		box-shadow: inset 0 0 0 1px rgba(232, 98, 90, 0.4);
	}
	.open-file-btn:hover {
		background: rgba(232, 98, 90, 0.08);
	}
	.open-file-btn:focus-visible {
		outline: 3px solid var(--state-error);
		outline-offset: 2px;
	}

	.actions {
		display: flex;
		gap: 5px;
	}
	.icon-btn {
		width: 28px;
		height: 28px;
		border-radius: 6px;
		border: none;
		background: transparent;
		color: var(--ink-text);
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.icon-btn:hover {
		background: rgba(221, 228, 233, 0.05);
	}
	.icon-btn:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}
	.icon-btn.primary {
		background: var(--saffron);
		color: var(--ink);
		box-shadow: none;
	}
	.icon-btn.primary:hover {
		filter: brightness(1.08);
	}
	.icon-btn.danger {
		color: var(--state-error);
	}

	@media (prefers-reduced-motion: reduce) {
		.row {
			transition: none;
		}
	}
</style>
