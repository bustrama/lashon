// State → pose. The engine owns which state shows and its timing; the
// creature only picks the gesture for each state (docs/adr/0041).
import type { CreatureData, GestureName } from '../data';
import type { CreatureState } from '../types';
import { GESTURE_LIBRARY, type Gesture, type GestureContext, type Pose } from './gestures';

export interface Cycle {
	/** Seconds per cycle. */
	period: number;
	/** An event: it plays once from the moment the state starts, then holds. */
	once: boolean;
	/** The key pose's phase, shown still when the user asked for less motion. */
	still: number;
}

/** Each state's rhythm (docs/design/ottid/states.html). */
export const CYCLES: Record<CreatureState, Cycle> = {
	idle: { period: 7, once: false, still: 0.5 },
	preparing: { period: 2.6, once: false, still: 0.3 },
	dictation: { period: 4, once: false, still: 0.5 },
	command: { period: 4, once: false, still: 0.5 },
	transcribing: { period: 4, once: false, still: 0.5 },
	thinking: { period: 4, once: false, still: 0.5 },
	tool: { period: 4, once: false, still: 0.5 },
	confirm: { period: 4, once: false, still: 0.5 },
	wake: { period: 2.8, once: true, still: 0.6 },
	error: { period: 3, once: true, still: 0.5 },
	'agent-needs-you': { period: 1.5, once: false, still: 0.5 },
	'agent-done': { period: 3, once: true, still: 0.3 }
};

/**
 * The phase of `state` after `elapsed` seconds in it: an event runs from 0
 * to 1 and holds there; a repeating state wraps. `ph` staggers repeating
 * cycles so several creatures in one view don't move in lockstep.
 */
export function cyclePhase(state: CreatureState, elapsed: number, ph = 0, reduced = false): number {
	const c = CYCLES[state];
	if (reduced) return c.still;
	const t = Math.max(0, elapsed);
	if (c.once) return Math.min(1, t / c.period);
	const u = ((t + ph) % c.period) / c.period;
	return u < 0 ? u + 1 : u;
}

/** The gesture `creature` makes in `state`. */
export function gestureFor(creature: CreatureData, state: CreatureState): Gesture {
	return GESTURE_LIBRARY[gestureName(creature, state)];
}

export function gestureName(creature: CreatureData, state: CreatureState): GestureName {
	const name = creature.poses[state];
	// The validator guarantees a known name; resting is the safe reading of
	// anything else, never an error on the privacy surface.
	return Object.prototype.hasOwnProperty.call(GESTURE_LIBRARY, name) ? name : 'rest';
}

/** The pose for `state`. Pure: the same inputs give the same pose. */
export function poseFor(creature: CreatureData, state: CreatureState, c: GestureContext): Pose {
	return gestureFor(creature, state).pose(c);
}
