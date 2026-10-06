<script lang="ts">
	import mark from '../../../static/ottid-mark.svg?raw';

	let {
		size = 112,
		color = 'var(--peach)',
		glow = null,
		title = 'Ottid'
	}: {
		size?: number;
		color?: string;
		glow?: string | null;
		title?: string;
	} = $props();
</script>

<span
	class="mark"
	role="img"
	aria-label={title}
	style:width={`${size}px`}
	style:height={`${size}px`}
	style:color={color}
	style:filter={glow ? `drop-shadow(0 0 14px ${glow})` : undefined}
>
	<!-- Only this bundled, trusted SVG is inlined. Color changes the lamp;
	     the creature's body and eyes retain their identity in every mode. -->
	{@html mark}
</span>

<style>
	.mark {
		display: inline-flex;
		flex: 0 0 auto;
		/* Let clicks reach the parent's Tauri drag region. */
		pointer-events: none;
	}
	.mark :global(svg) {
		display: block;
		width: 100%;
		height: 100%;
		color: inherit;
	}
</style>
