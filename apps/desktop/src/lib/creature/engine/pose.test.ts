import { describe, expect, it } from 'vitest';
import {
	CREATURE_STATES as STATES,
	DEFAULT_CREATURE,
	GESTURES,
	type CreatureData,
	type GestureName
} from '../data';
import { PLACEMENTS, type CreatureState, type Placement } from '../types';
import { geometry, penPalm, penTip, pad, type Vec2 } from './geometry';
import { GESTURE_LIBRARY, GESTURE_SIDE } from './gestures';
import { cyclePhase, CYCLES, gestureName, poseFor } from './pose';

function ctx(placement: Placement, T: number, u: number, level: number, creature = DEFAULT_CREATURE) {
	const g = geometry(creature, placement, T, 0.7);
	return { T, u, level, g, pen: penPalm(penTip(pad(g), 1.5, false, T)) };
}

/** 1 on the body's outline, 0 at its centre. */
function ellipseNorm([x, y]: Vec2, rx: number, ry: number): number {
	return Math.hypot(x / rx, y / ry);
}

describe('state → pose', () => {
	it('maps every state to the default creature\'s designed gesture', () => {
		const designed: Record<CreatureState, GestureName> = {
			idle: 'greet',
			preparing: 'stretch',
			dictation: 'write',
			command: 'ready',
			transcribing: 'scribble',
			thinking: 'scratch-head',
			tool: 'hammer',
			confirm: 'offer',
			wake: 'startle',
			error: 'droop',
			'agent-needs-you': 'beckon',
			'agent-done': 'cheer'
		};
		for (const state of STATES) expect(gestureName(DEFAULT_CREATURE, state)).toBe(designed[state]);
	});

	it('takes the gesture from the creature, not from the state', () => {
		const cheerful: CreatureData = {
			...DEFAULT_CREATURE,
			poses: { ...DEFAULT_CREATURE.poses, idle: 'cheer' }
		};
		const c = ctx('taskbar', 1.3, 0.2, 0);
		expect(poseFor(cheerful, 'idle', c)).toEqual(GESTURE_LIBRARY.cheer.pose(c));
		expect(poseFor(DEFAULT_CREATURE, 'idle', c)).toEqual(GESTURE_LIBRARY.greet.pose(c));
	});

	it('reads an unknown gesture as rest', () => {
		const odd = {
			...DEFAULT_CREATURE,
			poses: { ...DEFAULT_CREATURE.poses, idle: 'moonwalk' as GestureName }
		};
		expect(gestureName(odd, 'idle')).toBe('rest');
	});

	it('reads an inherited property name as rest', () => {
		const c = ctx('taskbar', 1.3, 0.2, 0);
		for (const name of ['toString', 'constructor', '__proto__']) {
			const odd = { ...DEFAULT_CREATURE, poses: { ...DEFAULT_CREATURE.poses, idle: name as GestureName } };
			expect(gestureName(odd, 'idle'), name).toBe('rest');
			expect(() => poseFor(odd, 'idle', c), name).not.toThrow();
		}
	});

	it('is pure: the same inputs give the same pose', () => {
		for (const state of STATES) {
			const a = poseFor(DEFAULT_CREATURE, state, ctx('float', 2.2, 0.3, 0.5));
			const b = poseFor(DEFAULT_CREATURE, state, ctx('float', 2.2, 0.3, 0.5));
			expect(a).toEqual(b);
		}
	});

	it('keeps every hand on the outline (the silhouette rule)', () => {
		// `write` holds the pencil over the notepad, a prop in front of the body.
		for (const name of GESTURES.filter((n) => n !== 'write')) {
			for (const placement of PLACEMENTS) {
				for (let T = 0; T < 6; T += 0.37) {
					for (const u of [0, 0.1, 0.3, 0.5, 0.9]) {
						for (const level of [0, 1]) {
							const c = ctx(placement, T, u, level);
							const pose = GESTURE_LIBRARY[name].pose(c);
							for (const hand of Object.values(pose.hands)) {
								const norm = ellipseNorm(hand as Vec2, c.g.rx, c.g.ry);
								expect(norm, `${name} on ${placement} at T=${T.toFixed(2)} u=${u}`).toBeGreaterThan(0.85);
							}
						}
					}
				}
			}
		}
	});

	it('keeps one hand on the ceiling while hanging', () => {
		for (const name of GESTURES) {
			for (let T = 0; T < 3; T += 0.5) {
				const pose = GESTURE_LIBRARY[name].pose(ctx('ceiling', T, 0.1, 1));
				for (const side of Object.keys(pose.hands).map(Number)) expect(side).toBe(GESTURE_SIDE);
			}
		}
	});

	it('locks the gaze on its own work while writing, scribbling or thinking', () => {
		for (const name of ['write', 'scribble', 'scratch-head', 'hammer', 'droop'] as GestureName[]) {
			expect(GESTURE_LIBRARY[name].pose(ctx('taskbar', 1, 0.5, 0)).gazeLock, name).toBe(true);
		}
		for (const name of ['rest', 'greet', 'ready', 'offer', 'beckon'] as GestureName[]) {
			expect(GESTURE_LIBRARY[name].pose(ctx('taskbar', 1, 0.5, 0)).gazeLock, name).toBe(false);
		}
	});

	it('only writing holds the pencil', () => {
		for (const name of GESTURES) expect(!!GESTURE_LIBRARY[name].writes, name).toBe(name === 'write');
	});

	it('gives events their impulse when they start', () => {
		expect(GESTURE_LIBRARY.startle.onEnter).toEqual({ kind: 'poke', strength: 0.9, smile: false });
		expect(GESTURE_LIBRARY.droop.onEnter).toEqual({ kind: 'flinch' });
		expect(GESTURE_LIBRARY.cheer.onEnter).toEqual({ kind: 'poke', strength: 0.9, smile: false });
		expect(GESTURE_LIBRARY.beckon.onCycle).toEqual({ kind: 'poke', strength: 0.6, smile: false });
	});
});

