<script lang="ts">
	// The hardware-tier picker — four selectable cards, one per tier. Shared by
	// the onboarding hardware step and the Hub's Hardware section so the chosen
	// tier reads the same in both. The detected tier is just the default
	// selection; Lashon never silently downgrades, so the user always picks
	// (docs/tech-stack.md, docs/adr/0013). RTL-native, design tokens only.
	import { t } from '$lib/i18n';
	import { TIERS, type Tier } from '$lib/hardware';

	let {
		value,
		detected = null,
		onchange
	}: {
		value: Tier | null;
		detected?: Tier | null;
		onchange: (tier: Tier) => void;
	} = $props();
</script>

<div class="tiers" role="radiogroup" aria-label={$t('hardware.label')}>
	{#each TIERS as tier}
		<button
			type="button"
			class="tier"
			class:selected={value === tier}
			role="radio"
			aria-checked={value === tier}
			onclick={() => onchange(tier)}
		>
			<span class="head">
				<span class="code" aria-hidden="true">{tier}</span>
				<span class="name">{$t(`hardware.tier${tier}.name`)}</span>
				{#if detected === tier}
					<span class="badge">{$t('hardware.detected')}</span>
				{/if}
			</span>
			<span class="desc">{$t(`hardware.tier${tier}.desc`)}</span>
		</button>
	{/each}
</div>

<style>
	.tiers {
		display: flex;
		flex-direction: column;
		gap: 8px;
		width: 100%;
	}

	.tier {
		display: flex;
		flex-direction: column;
		gap: 5px;
		width: 100%;
		box-sizing: border-box;
		text-align: start;
		font-family: inherit;
		padding: 12px 16px;
		border-radius: 13px;
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-subtle);
		cursor: pointer;
		transition:
			border-color 0.15s ease,
			background 0.15s ease;
	}
	.tier:hover {
		border-color: var(--stroke-strong);
	}
	.tier.selected {
		border-color: var(--accent-citron);
		background:
			radial-gradient(120% 140% at 100% 0%, rgba(231, 210, 74, 0.12), transparent 65%),
			var(--bg-elevated);
	}
	.tier:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	.head {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.code {
		flex: 0 0 auto;
		width: 26px;
		height: 26px;
		display: flex;
		align-items: center;
		justify-content: center;
		font-size: 14px;
		font-weight: 700;
		border-radius: 7px;
		color: var(--text-secondary);
		background: var(--bg-deep);
		border: 1px solid var(--stroke-subtle);
	}
	.tier.selected .code {
		color: #1a1709;
		background: var(--accent-citron);
		border-color: var(--accent-citron);
	}

	.name {
		font-size: 15px;
		font-weight: 700;
		color: var(--text-primary);
	}

	/* Marks the tier detection picked — the user can still choose another. */
	.badge {
		font-size: 11px;
		font-weight: 700;
		letter-spacing: 0.04em;
		color: var(--accent-aqua);
		border: 1px solid var(--accent-aqua);
		border-radius: 999px;
		padding: 2px 9px;
	}

	.desc {
		font-size: 13px;
		line-height: 1.6;
		color: var(--text-muted);
	}

	@media (prefers-reduced-motion: reduce) {
		.tier {
			transition: none;
		}
	}
</style>
