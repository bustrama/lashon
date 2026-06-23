<!--
	Input shell — the visual container the slot-fill modal wraps every
	field control in (text, number, file picker). Holds the focus ring
	+ background so the inner control stays plain. `mono` flips the
	direction to LTR and the font to monospace (number / date / file).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		mono = false,
		focus = false,
		width = null,
		children
	}: {
		mono?: boolean;
		focus?: boolean;
		width?: string | null;
		children: Snippet;
	} = $props();

	const widthStyle = $derived(width ? `width: ${width};` : '');
</script>

<div
	class="field"
	class:focus
	class:mono
	style={widthStyle}
>
	{@render children()}
</div>

<style>
	.field {
		background: var(--ink-2);
		padding: 8px 12px;
		border-radius: 8px;
		font-size: 13.5px;
		color: var(--ink-text);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		font-family: var(--font-he-sans);
		direction: rtl;
		transition: box-shadow 0.12s ease;
	}
	.field.focus {
		box-shadow: inset 0 0 0 1.5px var(--saffron);
	}
	.field.mono {
		font-family: var(--font-mono);
		direction: ltr;
	}
	@media (prefers-reduced-motion: reduce) {
		.field {
			transition: none;
		}
	}
</style>
