<!--
	KeyChord — `Ctrl + K`-style chord rendering as a row of Keycap
	primitives joined by a `+` separator. Always LTR + Latin (keyboard
	hardware doesn't translate). Used inside `key_chord` step cards.
-->
<script lang="ts">
	import Keycap from './Keycap.svelte';

	let { keys }: { keys: string[] } = $props();
</script>

<span class="chord" aria-label={`keyboard chord: ${keys.join(' plus ')}`}>
	{#each keys as key, i (i)}
		<Keycap label={key} />
		{#if i < keys.length - 1}
			<span class="plus" aria-hidden="true">+</span>
		{/if}
	{/each}
</span>

<style>
	.chord {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		direction: ltr;
	}
	.plus {
		color: var(--ink-faint);
		font-family: var(--font-mono);
		font-size: 11px;
		font-weight: 700;
	}
</style>
