// Collapse the raw lifecycle events into one creature state. The frontend
// holds no dictation state of its own (.claude/rules/frontend.md): these
// inputs are a render cache of the latest Rust events.
import type { DictationState } from '$lib/dictation';
import type { CreatureState } from './types';

export type TakeMode = 'idle' | 'dictation' | 'command';
export type CommandState = 'idle' | 'thinking' | 'tool';

export interface StateInputs {
	dictation: DictationState;
	takeMode: TakeMode;
	commandState: CommandState;
	/** A command waits for the user's approval. */
	confirming: boolean;
	/** The wake word was just heard (a short event). */
	woke: boolean;
}

/**
 * Most urgent first: an approval outranks everything, the working phases
 * outrank the listening that produced them, and the wake event shows over
 * the start of the take it triggers.
 */
export function creatureState(i: StateInputs): CreatureState {
	if (i.confirming) return 'confirm';
	if (i.commandState === 'tool') return 'tool';
	if (i.commandState === 'thinking') return 'thinking';
	if (i.dictation === 'transcribing') return 'transcribing';
	if (i.woke) return 'wake';
	if (i.dictation === 'capturing') return i.takeMode === 'command' ? 'command' : 'dictation';
	if (i.dictation === 'preparing') return 'preparing';
	if (i.dictation === 'error') return 'error';
	return 'idle';
}

/** States in which the voice level drives the creature. */
export function listens(state: CreatureState): boolean {
	return state === 'dictation' || state === 'command';
}
