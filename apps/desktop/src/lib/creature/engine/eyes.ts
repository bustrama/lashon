// Where the eyes go. The engine scales them (wide in a startle) and shifts
// them toward the gaze; the lamp must never end up under one (docs/adr/0040),
// so the validator checks a creature's lamp against every place an eye can
// reach. These limits are that contract: `ottid_core::creature` publishes them
// in the schema (`x-ottid-eye-motion`), and data.test.ts keeps the two equal.
import type { CreatureData } from '../data';
import type { Side } from './geometry';

export const EYE_MOTION = {
	/** The widest an eye gets: a startle. Spring overshoot is clipped to it. */
	scale_max: 1.25,
	/** How far an eye shifts toward the gaze, per unit of its radius_x … */
	shift_x: 0.72,
	/** … and of its radius_y. */
	shift_y: 0.4,
	/** The gaze bends through sin(gaze · gaze_bend), the gaze clamped to ±1. */
	gaze_bend: 0.9
} as const;

/** What the face springs say about the eyes this frame (`Frame['eyes']`). */
export interface EyePose {
	/** Scale. */
	sc: number;
	/** Gaze, about −1..1 (springs may overshoot). */
	gx: number;
	gy: number;
}

export interface EyeShape {
	/** Centre and radii, in body-local units. */
	x: number;
	y: number;
	rx: number;
	ry: number;
}

const clampUnit = (v: number) => Math.max(-1, Math.min(1, Number.isFinite(v) ? v : 0));

/** One eye as it is drawn this frame. */
export function placeEye(eyes: CreatureData['eyes'], pose: EyePose, side: Side): EyeShape {
	const sc = Math.min(pose.sc, EYE_MOTION.scale_max);
	const gx = Math.sin(clampUnit(pose.gx) * EYE_MOTION.gaze_bend);
	const gy = Math.sin(clampUnit(pose.gy) * EYE_MOTION.gaze_bend);
	return {
		x: side * eyes.x + EYE_MOTION.shift_x * eyes.radius_x * gx,
		y: eyes.y + EYE_MOTION.shift_y * eyes.radius_y * gy,
		// Looking aside narrows the eye a little; up or down flattens it.
		rx: eyes.radius_x * sc * (1 - 0.22 * Math.abs(gx)),
		ry: eyes.radius_y * sc * (1 - 0.15 * Math.abs(gy))
	};
}
