import { describe, expect, it } from 'vitest';
import { isSettled, snapSpring, spring, stepSpring, SUBSTEP_HZ } from './spring';

describe('spring', () => {
	it('takes ω from the response time', () => {
		expect(spring(0, 0.5, 0.7).w).toBeCloseTo((2 * Math.PI) / 0.5, 12);
	});

	it('settles on its target', () => {
		const s = spring(0, 0.3, 0.6);
		s.t = 10;
		for (let i = 0; i < 180; i++) stepSpring(s, 1 / 60);
		expect(s.x).toBeCloseTo(10, 3);
		expect(isSettled(s)).toBe(true);
	});

	it('accelerates by a = −ω²(x − target) − 2ζωv', () => {
		const h = 1 / SUBSTEP_HZ;
		const s = spring(3, 0.4, 0.3);
		s.t = 1;
		s.v = 2;
		const { w, z } = s;
		const v = 2 + (-w * w * (3 - 1) - 2 * z * w * 2) * h;
		stepSpring(s, h);
		// Semi-implicit Euler: the velocity first, then the position with it.
		expect(s.v).toBeCloseTo(v, 12);
		expect(s.x).toBeCloseTo(3 + v * h, 12);
	});

	it('tracks the exact solution', () => {
		// Under-damped from rest at 1 toward 0:
		// x(t) = e^(−ζωt)·(cos ω_d t + ζω/ω_d · sin ω_d t)
		const s = spring(1, 0.4, 0.3);
		s.t = 0;
		const { w, z } = s;
		const wd = w * Math.sqrt(1 - z * z);
		let worst = 0;
		for (let i = 1; i <= 90; i++) {
			stepSpring(s, 1 / 60);
			const t = i / 60;
			const exact = Math.exp(-z * w * t) * (Math.cos(wd * t) + ((z * w) / wd) * Math.sin(wd * t));
			worst = Math.max(worst, Math.abs(s.x - exact));
		}
		// The integrator is first order: at 240 Hz it stays within ~3% of the swing.
		expect(worst).toBeLessThan(0.03);
	});

	it('integrates in fixed 240 Hz sub-steps, whatever the frame rate', () => {
		const a = spring(0, 0.22, 0.5);
		const b = spring(0, 0.22, 0.5);
		a.t = b.t = 1;
		for (let i = 0; i < 4; i++) stepSpring(a, 1 / 60);
		stepSpring(b, 4 / 60);
		expect(a.x).toBeCloseTo(b.x, 12);
		expect(a.v).toBeCloseTo(b.v, 12);
		expect(SUBSTEP_HZ).toBe(240);
	});

	it('stays stable through a long frame', () => {
		const s = spring(0, 0.12, 0.9);
		s.t = 1;
		stepSpring(s, 2);
		expect(Number.isFinite(s.x)).toBe(true);
		expect(s.x).toBeCloseTo(1, 3);
	});

	it('overshoots when under-damped and not when critically damped', () => {
		const loose = spring(0, 0.38, 0.25);
		const firm = spring(0, 0.38, 1);
		loose.t = firm.t = 1;
		let looseMax = 0;
		let firmMax = 0;
		for (let i = 0; i < 240; i++) {
			stepSpring(loose, 1 / 120);
			stepSpring(firm, 1 / 120);
			looseMax = Math.max(looseMax, loose.x);
			firmMax = Math.max(firmMax, firm.x);
		}
		expect(looseMax).toBeGreaterThan(1.3);
		expect(firmMax).toBeLessThanOrEqual(1 + 1e-9);
	});

	it('ignores a zero or negative step', () => {
		const s = spring(0, 0.3, 0.6);
		s.t = 1;
		stepSpring(s, 0);
		stepSpring(s, -1);
		expect(s.x).toBe(0);
	});

	it('snaps to its target and stops', () => {
		const s = spring(0, 0.3, 0.6);
		s.t = 4;
		s.v = 9;
		snapSpring(s);
		expect(s).toMatchObject({ x: 4, v: 0, t: 4 });
	});
});
