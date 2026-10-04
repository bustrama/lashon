// The creature's body over time: every moving part on a spring, driven by
// the state's pose. No DOM here, so it runs the same in tests and in the app.
import type { CreatureData } from '../data';
import type { CreatureState, Gaze, Placement } from '../types';
import {
	geometry,
	pad,
	penPalm,
	penTip,
	rest,
	shoulder,
	SIDES,
	type Geometry,
	type Side,
	type Vec2
} from './geometry';
import { GESTURE_LIBRARY, type Impulse } from './gestures';
import { glow as glowProgram } from './lamp';
import { cyclePhase, CYCLES, gestureFor } from './pose';
import { snapSpring, spring, stepSpring, type Spring } from './spring';

/** What the host tells the creature (lib/creature/types.ts, plus `reduced`). */
export interface SimInput {
	state: CreatureState;
	placement: Placement;
	gaze: Gaze | null;
	level: number;
	wakeArmed: boolean;
	dragging: boolean;
	hovered: boolean;
	pokes: number;
	mode: 'dictation' | 'command' | null;
	/** prefers-reduced-motion. */
	reduced: boolean;
}

interface Arm {
	sx: Spring;
	sy: Spring;
	hx: Spring;
	hy: Spring;
}

export interface Sim {
	/** Seconds since the creature appeared. */
	T: number;
	/** A per-creature phase offset for its ambient motion. */
	ph: number;
	rot: Spring;
	sq: Spring;
	hop: Spring;
	jig: Spring;
	lt: Spring;
	lb: Spring;
	tilt: Spring;
	sc: Spring;
	gx: Spring;
	gy: Spring;
	arms: Record<Side, Arm>;
	glow: number;
	blink: number;
	nextBlink: number;
	smileUntil: number;
	/** Lines written on the current page. */
	write: number;
	/** Seconds left in a page flip. */
	flip: number;
	rainPhase: number;
	state: CreatureState;
	stateSince: number;
	lastU: number;
	pokesSeen: number;
	/** The last gaze target, and when it last moved. */
	gaze: Gaze | null;
	gazeMovedAt: number;
	/** Gestures in front of the body that the canvas draws (dictation's pencil). */
	writing: boolean;
}

/** How long a still cursor keeps Ottid's attention. */
const GAZE_ATTENTION = 4;
const SMILE_SECONDS = 0.9;
export const FLIP_SECONDS = 0.35;

function arm(): Arm {
	return {
		sx: spring(0, 0.3, 0.6),
		sy: spring(0, 0.3, 0.6),
		hx: spring(0, 0.22, 0.5),
		hy: spring(0, 0.22, 0.5)
	};
}

/** Every spring, for stepping and snapping in bulk. */
function springs(s: Sim): Spring[] {
	const out = [s.rot, s.sq, s.hop, s.jig, s.lt, s.lb, s.tilt, s.sc, s.gx, s.gy];
	for (const sd of SIDES) {
		const a = s.arms[sd];
		out.push(a.sx, a.sy, a.hx, a.hy);
	}
	return out;
}

export function createSim(creature: CreatureData, input: SimInput, ph = 0.7, random = Math.random): Sim {
	const s: Sim = {
		T: 0,
		ph,
		rot: spring(0, 1.1, 0.45),
		sq: spring(1, 0.38, 0.25),
		hop: spring(0, 0.42, 0.38),
		jig: spring(0, 0.5, 0.3),
		lt: spring(0.12, 0.12, 0.9),
		lb: spring(0, 0.18, 0.9),
		tilt: spring(0, 0.18, 0.9),
		sc: spring(1, 0.25, 0.7),
		gx: spring(0, 0.22, 0.85),
		gy: spring(0, 0.22, 0.85),
		arms: { [-1]: arm(), [1]: arm() } as Record<Side, Arm>,
		glow: 0,
		blink: 0,
		nextBlink: 1 + random() * 3,
		smileUntil: -1,
		write: 0,
		flip: 0,
		rainPhase: 0,
		state: input.state,
		stateSince: 0,
		lastU: 0,
		pokesSeen: input.pokes,
		gaze: input.gaze,
		gazeMovedAt: 0,
		writing: false
	};
	// Start in the pose, not flying into it.
	updateSim(s, creature, input, 0, random);
	for (const sp of springs(s)) snapSpring(sp);
	return s;
}

function applyImpulse(s: Sim, impulse: Impulse, hang: boolean): void {
	if (impulse.kind === 'flinch') {
		s.sq.v -= 3.5;
		s.jig.v += 3;
		return;
	}
	const k = impulse.strength;
	if (hang) s.sq.v += 2.6 * k;
	else {
		s.hop.v -= 150 * k;
		s.sq.v += 1.6 * k;
	}
	s.jig.v += 3 * k;
	if (impulse.smile) s.smileUntil = s.T + SMILE_SECONDS;
}

/** The gaze target, unless the cursor has sat still long enough to be boring. */
function liveGaze(s: Sim, gaze: Gaze | null): Gaze | null {
	const moved =
		(gaze === null) !== (s.gaze === null) ||
		(gaze !== null && s.gaze !== null && Math.hypot(gaze.x - s.gaze.x, gaze.y - s.gaze.y) > 0.02);
	if (moved) {
		s.gaze = gaze;
		s.gazeMovedAt = s.T;
	}
	return gaze !== null && s.T - s.gazeMovedAt < GAZE_ATTENTION ? gaze : null;
}

