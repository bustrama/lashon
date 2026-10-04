// A creature as data (docs/adr/0041). This mirrors `ottid_core::creature`'s
// types, which are the source of the published JSON Schema
// (creatures/schema/ottid-creature.schema.json). The Rust validator is the
// only gate a creature file passes; the engine never parses, evaluates or
// fetches anything from one.
import type { CreatureState } from './types';
import ottid from '../../../../../creatures/ottid/creature.json';

/** The twelve states, in the design system's order. */
export const CREATURE_STATES: readonly CreatureState[] = [
	'idle',
	'preparing',
	'dictation',
	'command',
	'transcribing',
	'thinking',
	'tool',
	'confirm',
	'wake',
	'error',
	'agent-needs-you',
	'agent-done'
];

/** The engine's gesture library, by the names a creature file uses. */
export const GESTURES = [
	'rest',
	'greet',
	'stretch',
	'write',
	'ready',
	'scribble',
	'scratch-head',
	'hammer',
	'offer',
	'startle',
	'droop',
	'beckon',
	'cheer'
] as const;
export type GestureName = (typeof GESTURES)[number];

/** Body colours, by design-token name. */
export type BodyColour = 'charcoal';
/** Eye colours, by design-token name. */
export type EyeColour = 'peach';

export interface CreatureData {
	schema: 1;
	id: string;
	name: { he: string; en: string };
	body: { radius_x: number; radius_y: number; wobble: number; colour: BodyColour };
	hands: { arm_radius: number; palm_radius: number; arm_softness: number; palm_softness: number };
	eyes: {
		style: 'ember';
		colour: EyeColour;
		radius_x: number;
		radius_y: number;
		/** Each eye's distance from the midline. */
		x: number;
		y: number;
	};
	/** Where the lamp sits. Its colour and strength are the engine's. */
	lamp: { x: number; y: number; radius: number };
	poses: Record<CreatureState, GestureName>;
}

/**
 * The bundled default creature. It is part of the app's own bundle, the same
 * file `ottid-core` compiles in and its tests validate, so it needs no
 * runtime check here. User creatures will arrive already validated by Rust.
 */
export const DEFAULT_CREATURE = ottid as CreatureData;

/** Design tokens behind the colour names a creature file may use. */
export const BODY_COLOUR_TOKEN: Record<BodyColour, string> = { charcoal: '--creature-charcoal' };
export const EYE_COLOUR_TOKEN: Record<EyeColour, string> = { peach: '--peach' };
