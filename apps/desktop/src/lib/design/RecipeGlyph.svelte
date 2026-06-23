<!--
	The ↯ cascade-chevron glyph. Used at the foot of the Tongue mark in
	recipe-match states, beside a recipe-match line in the conversation
	panel, and inside the Hub Recipes tab's empty-state badge. Pure
	presentational, no state.
-->
<script lang="ts">
	let {
		size = 14,
		color = 'var(--hearth)'
	}: { size?: number; color?: string } = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 16 16"
	fill="none"
	aria-hidden="true"
>
	<!-- Lightning-fold: a folded path that reads as "short-circuit". -->
	<path
		d="M9 2 L4 9 L7.5 9 L6.5 14 L12 7 L8.5 7 Z"
		fill={color}
		stroke={color}
		stroke-width="0.6"
		stroke-linejoin="round"
	/>
</svg>
