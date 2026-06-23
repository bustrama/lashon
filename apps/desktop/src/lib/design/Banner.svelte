<!--
	Banner — info / warn / success / recipe variants. Used for the
	"this recipe runs shell commands" warning, the M9 success readout,
	and the future cascade-match flash banner.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	type Kind = 'info' | 'warn' | 'success' | 'recipe';

	const PALETTE: Record<Kind, { color: string; bg: string }> = {
		info: { color: 'var(--garnet)', bg: 'rgba(63, 190, 204, 0.10)' },
		warn: { color: 'var(--state-error)', bg: 'rgba(232, 98, 90, 0.10)' },
		success: { color: 'var(--state-success)', bg: 'rgba(95, 184, 135, 0.10)' },
		recipe: { color: 'var(--hearth)', bg: 'var(--hearth-soft)' }
	};

	let {
		kind = 'info',
		title = null,
		children
	}: { kind?: Kind; title?: string | null; children: Snippet } = $props();

	const palette = $derived(PALETTE[kind]);
</script>

<div
	class="banner"
	style="background: {palette.bg}; box-shadow: inset 0 0 0 1px {palette.color}55;"
	role={kind === 'warn' || kind === 'success' ? 'alert' : 'status'}
>
	<span class="dot" style="background: {palette.color};" aria-hidden="true"></span>
	<div class="body">
		{#if title}
			<div class="title he-sans" style="color: {palette.color};">{title}</div>
		{/if}
		<div class="content he-sans">{@render children()}</div>
	</div>
</div>

<style>
	.banner {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		padding: 10px 12px;
		border-radius: 8px;
		color: var(--ink-text);
		direction: rtl;
	}
	.dot {
		width: 7px;
		height: 7px;
		border-radius: 999px;
		margin-top: 7px;
		flex: 0 0 auto;
	}
	.body {
		flex: 1;
		min-width: 0;
	}
	.title {
		font-size: 12.5px;
		font-weight: 700;
		margin-bottom: 2px;
	}
	.content {
		font-size: 12.5px;
		line-height: 1.5;
	}
</style>
