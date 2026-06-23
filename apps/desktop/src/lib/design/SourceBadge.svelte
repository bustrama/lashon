<!--
	Source pill — `bundled` (default, neutral dot), `user` (saffron dot),
	`mcp` (hearth dot). The Hub uses this on every recipe row beside
	the Run button so a glance tells the user whose recipe it is.
-->
<script lang="ts">
	type Kind = 'bundled' | 'user' | 'mcp';

	const MAP: Record<Kind, { he: string; en: string; dotVar: string }> = {
		bundled: { he: 'מובנה', en: 'bundled', dotVar: 'var(--ink-faint)' },
		user: { he: 'שלי', en: 'user', dotVar: 'var(--saffron)' },
		mcp: { he: 'דרך MCP', en: 'mcp', dotVar: 'var(--hearth)' }
	};

	let { kind = 'bundled' }: { kind?: Kind } = $props();
	const m = $derived(MAP[kind]);
</script>

<span class="pill he-sans">
	<span class="dot" style="background: {m.dotVar};" aria-hidden="true"></span>
	{m.he}<span class="lat">· {m.en}</span>
</span>

<style>
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		padding: 2px 8px;
		border-radius: 999px;
		font-size: 10.5px;
		font-weight: 600;
		letter-spacing: 0.3px;
		color: var(--ink-mute);
		background: transparent;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.dot {
		width: 5px;
		height: 5px;
		border-radius: 999px;
	}
	.lat {
		opacity: 0.55;
		font-size: 9.5px;
	}
</style>
