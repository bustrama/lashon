<script lang="ts">
	// The first-run interactive tutorial (issue #9; docs/roadmap.md §1.8 — the
	// "Onboarding Overlay" surface in docs/design-system.md). It runs in its
	// own frameless, transparent `tutorial` window, declared hidden in
	// tauri.conf.json and revealed by the Rust shell on first launch, over the
	// tongue. Finishing or skipping records `tutorial.completed` in the
	// tauri-plugin-store settings file, so it never reappears uninvited; the
	// tray "Tutorial" entry reopens it on demand.
	import { onMount } from 'svelte';
	import { t } from '$lib/i18n';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { listen } from '@tauri-apps/api/event';
	import { invoke } from '@tauri-apps/api/core';
	import { getSetting, setSetting } from '$lib/settings';
	import { type HardwareReport, type MicProbe, type Tier } from '$lib/hardware';
	import TierSelect from '$lib/components/TierSelect.svelte';
	import Mark from '$lib/components/Mark.svelte';
	import type { DictationState } from '$lib/dictation';

	// The tutorial steps, in order. Each key maps to a `tutorial.steps.*` entry
	// in the i18n catalogs (kicker, title, lead). `microphone` and `hardware`
	// are the M4 onboarding-hardware steps (docs/adr/0013).
	const STEP_KEYS = [
		'welcome',
		'microphone',
		'hardware',
		'tongue',
		'hotkey',
		'practice',
		'done'
	] as const;

	// The interactive practice step watches the live dictation FSM.
	const INTERACTIVE = STEP_KEYS.indexOf('practice');

	let step = $state(0);
	let liveState = $state<DictationState>('idle');
	let practiceStarted = $state(false); // saw capture begin
	let practiceDone = $state(false); // saw a full capture → transcribe cycle
	let transcript = $state(''); // the most recent transcribed text
	let preparing = $state(false); // STT model still warming up (first run)
	let prepPercent = $state<number | null>(null); // warm-up progress, 0–100
	let hotkeyToast = $state(false); // "hold on" nudge after an early hotkey press
	let toastTimer: ReturnType<typeof setTimeout> | undefined;

	// Onboarding mic step — probing also raises the macOS permission prompt.
	let micProbe = $state<MicProbe | null>(null);
	let micChecking = $state(false);
	// Onboarding hardware step — the detected report, the chosen tier, and
	// whether detection is in flight.
	let hardware = $state<HardwareReport | null>(null);
	let hwDetecting = $state(false);
	let tier = $state<Tier | null>(null);

	const current = $derived(STEP_KEYS[step]);
	const isLast = $derived(step === STEP_KEYS.length - 1);
	const primaryLabel = $derived(
		isLast
			? $t('tutorial.nav.finish')
			: step === INTERACTIVE && practiceDone
				? $t('tutorial.nav.continuePractice')
				: $t('tutorial.nav.continue')
	);

	interface LiveStatus {
		tone: 'wait' | 'live' | 'done';
		text: string;
	}

	const liveStatus = $derived<LiveStatus>(
		preparing
			? { tone: 'wait', text: $t('tutorial.live.preparing') }
			: liveState === 'error'
				? { tone: 'wait', text: $t('tutorial.live.error') }
				: practiceDone
					? { tone: 'done', text: $t('tutorial.live.done') }
					: liveState === 'capturing'
						? { tone: 'live', text: $t('tutorial.live.capturing') }
						: liveState === 'transcribing'
							? { tone: 'live', text: $t('tutorial.live.transcribing') }
							: { tone: 'wait', text: $t('tutorial.live.waiting') }
	);

	// The mic step's status line — the same wait/live/done tones the practice
	// step uses, so the two read consistently.
	const micStatus = $derived<LiveStatus>(
		micChecking || micProbe === null
			? { tone: 'live', text: $t('tutorial.mic.checking') }
			: micProbe.status === 'ready'
				? { tone: 'done', text: $t('tutorial.mic.ready') }
				: micProbe.status === 'no-device'
					? { tone: 'wait', text: $t('tutorial.mic.noDevice') }
					: { tone: 'wait', text: $t('tutorial.mic.blocked') }
	);

	// The detected RAM / GPU readings, shown above the tier picker so an
	// override is an informed choice.
	const hwReadings = $derived(
		hardware === null
			? ''
			: `${$t('hardware.ramLabel')}: ${Math.round(hardware.probe.ram_gb)} GB · ` +
				(hardware.probe.cuda
					? `${$t('hardware.gpuNvidia')} · ${hardware.probe.vram_gb.toFixed(1)} GB`
					: hardware.probe.vulkan
						? $t('hardware.gpuVulkan')
						: $t('hardware.gpuNone'))
	);

	// Probe the microphone. On macOS this is also what raises the OS
	// permission prompt the first time it runs.
	async function checkMic(): Promise<void> {
		micChecking = true;
		try {
			micProbe = await invoke<MicProbe>('probe_microphone');
		} catch (err) {
			// Outside a Tauri webview, or on a backend failure, treat the mic as
			// unavailable rather than letting the step throw.
			micProbe = { status: 'unavailable', reason: String(err) };
		} finally {
			micChecking = false;
		}
	}

	// Detect the hardware tier. A tier the user has already overridden (a
	// reopened tutorial) is kept; otherwise the detected tier is adopted and
	// persisted as the onboarding result.
	async function detectHardware(): Promise<void> {
		hwDetecting = true;
		try {
			const report = await invoke<HardwareReport>('detect_hardware');
			hardware = report;
			const saved = await getSetting('hardware.tier');
			tier = saved ?? report.tier;
			if (saved === null) {
				await setSetting('hardware.tier', report.tier);
			}
		} catch (err) {
			console.error('tutorial: hardware detection failed', err);
		} finally {
			hwDetecting = false;
		}
	}

	// Persist an explicit tier override from the picker.
	async function selectTier(next: Tier): Promise<void> {
		tier = next;
		try {
			await setSetting('hardware.tier', next);
		} catch (err) {
			console.error('tutorial: could not persist the hardware tier', err);
		}
	}

	// Run each onboarding probe when its step is first reached. The guards keep
	// the effect's other dependencies from re-triggering it; arriving at the
	// mic step is also when the macOS permission prompt should appear.
	$effect(() => {
		if (current === 'microphone' && micProbe === null && !micChecking) {
			void checkMic();
		}
		if (current === 'hardware' && hardware === null && !hwDetecting) {
			void detectHardware();
		}
	});

	async function finish(): Promise<void> {
		try {
			await setSetting('tutorial.completed', true);
		} catch (err) {
			// A failed write only means the tutorial may show again — never
			// block the user from leaving it.
			console.error('tutorial: could not persist completion', err);
		}
		await getCurrentWindow().hide();
	}

	function next(): void {
		if (isLast) {
			void finish();
			return;
		}
		step += 1;
	}

	function back(): void {
		if (step > 0) {
			step -= 1;
		}
	}

	// The window is frameless, so dragging is wired by hand: a mousedown
	// anywhere on the card hands off to the OS window manager. Clicks on
	// buttons are excluded so the controls keep working. (`data-tauri-drag-region`
	// proved unreliable — it only drags when the click target itself carries
	// the attribute.)
	function draggable(node: HTMLElement) {
		function onMouseDown(event: MouseEvent) {
			if (event.buttons !== 1) return;
			if ((event.target as HTMLElement).closest('button')) return;
			void getCurrentWindow().startDragging();
		}
		node.addEventListener('mousedown', onMouseDown);
		return {
			destroy() {
				node.removeEventListener('mousedown', onMouseDown);
			}
		};
	}

	onMount(() => {
		// The Rust dictation worker broadcasts `dictation:state` to every
		// window, so the practice step sees the same FSM the tongue renders.
		const stateUnlisten = listen<DictationState>('dictation:state', (event) => {
			const value = event.payload;
			// Any real FSM state means warm-up is over; `preparing` holds only
			// while the worker reports the "preparing" state.
			preparing = value === 'preparing';
			if (value === 'capturing') {
				practiceStarted = true;
			}
			if (practiceStarted && value === 'transcribing') {
				practiceDone = true;
			}
			liveState = value;
		});
		// First-run warm-up: the worker streams a status line while the STT
		// model downloads and loads. Surface it as live progress so the
		// tutorial never looks frozen.
		const preparingUnlisten = listen<string>('dictation:preparing', (event) => {
			preparing = true;
			const match = event.payload.match(/(\d+)\s*%/);
			prepPercent = match ? Number(match[1]) : null;
		});
		// The hotkey was pressed before the model is ready — show a brief,
		// self-dismissing nudge to wait.
		const notReadyUnlisten = listen('dictation:not-ready', () => {
			hotkeyToast = true;
			clearTimeout(toastTimer);
			toastTimer = setTimeout(() => {
				hotkeyToast = false;
			}, 3200);
		});
		// The worker broadcasts the transcribed text once a take finishes; the
		// practice step echoes it back so the user sees what Lashon heard.
		const transcriptUnlisten = listen<string>('dictation:transcript', (event) => {
			transcript = event.payload;
		});
		// Reopened from the tray — rewind to the start (the window is hidden,
		// not destroyed, so component state would otherwise persist).
		const openUnlisten = listen('tutorial:open', () => {
			step = 0;
			practiceStarted = false;
			practiceDone = false;
			transcript = '';
			// Re-probe on the next visit — mic permission or the hardware may
			// have changed since the tutorial was last open.
			micProbe = null;
			hardware = null;
		});
		return () => {
			void stateUnlisten.then((unlisten) => unlisten());
			void transcriptUnlisten.then((unlisten) => unlisten());
			void preparingUnlisten.then((unlisten) => unlisten());
			void notReadyUnlisten.then((unlisten) => unlisten());
			void openUnlisten.then((unlisten) => unlisten());
			clearTimeout(toastTimer);
		};
	});
