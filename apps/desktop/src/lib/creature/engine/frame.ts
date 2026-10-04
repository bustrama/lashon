// One frame's worth of numbers for the renderers: the simulation, placed in
// the stage. Pure, so the lamp's guarantees can be tested without a canvas.
import type { CreatureData } from '../data';
import { anchor, geometry, pad, SIDES, type Geometry, type Pad } from './geometry';
import { lampStrength, lampToken } from './lamp';
import { FLIP_SECONDS, type Sim, type SimInput } from './sim';

const TAU = 2 * Math.PI;

export interface Stage {
	/** CSS px. */
	width: number;
	height: number;
	/** CSS px per design unit. */
	unit: number;
	dpr: number;
}

export interface Frame {
	stage: Stage;
	g: Geometry;
	/** Body-local → stage CSS px: translate(ax, ay) · rotate(rot) · scale(unit·sx, unit·sy) · translate(0, −anchorY). */
	ax: number;
	ay: number;
	anchorY: number;
	rot: number;
	sx: number;
	sy: number;
	/** Outline wobble amplitude (units) and the three harmonics' phases. */
	wobble: number;
	phases: [number, number, number];
	/** Per side: shoulder x, y, hand x, y. Index 0 is the left (−1). */
	arms: [number, number, number, number][];
	lampToken: string;
	/** LAMP_IDLE at rest, LAMP_MIN…LAMP_MAX otherwise. */
	lampStrength: number;
	/** The smoothed glow, 0..1: the halo's size and strength. */
	glow: number;
	eyes: { lt: number; lb: number; tilt: number; sc: number; gx: number; gy: number; blink: number };
	pad: Pad | null;
	write: number;
	flipping: boolean;
	/** Progress through a page flip, 0..1. */
	flip: number;
	/** Command mode's code rain, or null. */
	rain: { phase: number; T: number; bright: number; top: number; span: number } | null;
	reduced: boolean;
}

/** The size the wobble harmonics are scaled to (the prototypes' R). */
const WOBBLE_SIZE = 22 / ((29 + 20.2) / 2);

export function describe(s: Sim, creature: CreatureData, input: SimInput, stage: Stage): Frame {
	const reduced = input.reduced;
	const T = reduced ? 0 : s.T;
	const g = geometry(creature, input.placement, T, s.ph);
	const bob = g.placement === 'float' && !reduced ? 2.6 * Math.sin(T * 1.8 + s.ph) : 0;
	const a = anchor(g, stage.width, stage.height, stage.unit, s.hop.x, bob);
	const sy = s.sq.x;
	const size = ((creature.body.radius_x + creature.body.radius_y) / 2) * WOBBLE_SIZE;
	const level = reduced ? 0 : Math.min(1, Math.max(0, input.level || 0));
	const wobble = reduced ? 0 : (creature.body.wobble + 0.07 * level + 0.05 * Math.max(0, s.jig.x)) * size;
	return {
		stage,
		g,
		ax: a.ax,
		ay: a.ay,
		anchorY: a.anchorY,
		rot: s.rot.x,
		sx: 1 / Math.sqrt(sy),
		sy,
		wobble,
		// Wrapped, so the shader's 32-bit floats keep their precision however
		// long the app runs.
		phases: [(1.3 * T + s.ph) % TAU, (-1.9 * T + 1) % TAU, (2.7 * T + 2) % TAU],
		arms: SIDES.map((sd) => {
			const arm = s.arms[sd];
			return [arm.sx.x, arm.sy.x, arm.hx.x, arm.hy.x] as [number, number, number, number];
		}),
		lampToken: lampToken(input),
		lampStrength: lampStrength(input.state, s.glow, input.wakeArmed),
		glow: s.glow,
		eyes: {
			lt: s.lt.x,
			lb: s.lb.x,
			tilt: s.tilt.x,
			sc: s.sc.x,
			gx: s.gx.x,
			gy: s.gy.x,
			blink: s.blink
		},
		// The notepad comes with the gesture that writes on it.
		pad: s.writing ? pad(g) : null,
		write: s.write,
		flipping: s.flip > 0,
		flip: s.flip > 0 ? 1 - s.flip / FLIP_SECONDS : 0,
		rain:
			input.state === 'command'
				? {
						phase: s.rainPhase,
						// Time in the state, which stays small for the shader.
						T: s.T - s.stateSince,
						bright: 0.45 + 0.55 * s.glow,
						top: -g.ry - 4,
						span: (g.flat ?? g.ry) + g.ry + 4 + 12
					}
				: null,
		reduced
	};
}
