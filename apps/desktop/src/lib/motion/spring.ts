// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: `Spring` and `cubicBezier` in windows/src/core/anim.ts. See
// THIRD-PARTY-NOTICES.
//
// Damped springs, as docs/design-system.md specifies them:
// a = −ω²(x − target) − 2ζωv with ω = 2π / response, integrated in fixed
// 240 Hz sub-steps so a dropped frame never destabilises them.

const SUBSTEP = 1 / 240;

export class Spring {
	value: number;
	target: number;
	velocity = 0;
	private omega: number;
	private zeta: number;

	constructor(value: number, response = 0.5, damping = 0.72) {
		this.value = value;
		this.target = value;
		this.omega = (2 * Math.PI) / response;
		this.zeta = damping;
	}

	configure(response: number, damping: number): void {
		this.omega = (2 * Math.PI) / response;
		this.zeta = damping;
	}

	/** Jump to `value` and stop. */
	set(value: number): void {
		this.value = value;
		this.target = value;
		this.velocity = 0;
	}

	get settled(): boolean {
		return Math.abs(this.target - this.value) < 1e-3 && Math.abs(this.velocity) < 1e-2;
	}

	/** Advance by `dt` seconds. */
	step(dt: number): void {
		const steps = Math.max(1, Math.ceil(dt / SUBSTEP));
		const h = dt / steps;
		for (let i = 0; i < steps; i++) {
			const acc =
				-this.omega * this.omega * (this.value - this.target) -
				2 * this.zeta * this.omega * this.velocity;
			this.velocity += acc * h;
			this.value += this.velocity * h;
		}
	}
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
	const spring = new Spring(0, response, damping);
	spring.target = 1;
	const samples = [0];
	const dt = 1 / 120;
	while (!spring.settled && samples.length < 600) {
		spring.step(dt);
		samples.push(spring.value);
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
