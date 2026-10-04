// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: `Spring` and `cubicBezier` in windows/src/core/anim.ts. See
// THIRD-PARTY-NOTICES.
//
// Damped springs, as docs/design-system.md ("Motion") specifies them:
// a = −ω²(x − target) − 2ζωv with ω = 2π / response, integrated in fixed
// 240 Hz sub-steps so a long or dropped frame never destabilises them. The
// creature and the island share them.
//
// Plain records rather than a class: the creature steps about twenty of these
// every frame, and its simulation snapshots and snaps them in bulk.

export const SUBSTEP_HZ = 240;

export interface Spring {
	/** Current value. */
	x: number;
	/** Velocity, per second. */
	v: number;
	/** Target. */
	t: number;
	/** Angular frequency ω, rad/s. */
	w: number;
	/** Damping ratio ζ. */
	z: number;
}

/** A spring at rest at `x`. `response` is in seconds. */
export function spring(x: number, response: number, damping: number): Spring {
	return { x, v: 0, t: x, w: (2 * Math.PI) / response, z: damping };
}

/** Advance by `dt` seconds in sub-steps of at most 1/240 s. */
export function stepSpring(s: Spring, dt: number): void {
	if (!(dt > 0)) return;
	// The epsilon keeps 1/60 s at exactly four sub-steps despite rounding.
	const n = Math.max(1, Math.ceil(dt * SUBSTEP_HZ - 1e-9));
	const h = dt / n;
	for (let i = 0; i < n; i++) {
		const a = -s.w * s.w * (s.x - s.t) - 2 * s.z * s.w * s.v;
		s.v += a * h;
		s.x += s.v * h;
	}
}

/** Jump to the target and stop (reduced motion). */
export function snapSpring(s: Spring): void {
	s.x = s.t;
	s.v = 0;
}

/** Close enough to the target, and slow enough, to stop drawing for it. */
export function isSettled(s: Spring, epsilon = 1e-3): boolean {
	return Math.abs(s.x - s.t) < epsilon && Math.abs(s.v) < epsilon * 10;
}

export type Easing = (t: number) => number;

/** CSS `cubic-bezier(x1, y1, x2, y2)` as an easing function. */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): Easing {
	const bx = (t: number) => 3 * (1 - t) ** 2 * t * x1 + 3 * (1 - t) * t * t * x2 + t ** 3;
	const by = (t: number) => 3 * (1 - t) ** 2 * t * y1 + 3 * (1 - t) * t * t * y2 + t ** 3;
	return (x) => {
		if (x <= 0) return 0;
		if (x >= 1) return 1;
		// Bisection on x; 16 halvings is well under a pixel at any size.
		let lo = 0;
		let hi = 1;
		let t = x;
		for (let i = 0; i < 16; i++) {
			if (bx(t) < x) lo = t;
			else hi = t;
			t = (lo + hi) / 2;
		}
		return by(t);
	};
}

/** The island's close curve: no overshoot, 340 ms. */
export const CLOSE_EASING = cubicBezier(0.45, 0, 0.2, 1);
export const CLOSE_MS = 340;

/**
 * A spring from 0 to 1 as a fixed-duration easing, for CSS transitions.
 * The spring is simulated once; `duration` is when it settles.
 */
export function springEasing(response = 0.5, damping = 0.72): { easing: Easing; duration: number } {
	const s = spring(0, response, damping);
	s.t = 1;
	const samples = [0];
	const dt = 1 / 120;
	while (!isSettled(s) && samples.length < 600) {
		stepSpring(s, dt);
		samples.push(s.x);
	}
	samples[samples.length - 1] = 1;
	const last = samples.length - 1;
	return {
		duration: Math.round(last * dt * 1000),
		easing: (t) => {
			if (t <= 0) return 0;
			if (t >= 1) return 1;
			const x = t * last;
			const i = Math.floor(x);
			return samples[i] + (samples[i + 1] - samples[i]) * (x - i);
		}
	};
}

/** True when the user asked the OS for less motion. */
export function prefersReducedMotion(): boolean {
	return (
		typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
	);
}
