<script lang="ts">
	// The listening waveform — a small equalizer that leans into the voice.
	// The Rust capture worker streams a loudness scalar on `dictation:level`
	// (~20 Hz; see src-tauri/src/dictation.rs); a requestAnimationFrame loop
	// eases the bars toward it at 60 fps so the motion stays smooth between
	// readings. The component is mounted only while the tongue is `capturing`,
	// so the subscription's lifetime matches the take.
	import { onMount } from 'svelte';
	import { listen } from '@tauri-apps/api/event';

	// The waveform self-calibrates to the mic: it measures each reading against
	// a decaying peak of the recent level, so a quiet mic and a hot one both
	// fill the bars. PEAK_FLOOR keeps room noise from reading as speech;
	// PEAK_DECAY (per ~20 Hz level event) lets the peak fall once the voice stops.
	const PEAK_FLOOR = 0.012;
	const PEAK_DECAY = 0.975;
	// Per-frame easing of the rendered level toward the latest reading.
	const EASE = 0.2;
	// Per-bar phase offsets so the equalizer ripples instead of moving as a block.
	const PHASES = [0, 1.4, 2.7, 0.8, 2];
	// Radians per millisecond of the live ripple.
	const RIPPLE_SPEED = 0.0064;

	// Bar fill, 0..1 (used as scaleY). The initial shape doubles as the
	// reduced-motion still.
	let bars = $state<number[]>([0.18, 0.26, 0.32, 0.22, 0.18]);

	onMount(() => {
		// Reduced motion: a static equalizer silhouette, no live tracking — the
		// tongue's ARIA-live region carries the "listening" feedback instead.
		if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
			return () => {};
		}

		let target = 0; // latest reading, normalised to the recent peak
		let smoothed = 0; // eased level the bars are drawn from
		let peak = PEAK_FLOOR; // decaying maximum each reading is measured against
		let frame = 0;

		const unlisten = listen<number>('dictation:level', (event) => {
			// Raw RMS from the capture worker. Tracking a decaying peak and
			// drawing the level relative to it is what makes the bars react the
			// same on a quiet mic as on a hot one.
			const raw = Math.max(0, event.payload);
			peak = Math.max(raw, peak * PEAK_DECAY, PEAK_FLOOR);
			target = Math.min(1, raw / peak);
		});

		const tick = (now: number) => {
			smoothed += (target - smoothed) * EASE;
			bars = PHASES.map((phase) => {
				// A gentle baseline shimmer keeps the equalizer alive in silence;
				// the swell rises and falls with the live mic level.
				const shimmer = 0.16 + 0.06 * Math.sin(now * 0.0045 + phase);
				const swell = smoothed * (0.55 + 0.45 * Math.sin(now * RIPPLE_SPEED + phase));
				return Math.max(shimmer, swell);
			});
			frame = requestAnimationFrame(tick);
		};
		frame = requestAnimationFrame(tick);

		return () => {
			cancelAnimationFrame(frame);
			void unlisten.then((u) => u());
		};
	});
</script>

<div class="waveform" aria-hidden="true">
	{#each bars as height}
		<span class="bar" style="transform: scaleY({height})"></span>
	{/each}
</div>

<style>
	/* Listening: a live equalizer in the icon's warm tone, with a soft halo. */
	.waveform {
		pointer-events: none;
		display: flex;
		align-items: center;
		gap: 4px;
		height: 30px;
		filter: drop-shadow(0 0 6px rgba(247, 200, 163, 0.55));
	}

	.bar {
		width: 4px;
		height: 100%;
		border-radius: 2px;
		/* The icon's warm tone — matches the transcribing dots in Tongue.svelte. */
		background: #f7c8a3;
		transform-origin: center;
	}
</style>
