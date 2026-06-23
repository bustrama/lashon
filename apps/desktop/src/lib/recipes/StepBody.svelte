<!--
	StepBody — the variant-typed primary content for one step. The
	`StepCard` parent renders the chrome (number gutter + icon +
	header label + comment); this component renders whatever the step
	says it does in human terms.

	Slot placeholders (`{{ recipient }}`) flow through `renderWithSlots`
	+ `SlotPill` so they render as Hearth-tinted pills inline.
-->
<script lang="ts">
	import CodeBlock from '../design/CodeBlock.svelte';
	import KeyChord from '../design/KeyChord.svelte';
	import SlotPill from '../design/SlotPill.svelte';
	import { t } from '$lib/i18n';
	import type { RecipeStep } from './types';

	let { step }: { step: RecipeStep } = $props();

	/** Tokenize a string into literal runs + `SlotPill` instances. */
	function tokenize(text: string): Array<{ kind: 'text' | 'slot'; value: string }> {
		const out: Array<{ kind: 'text' | 'slot'; value: string }> = [];
		const re = /\{\{\s*([\w.-]+)\s*\}\}/g;
		let last = 0;
		let m: RegExpExecArray | null;
		while ((m = re.exec(text)) !== null) {
			if (m.index > last) out.push({ kind: 'text', value: text.slice(last, m.index) });
			out.push({ kind: 'slot', value: m[1] });
			last = m.index + m[0].length;
		}
		if (last < text.length) out.push({ kind: 'text', value: text.slice(last) });
		return out;
	}

	/** Format a timeout-ms number as "N s" / "N.X s". */
	function asSeconds(ms: number): string {
		const seconds = ms / 1000;
		return seconds % 1 === 0 ? seconds.toFixed(0) : seconds.toFixed(1);
	}
</script>

{#if step.type === 'key_chord'}
	<KeyChord keys={step.keys} />
{:else if step.type === 'type_unicode'}
	<div class="inline-row">
		<span class="quote" dir="auto">
			{#each tokenize(step.text) as part, i (i)}
				{#if part.kind === 'slot'}
					<SlotPill name={part.value} />
				{:else}
					{part.value}
				{/if}
			{/each}
		</span>
		{#if step.rtl_safe}
			<span class="tag hearth mono">
				<svg width="9" height="9" viewBox="0 0 10 10" aria-hidden="true">
					<rect
						x="2"
						y="1.5"
						width="6"
						height="7"
						rx="1"
						stroke="currentColor"
						stroke-width="1.2"
						fill="none"
					/>
				</svg>
				{$t('hub.recipes.steps.bodyExtras.rtlSafe')}
			</span>
		{/if}
	</div>
{:else if step.type === 'click_label'}
	<div class="inline-row">
		<span class="quote" dir="auto">
			{#each tokenize(step.label) as part, i (i)}
				{#if part.kind === 'slot'}
					<SlotPill name={part.value} />
				{:else}
					{part.value}
				{/if}
			{/each}
		</span>
		{#if step.window}
			<span class="tag mono">{$t('hub.recipes.steps.bodyExtras.scopePrefix')} {step.window}</span>
		{/if}
	</div>
{:else if step.type === 'focus_window'}
	<span class="quote" dir="auto">
		{#each tokenize(step.title_contains) as part, i (i)}
			{#if part.kind === 'slot'}
				<SlotPill name={part.value} />
			{:else}
				{part.value}
			{/if}
		{/each}
	</span>
{:else if step.type === 'wait_for_window'}
	<div class="inline-row">
		<span class="quote" dir="auto">
			{#each tokenize(step.title_contains) as part, i (i)}
				{#if part.kind === 'slot'}
					<SlotPill name={part.value} />
				{:else}
					{part.value}
				{/if}
			{/each}
		</span>
		<span class="tag mono"
			>{$t('hub.recipes.steps.bodyExtras.upTo').replace('{seconds}', asSeconds(step.timeout_ms))}</span
		>
	</div>
{:else if step.type === 'wait_ms'}
	<span class="ms mono">{step.ms} ms</span>
{:else if step.type === 'screenshot_to_clipboard'}
	<span class="muted he-sans">{$t('hub.recipes.steps.bodyExtras.screenshotNote')}</span>
{:else if step.type === 'clipboard_set'}
	<span class="quote" dir="auto">
		{#each tokenize(step.text) as part, i (i)}
			{#if part.kind === 'slot'}
				<SlotPill name={part.value} />
			{:else}
				{part.value}
			{/if}
		{/each}
	</span>
{:else if step.type === 'clipboard_get_into'}
	<span class="get-into">
		<span class="he-sans muted">{$t('hub.recipes.steps.bodyExtras.saveInto')}</span>
		<span class="var mono">${step.var}</span>
	</span>
{:else if step.type === 'run_shell'}
	<div class="shell-wrap">
		<CodeBlock tint="#e8625a" lang="sh">{step.command}</CodeBlock>
		{#if step.dry_run}
			<div class="dry-run he-sans">{$t('hub.recipes.steps.bodyExtras.dryRunNote')}</div>
		{/if}
	</div>
{:else if step.type === 'open_url'}
	<span class="quote mono" dir="ltr">{step.url}</span>
{:else if step.type === 'open_app'}
	<span class="quote" dir="auto">{step.name}</span>
{/if}

<style>
	.inline-row {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}
	.quote {
		display: inline-block;
		padding: 4px 10px;
		border-radius: 6px;
		background: rgba(221, 228, 233, 0.05);
		color: var(--ink-text);
		font-size: 13px;
		line-height: 1.55;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		max-width: 100%;
		direction: rtl;
		font-family: var(--font-he-sans);
		word-break: break-word;
	}
	.quote.mono {
		font-family: var(--font-mono);
		direction: ltr;
	}
	.tag {
		font-size: 10px;
		padding: 1.5px 6px;
		border-radius: 4px;
		background: rgba(221, 228, 233, 0.04);
		color: var(--ink-faint);
		direction: ltr;
		margin-inline-start: 6px;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		display: inline-flex;
		align-items: center;
		gap: 3px;
	}
	.tag.hearth {
		color: var(--hearth);
	}
	.mono {
		font-family: var(--font-mono);
	}
	.muted {
		font-size: 12.5px;
		color: var(--ink-mute);
		font-style: italic;
	}
	.ms {
		font-size: 13px;
		color: var(--ink-text);
		direction: ltr;
		padding: 2px 8px;
		border-radius: 5px;
		background: rgba(221, 228, 233, 0.04);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		display: inline-block;
	}
	.get-into {
		display: inline-flex;
		align-items: center;
		gap: 6px;
	}
	.var {
		font-size: 12.5px;
		color: var(--hearth);
		direction: ltr;
		padding: 2px 8px;
		border-radius: 4px;
		background: var(--hearth-soft);
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.33);
		font-weight: 600;
	}
	.shell-wrap {
		margin-top: 2px;
	}
	.dry-run {
		font-size: 11px;
		color: var(--state-error);
		margin-top: 6px;
		font-style: italic;
	}
</style>