/**
 * Advance the creature by `dt` seconds toward the pose for `input`. Under
 * reduced motion time stands still: every spring snaps to its target and
 * nothing ambient moves; the lamp still shows the state.
 */
export function updateSim(
	s: Sim,
	creature: CreatureData,
	input: SimInput,
	dt: number,
	random: () => number = Math.random
): void {
	const reduced = input.reduced;
	if (!reduced) s.T += dt;
	const T = reduced ? 0 : s.T;
	const g: Geometry = geometry(creature, input.placement, T, s.ph);
	const level = reduced ? 0 : Math.min(1, Math.max(0, input.level || 0));

	// The state's clock, and its events.
	const entered = input.state !== s.state;
	if (entered) {
		s.state = input.state;
		s.stateSince = s.T;
		s.lastU = 0;
		s.write = 0;
		s.flip = 0;
		s.rainPhase = 0;
	}
	const gesture = gestureFor(creature, input.state);
	const u = cyclePhase(input.state, s.T - s.stateSince, s.ph, reduced);
	const wrapped = !CYCLES[input.state].once && !entered && u < s.lastU;
	s.lastU = u;
	if (!reduced) {
		if (entered && gesture.onEnter) applyImpulse(s, gesture.onEnter, g.hang);
		if (wrapped && gesture.onCycle) applyImpulse(s, gesture.onCycle, g.hang);
		if (input.pokes !== s.pokesSeen) applyImpulse(s, { kind: 'poke', strength: 1, smile: true }, g.hang);
	}
	s.pokesSeen = input.pokes;

	// Dictation's pencil and the command rain run on the voice.
	s.writing = !!gesture.writes && input.state === 'dictation' && !input.dragging;
	const p = pad(g);
	if (s.writing && !reduced) {
		if (s.flip > 0) s.flip = Math.max(0, s.flip - dt);
		else {
			s.write += dt * (0.25 + 1.1 * level);
			if (s.write >= p.lines) {
				s.write = 0;
				s.flip = FLIP_SECONDS;
			}
		}
	} else if (reduced) {
		// A still page with a few lines on it.
		s.write = 2.6;
		s.flip = 0;
	}
	if (input.state === 'command' && !reduced) s.rainPhase += dt * (1 + 2.5 * level);

	const pose = gesture.pose({ T, u, level, g, pen: penPalm(penTip(p, s.write, s.flip > 0, T)) });

	// Held by the cursor: both hands up, stretched, eyes wide. The lamp stays the state's.
	if (input.dragging) {
		pose.hands = GESTURE_LIBRARY.stretch.pose({ T, u: 0, level, g, pen: [0, 0] }).hands;
		pose.lt = 0;
		pose.sc = 1.1;
		pose.squashTo = 1.08;
		pose.gaze = [0, 0.5];
		pose.gazeLock = true;
	} else if (input.hovered) {
		pose.lt = Math.min(pose.lt, 0.05);
		pose.lb = Math.max(pose.lb, 0.35);
	}

	// Hands: the pose's targets, or rest.
	for (const sd of SIDES) {
		const a = s.arms[sd];
		const [shx, shy] = shoulder(sd, g);
		const target: Vec2 = pose.hands[sd] ?? rest(sd, T, g, s.ph);
		a.sx.t = shx;
		a.sy.t = shy;
		a.hx.t = target[0];
		a.hy.t = target[1];
	}

	// Body and face.
	const breath = reduced ? 1 : 1 + 0.02 * Math.sin(T * 1.6 + s.ph);
	const sway = reduced
		? 0
		: g.hang
			? 0.06 * (0.75 * Math.sin(T * 1.25 + s.ph) + 0.25 * Math.sin(T * 2.9 + 1 + s.ph))
			: 0.025 * Math.sin(T * 0.9 + s.ph);
	s.sq.t = pose.squashTo ?? breath + pose.squash;
	s.rot.t = sway + (reduced ? 0 : pose.rot);
	s.lt.t = pose.lt;
	s.lb.t = s.T < s.smileUntil ? 0.62 : pose.lb;
	s.tilt.t = pose.tilt;
	s.sc.t = pose.sc;
	const cursor = liveGaze(s, input.gaze);
	const look: Vec2 =
		pose.gazeLock && pose.gaze
			? pose.gaze
			: cursor
				? [cursor.x, cursor.y]
				: (pose.gaze ?? (reduced ? [0, 0] : [0.4 * Math.sin(T * 0.55 + s.ph), 0.15 * Math.sin(T * 0.8 + s.ph)]));
	s.gx.t = look[0];
	s.gy.t = look[1];

	// The lamp's own program; the strength floor is applied when it is drawn.
	const target = glowProgram({ state: input.state, T, u, level, reduced });
	s.glow = reduced ? target : s.glow + (target - s.glow) * Math.min(1, dt * 10);

	if (reduced) {
		for (const sp of springs(s)) snapSpring(sp);
		s.blink = 0;
		return;
	}
	for (const sp of springs(s)) stepSpring(sp, dt);
	s.nextBlink -= dt;
	if (s.nextBlink <= 0) {
		s.blink = 1;
		s.nextBlink = 2.4 + random() * 3.6;
	}
	s.blink = Math.max(0, s.blink - dt * 7);
}

/** True while something is still moving that a still frame would cut short. */
export function isBusy(s: Sim): boolean {
	const moving = (sp: Spring) => Math.abs(sp.v) > 0.05;
	return moving(s.hop) || moving(s.jig) || moving(s.sq) || s.T < s.smileUntil || s.blink > 0;
}
