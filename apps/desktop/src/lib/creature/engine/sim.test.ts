import { describe, expect, it } from 'vitest';
import { CREATURE_STATES, DEFAULT_CREATURE } from '../data';
import { PLACEMENTS } from '../types';
import { describe as describeFrame, type Frame } from './frame';
import { createSim, isBusy, updateSim, type Sim, type SimInput } from './sim';

const stage = { width: 220, height: 120, unit: 1.5, dpr: 1 };
const fixed = () => 0.5;
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

function run(sim: Sim, input: SimInput, seconds: number): Frame {
	for (let t = 0; t < seconds; t += 1 / 60) updateSim(sim, DEFAULT_CREATURE, input, 1 / 60, fixed);
	return describeFrame(sim, DEFAULT_CREATURE, input, stage);
}

describe('the simulation', () => {
	it('starts in its pose, not flying into it', () => {
		const sim = createSim(DEFAULT_CREATURE, base, 0.7, fixed);
		expect(sim.hop.v).toBe(0);
		expect(sim.arms[1].hx.x).toBe(sim.arms[1].hx.t);
	});

	it('stands still under reduced motion, in every state and placement', () => {
		for (const placement of PLACEMENTS) {
			for (const state of CREATURE_STATES) {
				const input = { ...base, state, placement, level: 0.8, reduced: true };
				const sim = createSim(DEFAULT_CREATURE, { ...base, placement, reduced: true }, 0.7, fixed);
				const a = run(sim, { ...input, pokes: 3 }, 0.5);
				const b = run(sim, { ...input, pokes: 9 }, 2);
				expect(b, `${state} on ${placement}`).toEqual(a);
				expect(a.wobble).toBe(0);
				expect(isBusy(sim)).toBe(false);
				// The lamp still shows the state.
				if (state !== 'idle') expect(a.lampStrength).toBeGreaterThanOrEqual(0.3);
			}
		}
	});

	it('hops when poked, and settles again', () => {
		const sim = createSim(DEFAULT_CREATURE, base, 0.7, fixed);
		run(sim, base, 0.1);
		updateSim(sim, DEFAULT_CREATURE, { ...base, pokes: 1 }, 1 / 60, fixed);
		expect(sim.hop.x).toBeLessThan(0);
		expect(isBusy(sim)).toBe(true);
		run(sim, { ...base, pokes: 1 }, 4);
		expect(Math.abs(sim.hop.x)).toBeLessThan(0.05);
	});

	it('keeps the taskbar floor on the stage\'s bottom edge, even mid-hop', () => {
		const sim = createSim(DEFAULT_CREATURE, base, 0.7, fixed);
		updateSim(sim, DEFAULT_CREATURE, { ...base, pokes: 1 }, 1 / 60, fixed);
		for (let i = 0; i < 120; i++) {
			const f = run(sim, { ...base, pokes: 1 }, 1 / 60);
			// The body rises off the floor; it never sinks below it.
			expect(f.ay).toBeLessThanOrEqual(stage.height);
		}
		const f = run(sim, { ...base, pokes: 1 }, 4);
		expect(f.ay).toBeCloseTo(stage.height, 1);
		expect(f.anchorY).toBeCloseTo(DEFAULT_CREATURE.body.radius_y * (11 / 20.2), 9);
	});

	it('hangs from the stage\'s top edge on the ceiling', () => {
		const input = { ...base, placement: 'ceiling' as const };
		const f = run(createSim(DEFAULT_CREATURE, input, 0.7, fixed), input, 1);
		expect(f.ay).toBe(0);
		expect(f.g.hang).toBe(true);
	});

	it('looks at the cursor, then loses interest when it stops moving', () => {
		const sim = createSim(DEFAULT_CREATURE, base, 0.7, fixed);
		const input = { ...base, gaze: { x: -1, y: 0.4 } };
		run(sim, input, 1);
		expect(sim.gx.x).toBeCloseTo(-1, 2);
		expect(sim.gy.x).toBeCloseTo(0.4, 2);
		run(sim, input, 5);
		expect(sim.gx.x).toBeGreaterThan(-0.5);
	});

	it('keeps its eyes on its work while writing, wherever the cursor is', () => {
		const input = { ...base, state: 'dictation' as const, gaze: { x: -1, y: -1 } };
		const sim = createSim(DEFAULT_CREATURE, input, 0.7, fixed);
		const f = run(sim, input, 1);
		expect(f.eyes.gx).toBeCloseTo(0.65, 2);
		expect(f.eyes.gy).toBeCloseTo(0.45, 2);
		expect(f.pad).not.toBeNull();
		expect(f.write).toBeGreaterThan(0);
	});

	it('writes faster the louder the voice, and turns the page when it is full', () => {
		const quiet = { ...base, state: 'dictation' as const, level: 0 };
		const loud = { ...quiet, level: 1 };
		const a = createSim(DEFAULT_CREATURE, quiet, 0.7, fixed);
		const b = createSim(DEFAULT_CREATURE, loud, 0.7, fixed);
		expect(run(b, loud, 2).write).toBeGreaterThan(run(a, quiet, 2).write);
		let flipped = false;
		for (let i = 0; i < 600 && !flipped; i++) flipped = run(b, loud, 1 / 60).flipping;
		expect(flipped).toBe(true);
	});

	it('puts the notepad away while dragged, and hangs from the cursor', () => {
		const input = { ...base, state: 'dictation' as const, dragging: true };
		const f = run(createSim(DEFAULT_CREATURE, input, 0.7, fixed), input, 1);
		expect(f.pad).toBeNull();
		expect(f.eyes.gy).toBeCloseTo(0.5, 2);
		expect(f.sy).toBeGreaterThan(1.05);
	});

	it('rains code only in command mode', () => {
		for (const state of CREATURE_STATES) {
			const input = { ...base, state };
			const f = run(createSim(DEFAULT_CREATURE, input, 0.7, fixed), input, 0.2);
			expect(f.rain !== null, state).toBe(state === 'command');
		}
	});

	it('keeps the numbers it hands the shader small however long it runs', () => {
		const input = { ...base, state: 'command' as const };
		const sim = createSim(DEFAULT_CREATURE, input, 0.7, fixed);
		sim.T = 1e6;
		sim.stateSince = 1e6;
		const f = run(sim, input, 0.5);
		for (const p of f.phases) expect(Math.abs(p)).toBeLessThan(2 * Math.PI);
		expect(f.rain!.T).toBeLessThan(1);
	});

	it('starts the rain again on each command', () => {
		const command = { ...base, state: 'command' as const };
		const sim = createSim(DEFAULT_CREATURE, command, 0.7, fixed);
		run(sim, command, 2);
		run(sim, { ...base, state: 'thinking' }, 0.5);
		const f = run(sim, command, 0.1);
		expect(f.rain!.phase).toBeLessThan(0.5);
		expect(f.rain!.T).toBeLessThan(0.2);
	});
});
