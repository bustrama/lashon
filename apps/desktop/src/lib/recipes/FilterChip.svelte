<!--
	Single filter chip in the Hub Recipes toolbar. `active` flips the
	background; `danger` recolours the text rose ("הרסניים" filter).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		active = false,
		danger = false,
		onclick,
		children
	}: {
		active?: boolean;
		danger?: boolean;
		onclick: () => void;
		children: Snippet;
	} = $props();
</script>

<button
	type="button"
	class="chip he-sans"
	class:active
	class:danger
	aria-pressed={active}
	{onclick}
>
	{@render children()}
</button>

<style>
	.chip {
		padding: 7px 12px;
		border-radius: 8px;
		border: none;
		background: transparent;
		color: var(--ink-mute);
		font-family: inherit;
		font-weight: 600;
		font-size: 12px;
		cursor: pointer;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		transition: background 0.12s ease, color 0.12s ease;
	}
	.chip:hover {
		color: var(--ink-text);
	}
	.chip.active {
		background: var(--ink-4);
		color: var(--ink-text);
	}
	.chip.danger {
		color: var(--state-error);
		box-shadow: inset 0 0 0 1px rgba(232, 98, 90, 0.35);
	}
	.chip:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}
	@media (prefers-reduced-motion: reduce) {
		.chip {
			transition: none;
		}
	}
</style>
