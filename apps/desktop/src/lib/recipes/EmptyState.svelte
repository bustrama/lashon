<!--
	Hub Recipes empty state. The 10 starter recipes ship with the
	installer, so the empty state shouldn't fire in practice — but the
	design wants it specified anyway (e.g. if a user nukes the user
	dir AND the bundle is corrupted). Used by the parent route when
	the listing returns zero rows.
-->
<script lang="ts">
	import RecipeGlyph from '$lib/design/RecipeGlyph.svelte';

	let {
		onbrowsebundled,
		onopenmcp
	}: {
		onbrowsebundled?: () => void;
		onopenmcp?: () => void;
	} = $props();
</script>

<div class="empty">
	<div class="badge">
		<RecipeGlyph size={28} />
	</div>
	<div class="title he-display">עדיין אין לך מתכונים</div>
	<div class="lead he-sans">
		פתח את לשונית שרת ה-MCP כדי לכתוב מתכונים דרך Claude Desktop,<br />
		או דפדף בעשרת המתכונים הראשונים שמצורפים לתוכנה.
	</div>
	<div class="cta">
		<button type="button" class="primary he-sans" onclick={() => onbrowsebundled?.()}>
			עיין במובנים
		</button>
		<button type="button" class="secondary he-sans" onclick={() => onopenmcp?.()}>
			פתח שרת MCP
		</button>
	</div>
</div>

<style>
	.empty {
		margin-top: 20px;
		padding: 40px 28px;
		border-radius: 14px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		text-align: center;
	}
	.badge {
		width: 64px;
		height: 64px;
		border-radius: 999px;
		background: var(--hearth-soft);
		margin: 0 auto 16px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.27);
	}
	.title {
		font-size: 22px;
		font-weight: 500;
		margin-bottom: 6px;
		color: var(--ink-text);
	}
	.lead {
		font-size: 13.5px;
		color: var(--ink-mute);
		max-width: 440px;
		margin: 0 auto 22px;
		line-height: 1.55;
	}
	.cta {
		display: flex;
		gap: 10px;
		justify-content: center;
	}
	.primary,
	.secondary {
		padding: 9px 18px;
		border-radius: 8px;
		font-family: inherit;
		font-weight: 700;
		font-size: 13px;
		cursor: pointer;
	}
	.primary {
		border: none;
		background: var(--hearth);
		color: #fff;
	}
	.primary:hover {
		filter: brightness(1.08);
	}
	.secondary {
		border: 1px solid var(--ink-line-2);
		background: transparent;
		color: var(--ink-text);
		font-weight: 600;
	}
	.secondary:hover {
		background: rgba(221, 228, 233, 0.04);
	}
	.primary:focus-visible,
	.secondary:focus-visible {
		outline: 3px solid var(--garnet);
		outline-offset: 2px;
	}
</style>
