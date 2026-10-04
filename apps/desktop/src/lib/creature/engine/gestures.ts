// The engine's gesture library (docs/adr/0041: a creature picks a gesture per
// state from this set; it never ships motion of its own). Each gesture is the
// body language of one state in docs/design/ottid/states.html, made a pure
// function of time, cycle phase, voice level and placement.
//
// The silhouette rule holds for every gesture: the body is one dark shape, so
// hands work on the outline (the sides, above the head, the floor), never in
// front of the belly. `write` is the exception the rule allows: its hand holds
// the pencil over the notepad, a prop drawn in front of the body.
import type { GestureName } from '../data';
import { reach, shoulder, SIDES, type Geometry, type Side, type Vec2 } from './geometry';

export interface GestureContext {
	/** Seconds. */
	T: number;
	/** The state's cycle phase, 0..1. */
	u: number;
	/** Voice level, 0..1. */
	level: number;
	g: Geometry;
	/** The palm on the pencil, for `write`. */
	pen: Vec2;
}

export interface Pose {
	/** Hand targets; a side left out rests. */
	hands: Partial<Record<Side, Vec2>>;
	/** Top lid, 0 open … 1 shut. */
	lt: number;
	/** Bottom lid (a smile), 0 … 1. */
	lb: number;
	tilt: number;
	/** Eye scale. */
	sc: number;
	/** Gaze target, each axis in [-1, 1], or null to wander. */
	gaze: Vec2 | null;
	/** The gaze belongs to the gesture (looking at its own work); the cursor can't pull it. */
	gazeLock: boolean;
	/** Added to the breathing squash. */
	squash: number;
	/** Replaces the squash outright (a flinch, a stretch). */
	squashTo: number | null;
	/** Added to the body's sway. */
	rot: number;
}

/** A push given to the body's springs, with or without a smile. */
export type Impulse = { kind: 'poke'; strength: number; smile: boolean } | { kind: 'flinch' };

export interface Gesture {
	pose(c: GestureContext): Pose;
	/** Given once when the state starts. */
	onEnter?: Impulse;
	/** Given each time a repeating state's cycle wraps. */
	onCycle?: Impulse;
	/** Its hand holds the pencil (dictation's prop). */
	writes?: boolean;
}

/** The hand that gestures. Hanging from the ceiling, the other one holds on. */
export const GESTURE_SIDE: Side = 1;

function neutral(): Pose {
	return {
		hands: {},
		lt: 0.12,
		lb: 0,
		tilt: 0,
		sc: 1,
		gaze: null,
		gazeLock: false,
		squash: 0,
		squashTo: null,
		rot: 0
	};
}

/** Both hands, or only the gesturing one while hanging. */
function both(g: Geometry, f: (sd: Side) => Vec2): Partial<Record<Side, Vec2>> {
	const hands: Partial<Record<Side, Vec2>> = {};
	for (const sd of SIDES) if (!g.hang || sd === GESTURE_SIDE) hands[sd] = f(sd);
	return hands;
}

function one(f: (sd: Side) => Vec2): Partial<Record<Side, Vec2>> {
	return { [GESTURE_SIDE]: f(GESTURE_SIDE) };
}

