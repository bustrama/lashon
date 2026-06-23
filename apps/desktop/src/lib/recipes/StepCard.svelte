<!--
	StepCard — one step in the Steps panel. Three-column grid: number
	gutter (right / inline-start RTL) · 28px icon tile · primary
	content. Below the content: the editable comment line.

	`run_shell` steps get a rose-tinted ring + icon background so the
	user sees at a glance which steps run shell commands.

	`notImplemented` (currently only `click_label` in the v1 runtime)
	surfaces a small "v1.1 · not yet runnable" badge so the user
	understands the recipe will fail when it hits that step.
-->
<script lang="ts">
	import EditableComment from './EditableComment.svelte';
	import StepBody from './StepBody.svelte';
	import StepIcon from '../design/StepIcon.svelte';
	import { t } from '$lib/i18n';
	import type { RecipeStep, StepVariant } from './types';

	let {
		step,
		index,
		total,
		editable = false,
		notImplemented = false,
		onCommentSave
	}: {
		step: RecipeStep;
		index: number;
		total: number;
		editable?: boolean;
		notImplemented?: boolean;
		onCommentSave?: (next: string | null) => void;
	} = $props();

	const variant = $derived(step.type as StepVariant);
	const isShell = $derived(variant === 'run_shell');
	const label = $derived($t(`hub.recipes.steps.variants.${variant}`));
	const enLabel = $derived(variant.replace(/_/g, ' '));
	const ariaLabel = $derived(
		$t('hub.recipes.steps.stepAria')
			.replace('{index}', String(index + 1))
			.replace('{total}', String(total)) + ` — ${label}`
	);
	// Step numbers stay LTR + Latin even on a Hebrew locale (matches
	// the design call-out: "Latin digits on the inline-start gutter").
	const stepNumber = $derived(String(index + 1).padStart(2, '0'));
</script>

<!-- The card itself is non-interactive; the editable comment within
     provides the focusable surface. Keeping the group role means screen
     readers announce the bundle, without making the whole tile a tab stop. -->
<div class="card" class:shell={isShell} role="group" aria-label={ariaLabel}>
	<div class="num mono">{stepNumber}</div>
	<div class="icon-tile">
		<StepIcon
			variant={variant}
			size={16}
			color={isShell ? 'var(--state-error)' : 'currentColor'}
		/>
	</div>
	<div class="content">
		<div class="header">
			<span class="label he-sans" class:shell={isShell}>{label}</span>
			<span class="en lat">· {enLabel}</span>
			{#if notImplemented}
				<span
					class="not-yet mono"
					title={$t('hub.recipes.steps.notYetRunnableTitle')}
				>
					{$t('hub.recipes.steps.notYetRunnable')}
				</span>
			{/if}
		</div>
		<StepBody {step} />
		<EditableComment value={step.comment} {editable} onsave={onCommentSave} />
	</div>
</div>

<style>
	.card {
		display: grid;
		grid-template-columns: 28px 28px 1fr;
		gap: 12px;
		padding: 14px 14px 12px;
		border-radius: 10px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line);
		direction: rtl;
		transition: box-shadow 0.12s ease;
	}
	.card:focus-visible {
		outline: none;
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.66);
	}
	.card.shell {
		background: rgba(232, 98, 90, 0.05);
		box-shadow: inset 0 0 0 1px rgba(232, 98, 90, 0.33);
	}
	.num {
		font-size: 13px;
		font-weight: 700;
		color: var(--ink-faint);
		direction: ltr;
		text-align: right;
		padding-top: 1px;
		user-select: none;
	}
	.icon-tile {
		width: 28px;
		height: 28px;
		border-radius: 7px;
		background: rgba(221, 228, 233, 0.04);
		display: flex;
		align-items: center;
		justify-content: center;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		flex: 0 0 auto;
		color: var(--ink-text);
	}
	.card.shell .icon-tile {
		background: rgba(232, 98, 90, 0.10);
		box-shadow: inset 0 0 0 1px rgba(232, 98, 90, 0.27);
	}
	.content {
		min-width: 0;
	}
	.header {
		display: flex;
		align-items: baseline;
		gap: 8px;
		margin-bottom: 6px;
		flex-wrap: wrap;
	}
	.label {
		font-size: 12px;
		font-weight: 700;
		letter-spacing: 0.2px;
		color: var(--ink-text);
	}
	.label.shell {
		color: var(--state-error);
	}
	.en {
		font-size: 10.5px;
		color: var(--ink-faint);
		font-style: italic;
	}
	.not-yet {
		font-size: 9px;
		padding: 1.5px 6px;
		border-radius: 3px;
		font-weight: 700;
		letter-spacing: 0.5px;
		color: var(--saffron);
		background: rgba(232, 177, 74, 0.10);
		box-shadow: inset 0 0 0 1px rgba(232, 177, 74, 0.33);
		direction: ltr;
		text-transform: uppercase;
	}
	@media (prefers-reduced-motion: reduce) {
		.card {
			transition: none;
		}
	}
</style>
