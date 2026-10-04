// The lamp: a glow inside the body in the state's colour. It is the privacy
// signal (docs/adr/0040), so it belongs to the engine. Nothing here reads the
// creature: a creature only chooses where the lamp sits (docs/adr/0041).
import type { CreatureState } from '../types';

/** Lamp strength at rest. */
export const LAMP_IDLE = 0.18;
/** The floor in every other state: while Ottid listens, never below 30%. */
export const LAMP_MIN = 0.3;
export const LAMP_MAX = 0.85;

export interface LampInput {
	state: CreatureState;
	mode: 'dictation' | 'command' | null;
	wakeArmed: boolean;
}

/** The design token for the lamp's colour (docs/design-system.md, "States"). */
export function lampToken({ state, mode, wakeArmed }: LampInput): string {
	switch (state) {
		case 'idle':
			// Armed for the wake word, the microphone is open: show it.
			return wakeArmed ? '--state-cloud' : '--peach';
		case 'preparing':
		case 'dictation':
			return '--saffron';
		case 'command':
		case 'tool':
			return '--garnet';
		case 'transcribing':
		case 'thinking':
			return '--state-cloud';
		case 'confirm':
			return '--peach';
		case 'wake':
			return mode === 'command' ? '--garnet' : '--saffron';
		case 'error':
			return '--state-error';
		case 'agent-needs-you':
			return '--aqua';
		case 'agent-done':
			return '--state-success';
	}
}

export interface GlowInput {
	state: CreatureState;
	/** Seconds. */
	T: number;
	/** The state's cycle phase, 0..1 (see pose.ts). */
	u: number;
	/** Voice level, 0..1. */
	level: number;
	reduced: boolean;
}

/** Steady glow per state when the user asked for less motion. */
const STEADY: Record<CreatureState, number> = {
	idle: 0,
	preparing: 0.5,
	dictation: 0.6,
	command: 0.6,
	transcribing: 0.35,
	thinking: 0.45,
	tool: 0.45,
	confirm: 0.6,
	wake: 0.45,
	error: 0.3,
	'agent-needs-you': 0.7,
	'agent-done': 0.25
};

/** How lit the lamp wants to be, 0..1: pulses, flashes and the voice. */
export function glow({ state, T, u, level, reduced }: GlowInput): number {
	if (reduced) return STEADY[state];
	switch (state) {
		case 'idle':
			return 0;
		case 'preparing':
			return 0.12 + 0.5 * u;
		case 'dictation':
		case 'command':
			return 0.3 + 0.7 * level;
		case 'transcribing':
			return 0.35 + 0.25 * Math.sin(T * 4);
		case 'thinking':
			return 0.3 + 0.3 * (0.5 + 0.5 * Math.sin(T * 2.6));
		case 'tool':
			return 0.45 + 0.2 * Math.sin(T * 10);
		case 'confirm':
			return 0.6 + 0.1 * Math.sin(T * 2);
		case 'wake':
			return u < 0.4 ? 1 - u * 1.2 : 0.45;
		case 'error':
			return Math.max(0.3, 1 - u * 2.5);
		case 'agent-needs-you':
			return 0.55 + 0.45 * Math.sin(T * 6.5);
		case 'agent-done':
			return Math.max(0.25, 1 - u * 1.6);
	}
}

/**
 * The lamp's strength: LAMP_IDLE at rest, and between LAMP_MIN and LAMP_MAX
 * in every other state, whatever the glow program asks for.
 */
export function lampStrength(state: CreatureState, glowValue: number, wakeArmed: boolean): number {
	if (state === 'idle' && !wakeArmed) return LAMP_IDLE;
	const g = Math.min(1, Math.max(0, Number.isFinite(glowValue) ? glowValue : 0));
	// Weighted this way, the ends land exactly on LAMP_MIN and LAMP_MAX.
	return LAMP_MIN * (1 - g) + LAMP_MAX * g;
}
