// The eyes move, and the lamp must stay visible (docs/adr/0040): the engine
// keeps every eye inside the limits the Rust validator checks a creature's
// lamp against.
import { describe, expect, it } from 'vitest';
import { CREATURE_STATES, DEFAULT_CREATURE } from '../data';
import type { Gaze } from '../types';
import { EYE_MOTION, placeEye } from './eyes';
import { SIDES } from './geometry';
import { createSim, updateSim, type SimInput } from './sim';

const eyes = DEFAULT_CREATURE.eyes;
const bend = Math.sin(EYE_MOTION.gaze_bend);
const EPS = 1e-9;

const base: SimInput = {
	state: 'idle',
	placement: 'taskbar',
	gaze: null,
	level: 0,
	wakeArmed: false,
	dragging: false,
	hovered: false,
	pokes: 0,
	mode: null,
	reduced: false
};

// The cursor darting between the far corners, so the gaze springs overshoot.
const darting: (Gaze | null)[] = [
	{ x: -1, y: -1 },
	{ x: 1, y: 1 },
	{ x: 1, y: -1 },
	{ x: -1, y: 1 },
	null
];

/** Every eye pose the engine reaches in each state, held or not. */
function* sweep(): Generator<{ label: string; sc: number; scTarget: number; gx: number; gy: number }> {
	for (const state of CREATURE_STATES) {
		for (const dragging of [false, true]) {
			const input = { ...base, state, dragging };
			const sim = createSim(DEFAULT_CREATURE, { ...base, dragging }, 0.7, () => 0.5);
			for (let frame = 0; frame < 60 * 4; frame++) {
				const gaze = darting[Math.floor(frame / 20) % darting.length];
				updateSim(sim, DEFAULT_CREATURE, { ...input, gaze, pokes: frame === 30 ? 1 : 0 }, 1 / 60, () => 0.5);
				yield {
					label: `${state}${dragging ? ', held' : ''}, frame ${frame}`,
					sc: sim.sc.x,
					scTarget: sim.sc.t,
					gx: sim.gx.x,
					gy: sim.gy.x
				};
			}
		}
	}
}

describe('the eyes', () => {
	it('never move further than the limits the validator checks', () => {
		const values = [-3, -1.2, -1, -0.5, 0, 0.5, 1, 1.2, 3, Number.NaN];
		for (const sc of [0, 0.5, 1, 1.25, 1.4, 3]) {
			for (const gx of values) {
				for (const gy of values) {
					for (const side of SIDES) {
						const e = placeEye(eyes, { sc, gx, gy }, side);
						expect(Math.abs(e.x - side * eyes.x)).toBeLessThanOrEqual(EYE_MOTION.shift_x * bend * eyes.radius_x + EPS);
						expect(Math.abs(e.y - eyes.y)).toBeLessThanOrEqual(EYE_MOTION.shift_y * bend * eyes.radius_y + EPS);
						expect(e.rx).toBeLessThanOrEqual(eyes.radius_x * EYE_MOTION.scale_max + EPS);
						expect(e.ry).toBeLessThanOrEqual(eyes.radius_y * EYE_MOTION.scale_max + EPS);
					}
				}
			}
		}
	});

	it('are never posed wider than the widest, so the clamp only trims overshoot', () => {
		let overshot = false;
		for (const p of sweep()) {
			expect(p.scTarget, p.label).toBeLessThanOrEqual(EYE_MOTION.scale_max);
			if (p.sc > EYE_MOTION.scale_max) overshot = true;
		}
		// The startle's spring does overshoot: the clamp is what holds the limit.
		expect(overshot).toBe(true);
	});

	it('never cover the bundled creature\'s lamp', () => {
		const { x: lx, y: ly } = DEFAULT_CREATURE.lamp;
		for (const p of sweep()) {
			for (const side of SIDES) {
				const e = placeEye(eyes, p, side);
				const inside = ((lx - e.x) / e.rx) ** 2 + ((ly - e.y) / e.ry) ** 2;
				expect(inside, `${p.label}, side ${side}`).toBeGreaterThan(1);
			}
		}
	});
});
