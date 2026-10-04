// Runs one creature on a pair of canvases: the WebGL body underneath, the 2D
// eyes and props on top. It owns the frame loop and keeps it cheap:
//
// - it draws only while the creature is visible (page and element);
// - at rest it drops to 30 fps, and under reduced motion it draws only when
//   something changes;
// - it measures what each frame costs (`stats`).
import type { CreatureData } from '../data';
import { drawEyes, drawFallbackBody, drawPad, sizeCanvas } from './canvas2d';
import { describe, type Frame, type Stage } from './frame';
import { createGlBody, type GlBody } from './gl';
import { mix, readPalette, type Palette, type Rgba } from './palette';
import { createSim, isBusy, updateSim, type Sim, type SimInput } from './sim';

export interface FrameStats {
	renderer: 'webgl' | '2d';
	/** Frames drawn per second, over the last second. */
	fps: number;
	/** Mean and worst CPU time of a frame (update + draw calls) over the last few seconds, ms. */
	meanMs: number;
	maxMs: number;
}

export interface Engine {
	setInput(input: SimInput): void;
	setCreature(creature: CreatureData): void;
	/** Re-read the stage size and device pixel ratio. */
	resize(width: number, height: number): void;
	stats(): FrameStats;
	destroy(): void;
}

/** CSS px per design unit (docs/design-system.md). */
export const UNIT = 1.5;
const IDLE_FPS = 30;
/** How long frame costs count toward the stats, ms. */
const STATS_WINDOW = 5000;
/** How fast the lamp's colour crosses to a new state's, per second. */
const LAMP_FADE = 12;

/** The inputs a still (reduced-motion) creature shows. */
function stillKey(i: SimInput): string {
	const gaze = i.gaze ? `${i.gaze.x.toFixed(3)},${i.gaze.y.toFixed(3)}` : '';
	return [i.state, i.placement, i.wakeArmed, i.dragging, i.hovered, i.mode, gaze].join('|');
}