export const GESTURE_LIBRARY: Record<GestureName, Gesture> = {
	rest: { pose: () => neutral() },

	greet: {
		pose: ({ T, u, g }) => {
			const p = neutral();
			if (u < 0.22) {
				p.hands = one((sd) => reach(sd, 0.95 + 0.4 * Math.sin(T * 9), 20, g));
				p.lb = 0.62;
			}
			return p;
		}
	},

	stretch: {
		pose: ({ T, u, g }) => {
			const p = neutral();
			p.lt = 0.05;
			p.sc = 1.05;
			if (u < 0.55) {
				p.hands = both(g, (sd) => reach(sd, 1.05 + 0.06 * Math.sin(T * 20), 17, g));
				p.squashTo = 1.07;
			}
			return p;
		}
	},

	write: {
		writes: true,
		pose: ({ level, pen }) => {
			const p = neutral();
			p.lt = 0;
			p.sc = 1.08;
			p.gaze = [0.65, 0.45];
			p.gazeLock = true;
			p.squash = 0.07 * level;
			p.hands = { [GESTURE_SIDE]: pen };
			return p;
		}
	},

	ready: {
		pose: ({ level, g }) => {
			const p = neutral();
			p.lt = 0.06;
			p.sc = 1.08;
			p.tilt = 0.08;
			p.gaze = [0, 0.1];
			p.hands = both(g, (sd) => reach(sd, 0.35 + 0.1 * level, 15, g));
			p.squash = 0.07 * level;
			return p;
		}
	},

	scribble: {
		pose: ({ T, g }) => {
			const p = neutral();
			p.lt = 0.22;
			p.gaze = [0.6, 0.55];
			p.gazeLock = true;
			p.hands = one((sd) => {
				const [x, y] = shoulder(sd, g);
				return [x + sd * 9 + 2.2 * Math.cos(T * 11), y + 5 + 1.6 * Math.sin(T * 14)];
			});
			p.squash = 0.015 * Math.sin(T * 9);
			return p;
		}
	},

	'scratch-head': {
		pose: ({ T, g }) => {
			const p = neutral();
			p.lt = 0.28;
			p.lb = 0.1;
			p.gaze = [0.45, -0.85];
			p.gazeLock = true;
			p.hands = one((sd) => [sd * (11 + 1.5 * Math.sin(T * 10)), -g.ry - 4 + 1.2 * Math.cos(T * 10)]);
			if (!g.hang) p.rot = 0.05;
			return p;
		}
	},

	hammer: {
		pose: ({ T, g }) => {
			const p = neutral();
			p.lt = 0.2;
			p.gaze = [0.4 * Math.sin(T * 6), 0.6];
			p.gazeLock = true;
			p.hands = both(g, (sd) => reach(sd, -0.25 + 0.45 * Math.sin(T * 12 + (sd > 0 ? 0 : Math.PI)), 13, g));
			p.squash = 0.015 * Math.sin(T * 14);
			return p;
		}
	},

	offer: {
		pose: ({ T, g }) => {
			const p = neutral();
			p.lt = 0;
			p.sc = 1.06;
			p.tilt = -0.14;
			p.gaze = [0, 0.1];
			p.hands = both(g, (sd) => reach(sd, 0.1 + 0.06 * Math.sin(T * 3), 18, g));
			return p;
		}
	},

	startle: {
		onEnter: { kind: 'poke', strength: 0.9, smile: false },
		pose: ({ u, g }) => {
			const p = neutral();
			const startled = u < 0.4;
			p.lt = 0;
			p.sc = startled ? 1.25 : 1.12;
			p.gaze = [0, 0.1];
			p.hands = startled ? both(g, (sd) => reach(sd, 1.3, 18, g)) : one((sd) => reach(sd, 0.95, 14, g));
			return p;
		}
	},

	droop: {
		onEnter: { kind: 'flinch' },
		pose: ({ u, g }) => {
			const p = neutral();
			p.lt = 0.42;
			p.tilt = 0.32;
			p.lb = 0.22;
			p.sc = 0.95;
			p.gaze = [0, 0.75];
			p.gazeLock = true;
			p.hands = both(g, (sd) => reach(sd, -1.15, 12, g));
			if (u < 0.15) p.squashTo = 0.82;
			return p;
		}
	},

	beckon: {
		onCycle: { kind: 'poke', strength: 0.6, smile: false },
		pose: ({ T, g }) => {
			const p = neutral();
			p.lt = 0;
			p.sc = 1.2;
			p.tilt = -0.06;
			p.hands = one((sd) => reach(sd, 0.95 + 0.4 * Math.sin(T * 9), 20, g));
			return p;
		}
	},

	cheer: {
		onEnter: { kind: 'poke', strength: 0.9, smile: false },
		pose: ({ T, u, g }) => {
			const p = neutral();
			p.lb = u < 0.8 ? 0.62 : 0;
			p.sc = 1.05;
			if (u < 0.5) p.hands = both(g, (sd) => reach(sd, 1.25 + 0.15 * Math.sin(T * 16 + sd), 18, g));
			return p;
		}
	}
};
