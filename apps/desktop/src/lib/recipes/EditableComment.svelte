<!--
	EditableComment — the per-step comment line under a StepCard. Three
	visual states:
	- **populated**: shows `# <value>` in italic muted type
	- **empty + editable**: shows a `+ Add comment` affordance
	- **editing**: `<input>` inside a FieldShell, Enter saves / Esc reverts

	When `editable` is false (bundled recipe), the empty state is
	suppressed entirely — there's no affordance and nothing to read.
	When the comment is present but editable is false (bundled
	recipe with author-supplied comment), the populated state
	renders read-only.

	Save / revert events bubble up to the StepsPanel via the
	`onsave` / `oncancel` callbacks; the parent owns the network
	call to the `update_recipe_comment` Tauri command and the
	error-toast plumbing.
-->
<script lang="ts">
	import { onMount, tick } from 'svelte';
	import FieldShell from '../design/FieldShell.svelte';
	import { t } from '$lib/i18n';

	let {
		value,
		editable = false,
		onsave,
		oncancel
	}: {
		value: string | null | undefined;
		editable?: boolean;
		onsave?: (next: string | null) => void;
		oncancel?: () => void;
	} = $props();

	let editing = $state(false);
	let draft = $state('');
	let inputEl: HTMLInputElement | undefined = $state(undefined);

	const isEmpty = $derived(!value || value.trim().length === 0);

	function startEdit() {
		if (!editable) return;
		draft = value ?? '';
		editing = true;
		void tick().then(() => inputEl?.focus());
	}

	function commit() {
		if (!editing) return;
		const next = draft.trim();
		editing = false;
		// Normalise empty to `null` so the YAML loses the field entirely
		// — matches what the backend's `set_step_comment(None)` does.
		onsave?.(next.length === 0 ? null : next);
	}

	function cancel() {
		if (!editing) return;
		editing = false;
		draft = '';
		oncancel?.();
	}

	function onKey(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			commit();
		} else if (event.key === 'Escape') {
			event.preventDefault();
			cancel();
		}
	}
</script>

{#if editing}
	<div class="row">
		<FieldShell focus>
			<input
				bind:this={inputEl}
				bind:value={draft}
				type="text"
				class="input he-sans"
				dir="auto"
				placeholder={$t('hub.recipes.steps.commentPlaceholder')}
				aria-label={$t('hub.recipes.steps.commentPlaceholder')}
				onkeydown={onKey}
				onblur={commit}
			/>
		</FieldShell>
		<div class="hint mono">{$t('hub.recipes.steps.saveRevertHint')}</div>
	</div>
{:else if isEmpty}
	{#if editable}
		<button class="add he-sans" type="button" onclick={startEdit}>
			<svg width="9" height="9" viewBox="0 0 10 10" aria-hidden="true">
				<path
					d="M5 1v8M1 5h8"
					stroke="currentColor"
					stroke-width="1.4"
					stroke-linecap="round"
				/>
			</svg>
			{$t('hub.recipes.steps.addComment')}
		</button>
	{/if}
{:else if editable}
	<!-- Author-supplied comment on a user recipe — click / keyboard
	     activates the inline editor. Genuine `role="button"` so a
	     tabindex of 0 is appropriate. -->
	<div
		class="comment he-sans interactive"
		role="button"
		tabindex={0}
		onclick={startEdit}
		onkeydown={(e) => {
			if (e.key === 'Enter' || e.key === ' ') {
				e.preventDefault();
				startEdit();
			}
		}}
	>
		<span class="hash" aria-hidden="true">#</span>
		{value}
	</div>
{:else}
	<!-- Bundled recipe — comment is read-only chrome, not a focus
	     stop. Plain text; screen readers still announce it. -->
	<div class="comment he-sans">
		<span class="hash" aria-hidden="true">#</span>
		{value}
	</div>
{/if}

<style>
	.row {
		margin-top: 8px;
		margin-inline-start: 36px;
	}
	.input {
		width: 100%;
		background: transparent;
		border: none;
		outline: none;
		font: inherit;
		color: inherit;
		font-style: italic;
		font-size: 12.5px;
		direction: inherit;
	}
	.hint {
		font-size: 9.5px;
		letter-spacing: 0.4px;
		color: var(--ink-faint);
		margin-top: 4px;
		direction: ltr;
	}
	.add {
		margin-top: 4px;
		margin-inline-start: 36px;
		background: transparent;
		border: none;
		color: var(--ink-faint);
		font-size: 11.5px;
		font-style: italic;
		cursor: pointer;
		padding: 2px 0;
		display: inline-flex;
		align-items: center;
		gap: 5px;
		direction: rtl;
	}
	.add:hover,
	.add:focus-visible {
		color: var(--ink-text);
		outline: none;
	}
	.comment {
		margin-top: 6px;
		margin-inline-start: 32px;
		font-size: 12px;
		font-style: italic;
		color: var(--ink-mute);
		line-height: 1.55;
		border-radius: 4px;
		padding: 2px 4px;
		cursor: inherit;
	}
	.comment.interactive {
		cursor: text;
	}
	.comment.interactive:hover,
	.comment.interactive:focus-visible {
		color: var(--ink-text);
		background: rgba(221, 228, 233, 0.03);
		outline: none;
	}
	.hash {
		color: var(--ink-faint);
		margin-inline-end: 6px;
		font-style: normal;
	}
</style>
