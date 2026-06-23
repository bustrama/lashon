<!--
	Mono code block — reused for the shell preview in the slot-fill
	modal's `run_shell` confirmation and for YAML previews in the
	bundled-recipe Eye affordance (when wired). Renders children
	verbatim; the caller is responsible for any syntax highlighting.

	`tint` colours the inset ring 1px (used by the shell preview to
	flash rose). `lang` shows the language tag in the top-end corner.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		tint = null,
		lang = null,
		maxHeight = null,
		children
	}: {
		tint?: string | null;
		lang?: string | null;
		maxHeight?: string | null;
		children: Snippet;
	} = $props();

	const ringStyle = $derived(
		tint
			? `box-shadow: inset 0 0 0 1px ${tint}66;`
			: ''
	);
	const heightStyle = $derived(maxHeight ? `max-height: ${maxHeight}; overflow: auto;` : '');
</script>

<pre style="{ringStyle} {heightStyle}">
	{#if lang}<span class="lang" aria-hidden="true">{lang}</span>{/if}
	{@render children()}
</pre>

<style>
	pre {
		position: relative;
		background: rgba(0, 0, 0, 0.35);
		border-radius: 8px;
		padding: 10px 12px;
		margin: 0;
		font-family: var(--font-mono);
		font-size: 12px;
		line-height: 1.55;
		color: var(--ink-text);
		direction: ltr;
		text-align: left;
		box-shadow: inset 0 0 0 1px var(--ink-line);
		white-space: pre;
		overflow: auto;
	}
	.lang {
		position: absolute;
		top: 8px;
		inset-inline-end: 10px;
		font-size: 9px;
		letter-spacing: 1px;
		text-transform: uppercase;
		color: var(--ink-faint);
		font-weight: 700;
	}
</style>
