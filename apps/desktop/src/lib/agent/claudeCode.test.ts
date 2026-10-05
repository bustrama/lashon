import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { locale, t } from '$lib/i18n';
import {
	canInstall,
	canUninstall,
	errorKey,
	hookState,
	type AgentHooksStatus
} from './claudeCode';

function status(over: Partial<AgentHooksStatus> = {}): AgentHooksStatus {
	return {
		settings_path: 'C:\\Users\\דנה\\.claude\\settings.json',
		hook_exe: 'C:\\Program Files\\Ottid\\binaries\\ottid-hook\\ottid-hook.exe',
		installed: false,
		current: false,
		hooks_disabled: false,
		listening: false,
		error: null,
		...over
	};
}

describe('hookState', () => {
	it('reads the status', () => {
		expect(hookState(status())).toBe('off');
		expect(hookState(status({ installed: true, current: true }))).toBe('on');
		expect(hookState(status({ installed: true }))).toBe('stale');
		expect(hookState(status({ hook_exe: null }))).toBe('missing');
		// Installed by another copy, with none here: it can still be removed.
		expect(hookState(status({ hook_exe: null, installed: true }))).toBe('stale');
		// A file it can't read wins over everything else.
		expect(hookState(status({ installed: true, current: true, error: 'not-json' }))).toBe(
			'unreadable'
		);
	});

	it('offers install and uninstall only where they make sense', () => {
		const both = (s: AgentHooksStatus) => [canInstall(s), canUninstall(s)];
		expect(both(status())).toEqual([true, false]);
		expect(both(status({ installed: true, current: true }))).toEqual([false, true]);
		expect(both(status({ installed: true }))).toEqual([true, true]);
		expect(both(status({ hook_exe: null }))).toEqual([false, false]);
		// Another copy's hook, none here: remove it, but nothing to point at.
		expect(both(status({ hook_exe: null, installed: true }))).toEqual([false, true]);
		expect(both(status({ installed: true, error: 'io' }))).toEqual([false, false]);
	});
});

describe('errorKey', () => {
	it('explains every code the shell sends, in both languages', () => {
		const codes = [
			'no-home',
			'no-hook-binary',
			'not-the-hub',
			'not-json',
			'not-object',
			'hooks-not-object',
			'event-not-list',
			'duplicate-key',
			'not-text',
			'changed',
			'io'
		];
		for (const lang of ['he', 'en'] as const) {
			locale.set(lang);
			const tr = get(t);
			for (const code of [...codes, 'something-new']) {
				const key = errorKey(code);
				expect(tr(key)).not.toBe(key);
			}
		}
		locale.set('he');
	});

	it('falls back to the generic message', () => {
		expect(errorKey('bad-action')).toBe('hub.agents.error.other');
		expect(errorKey(new Error('ipc'))).toBe('hub.agents.error.other');
		expect(errorKey(undefined)).toBe('hub.agents.error.other');
		expect(errorKey('changed')).toBe('hub.agents.error.changed');
	});
});
