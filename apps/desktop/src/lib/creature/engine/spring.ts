// Damped springs, as docs/design-system.md ("Motion") specifies them:
// a = −ω²(x − target) − 2ζωv with ω = 2π / response, integrated in fixed
// 240 Hz sub-steps so a long or dropped frame never destabilises them.
//
// Plain records rather than a class: the creature steps about twenty of these
// every frame, and the simulation snapshots and snaps them in bulk.

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
