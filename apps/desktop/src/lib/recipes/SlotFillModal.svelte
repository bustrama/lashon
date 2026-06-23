<!--
	Slot-fill modal — renders typed inputs per `parameter.input_type`,
	validates required slots, surfaces a runtime error inline. The
	`run_shell` preview banner is rendered when any step in the
	recipe's host-OS variant is a `RunShell`; the literal interpolated
	command is rendered later by the M8 confirmation modal (the same
	modal `command_mode::run_command` uses), so the rose "run anyway"
	state lives there, not here.

	Wiring:
	- onsubmit({ id, args }) — caller runs the recipe; the modal
	  remains open during the run, with the submit button in a
	  loading state. The caller closes the modal on completion (or
	  the user dismisses it during a long run).
	- oncancel() — close without running.

	Focus: the first input gets autofocus when the modal opens via
	the `autofocus` Svelte action. Enter on any input submits;
	Esc cancels.
-->
<script lang="ts">
	import FieldShell from '$lib/design/FieldShell.svelte';
	import PermissionBadge from '$lib/design/PermissionBadge.svelte';
	import RecipeGlyph from '$lib/design/RecipeGlyph.svelte';
	import SourceBadge from '$lib/design/SourceBadge.svelte';
	import Banner from '$lib/design/Banner.svelte';
	import type { Parameter, RecipeSource } from '$lib/recipes/types';

	type ModalRecipe = {
		id: string;
		name: string;
		description: string;
		parameters: Parameter[];
		permissions: string[];
		// Whether the host-OS variant contains a `run_shell` step.
		// The parent route computes this from the loaded recipe and
		// hands it in so the modal can decide whether to show the
		// warning banner without re-walking os_steps.
		has_shell_step?: boolean;
		source: RecipeSource;
	};

	let {
		recipe,
		runtime_error = null,
		running = false,
		onsubmit,
		oncancel
	}: {
		recipe: ModalRecipe;
		runtime_error?: string | null;
		running?: boolean;
		onsubmit: (args: Record<string, string>) => void;
		oncancel: () => void;
	} = $props();

	// One slot of state per declared parameter. Initialised from
	// `default` when present, falling back to type-appropriate empty.
	let args = $state<Record<string, string>>({});
	let booleanArgs = $state<Record<string, boolean>>({});

	$effect(() => {
		const next: Record<string, string> = {};
		const nextBool: Record<string, boolean> = {};
		for (const p of recipe.parameters) {
			if (p.input_type === 'boolean') {
				nextBool[p.key] = Boolean(p.default ?? false);
				next[p.key] = nextBool[p.key] ? 'true' : 'false';
			} else if (p.default !== null && p.default !== undefined) {
				next[p.key] = String(p.default);
			} else {
				next[p.key] = '';
			}
		}
		args = next;
		booleanArgs = nextBool;
	});

	const missingRequired = $derived(
		recipe.parameters
			.filter((p) => p.requirement === 'required' && p.input_type !== 'boolean')
			.filter((p) => !(args[p.key] ?? '').trim())
	);
	const canSubmit = $derived(!running && missingRequired.length === 0);

	// Autofocus action — applied to the first parameter's input
	// only. Cheaper than tracking refs across all of them.
	function autofocus(node: HTMLElement) {
		// Microtask delay: the modal's $effect that resets `args` may
		// have just rendered, and focusing before the browser paints
		// occasionally drops the focus ring. queueMicrotask
		// guarantees we run after the current tick.
		queueMicrotask(() => node.focus());
	}

	function submit() {
		if (!canSubmit) return;
		// Coerce the boolean state into the args dict before sending.
		const payload: Record<string, string> = { ...args };
		for (const [k, v] of Object.entries(booleanArgs)) {
			payload[k] = v ? 'true' : 'false';
		}
		onsubmit(payload);
	}

	function handleKey(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			event.preventDefault();
			oncancel();
		} else if (event.key === 'Enter') {
			const target = event.target as HTMLElement | null;
			if (target?.tagName === 'TEXTAREA') return;
			event.preventDefault();
			submit();
		}
	}
</script>

<svelte:window onkeydown={handleKey} />

<!-- svelte-ignore a11y_click_events_have_key_events — clicks-outside
     are best-effort dismissal; the Esc key (`handleKey` on the
     window) is the keyboard equivalent. -->
<div
	class="backdrop"
	role="dialog"
	aria-modal="true"
	aria-labelledby="slot-fill-title"
	tabindex="-1"
	onclick={(event) => {
		// Click outside the card closes — but only on the backdrop,
		// not on bubbled clicks from inside the card.
		if (event.target === event.currentTarget) oncancel();
	}}