export function createEngine(
	host: HTMLElement,
	bodyCanvas: HTMLCanvasElement,
	faceCanvas: HTMLCanvasElement,
	creature: CreatureData,
	input: SimInput,
	onStats?: (stats: FrameStats) => void
): Engine {
	let current = creature;
	let latest = input;
	const sim: Sim = createSim(current, latest);
	const stage: Stage = { width: host.clientWidth, height: host.clientHeight, unit: UNIT, dpr: devicePixelRatio || 1 };

	const font = getComputedStyle(host).getPropertyValue('--font-mono').trim() || 'monospace';
	let gl: GlBody | null = createGlBody(bodyCanvas, font);
	let palette: Palette = readPalette(host);
	const darkQuery = matchMedia('(prefers-color-scheme: dark)');
	let dark = darkQuery.matches;
	let lamp: Rgba | null = null;

	let raf = 0;
	let last = 0;
	let lastDrawn = 0;
	/** Seconds since the last drawn frame. */
	let pending = 0;
	let visible = true;
	/** When each recent frame was drawn, and what it cost. */
	const costs: { at: number; ms: number }[] = [];
	const drawnAt: number[] = [];

	/** Forget frames older than the stats windows. */
	function age(now: number): void {
		while (costs.length && now - costs[0].at > STATS_WINDOW) costs.shift();
		while (drawnAt.length && now - drawnAt[0] > 1000) drawnAt.shift();
	}

	function lampColour(f: Frame, dt: number): Rgba {
		const target = palette[f.lampToken] ?? palette['--peach'];
		lamp = !lamp || f.reduced ? target : mix(lamp, target, Math.min(1, dt * LAMP_FADE));
		return lamp;
	}

	function draw(dt: number): void {
		const t0 = performance.now();
		updateSim(sim, current, latest, dt);
		const f = describe(sim, current, latest, stage);
		const colour = lampColour(f, dt);
		const face = sizeCanvas(faceCanvas, f);
		if (gl) gl.draw(f, current, palette, colour, dark);
		else if (face) drawFallbackBody(face, f, current, palette, colour, dark);
		if (face) {
			drawEyes(face, f, current, palette);
			if (f.pad && sim.writing) drawPad(face, f, current, palette, dark, [f.arms[1][2], f.arms[1][3]]);
		}
		costs.push({ at: t0, ms: performance.now() - t0 });
		drawnAt.push(t0);
		age(t0);
	}

	function calm(): boolean {
		return latest.state === 'idle' && !latest.dragging && !latest.hovered && !isBusy(sim);
	}

	function tick(now: number): void {
		raf = 0;
		if (!visible) return;
		pending += last ? (now - last) / 1000 : 0;
		last = now;
		try {
			// At rest, skip frames down to IDLE_FPS; the time still counts.
			if (!(calm() && now - lastDrawn < 1000 / IDLE_FPS - 2)) {
				const dt = Math.min(0.1, pending);
				pending = 0;
				lastDrawn = now;
				draw(dt);
			}
		} finally {
			// One bad frame must not stop the creature, or its lamp.
			schedule();
		}
	}

	function schedule(): void {
		if (raf || !visible) return;
		if (latest.reduced) return;
		raf = requestAnimationFrame(tick);
	}

	/** A retry of a frame that threw, waiting for the next animation frame. */
	let retry = 0;

	/** Draw one frame now (reduced motion, or a change while paused). */
	function drawOnce(): void {
		if (retry) cancelAnimationFrame(retry);
		retry = 0;
		if (!visible) return;
		try {
			draw(0);
		} catch (err) {
			// A frame that throws must not break whoever asked for it (an effect
			// in Creature.svelte). Under reduced motion no loop draws again, and
			// the lamp would keep showing the last state: try again next frame,
			// as the loop would.
			console.error('creature: drawing a frame failed', err);
			if (latest.reduced) {
				retry = requestAnimationFrame(() => {
					retry = 0;
					drawOnce();
				});
			}
		}
	}

	function setVisible(next: boolean): void {
		if (next === visible) return;
		visible = next;
		last = 0;
		pending = 0;
		if (visible) {
			drawOnce();
			schedule();
		} else if (raf) {
			cancelAnimationFrame(raf);
			raf = 0;
		}
	}

	const onVisibility = () => setVisible(document.visibilityState === 'visible' && onScreen);
	let onScreen = true;
	const io = new IntersectionObserver((entries) => {
		onScreen = entries.some((e) => e.isIntersecting);
		onVisibility();
	});
	io.observe(host);
	document.addEventListener('visibilitychange', onVisibility);

	const onTheme = () => {
		dark = darkQuery.matches;
		palette = readPalette(host);
		lamp = null;
		drawOnce();
	};
	darkQuery.addEventListener('change', onTheme);

	// The device pixel ratio changes without a resize when the window moves to
	// a screen with another scale, or the scale changes: redraw sharp.
	let dprQuery: MediaQueryList | null = null;
	const onDpr = () => {
		watchDpr();
		resize(stage.width, stage.height);
	};
	function watchDpr(): void {
		dprQuery?.removeEventListener('change', onDpr);
		dprQuery = matchMedia(`(resolution: ${devicePixelRatio || 1}dppx)`);
		dprQuery.addEventListener('change', onDpr);
	}
	watchDpr();

	function resize(width: number, height: number): void {
		stage.width = width;
		stage.height = height;
		stage.dpr = devicePixelRatio || 1;
		drawOnce();
	}

	const onLost = (event: Event) => {
		// Fall back to 2D until the GPU comes back; the lamp keeps showing.
		event.preventDefault();
		gl?.dispose(false);
		gl = null;
		drawOnce();
	};
	const onRestored = () => {
		gl = createGlBody(bodyCanvas, font);
		drawOnce();
	};
	bodyCanvas.addEventListener('webglcontextlost', onLost);
	bodyCanvas.addEventListener('webglcontextrestored', onRestored);

	const statsTimer = onStats ? setInterval(() => onStats(stats()), 1000) : undefined;

	function stats(): FrameStats {
		// Paused, nothing draws to age the windows out; do it here.
		age(performance.now());
		const ms = costs.map((c) => c.ms);
		return {
			renderer: gl ? 'webgl' : '2d',
			fps: drawnAt.length,
			meanMs: ms.length ? ms.reduce((a, b) => a + b, 0) / ms.length : 0,
			maxMs: ms.length ? Math.max(...ms) : 0
		};
	}

	drawOnce();
	schedule();

	return {
		setInput(next) {
			const previous = latest;
			const wasReduced = previous.reduced;
			latest = next;
			if (next.reduced) {
				if (raf) cancelAnimationFrame(raf);
				raf = 0;
				// The voice level and pokes don't move a still creature; only draw
				// for what shows.
				if (!wasReduced || stillKey(next) !== stillKey(previous)) drawOnce();
			} else {
				if (wasReduced) {
					last = 0;
					pending = 0;
				}
				schedule();
			}
		},
		setCreature(next) {
			current = next;
			drawOnce();
		},
		resize,
		stats,
		destroy() {
			if (raf) cancelAnimationFrame(raf);
			raf = 0;
			if (retry) cancelAnimationFrame(retry);
			retry = 0;
			visible = false;
			clearInterval(statsTimer);
			io.disconnect();
			document.removeEventListener('visibilitychange', onVisibility);
			darkQuery.removeEventListener('change', onTheme);
			dprQuery?.removeEventListener('change', onDpr);
			bodyCanvas.removeEventListener('webglcontextlost', onLost);
			bodyCanvas.removeEventListener('webglcontextrestored', onRestored);
			gl?.dispose(true);
			gl = null;
		}
	};
}
