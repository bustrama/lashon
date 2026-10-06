// Where the creature stands, in design units (docs/design-system.md: one unit
// is 1.5 CSS px at the default scale). The numbers are the prototypes'
// (docs/design/ottid/states.html), written relative to the body's radii so a
// creature with a different body keeps its proportions.
import type { CreatureData } from '../data';
import type { Placement } from '../types';

export type Vec2 = [number, number];
/** -1 is the creature's left (screen left), 1 its right. */
export type Side = -1 | 1;
export const SIDES: readonly Side[] = [-1, 1];

/** The prototypes' body, which the relative constants below were taken from. */
const REF_RX = 29;
const REF_RY = 20.2;

/** The taskbar floor, below the body's centre (11 units on the default). */
const FLOOR_FRACTION = 11 / REF_RY;
/** Hanging stretches the body a little. */
const HANG_STRETCH = 1.08;
/** Gap between the hanging body's top and the ceiling line. */
const HANG_GAP = 11;

export interface Geometry {
	placement: Placement;
	hang: boolean;
	rx: number;
	/** Half-height, stretched when hanging. */
	ry: number;
	/** The ceiling line in body-local y (hanging only). */
	ceil: number;
	/** The taskbar floor in body-local y, or null off the taskbar. */
	flat: number | null;
}

/** The placement's geometry at time `T` (the hanging body sways up and down). */
export function geometry(creature: CreatureData, placement: Placement, T: number, ph: number): Geometry {
	const hang = placement === 'ceiling';
	const rx = creature.body.radius_x;
	const ry = hang ? creature.body.radius_y * HANG_STRETCH : creature.body.radius_y;
	return {
		placement,
		hang,
		rx,
		ry,
		ceil: -(ry + HANG_GAP + 3 * Math.sin(T * 1.3 + ph)),
		flat: placement === 'taskbar' ? creature.body.radius_y * FLOOR_FRACTION : null
	};
}

/** Where an arm leaves the body. */
export function shoulder(sd: Side, g: Geometry): Vec2 {
	return g.hang ? [sd * g.rx * (21 / REF_RX), -g.ry * 0.35] : [sd * g.rx * (24 / REF_RX), 1];
}

/** A hand `L` units from its shoulder at elevation `e` (radians, up is positive). */
export function reach(sd: Side, e: number, L: number, g: Geometry): Vec2 {
	const [x, y] = shoulder(sd, g);
	return [x + sd * L * Math.cos(e), y - L * Math.sin(e)];
}

/** Where a hand rests when no gesture uses it. */
export function rest(sd: Side, T: number, g: Geometry, ph: number): Vec2 {
	if (g.hang) return [sd * g.rx * (31 / REF_RX), g.ceil + 2];
	if (g.placement === 'float') return reach(sd, -0.2 + 0.25 * Math.sin(T * 2.2 + sd * 0.8 + ph), 14, g);
	return reach(sd, -0.6, 13, g);
}

/** The dictation notepad, held at the creature's right side (body-local units). */
export interface Pad {
	cx: number;
	cy: number;
	w: number;
	h: number;
	rot: number;
	lines: number;
}

export function pad(g: Geometry): Pad {
	return { cx: g.rx * (23 / REF_RX), cy: -1, w: 17, h: 21, rot: 0.12, lines: 5 };
}

/** A point on the pad, from its unit square (u right, v down). */
export function padPoint(p: Pad, u: number, v: number): Vec2 {
	const x = (u - 0.5) * p.w;
	const y = (v - 0.5) * p.h;
	const c = Math.cos(p.rot);
	const s = Math.sin(p.rot);
	return [p.cx + x * c - y * s, p.cy + x * s + y * c];
}

/** The pencil's direction, tip to eraser, and where the palm grips it. */
export const PEN_DIR: Vec2 = [Math.cos(-0.95), Math.sin(-0.95)];
export const PEN_GRIP = 5.6;
export const PEN_LENGTH = 17;

/** The pencil tip's spot on the page for the writing progress `write`. */
export function penTip(p: Pad, write: number, flipping: boolean, T: number): Vec2 {
	const line = Math.min(p.lines - 1, Math.floor(write));
	const frac = flipping ? 0 : write - Math.floor(write);
	return padPoint(p, 0.16 + 0.68 * frac + 0.012 * Math.sin(T * 31), 0.32 + line * 0.13 + 0.012 * Math.sin(T * 23));
}

/** The palm holding the pencil whose tip is at `tip`. */
export function penPalm(tip: Vec2): Vec2 {
	return [tip[0] + PEN_DIR[0] * PEN_GRIP, tip[1] + PEN_DIR[1] * PEN_GRIP];
}

/** How the body sits in the stage, in CSS px. */
export interface Anchor {
	/** The stage point the body's local `anchorY` maps to. */
	ax: number;
	ay: number;
	/** Body-local y pinned to (ax, ay): the floor, the ceiling or the centre. */
	anchorY: number;
}

/** The float placement's resting height, as a fraction of the stage. */
const FLOAT_Y = 0.53;

/**
 * The anchor for a stage `w` × `h` CSS px at `unit` px per design unit. The
 * stage contract (lib/creature/types.ts): the taskbar floor is the stage's
 * bottom edge, the ceiling line its top edge, and float is free inside.
 */
export function anchor(
	g: Geometry,
	w: number,
	h: number,
	unit: number,
	hop: number,
	bob: number
): Anchor {
    const ax = g.placement === 'left' ? g.rx * unit + 4 : g.placement === 'right' ? w - g.rx * unit - 4 : w / 2;
	if (g.hang) return { ax, ay: 0, anchorY: g.ceil };
	if (g.flat !== null) return { ax, ay: h + Math.min(0, hop) * unit, anchorY: g.flat };
	return { ax, ay: h * FLOAT_Y + (bob + hop) * unit, anchorY: 0 };
}

/**
 * The body's resting box in stage CSS px (the hit area), generous enough to
 * cover the hop, bob and swing without following them every frame.
 */
export function restingBox(
	creature: CreatureData,
	placement: Placement,
	w: number,
	h: number,
	unit: number
): { x: number; y: number; width: number; height: number } {
	const g = geometry(creature, placement, 0, 0);
	const a = anchor(g, w, h, unit, 0, 0);
	const margin = 3;
	const halfW = (g.rx + margin) * unit;
	const top = a.ay + (-g.ry - margin - a.anchorY) * unit;
	const bottom = g.flat !== null ? a.ay : a.ay + (g.ry + margin - a.anchorY) * unit;
	return { x: a.ax - halfW, y: top, width: halfW * 2, height: bottom - top };
}