>
	<div class="card">
		<!-- Header -->
		<div class="header">
			<div class="header-icon">
				<RecipeGlyph size={18} />
			</div>
			<div class="header-text">
				<div id="slot-fill-title" class="title he-display" dir="auto">{recipe.name}</div>
				<div class="subtitle">
					<span class="id mono">{recipe.id}</span>
					<span class="sep" aria-hidden="true">·</span>
					<SourceBadge kind={recipe.source} />
				</div>
			</div>
			<button
				type="button"
				class="close"
				aria-label="סגור"
				onclick={oncancel}
			>✕</button>
		</div>

		<!-- Description -->
		{#if recipe.description}
			<div class="description he-sans" dir="auto">{recipe.description}</div>
		{/if}

		<!-- Permissions summary -->
		{#if recipe.permissions.length > 0}
			<div class="perms">
				{#each recipe.permissions as perm (perm)}
					<PermissionBadge kind={perm} size="md" />
				{/each}
			</div>
		{/if}

		<!-- Shell warning banner — shown when the recipe contains any
		     run_shell step. The literal interpolated command is shown
		     by the M8 confirmation modal at run time, not here, since
		     we don't have the interpolated value until execution. -->
		{#if recipe.has_shell_step}
			<div class="banner-wrap">
				<Banner kind="warn" title="המתכון הזה מריץ פקודות מעטפת">
					לפני כל פקודה תופיע דרישת אישור עם הפקודה המדויקת.
				</Banner>
			</div>
		{/if}

		<!-- Parameter fields -->
		<div class="fields">
			{#each recipe.parameters as param, i (param.key)}
				<div class="field-block">
					<div class="field-head">
						<label
							class="field-label he-sans"
							for={`slot-${param.key}`}
							dir="auto"
						>{param.description || param.key}</label>
						<span class="field-meta mono">{param.key}: {param.input_type}</span>
					</div>

					{#if param.input_type === 'boolean'}
						<!-- Toggle. Keyboard: Space flips, Enter submits. -->
						<div class="toggle-row">
							{#if i === 0}
								<button
									type="button"
									role="switch"
									aria-checked={booleanArgs[param.key]}
									aria-label={param.description || param.key}
									class="toggle"
									class:on={booleanArgs[param.key]}
									onclick={() => (booleanArgs[param.key] = !booleanArgs[param.key])}
									id={`slot-${param.key}`}
									use:autofocus
								>
									<span class="toggle-thumb"></span>
								</button>
							{:else}
								<button
									type="button"
									role="switch"
									aria-checked={booleanArgs[param.key]}
									aria-label={param.description || param.key}
									class="toggle"
									class:on={booleanArgs[param.key]}
									onclick={() => (booleanArgs[param.key] = !booleanArgs[param.key])}
									id={`slot-${param.key}`}
								>
									<span class="toggle-thumb"></span>
								</button>
							{/if}
							<span class="toggle-state he-sans">
								{booleanArgs[param.key] ? 'פעיל' : 'כבוי'}
							</span>
						</div>
					{:else if param.input_type === 'number'}
						<FieldShell mono width="160px">
							{#if i === 0}
								<input
									type="number"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
									use:autofocus
								/>
							{:else}
								<input
									type="number"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
								/>
							{/if}
						</FieldShell>
					{:else if param.input_type === 'date'}
						<FieldShell mono width="200px">
							{#if i === 0}
								<input
									type="date"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
									use:autofocus
								/>
							{:else}
								<input
									type="date"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
								/>
							{/if}
						</FieldShell>
					{:else if param.input_type === 'file'}
						<!-- File picker — the modal can't open a native
						     OS picker without a Tauri bridge yet, so v1
						     accepts a free-text path. A follow-up will
						     wire `dialog.open` from Tauri here. -->
						<FieldShell mono>
							{#if i === 0}
								<input
									type="text"
									class="bare-input"
									id={`slot-${param.key}`}
									placeholder="~/Downloads"
									bind:value={args[param.key]}
									use:autofocus
								/>
							{:else}
								<input
									type="text"
									class="bare-input"
									id={`slot-${param.key}`}
									placeholder="~/Downloads"
									bind:value={args[param.key]}
								/>
							{/if}
						</FieldShell>
					{:else}
						<!-- string -->
						<FieldShell>
							{#if i === 0}
								<input
									type="text"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
									dir="auto"
									use:autofocus
								/>
							{:else}
								<input
									type="text"
									class="bare-input"
									id={`slot-${param.key}`}
									bind:value={args[param.key]}
									dir="auto"
								/>
							{/if}
						</FieldShell>
					{/if}
				</div>
			{/each}
		</div>

		<!-- Runtime error — surfaced after a failed run. -->
		{#if runtime_error}
			<div class="runtime-error" role="alert" dir="auto">
				<Banner kind="warn" title="ההרצה נכשלה">
					{runtime_error}
				</Banner>
			</div>
		{/if}

		<!-- Buttons -->
		<div class="buttons">
			<button
				type="button"
				class="submit he-sans"
				disabled={!canSubmit}
				onclick={submit}
			>
				<svg width="10" height="11" viewBox="0 0 9 10" aria-hidden="true">
					<path d="M1 1l7 4-7 4z" fill="currentColor" />
				</svg>
				{running ? 'מריץ…' : 'הרץ מתכון'}
			</button>
			<button
				type="button"
				class="cancel he-sans"
				onclick={oncancel}
				disabled={running}
			>
				בטל
			</button>
		</div>
		<div class="hint mono" aria-hidden="true">↵ to run · Esc to cancel</div>
	</div>
</div>

<style>
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
	.card {
		background: var(--ink-2);
		border-radius: 14px;
		padding: 20px 22px;
		width: 520px;
		max-width: 100%;
		max-height: 90vh;
		overflow: auto;
		color: var(--ink-text);
		box-shadow:
			0 24px 64px rgba(0, 0, 0, 0.6),
			inset 0 0 0 1px var(--ink-line-2);
	}

	.header {
		display: flex;
		align-items: flex-start;
		gap: 10px;
		margin-bottom: 16px;
	}
	.header-icon {
		width: 36px;
		height: 36px;
		border-radius: 8px;
		flex: 0 0 auto;
		background: var(--hearth-soft);
		display: inline-flex;
		align-items: center;
		justify-content: center;
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.33);
	}
	.header-text {
		flex: 1;
		min-width: 0;
	}
	.title {
		font-size: 19px;
		font-weight: 500;
		line-height: 1.3;
		margin-bottom: 2px;
	}
	.subtitle {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.id {
		font-size: 10.5px;
		color: var(--ink-faint);
		direction: ltr;
		font-family: var(--font-mono);
	}
	.sep {
		color: var(--ink-faint);
	}
	.close {
		background: transparent;
		border: none;
		color: var(--ink-mute);
		cursor: pointer;
		font-size: 16px;
		padding: 0;
		line-height: 1;
		font-family: inherit;
	}
	.close:hover {
		color: var(--ink-text);
	}
	.close:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	.description {
		font-size: 13px;
		color: var(--ink-mute);
		line-height: 1.55;
		margin-bottom: 14px;
	}

	.perms {
		display: flex;
		gap: 5px;
		flex-wrap: wrap;
		margin-bottom: 18px;
	}

	.banner-wrap {
		margin-bottom: 14px;
	}

	.fields {
		display: flex;
		flex-direction: column;
		gap: 14px;
		margin-bottom: 18px;
	}
	.field-block {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.field-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		margin-bottom: 4px;
	}
	.field-label {
		font-size: 12.5px;
		font-weight: 600;
		color: var(--ink-text);
	}
	.field-meta {
		font-size: 9.5px;
		color: var(--ink-faint);
		direction: ltr;
	}

	.bare-input {
		all: unset;
		display: block;
		width: 100%;
		box-sizing: border-box;
		color: inherit;
		font-family: inherit;
		font-size: inherit;
		direction: inherit;
	}
	.bare-input::placeholder {
		color: var(--ink-faint);
	}

	.toggle-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.toggle {
		all: unset;
		display: inline-block;
		width: 38px;
		height: 22px;
		border-radius: 999px;
		background: var(--ink-3);
		position: relative;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		cursor: pointer;
		vertical-align: middle;
	}
	.toggle.on {
		background: var(--saffron);
	}
	.toggle-thumb {
		position: absolute;
		top: 2px;
		inset-inline-start: 18px;
		width: 18px;
		height: 18px;
		border-radius: 999px;
		background: #fff;
		box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
		transition: inset-inline-start 0.12s ease;
	}
	.toggle.on .toggle-thumb {
		inset-inline-start: 2px;
	}
	.toggle:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}
	.toggle-state {
		font-size: 12px;
		color: var(--ink-mute);
	}
	@media (prefers-reduced-motion: reduce) {
		.toggle-thumb {
			transition: none;
		}
	}

	.runtime-error {
		margin-bottom: 14px;
	}

	.buttons {
		display: flex;
		gap: 8px;
		margin-top: 4px;
	}
	.submit {
		flex: 1;
		padding: 10px 12px;
		border-radius: 9px;
		border: none;
		background: var(--saffron);
		color: var(--ink);
		font-family: inherit;
		font-weight: 700;
		font-size: 14px;
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
	}
	.submit:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	.submit:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}
	.cancel {
		padding: 10px 18px;
		border-radius: 9px;
		background: transparent;
		color: var(--ink-text);
		border: 1px solid var(--ink-line-2);
		font-family: inherit;
		font-weight: 600;
		font-size: 14px;
		cursor: pointer;
	}
	.cancel:hover:not(:disabled) {
		background: rgba(221, 228, 233, 0.04);
	}
	.cancel:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}
	.cancel:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.hint {
		font-size: 9.5px;
		color: var(--ink-faint);
		margin-top: 10px;
		text-align: center;
		letter-spacing: 0.5px;
		direction: ltr;
	}
</style>
