<!--
	Permission badge — one of eight variants from the design system.
	Carries descriptive text (`aria-label="הקלדה · keyboard.type"`), not
	just glyph, so a screen reader announces it intelligibly. Danger
	tone (destructive, shell.run) carries visible weight via rose ring.

	The set is closed in v1 — unknown permission strings render as the
	neutral "מותאם · custom" pill so an author who declared a
	post-Phase-1 permission still gets a row in the Hub. The full
	8-variant table is in `recipe-system.jsx` in the design source.
-->
<script lang="ts">
	type Kind =
		| 'keyboard.type'
		| 'app.focus'
		| 'app.open'
		| 'shell.run'
		| 'destructive'
		| 'clipboard'
		| 'screenshot'
		| 'network';

	type Tone = 'cool' | 'warn' | 'danger';
	type Glyph = 'kbd' | 'window' | 'launch' | 'shell' | 'warn' | 'clip' | 'cam' | 'globe';

	const DEFS: Record<Kind, { he: string; en: string; glyph: Glyph; tone: Tone }> = {
		'keyboard.type': { he: 'הקלדה', en: 'keyboard.type', glyph: 'kbd', tone: 'cool' },
		'app.focus': { he: 'מיקוד חלון', en: 'app.focus', glyph: 'window', tone: 'cool' },
		'app.open': { he: 'פותח אפליקציה', en: 'app.open', glyph: 'launch', tone: 'cool' },
		'shell.run': { he: 'מעטפת', en: 'shell.run', glyph: 'shell', tone: 'warn' },
		destructive: { he: 'הרסני', en: 'destructive', glyph: 'warn', tone: 'danger' },
		clipboard: { he: 'לוח', en: 'clipboard', glyph: 'clip', tone: 'cool' },
		screenshot: { he: 'צילום מסך', en: 'screenshot', glyph: 'cam', tone: 'cool' },
		network: { he: 'רשת', en: 'network', glyph: 'globe', tone: 'warn' }
	};

	let {
		kind,
		size = 'sm'
	}: {
		// `kind` is a string so the Hub can pass arbitrary author-declared
		// values (e.g. file.write). Known kinds resolve to the table
		// above; unknown ones render the fallback custom pill.
		kind: string;
		size?: 'sm' | 'md';
	} = $props();

	const def = $derived(DEFS[kind as Kind] ?? null);
</script>

{#if def}
	{@const isDanger = def.tone === 'danger'}
	{@const isWarn = def.tone === 'warn'}
	<span
		role="img"
		class="badge"
		class:danger={isDanger}
		class:warn={isWarn}
		class:cool={!isDanger && !isWarn}
		class:compact={size === 'sm'}
		aria-label={`${def.he} · ${def.en}`}
		title={`${def.he} · ${def.en}`}
	>
		<!-- Glyph: one per `def.glyph`. SVG path data lifted from the
		     design source verbatim so the icons stay byte-exact between
		     the JSX mock and shipped UI. -->
		<svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
			{#if def.glyph === 'kbd'}
				<rect x="1.5" y="4" width="13" height="8" rx="1.2" stroke="currentColor" stroke-width="1.4"/>
				<path d="M4 7h.6M6.5 7h.6M9 7h.6M11.5 7h.6M5 10h6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
			{:else if def.glyph === 'window'}
				<rect x="2" y="3" width="12" height="10" rx="1.2" stroke="currentColor" stroke-width="1.4"/>
				<path d="M2 6h12" stroke="currentColor" stroke-width="1.4"/>
			{:else if def.glyph === 'launch'}
				<path d="M3 13 L13 3 M13 3 H7 M13 3 V9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
			{:else if def.glyph === 'shell'}
				<path d="M3 5l3 3-3 3 M9 11h4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
			{:else if def.glyph === 'warn'}
				<path d="M8 2 L14 13 H2 Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/>
				<path d="M8 6v3.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
				<circle cx="8" cy="11.5" r="0.7" fill="currentColor"/>
			{:else if def.glyph === 'clip'}
				<rect x="3.5" y="3" width="9" height="11" rx="1.2" stroke="currentColor" stroke-width="1.4"/>
				<rect x="5.5" y="1.5" width="5" height="2.4" rx="0.8" stroke="currentColor" stroke-width="1.4" fill="none"/>
			{:else if def.glyph === 'cam'}
				<rect x="1.5" y="4.5" width="13" height="9" rx="1.4" stroke="currentColor" stroke-width="1.4"/>
				<path d="M5.5 4.5L6.5 3h3l1 1.5" stroke="currentColor" stroke-width="1.4"/>
				<circle cx="8" cy="9" r="2.2" stroke="currentColor" stroke-width="1.4"/>
			{:else if def.glyph === 'globe'}
				<circle cx="8" cy="8" r="5.5" stroke="currentColor" stroke-width="1.4"/>
				<ellipse cx="8" cy="8" rx="2.4" ry="5.5" stroke="currentColor" stroke-width="1.4"/>
				<path d="M2.5 8h11" stroke="currentColor" stroke-width="1.4"/>
			{/if}
		</svg>
		<span class="en">{def.en}</span>
	</span>
{:else}
	<!-- Unknown permission — neutral pill, still gives the author signal
	     in the Hub. Glyph-less; the raw kind string is the label. -->
	<span class="badge cool compact" role="img" aria-label={`${kind} · custom`} title={kind}>
		<span class="en">{kind}</span>
	</span>
{/if}

<style>
	.badge {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 4px 10px;
		border-radius: 6px;
		font-size: 12px;
		font-weight: 600;
		letter-spacing: 0.2px;
		font-family: var(--font-mono);
		direction: ltr;
	}
	.badge.compact {
		padding: 2px 7px;
		font-size: 10.5px;
	}
	.badge.cool {
		color: var(--ink-mute);
		background: rgba(221, 228, 233, 0.04);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.badge.warn {
		color: #c19233;
		background: rgba(232, 177, 74, 0.10);
		box-shadow: inset 0 0 0 1px rgba(232, 177, 74, 0.35);
	}
	.badge.danger {
		color: var(--state-error);
		background: rgba(232, 98, 90, 0.10);
		box-shadow: inset 0 0 0 1px rgba(232, 98, 90, 0.45);
	}
	.en {
		line-height: 1;
	}
</style>