describe('cycles', () => {
	it('plays events once, then holds the end pose', () => {
		for (const state of ['wake', 'error', 'agent-done'] as CreatureState[]) {
			expect(CYCLES[state].once).toBe(true);
			const period = CYCLES[state].period;
			expect(cyclePhase(state, 0)).toBe(0);
			expect(cyclePhase(state, period / 2)).toBeCloseTo(0.5, 12);
			expect(cyclePhase(state, period * 5)).toBe(1);
		}
	});

	it('repeats the other states', () => {
		const period = CYCLES.idle.period;
		expect(cyclePhase('idle', period * 3 + period / 4)).toBeCloseTo(0.25, 9);
		for (let t = 0; t < 30; t += 0.7) {
			const u = cyclePhase('agent-needs-you', t, 0.7);
			expect(u).toBeGreaterThanOrEqual(0);
			expect(u).toBeLessThan(1);
		}
	});

	it('shows the key pose, still, under reduced motion', () => {
		for (const state of STATES) {
			expect(cyclePhase(state, 0, 0, true)).toBe(CYCLES[state].still);
			expect(cyclePhase(state, 123, 0.7, true)).toBe(CYCLES[state].still);
		}
		// The still poses carry the state: a cheer, a startle settling, a droop.
		const still = (state: CreatureState) =>
			poseFor(DEFAULT_CREATURE, state, ctx('float', 0, CYCLES[state].still, 0));
		expect(Object.keys(still('agent-done').hands)).toHaveLength(2);
		expect(still('agent-done').lb).toBeGreaterThan(0.5);
		expect(Object.keys(still('wake').hands)).toHaveLength(1);
		expect(still('error').lt).toBeGreaterThan(0.4);
	});
});