</script>

<div class="tut">
	<!-- Slate slab inside a transparent window. The lamp vignette + faint
	     film grain (stacked radial gradients) keep the slate from feeling
	     synthetic; the chrome on top is the drag surface. -->
	<div class="tut-card" use:draggable>
		<div class="tut-vignette" aria-hidden="true"></div>
		<div class="tut-grain" aria-hidden="true"></div>

		<!-- Top chrome: close X (leading), centered brand+title, Esc hint (trailing). -->
		<header class="tut-chrome">
			<button class="tut-close" type="button" onclick={() => void finish()} aria-label={$t('hub.close')}>
				<svg width="9" height="9" viewBox="0 0 9 9" fill="none" aria-hidden="true">
					<path d="M1 1l7 7M8 1l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
				</svg>
			</button>
			<div class="tut-title">
				<span class="he-display tut-brand">לָשׁוֹן</span>
				<span class="tut-sep">·</span>
				<span class="he-sans tut-section">{$t('tutorial.window.title')}</span>
			</div>
			<span class="mono tut-esc-hint" aria-hidden="true">ESC TO QUIT</span>
		</header>

		<!-- First-run warm-up indicator on every step (sticks until ready). -->
		{#if preparing}
			<div class="tut-warmup" role="status" aria-live="polite">
				<div class="tut-warmup-head">
					<span class="he-sans tut-warmup-label">{$t('tutorial.warmup.label')}</span>
					{#if prepPercent !== null}
						<span class="mono tut-warmup-pct" dir="ltr">{prepPercent}%</span>
					{/if}
				</div>
				<div class="tut-warmup-track" class:indeterminate={prepPercent === null}>
					<div class="tut-warmup-fill" style:width={prepPercent !== null ? `${prepPercent}%` : ''}></div>
				</div>
				<p class="he-sans tut-warmup-hint">{$t('tutorial.warmup.hint')}</p>
			</div>
		{/if}

		<!-- Step body — TwoCol for narrative steps, centred-stack for hero steps. -->
		<div class="tut-body" class:center={current === 'tongue' || current === 'hotkey' || current === 'practice'}>
			{#if current === 'welcome'}
				<div class="tut-two-col">
					<div class="tut-visual">
						<div class="welcome-glow" aria-hidden="true"></div>
						<svg class="welcome-rings" width="300" height="300" viewBox="0 0 300 300" aria-hidden="true">
							<circle cx="150" cy="150" r="80" fill="none" stroke="var(--peach)" stroke-width="0.5" opacity="0.25"/>
							<circle cx="150" cy="150" r="110" fill="none" stroke="var(--peach)" stroke-width="0.5" opacity="0.18"/>
							<circle cx="150" cy="150" r="140" fill="none" stroke="var(--peach)" stroke-width="0.5" opacity="0.11"/>
						</svg>
						<div class="welcome-mark">
							<Mark size={170} color="var(--peach)" glow="rgba(247, 200, 163, 0.35)" />
						</div>
					</div>
					<div class="tut-text">
						<div class="mono tut-eyebrow">STEP · 01 / 07 · WELCOME</div>
						<h1 class="he-display tut-title-text">{$t(`tutorial.steps.${current}.title`)}</h1>
						<p class="he-sans tut-lede">{$t(`tutorial.steps.${current}.lead`)}</p>
						<div class="welcome-meta">
							<span class="mono">~ 2 MIN</span>
							<span class="dot-sep" aria-hidden="true"></span>
							<span class="mono">LOCAL · NO SIGN-IN</span>
						</div>
					</div>
				</div>
			{:else if current === 'microphone'}
				{@const probeState =
					micChecking || micProbe === null
						? 'probing'
						: micProbe.status === 'ready'
							? 'live'
							: 'error'}
				<div class="tut-two-col">
					<div class="tut-visual">
						<div class="mic-frame mic-{probeState}">
							{#if probeState === 'live'}
								<div class="mic-glow" aria-hidden="true"></div>
							{/if}
							<svg class="mic-wheel mic-wheel-{probeState}" width="260" height="260" viewBox="0 0 260 260" aria-hidden="true">
								<circle cx="130" cy="130" r="108" fill="none" stroke="var(--ink-line-2)" stroke-width="1"/>
								<!-- 64 short radial lines. Each carries a CSS animation
								     with a staggered delay so the whole ring breathes
								     as a live wave — reads as "the mic is alive". The
								     `transform-origin` is the line's INNER endpoint so
								     scaleY extends each line outward, not from its
								     midpoint. -->
								{#each Array(64) as _, i}
									{@const ang = (i / 64) * Math.PI * 2 - Math.PI / 2}
									{@const r1 = 108}
									{@const x1 = 130 + Math.cos(ang) * r1}
									{@const y1 = 130 + Math.sin(ang) * r1}
									<line
										class="mic-wheel-bar"
										x1={x1}
										y1={y1}
										x2={130 + Math.cos(ang) * (r1 + 22)}
										y2={130 + Math.sin(ang) * (r1 + 22)}
										stroke={probeState === 'live'
											? 'var(--saffron)'
											: probeState === 'error'
												? 'var(--state-error)'
												: 'var(--state-cloud)'}
										stroke-width="2"
										stroke-linecap="round"
										style="transform-origin: {x1}px {y1}px; animation-delay: {(i * 31) % 900}ms"
									/>
								{/each}
							</svg>
							<svg class="mic-glyph" width="64" height="86" viewBox="0 0 64 86" fill="none" aria-hidden="true">
								<rect x="22" y="6" width="20" height="42" rx="10" stroke="currentColor" stroke-width="2" fill="rgba(232,177,74,0.06)"/>
								<path d="M10 36c0 12 10 22 22 22s22-10 22-22" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>
								<path d="M32 58v14M20 72h24" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/>
								{#if probeState === 'error'}
									<line x1="6" y1="6" x2="58" y2="80" stroke="var(--state-error)" stroke-width="3" stroke-linecap="round"/>
								{/if}
							</svg>
						</div>
					</div>
					<div class="tut-text">
						<div class="mono tut-eyebrow">STEP · 02 / 07 · MICROPHONE</div>
						<h1 class="he-display tut-title-text">{$t(`tutorial.steps.${current}.title`)}</h1>
						<p class="he-sans tut-lede">{$t(`tutorial.steps.${current}.lead`)}</p>
						<div class="mic-status mic-{probeState}" role="status" aria-live="polite">
							<span class="mic-status-dot" aria-hidden="true"></span>
							<span class="he-sans">{micStatus.text}</span>
						</div>
						<button class="tut-secondary" type="button" onclick={() => void checkMic()} disabled={micChecking}>
							{$t('tutorial.mic.recheck')}
						</button>
					</div>
				</div>
			{:else if current === 'hardware'}
				<div class="tut-two-col">
					<div class="tut-visual">
						{#if hardware === null}
							<div class="hw-probing">
								<svg width="220" height="220" viewBox="0 0 220 220" aria-hidden="true">
									<circle cx="110" cy="110" r="92" fill="none" stroke="var(--ink-line-2)" stroke-width="1"/>
									<circle cx="110" cy="110" r="92" fill="none" stroke="var(--saffron)" stroke-width="2"
										stroke-dasharray="120 460" stroke-linecap="round"
										transform="rotate(-90 110 110)" class="orbit-spin"/>
								</svg>
								<div class="hw-probing-text">
									<div class="mono">PROBING</div>
								</div>
							</div>
						{:else}
							<div class="hw-tier">
								<div class="hw-tier-glow" aria-hidden="true"></div>
								<svg width="220" height="220" viewBox="0 0 220 220" aria-hidden="true">
									<circle cx="110" cy="110" r="96" fill="none" stroke="var(--saffron)" stroke-width="1.5" opacity="0.5"/>
									<circle cx="110" cy="110" r="84" fill="none" stroke="var(--saffron)" stroke-width="0.5" opacity="0.3"/>
								</svg>
								<div class="hw-tier-letter">
									<div class="he-display hw-tier-glyph">{hardware.tier}</div>
									<div class="mono hw-tier-sub">TIER · ASSIGNED</div>
								</div>
							</div>
						{/if}
					</div>
					<div class="tut-text">
						<div class="mono tut-eyebrow">STEP · 03 / 07 · HARDWARE</div>
						<h1 class="he-display tut-title-text">{$t(`tutorial.steps.${current}.title`)}</h1>
						<p class="he-sans tut-lede">{$t(`tutorial.steps.${current}.lead`)}</p>
						{#if hardware !== null}
							<p class="he-sans tut-hw-readings" dir="auto">{hwReadings}</p>
							<TierSelect value={tier} detected={hardware.tier} onchange={selectTier} />
							<button class="tut-secondary" type="button" onclick={() => void detectHardware()} disabled={hwDetecting}>
								{$t('tutorial.hardware.redetect')}
							</button>
						{/if}
					</div>
				</div>
			{:else if current === 'tongue'}
				<div class="tut-centered-stack">
					<div class="mono tut-eyebrow">STEP · 04 / 07 · THE TONGUE</div>
					<h1 class="he-display tut-title-text centered">{$t(`tutorial.steps.${current}.title`)}</h1>
					<p class="he-sans tut-lede centered">{$t(`tutorial.steps.${current}.lead`)}</p>
					<div class="tongue-teaching" dir="rtl">
						{#each [{ mode: 'dictate', tint: 'var(--saffron)', glyph: 'pen' }, { mode: 'command', tint: 'var(--garnet)', glyph: 'gear' }, { mode: 'chat', tint: 'var(--indigo)', glyph: 'bubble' }] as col}
							<div class="tongue-teach-col">
								<div class="tongue-teach-glyph">
									<!-- A miniature tongue preview — same shapes the real
									     tongue draws when this mode is armed: peach mark
									     tinted to the mode hue, soft halo of the same hue,
									     supporting glyph badge at the bottom-left corner. -->
									<div class="teach-stage">
										<div
											class="teach-halo"
											style="background: radial-gradient(circle, {col.tint} 0%, transparent 42%);"
											aria-hidden="true"
										></div>
										<div class="teach-mark">
											<Mark size={64} color={col.tint} glow={col.tint} />
										</div>
										<div
											class="teach-badge"
											style="box-shadow: 0 0 0 1.5px {col.tint};"
											aria-hidden="true"
										>
											{#if col.glyph === 'pen'}
												<svg width="12" height="12" viewBox="0 0 16 16" fill="none">
													<path d="M2 14L4 12L11 5L13 7L6 14L2 14Z M11 5L12.5 3.5L13.5 4.5L12 6"
														stroke={col.tint} stroke-width="1.5" stroke-linejoin="round"/>
												</svg>
											{:else if col.glyph === 'gear'}
												<svg width="12" height="12" viewBox="0 0 16 16" fill="none">
													<circle cx="8" cy="8" r="2.4" stroke={col.tint} stroke-width="1.5"/>
													<path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"
														stroke={col.tint} stroke-width="1.5" stroke-linecap="round"/>
												</svg>
											{:else}
												<svg width="12" height="12" viewBox="0 0 16 16" fill="none">
													<path d="M3 4h10v6H7l-3 3v-3H3z" stroke={col.tint} stroke-width="1.5" stroke-linejoin="round"/>
												</svg>
											{/if}
										</div>
									</div>
								</div>
								<div class="tongue-teach-pill" style="--c: {col.tint};">
									<span class="tongue-teach-pill-dot"></span>
									<span class="he-sans">{$t(`tutorial.states.${col.mode}`)}</span>
									<span class="mono">· {col.mode}</span>
								</div>
								<p class="he-sans tongue-teach-blurb">{$t(`tutorial.states.${col.mode}Blurb`)}</p>
							</div>
						{/each}
					</div>
				</div>
			{:else if current === 'hotkey'}
				<div class="tut-centered-stack">
					<div class="mono tut-eyebrow">STEP · 05 / 07 · HOTKEY</div>
					<h1 class="he-display tut-title-text centered">{$t(`tutorial.steps.${current}.title`)}</h1>
					<p class="he-sans tut-lede centered">{$t(`tutorial.steps.${current}.lead`)}</p>
					<div class="chord" dir="ltr" aria-label={$t('tutorial.howto.combo')}>
						<span class="keycap keycap-lit">
							<span class="keycap-label">⌃</span>
							<span class="keycap-sub">Ctrl</span>
						</span>
						<span class="chord-plus">+</span>
						<span class="keycap keycap-wide keycap-lit">
							<span class="keycap-label">Space</span>
						</span>
					</div>
					<div class="chord-others" dir="rtl">
						<div class="chord-other" style="--c: var(--garnet);">
							<span class="chord-other-dot"></span>
							<span class="he-sans">{$t('tutorial.states.command')}</span>
							<span class="chord-other-keys" dir="ltr">
								<span class="mono">⌃</span><span class="chord-other-plus">+</span>
								<span class="mono">⇧</span><span class="chord-other-plus">+</span>
								<span class="mono">Space</span>
							</span>
						</div>
						<div class="chord-other" style="--c: var(--indigo);">
							<span class="chord-other-dot"></span>
							<span class="he-sans">{$t('tutorial.states.chat')}</span>
							<span class="chord-other-keys" dir="ltr">
								<span class="mono">⌃</span><span class="chord-other-plus">+</span>
								<span class="mono">⌥</span><span class="chord-other-plus">+</span>
								<span class="mono">Space</span>
							</span>
						</div>
					</div>
					<p class="he-sans tut-fineprint">{$t('tutorial.howto.fineprint')}</p>
				</div>
			{:else if current === 'practice'}
				<div class="tut-centered-stack practice">
					<div class="mono tut-eyebrow"
						class:eyebrow-success={liveStatus.tone === 'done'}
						class:eyebrow-live={liveStatus.tone === 'live'}>
						STEP · 06 / 07 · {liveStatus.tone === 'done' ? '✓ COMPLETE' : liveStatus.tone === 'live' ? '◉ LISTENING' : 'READY — HOLD TO SPEAK'}
					</div>
					<h1 class="he-display tut-title-text centered">{$t(`tutorial.steps.${current}.title`)}</h1>
					<div class="practice-tongue-mirror">
						<!-- The live tongue states mirror what the real tongue shows.
						     We use a static mark + halo for the mirror; the real one
						     is in its own window. -->
						<div class="practice-mark-stage practice-state-{liveState}">
							<div class="practice-halo" aria-hidden="true"></div>
							<Mark size={96} color={liveState === 'capturing' ? 'var(--saffron)' : 'var(--peach)'} />
						</div>
						<div class="mono practice-mirror-hint">LIVE TONGUE · MIRROR</div>
					</div>
					{#if transcript}
						<div class="practice-done">
							<div class="mono practice-done-eyebrow">✓ HEARD YOU</div>
							<div class="practice-done-card">
								<span class="he practice-done-text" dir="auto">{transcript}</span>
							</div>
							<p class="he-sans tut-fineprint">{$t('tutorial.practice.heardHint')}</p>
						</div>
					{:else if liveState === 'capturing'}
						<div class="practice-listening">
							<!-- 48 vertical bars, each on its own staggered scaleY
							     animation. The static sine envelope sets a base
							     height per bar; the animation breathes them up and
							     down so the equalizer feels live without needing
							     real mic-level data piped in. -->
							<div class="practice-wave" aria-hidden="true">
								{#each Array(48) as _, i}
									{@const base = 8 + Math.abs(Math.sin(i * 0.55 + 0.4)) * 22 + (i > 6 && i < 38 ? 4 : 0)}
									<span
										style="
											height: {base}px;
											animation-delay: {(i * 35) % 700}ms;
										"
									></span>
								{/each}
							</div>
							<p class="he-sans tut-fineprint">
								<span>{$t('tutorial.practice.holdHint')}</span>
								<span class="mono saffron-text"> · release to send</span>
							</p>
						</div>
					{:else}
						<div class="practice-idle">
							<p class="he-sans practice-prompt-line">
								<span>{$t('tutorial.practice.holdPrefix')}</span>
								<span class="kbd-inline">⌃ Ctrl</span>
								<span class="chord-plus-small">+</span>
								<span class="kbd-inline">Space</span>
								<span>{$t('tutorial.practice.holdSuffix')}</span>
							</p>
							<div class="practice-suggestion">
								<div class="mono practice-suggestion-eyebrow">SUGGESTION</div>
								<div class="he practice-suggestion-text">"{$t('tutorial.practice.phrase')}"</div>
							</div>
						</div>
					{/if}
				</div>
			{:else}
				<div class="tut-two-col">
					<div class="tut-visual">
						<div class="done-edge-hint mono" aria-hidden="true">RIGHT EDGE OF SCREEN</div>
						<svg class="done-edge-line" width="320" height="300" viewBox="0 0 320 300" aria-hidden="true">
							<line x1="260" y1="20" x2="260" y2="280" stroke="var(--ink-line-2)" stroke-dasharray="3 5"/>
						</svg>
						<div class="done-mark-wrap">
							<div class="done-mark-glow" aria-hidden="true"></div>
							<Mark size={68} color="var(--peach)" glow="rgba(247, 200, 163, 0.4)" />
						</div>
					</div>
					<div class="tut-text">
						<div class="mono tut-eyebrow eyebrow-success">STEP · 07 / 07 · READY</div>
						<h1 class="he-display tut-title-text">{$t(`tutorial.steps.${current}.title`)}</h1>
						<p class="he-sans tut-lede">{$t(`tutorial.steps.${current}.lead`)}</p>
						<div class="done-chord" dir="ltr">
							<span class="he-sans done-chord-label" dir="rtl">{$t('tutorial.done.chordLabel')}</span>
							<span class="kbd-inline">⌃ Ctrl</span>
							<span class="chord-plus-small">+</span>
							<span class="kbd-inline">Space</span>
						</div>
					</div>
				</div>
			{/if}
		</div>

		<!-- Footer: back + progress dots (active stretches) + next button. -->
		<footer class="tut-footer">
			<!-- RTL navigation: NEXT moves visually leftward (forward in Hebrew),
			     so the next button carries `←`; BACK moves rightward (backward),
			     so it carries `→`. The first pass had these swapped per Latin
			     left-to-right convention. -->
			<button
				class="tut-back"
				type="button"
				onclick={back}
				disabled={step === 0}
			>
				<span class="tut-back-arrow" aria-hidden="true">→</span>
				<span class="he-sans">{$t('tutorial.nav.back')}</span>
			</button>
			<div class="tut-dots" aria-hidden="true">
				{#each STEP_KEYS as _, index}
					<span class="tut-dot" class:active={index === step} class:past={index < step}></span>
				{/each}
			</div>
			<button
				class="tut-next"
				type="button"
				onclick={next}
				disabled={step === INTERACTIVE && (preparing || !practiceDone)}
			>
				<span class="he-sans">{primaryLabel}</span>
				<span class="tut-next-arrow" aria-hidden="true">←</span>
			</button>
		</footer>
	</div>

	{#if hotkeyToast}
		<div class="hotkey-toast" role="status" aria-live="polite">
			{$t('tutorial.toast')}
		</div>
	{/if}
</div>

<style>
	/* ── REDESIGN — "Lamp" tutorial ──────────────────────────────────────
	   Calmer than the rest of the app — "welcome to a craft, not a setup
	   wizard". Serif titles, generous breathing room, one warm light from
	   above on cool slate. */

	.he-display { font-family: var(--font-he-display); direction: rtl; }
	.he-sans { font-family: var(--font-he-sans); direction: rtl; }
	.lat { font-family: var(--font-lat-sans); direction: ltr; }
	.mono { font-family: var(--font-mono); direction: ltr; }

	.tut {
		position: fixed;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 22px;
		box-sizing: border-box;
		background: transparent;
	}

	.tut-card {
		position: relative;
		width: 100%;
		max-width: 880px;
		height: 100%;
		max-height: 600px;
		border-radius: 14px;
		background: var(--ink);
		color: var(--ink-text);
		direction: rtl;
		font-family: var(--font-he-sans);
		box-shadow:
			0 32px 100px rgba(0, 0, 0, 0.55),
			0 0 0 1px rgba(255, 255, 255, 0.04);
		overflow: hidden;
		cursor: grab;
	}
	.tut-card:active {
		cursor: grabbing;
	}

	/* Lamp vignette — a warm radial bloom from above, the design's "single
	   warm light in a cool room". */
	.tut-vignette {
		position: absolute;
		inset: 0;
		pointer-events: none;
		background: radial-gradient(
			ellipse 700px 420px at 50% 18%,
			rgba(247, 200, 163, 0.05) 0%,
			transparent 70%
		);
		z-index: 0;
	}
	.tut-grain {
		position: absolute;
		inset: 0;
		pointer-events: none;
		opacity: 0.5;
		background: radial-gradient(circle at 20% 90%, rgba(63, 190, 204, 0.03), transparent 40%);
		z-index: 0;
	}

	/* ── Chrome ─────────────────────────────────────────────────────── */
	.tut-chrome {
		position: absolute;
		top: 0;
		left: 0;
		right: 0;
		height: 44px;
		display: flex;
		align-items: center;
		padding: 0 14px;
		direction: ltr;
		z-index: 5;
	}
	.tut-close {
		width: 22px;
		height: 22px;
		border-radius: 999px;
		border: 0;
		padding: 0;
		background: rgba(221, 228, 233, 0.08);
		color: var(--ink-faint);
		cursor: pointer;
		display: flex;
		align-items: center;
		justify-content: center;
		transition: background 0.15s, color 0.15s;
	}
	.tut-close:hover {
		background: var(--state-error);
		color: #fff;
	}
	.tut-close:focus-visible {
		outline: 2px solid var(--garnet);
		outline-offset: 2px;
	}
	.tut-title {
		flex: 1;
		text-align: center;
		font-size: 13px;
		color: var(--ink-mute);
	}
	.tut-sep {
		margin: 0 6px;
		opacity: 0.4;
	}
	.tut-esc-hint {
		font-size: 10px;
		color: var(--ink-faint);
		letter-spacing: 1px;
		opacity: 0.7;
	}

	/* ── Warm-up indicator (sticks above content while STT loads) ──── */
	.tut-warmup {
		position: absolute;
		top: 44px;
		left: 0;
		right: 0;
		padding: 14px 56px;
		background: rgba(11, 18, 22, 0.6);
		border-bottom: 1px solid var(--ink-line);
		direction: rtl;
		z-index: 4;
	}
	.tut-warmup-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		font-size: 12px;
		margin-bottom: 6px;
	}
	.tut-warmup-label {
		color: var(--ink-text);
		font-weight: 600;
	}
	.tut-warmup-pct {
		font-size: 11px;
		color: var(--saffron);
	}
	.tut-warmup-track {
		height: 3px;
		border-radius: 99px;
		background: var(--ink-line-2);
		direction: ltr;
		overflow: hidden;
	}
	.tut-warmup-fill {
		height: 100%;
		background: var(--saffron);
		border-radius: 99px;
		box-shadow: 0 0 14px rgba(232, 177, 74, 0.55);
		transition: width 0.4s ease;
	}
	.tut-warmup-track.indeterminate .tut-warmup-fill {
		width: 30%;
		animation: warmup-march 1.8s linear infinite;
	}
	@keyframes warmup-march {
		0% { transform: translateX(-100%); }
		100% { transform: translateX(330%); }
	}
	.tut-warmup-hint {
		margin: 8px 0 0;
		font-size: 11.5px;
		color: var(--ink-mute);
		line-height: 1.5;
	}

	/* ── Body ────────────────────────────────────────────────────────── */
	.tut-body {
		position: absolute;
		top: 44px;
		left: 0;
		right: 0;
		bottom: 72px;
		padding: 0 64px;
		display: flex;
		align-items: center;
		z-index: 1;
		overflow: hidden;
	}
	.tut-body.center {
		align-items: flex-start;
		padding-top: 28px;
	}

	.tut-two-col {
		width: 100%;
		display: flex;
		align-items: center;
		gap: 56px;
		direction: rtl;
	}
	.tut-visual {
		width: 300px;
		flex: 0 0 300px;
		height: 300px;
		display: flex;
		align-items: center;
		justify-content: center;
		position: relative;
	}
	.tut-text {
		flex: 1;
		min-width: 0;
	}

	.tut-centered-stack {
		width: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		text-align: center;
		gap: 0;
	}
	.tut-centered-stack.practice {
		justify-content: flex-start;
	}

	.tut-eyebrow {
		font-size: 10.5px;
		letter-spacing: 1.6px;
		color: var(--ink-faint);
		text-transform: uppercase;
		direction: ltr;
		margin: 0 0 14px;
	}
	.tut-eyebrow.eyebrow-success {
		color: var(--state-success);
	}
	.tut-eyebrow.eyebrow-live {
		color: var(--saffron);
	}
	.tut-title-text {
		font-family: var(--font-he-display);
		font-size: 44px;
		font-weight: 500;
		line-height: 1.12;
		margin: 0;
		color: var(--ink-text);
		letter-spacing: -0.3px;
		text-wrap: pretty;
	}
	.tut-title-text.centered {
		text-align: center;
		font-size: 38px;
	}
	.tut-lede {
		font-size: 16px;
		line-height: 1.6;
		margin: 14px 0 0;
		color: var(--ink-mute);
		max-width: 460px;
		text-wrap: pretty;
		font-weight: 400;
	}
	.tut-lede.centered {
		max-width: 540px;
		margin-inline: auto;
	}
	.tut-fineprint {
		margin-top: 22px;
		font-size: 12px;
		color: var(--ink-faint);
		line-height: 1.6;
	}

	/* ── Welcome step visuals ─────────────────────────────────────────── */
	.welcome-glow {
		position: absolute;
		width: 260px;
		height: 260px;
		border-radius: 50%;
		background: radial-gradient(circle, rgba(247, 200, 163, 0.18) 0%, transparent 65%);
		filter: blur(24px);
	}
	.welcome-rings {
		position: absolute;
	}
	.welcome-mark {
		position: relative;
		display: flex;
	}
	.welcome-meta {
		margin-top: 28px;
		display: flex;
		align-items: center;
		gap: 18px;
		font-size: 10.5px;
		color: var(--ink-faint);
		letter-spacing: 1.3px;
	}
	.dot-sep {
		width: 3px;
		height: 3px;
		border-radius: 999px;
		background: var(--ink-line-2);
	}

	/* ── Microphone step visuals ──────────────────────────────────────── */
	.mic-frame {
		position: relative;
		width: 300px;
		height: 300px;
		display: flex;
		align-items: center;
		justify-content: center;
		color: var(--saffron);
	}
	.mic-frame.mic-probing { color: var(--state-cloud); }
	.mic-frame.mic-live { color: var(--saffron); }
	.mic-frame.mic-error { color: var(--state-error); }
	.mic-glow {
		position: absolute;
		inset: 20px;
		border-radius: 50%;
		background: radial-gradient(circle, rgba(232, 177, 74, 0.16) 0%, transparent 65%);
		filter: blur(20px);
	}
	.mic-wheel { position: absolute; }
	.mic-glyph { position: relative; }

	/* Each of the 64 radial lines on the mic wheel pulses stroke-width +
	   opacity. Staggered delays (set inline per-line) make the whole ring
	   read as a live audio wave even though no actual capture is running
	   during the probe — we just want the user to SEE the mic is alive. */
	.mic-wheel-bar {
		animation: mic-bar-pulse 1.1s ease-in-out infinite;
	}
	.mic-wheel-probing .mic-wheel-bar {
		/* Steel grey, slower, dimmer — reads as "still checking". */
		animation: mic-bar-pulse-quiet 1.6s ease-in-out infinite;
	}
	.mic-wheel-error .mic-wheel-bar {
		/* Stay still in error state — no false sense of liveliness. */
		animation: none;
		stroke-width: 2;
		opacity: 0.6;
	}
	@keyframes mic-bar-pulse {
		0%, 100% { stroke-width: 1; opacity: 0.35; }
		50%      { stroke-width: 3; opacity: 1;    }
	}
	@keyframes mic-bar-pulse-quiet {
		0%, 100% { stroke-width: 1; opacity: 0.25; }
		50%      { stroke-width: 2; opacity: 0.7;  }
	}
	.mic-status {
		margin-top: 24px;
		display: flex;
		align-items: center;
		gap: 10px;
		font-size: 14px;
		color: var(--ink-text);
	}
	.mic-status-dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
	}
	.mic-status.mic-live .mic-status-dot { background: var(--state-success); box-shadow: 0 0 10px var(--state-success); }
	.mic-status.mic-probing .mic-status-dot { background: var(--state-cloud); }
	.mic-status.mic-error .mic-status-dot { background: var(--state-error); box-shadow: 0 0 10px var(--state-error); }
	.tut-secondary {
		margin-top: 14px;
		background: transparent;
		color: var(--ink-mute);
		border: 1px solid var(--ink-line-2);
		border-radius: 7px;
		padding: 7px 14px;
		font-family: var(--font-he-sans);
		font-weight: 600;
		font-size: 12.5px;
		cursor: pointer;
		align-self: flex-start;
	}
	.tut-secondary:hover:not(:disabled) {
		color: var(--ink-text);
		border-color: var(--ink-faint);
	}
	.tut-secondary:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}

	/* ── Hardware step visuals ────────────────────────────────────────── */
	.hw-probing {
		position: relative;
		width: 300px;
		height: 300px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.hw-probing-text {
		position: absolute;
		text-align: center;
		font-size: 11px;
		color: var(--ink-faint);
		letter-spacing: 1.4px;
	}
	.hw-tier {
		position: relative;
		width: 300px;
		height: 300px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.hw-tier-glow {
		position: absolute;
		width: 200px;
		height: 200px;
		border-radius: 50%;
		background: radial-gradient(circle, rgba(232, 177, 74, 0.1) 0%, transparent 65%);
		filter: blur(20px);
	}
	.hw-tier-letter {
		position: absolute;
		text-align: center;
		direction: ltr;
	}
	.hw-tier-glyph {
		font-family: var(--font-he-display);
		font-size: 120px;
		line-height: 1;
		color: var(--saffron);
		font-weight: 500;
		text-shadow: 0 0 32px rgba(232, 177, 74, 0.4);
	}
	.hw-tier-sub {
		font-size: 10.5px;
		letter-spacing: 1.6px;
		color: var(--ink-faint);
		margin-top: 8px;
	}
	.tut-hw-readings {
		margin: 20px 0 14px;
		padding: 12px 14px;
		border-radius: 9px;
		background: rgba(221, 228, 233, 0.03);
		border: 1px solid var(--ink-line);
		font-family: var(--font-mono);
		font-size: 12.5px;
		color: var(--ink-text);
		direction: ltr;
	}

	/* ── Tongue step (teaching panel) ─────────────────────────────────── */
	.tongue-teaching {
		margin-top: 30px;
		width: 100%;
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 0;
		align-items: start;
	}
	.tongue-teach-col {
		padding: 10px 14px 0;
		border-inline-start: 1px dashed var(--ink-line);
		text-align: center;
	}
	.tongue-teach-col:first-child {
		border-inline-start: none;
	}
	.tongue-teach-glyph {
		height: 140px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	/* Miniature tongue preview — mark + halo + glyph badge. Mirrors the
	   real Tongue.svelte structure (Mark in mode hue, soft radial halo,
	   small glyph badge in the bottom-left corner) but rendered statically
	   here without any FSM or ResizeObserver wiring. */
	.teach-stage {
		position: relative;
		width: 96px;
		height: 96px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.teach-halo {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 130px;
		height: 130px;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		opacity: 0.6;
		filter: blur(22px);
		pointer-events: none;
	}
	.teach-mark {
		position: relative;
		display: inline-flex;
	}
	.teach-badge {
		position: absolute;
		bottom: 6px;
		inset-inline-start: 6px;
		width: 22px;
		height: 22px;
		border-radius: 50%;
		background: var(--ink);
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.tongue-teach-pill {
		margin-top: 4px;
		display: inline-flex;
		align-items: center;
		gap: 8px;
		padding: 4px 12px;
		border-radius: 999px;
		background: color-mix(in srgb, var(--c) 12%, transparent);
		border: 1px solid color-mix(in srgb, var(--c) 30%, transparent);
		color: var(--c);
		font-size: 13px;
		font-weight: 600;
	}
	.tongue-teach-pill-dot {
		width: 5px;
		height: 5px;
		border-radius: 99px;
		background: var(--c);
		box-shadow: 0 0 8px var(--c);
	}
	.tongue-teach-pill .mono {
		font-size: 10px;
		opacity: 0.7;
		letter-spacing: 1px;
	}
	.tongue-teach-blurb {
		margin: 12px auto 0;
		font-size: 13px;
		color: var(--ink-mute);
		line-height: 1.55;
		max-width: 200px;
	}

	/* ── Hotkey step ─────────────────────────────────────────────────── */
	.chord {
		margin-top: 30px;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 12px;
	}
	.keycap {
		min-width: 56px;
		height: 56px;
		border-radius: 9px;
		background: var(--ink-2);
		border: 1px solid var(--ink-line-2);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04), 0 2px 0 rgba(0, 0, 0, 0.4);
		color: var(--ink-text);
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		padding: 0 14px;
		font-family: var(--font-lat-sans);
		font-weight: 600;
	}
	.keycap-wide { min-width: 168px; }
	.keycap-lit {
		background: linear-gradient(180deg, rgba(232, 177, 74, 0.25), rgba(232, 177, 74, 0.09));
		border-color: var(--saffron);
		box-shadow: 0 0 32px rgba(232, 177, 74, 0.4), inset 0 1px 0 rgba(255, 255, 255, 0.08);
		color: var(--saffron);
	}
	.keycap-label { font-size: 17px; }
	.keycap-wide .keycap-label { font-size: 14px; }
	.keycap-sub { font-size: 10px; opacity: 0.6; margin-top: 2px; letter-spacing: 0.5px; }
	.chord-plus { color: var(--ink-faint); font-size: 20px; font-weight: 300; }

	.chord-others {
		margin-top: 28px;
		display: flex;
		justify-content: center;
		gap: 28px;
		flex-wrap: wrap;
	}
	.chord-other {
		display: flex;
		align-items: center;
		gap: 8px;
		direction: ltr;
	}
	.chord-other-dot {
		width: 4px;
		height: 4px;
		border-radius: 99px;
		background: var(--c);
	}
	.chord-other .he-sans {
		font-size: 12px;
		color: var(--ink-mute);
		direction: rtl;
	}
	.chord-other-keys {
		display: flex;
		gap: 3px;
		align-items: center;
	}
	.chord-other-keys .mono {
		padding: 3px 7px;
		font-size: 10.5px;
		border-radius: 4px;
		background: var(--ink-2);
		color: var(--ink-mute);
		border: 1px solid var(--ink-line);
	}
	.chord-other-plus {
		color: var(--ink-faint);
		font-size: 10px;
	}

	/* ── Practice step ───────────────────────────────────────────────── */
	.practice-tongue-mirror {
		margin-top: 18px;
		display: flex;
		flex-direction: column;
		align-items: center;
		position: relative;
	}
	.practice-mark-stage {
		position: relative;
		width: 96px;
		height: 96px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.practice-halo {
		position: absolute;
		top: 50%;
		left: 50%;
		width: 178px;
		height: 178px;
		transform: translate(-50%, -50%);
		border-radius: 50%;
		background: radial-gradient(circle, var(--peach) 0%, transparent 45%);
		opacity: 0;
		filter: blur(22px);
		pointer-events: none;
	}
	.practice-state-capturing .practice-halo {
		background: radial-gradient(circle, var(--saffron) 0%, transparent 38%);
		opacity: 0.55;
	}
	.practice-mirror-hint {
		margin-top: 8px;
		font-size: 9.5px;
		color: var(--ink-faint);
		letter-spacing: 1.3px;
		white-space: nowrap;
	}

	.practice-idle {
		margin-top: 18px;
		text-align: center;
	}
	.practice-prompt-line {
		font-size: 15px;
		color: var(--ink-mute);
		line-height: 1.7;
		margin-bottom: 18px;
		display: inline-flex;
		align-items: center;
		gap: 8px;
		flex-wrap: wrap;
		justify-content: center;
	}
	.kbd-inline {
		padding: 3px 9px;
		border-radius: 5px;
		background: var(--ink-2);
		border: 1px solid var(--ink-line-2);
		color: var(--ink-text);
		font-family: var(--font-lat-sans);
		font-weight: 600;
		font-size: 12px;
		direction: ltr;
	}
	.chord-plus-small { color: var(--ink-faint); font-size: 11px; }
	.practice-suggestion {
		display: inline-block;
		padding: 16px 24px;
		border-radius: 12px;
		background: rgba(247, 200, 163, 0.05);
		border: 1px dashed rgba(247, 200, 163, 0.32);
	}
	.practice-suggestion-eyebrow {
		font-size: 9.5px;
		color: var(--ink-faint);
		letter-spacing: 1.4px;
		margin-bottom: 6px;
	}
	.practice-suggestion-text {
		font-family: var(--font-he-display);
		font-size: 22px;
		color: var(--peach);
		font-style: italic;
		font-weight: 400;
		line-height: 1.3;
	}

	.practice-listening {
		margin-top: 18px;
		text-align: center;
	}
	.practice-wave {
		display: flex;
		justify-content: center;
		gap: 3px;
		align-items: center;
		height: 28px;
		margin-bottom: 14px;
	}
	.practice-wave span {
		width: 2px;
		background: var(--saffron);
		border-radius: 1px;
		transform-origin: center;
		animation: practice-wave-bar 0.8s ease-in-out infinite;
		opacity: 0.7;
	}
	@keyframes practice-wave-bar {
		0%, 100% { transform: scaleY(0.35); opacity: 0.5; }
		50%      { transform: scaleY(1);    opacity: 1;   }
	}
	.saffron-text { color: var(--saffron); opacity: 0.85; }

	.practice-done {
		margin-top: 14px;
		text-align: center;
	}
	.practice-done-eyebrow {
		font-size: 10.5px;
		color: var(--state-success);
		letter-spacing: 1.5px;
		margin-bottom: 14px;
	}
	.practice-done-card {
		display: inline-block;
		padding: 20px 28px;
		border-radius: 12px;
		background: rgba(95, 184, 135, 0.05);
		border: 1px solid rgba(95, 184, 135, 0.28);
		max-width: 580px;
	}
	.practice-done-text {
		font-size: 22px;
		color: var(--ink-text);
		line-height: 1.5;
	}

	/* ── Done step ───────────────────────────────────────────────────── */
	.done-edge-hint {
		position: absolute;
		top: 16px;
		right: 6px;
		font-size: 9px;
		color: var(--ink-faint);
		letter-spacing: 1.2px;
		writing-mode: vertical-rl;
		transform: rotate(180deg);
	}
	.done-edge-line { position: absolute; opacity: 0.4; }
	.done-mark-wrap {
		position: absolute;
		right: 60px;
		top: 50%;
		transform: translateY(-50%);
	}
	.done-mark-glow {
		position: absolute;
		inset: -30px;
		border-radius: 50%;
		background: radial-gradient(circle, rgba(247, 200, 163, 0.18) 0%, transparent 65%);
		filter: blur(20px);
	}
	.done-chord {
		margin-top: 24px;
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.done-chord-label {
		font-size: 11px;
		color: var(--ink-faint);
		letter-spacing: 1px;
	}

	/* ── Footer ──────────────────────────────────────────────────────── */
	.tut-footer {
		position: absolute;
		bottom: 0;
		left: 0;
		right: 0;
		height: 72px;
		border-top: 1px solid var(--ink-line);
		background: rgba(11, 18, 22, 0.55);
		backdrop-filter: blur(8px);
		-webkit-backdrop-filter: blur(8px);
		display: flex;
		align-items: center;
		padding: 0 26px;
		direction: rtl;
		z-index: 4;
	}

	.tut-back, .tut-next {
		font-family: var(--font-he-sans);
		font-weight: 600;
		font-size: 14px;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 11px 22px;
		border-radius: 8px;
		border: 0;
		transition: background 0.15s, opacity 0.15s;
	}
	.tut-back {
		background: transparent;
		color: var(--ink-mute);
		padding: 8px 10px;
	}
	.tut-back:hover:not(:disabled) {
		color: var(--ink-text);
	}
	.tut-back:disabled {
		opacity: 0.32;
		cursor: default;
	}
	.tut-back-arrow {
		font-size: 14px;
		display: inline-block;
	}

	.tut-dots {
		flex: 1;
		display: flex;
		justify-content: center;
		gap: 8px;
		/* Inherit RTL from the footer so the active dot progresses
		   right-to-left as the user advances through steps. */
	}
	.tut-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		background: rgba(221, 228, 233, 0.16);
		transition: all 0.25s ease;
	}
	.tut-dot.past { background: rgba(232, 177, 74, 0.42); }
	.tut-dot.active {
		width: 24px;
		background: var(--saffron);
	}

	.tut-next {
		background: var(--saffron);
		color: var(--ink);
		letter-spacing: 0.2px;
		box-shadow: 0 0 28px rgba(232, 177, 74, 0.25);
	}
	.tut-next:hover:not(:disabled) {
		filter: brightness(1.08);
	}
	.tut-next:disabled {
		background: rgba(232, 177, 74, 0.14);
		color: var(--ink-faint);
		cursor: not-allowed;
		box-shadow: none;
	}
	.tut-next-arrow { font-size: 14px; }

	/* Hotkey-press-too-soon toast (renders ABOVE the card if visible). */
	.hotkey-toast {
		position: fixed;
		bottom: 100px;
		left: 50%;
		transform: translateX(-50%);
		padding: 10px 18px;
		border-radius: 999px;
		background: rgba(232, 177, 74, 0.14);
		color: var(--saffron);
		border: 1px solid rgba(232, 177, 74, 0.38);
		font-family: var(--font-he-sans);
		font-size: 12.5px;
		font-weight: 600;
		z-index: 10;
		animation: toast-in 0.18s ease-out both;
	}
	@keyframes toast-in {
		from { opacity: 0; transform: translate(-50%, 6px); }
		to { opacity: 1; transform: translate(-50%, 0); }
	}

	.orbit-spin {
		animation: orbit-spin 6s linear infinite;
		transform-origin: center;
	}
	@keyframes orbit-spin {
		0% { transform: rotate(0); }
		100% { transform: rotate(360deg); }
	}

	@media (prefers-reduced-motion: reduce) {
		.tut-warmup-track.indeterminate .tut-warmup-fill,
		.orbit-spin,
		.hotkey-toast,
		.mic-wheel-bar,
		.practice-wave span {
			animation: none;
		}
		.tut-dot { transition: none; }
	}
</style>
