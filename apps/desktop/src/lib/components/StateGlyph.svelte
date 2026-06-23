<script lang="ts">
	/**
	 * A small (~14 px) supporting glyph that rides next to the mark.
	 *
	 * The redesign brief calls this out explicitly: "Reduced motion must
	 * disable any animated states. The product still has to look alive
	 * when motion is off — relying on motion alone for state is a bug."
	 * The halo color tells you the mode at a glance; this glyph is the
	 * non-motion reinforcement so a screenshot of a frozen frame still
	 * communicates which state the tongue is in.
	 */
	let {
		kind,
		color = 'currentColor'
	}: {
		kind:
			| 'pen'
			| 'gear'
			| 'gear-spin'
			| 'bubble'
			| 'dots'
			| 'orbit'
			| 'spark'
			| 'wave'
			| 'question'
			| 'antenna'
			| 'cross'
			| 'ring';
		color?: string;
	} = $props();

	const sw = 1.5;
</script>

{#if kind === 'pen'}
	<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
		<path
			d="M2 14L4 12L11 5L13 7L6 14L2 14Z M11 5L12.5 3.5L13.5 4.5L12 6"
			stroke={color}
			stroke-width={sw}
			stroke-linejoin="round"
		/>
	</svg>
{:else if kind === 'gear'}
	<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
		<circle cx="8" cy="8" r="2.4" stroke={color} stroke-width={sw} />
		<path
			d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"
			stroke={color}
			stroke-width={sw}
			stroke-linecap="round"
		/>
	</svg>
{:else if kind === 'gear-spin'}
	<svg class="orbit-spin" width="14" height="14" viewBox="0 0 16 16" fill="none">
		<circle cx="8" cy="8" r="2.4" stroke={color} stroke-width={sw} />
		<path
			d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"
			stroke={color}
			stroke-width={sw}
			stroke-linecap="round"
		/>
	</svg>
{:else if kind === 'bubble'}
	<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
		<path
			d="M3 4h10v6H7l-3 3v-3H3z"
			stroke={color}
			stroke-width={sw}
			stroke-linejoin="round"
		/>
	</svg>
{:else if kind === 'dots'}
	<div class="dots">
		<span class="dot" style="background: {color}; animation-delay: 0s"></span>
		<span class="dot" style="background: {color}; animation-delay: 0.15s"></span>
		<span class="dot" style="background: {color}; animation-delay: 0.3s"></span>
	</div>
{:else if kind === 'orbit'}
	<svg class="orbit-spin" width="16" height="16" viewBox="0 0 16 16">
		<circle cx="8" cy="8" r="6" stroke={color} stroke-width="0.8" fill="none" opacity="0.4" />
		<circle cx="8" cy="2" r="1.4" fill={color} />
	</svg>
{:else if kind === 'spark'}
	<svg width="12" height="12" viewBox="0 0 12 12" fill="none">
		<path d="M6 1v3M6 8v3M1 6h3M8 6h3" stroke={color} stroke-width={sw} stroke-linecap="round" />
	</svg>
{:else if kind === 'wave'}
	<div class="wave-bars">
		<span class="bar" style="background: {color}; animation-delay: 0s"></span>
		<span class="bar" style="background: {color}; animation-delay: 0.12s"></span>
		<span class="bar" style="background: {color}; animation-delay: 0.24s"></span>
		<span class="bar" style="background: {color}; animation-delay: 0.36s"></span>
	</div>
{:else if kind === 'question'}
	<span class="question" style="color: {color}">?</span>
{:else if kind === 'antenna'}
	<svg width="14" height="14" viewBox="0 0 16 16" fill="none">
		<path
			d="M3 6c0-2 2-3 5-3s5 1 5 3M5 7c0-1.2 1.2-2 3-2s3 .8 3 2"
			stroke={color}
			stroke-width={sw}
			fill="none"
			stroke-linecap="round"
		/>
		<circle cx="8" cy="9" r="1.2" fill={color} />
	</svg>
{:else if kind === 'cross'}
	<svg width="12" height="12" viewBox="0 0 12 12" fill="none">
		<path d="M3 3l6 6M9 3l-6 6" stroke={color} stroke-width="2" stroke-linecap="round" />
	</svg>
{:else if kind === 'ring'}
	<svg width="16" height="16" viewBox="0 0 16 16">
		<circle cx="8" cy="8" r="6" stroke={color} stroke-width="1.2" fill="none" opacity="0.3" />
		<circle
			cx="8"
			cy="8"
			r="6"
			stroke={color}
			stroke-width="1.6"
			fill="none"
			stroke-dasharray="14 24"
			stroke-linecap="round"
			transform="rotate(-90 8 8)"
		/>
	</svg>
{/if}

<style>
	.dots {
		display: flex;
		gap: 3px;
	}
	.dot {
		width: 4px;
		height: 4px;
		border-radius: 50%;
		animation: tongue-pulse-fast 1.1s ease-in-out infinite;
	}

	.wave-bars {
		display: flex;
		align-items: center;
		gap: 2px;
		height: 14px;
	}
	.bar {
		width: 2.5px;
		height: 14px;
		border-radius: 2px;
		transform-origin: center;
		animation: tongue-wave-bar 0.8s ease-in-out infinite;
	}

	.question {
		font-family: var(--font-he-sans);
		font-weight: 700;
		font-size: 13px;
		line-height: 1;
	}

	.orbit-spin {
		animation: tongue-orbit 6s linear infinite;
		transform-origin: center;
	}

	@keyframes tongue-pulse-fast {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.4;
		}
	}

	@keyframes tongue-wave-bar {
		0%,
		100% {
			transform: scaleY(0.35);
		}
		50% {
			transform: scaleY(1);
		}
	}

	@keyframes tongue-orbit {
		0% {
			transform: rotate(0);
		}
		100% {
			transform: rotate(360deg);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.dot,
		.bar,
		.orbit-spin {
			animation: none;
		}
	}
</style>
