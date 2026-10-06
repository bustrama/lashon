import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Island from './Island.svelte';

describe('concurrent island cards', () => {
	for (const side of ['above', 'below'] as const) {
		it(`keeps Hebrew dictation and multiple named sessions visible ${side}`, () => {
			const { body } = render(Island, { props: {
				side, listening: true, takeMode: 'dictation',
				partial: { committed: 'הטקסט שאני אומר', provisional: 'ממשיך' },
				commandState: 'idle', commandToolName: null, commandToolLabel: null,
				commandTranscript: null, commandCancellable: false, commandFlash: null,
				agentSessions: [
					{ id: 1, agent: 'Codex', state: 'working', tool: 'Read', title: 'בדיקת אוטיד', project: 'ottid' },
					{ id: 2, agent: 'Claude', state: 'working', tool: null }
				],
				approval: { id: 3, source: 'command', tool: 'run_command', command: 'echo test',
					cwd: 'ottid', waiting: 0, expires_in_ms: 30000,
					keys: { allow: ['Ctrl', 'Y'], deny: ['Ctrl', 'N'] } },
				approvalNudge: null, onApprovalArmed: () => {}, onApprovalAnswer: () => {}, onCancel: () => {}
			} });
			expect(body).toContain('הטקסט שאני אומר');
			expect(body).toContain('בדיקת אוטיד');
			expect(body).toContain('Claude · #2');
			expect(body).toContain('card-approval');
			expect(body).toContain('bubble-partial');
			expect(body.indexOf('session-stack')).toBeLessThan(body.indexOf('foreground-stack'));
		});
	}
});
