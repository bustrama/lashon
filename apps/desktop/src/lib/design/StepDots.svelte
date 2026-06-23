<!--
	Step-dot row — `total` dots, `current` is highlighted (elongated
	pill). Used inside the Hub during a recipe run when steps > 3; for
	shorter recipes the count is shown plainly. M9 progress signal.
-->
<script lang="ts">
	let {
		total,
		current,
		tint = 'var(--hearth)'
	}: { total: number; current: number; tint?: string } = $props();
</script>

<div class="row" aria-label={`צעד ${current + 1} מתוך ${total}`}>
	{#each Array.from({ length: total }) as _, i}
		<span
			class="dot"
			class:current={i === current}
			class:done={i < current}
			style={i === current
				? `width: 14px; background: ${tint};`
				: i < current
					? `background: ${tint}; opacity: 0.55;`
					: ''}
		></span>
	{/each}
</div>

<style>
	.row {
		display: inline-flex;
		align-items: center;
		gap: 4px;
	}
	.dot {
		width: 5px;
		height: 5px;
		border-radius: 999px;
		background: rgba(221, 228, 233, 0.18);
		transition: all 0.25s ease;
	}
	@media (prefers-reduced-motion: reduce) {
		.dot {
			transition: none;
		}
	}
</style>
