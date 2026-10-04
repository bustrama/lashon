import { describe, expect, it } from 'vitest';
import { CREATURE_STATES as STATES, DEFAULT_CREATURE, type CreatureData } from '../data';
import type { CreatureState } from '../types';
import { describe as describeFrame } from './frame';
import { glow, LAMP_IDLE, LAMP_MAX, LAMP_MIN, lampStrength, lampToken } from './lamp';
import { createSim, updateSim, type SimInput } from './sim';

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

describe('the lamp', () => {
	it('takes the design system\'s state colours', () => {
		const expected: Record<CreatureState, string> = {
			idle: '--peach',
			preparing: '--saffron',
			dictation: '--saffron',
			command: '--garnet',
			transcribing: '--state-cloud',
			thinking: '--state-cloud',
			tool: '--garnet',
			confirm: '--peach',
			wake: '--saffron',
			error: '--state-error',
			'agent-needs-you': '--aqua',
			'agent-done': '--state-success'
		};
		for (const state of STATES) expect(lampToken({ state, mode: null, wakeArmed: false })).toBe(expected[state]);
	});

	it('flashes the listening mode\'s colour on wake', () => {
		expect(lampToken({ state: 'wake', mode: 'command', wakeArmed: false })).toBe('--garnet');
		expect(lampToken({ state: 'wake', mode: 'dictation', wakeArmed: false })).toBe('--saffron');
	});

	it('shows an open wake-word microphone at rest', () => {
		expect(lampToken({ state: 'idle', mode: null, wakeArmed: true })).toBe('--state-cloud');
		expect(lampStrength('idle', 0, true)).toBeGreaterThanOrEqual(LAMP_MIN);
	});

	it('is dim at rest and never below 30% in any other state', () => {
		for (const state of STATES) {
			for (const reduced of [false, true]) {
				for (let T = 0; T < 8; T += 0.13) {
					for (const u of [0, 0.2, 0.5, 0.99, 1]) {
						for (const level of [0, 0.4, 1]) {
							const strength = lampStrength(state, glow({ state, T, u, level, reduced }), false);
							if (state === 'idle') expect(strength).toBe(LAMP_IDLE);
							else {
								expect(strength, `${state} T=${T} u=${u}`).toBeGreaterThanOrEqual(LAMP_MIN);
								expect(strength).toBeLessThanOrEqual(LAMP_MAX);
							}
						}
					}
				}
			}
		}
	});

	it('clamps whatever glow it is given', () => {
		for (const g of [-5, Number.NaN, Number.POSITIVE_INFINITY, 40]) {
			const strength = lampStrength('dictation', g, false);
			expect(strength).toBeGreaterThanOrEqual(LAMP_MIN);
			expect(strength).toBeLessThanOrEqual(LAMP_MAX);
		}
	});

	it('follows the voice while listening, and holds steady under reduced motion', () => {
		const quiet = glow({ state: 'dictation', T: 1, u: 0, level: 0, reduced: false });
		const loud = glow({ state: 'dictation', T: 1, u: 0, level: 1, reduced: false });
		expect(loud).toBeGreaterThan(quiet);
		const a = glow({ state: 'thinking', T: 1, u: 0, level: 0, reduced: true });
		const b = glow({ state: 'thinking', T: 7, u: 0.5, level: 1, reduced: true });
		expect(a).toBe(b);
	});

	it('is the engine\'s: a creature cannot change its colour or strength', () => {
		const other: CreatureData = {
			...DEFAULT_CREATURE,
			body: { ...DEFAULT_CREATURE.body, radius_x: 24, radius_y: 16, wobble: 0 },
			lamp: { x: 3, y: 2, radius: 18 },
			poses: { ...DEFAULT_CREATURE.poses, dictation: 'rest' }
		};
		const stage = { width: 220, height: 120, unit: 1.5, dpr: 1 };
		for (const state of STATES) {
			const input = { ...base, state, level: 0.6 };
			const frames = [DEFAULT_CREATURE, other].map((creature) => {
				const sim = createSim(creature, input, 0.7, () => 0.5);
				for (let i = 0; i < 30; i++) updateSim(sim, creature, input, 1 / 60, () => 0.5);
				return describeFrame(sim, creature, input, stage);
			});
			expect(frames[1].lampToken).toBe(frames[0].lampToken);
			expect(frames[1].lampStrength).toBeCloseTo(frames[0].lampStrength, 9);
		}
	});
});
