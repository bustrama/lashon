import { describe, expect, it } from 'vitest';
import { creatureState, type StateInputs } from './state';

const quiet: StateInputs = {
	dictation: 'idle',
	takeMode: 'idle',
	commandState: 'idle',
	confirming: false,
	woke: false
};

describe('creatureState', () => {
	it('rests when nothing is happening', () => {
		expect(creatureState(quiet)).toBe('idle');
	});

	it('shows the aqua "needs you" state while an approval waits', () => {
		expect(creatureState({ ...quiet, confirming: true })).toBe('agent-needs-you');
	});

	it('puts a pending approval above every other phase', () => {
		const busy: StateInputs = {
			dictation: 'capturing',
			takeMode: 'command',
			commandState: 'tool',
			confirming: true,
			woke: true
		};
		expect(creatureState(busy)).toBe('agent-needs-you');
		expect(creatureState({ ...busy, commandState: 'thinking' })).toBe('agent-needs-you');
	});

	it('goes back to the take once the approval is answered', () => {
		expect(creatureState({ ...quiet, commandState: 'tool' })).toBe('tool');
		expect(creatureState({ ...quiet, commandState: 'thinking' })).toBe('thinking');
	});

	it('tells dictation from command while listening', () => {
		expect(creatureState({ ...quiet, dictation: 'capturing', takeMode: 'dictation' })).toBe(
			'dictation'
		);
		expect(creatureState({ ...quiet, dictation: 'capturing', takeMode: 'command' })).toBe(
			'command'
		);
	});

	it('shows the wake event over the start of its take', () => {
		expect(creatureState({ ...quiet, dictation: 'capturing', woke: true })).toBe('wake');
		expect(creatureState({ ...quiet, dictation: 'transcribing', woke: true })).toBe(
			'transcribing'
		);
	});
});
