// The Hub's Claude Code section (docs/adr/0050), kept free of the DOM so it
// is unit-tested: what the shell's `agent_hooks_*` commands return, and what
// the section makes of it.

export type HookAction = 'install' | 'uninstall';

/** `agent_hooks_status` (`Status` in agent_hooks.rs). */
export interface AgentHooksStatus {
	/** Claude Code's user settings file. */
	settings_path: string;
	/** The hook binary, if this build has one. */
	hook_exe: string | null;
	/** Ottid's hook is in the settings file. */
	installed: boolean;
	/** ...and runs this build's hook binary. */
	current: boolean;
	/** `disableAllHooks` is on in the settings file. */
	hooks_disabled: boolean;
	/** The listener is running. */
	listening: boolean;
	/** Why the settings file can't be read, if it can't. */
	error: string | null;
}

/** `agent_hooks_preview` (`Preview` in agent_hooks.rs). */
export interface AgentHooksPreview {
	settings_path: string;
	/** The file's state now; the change is applied only to this. */
	fingerprint: string;
	/** The file exists, so it is backed up first. */
	exists: boolean;
	/** The matcher group added, as it will be written, or null. */
	added: string | null;
	/** Ottid's handlers taken out. */
	removed: number;
	/** Whether the file changes at all. */
	changes: boolean;
}

/** `agent_hooks_apply` (`Applied` in claude_settings.rs). */
export interface AgentHooksApplied {
	/** The copy of the file from before, if there was a file. */
	backup: string | null;
	changed: boolean;
}

/**
 * Where the section stands:
 * - `unreadable`: the settings file can't be read, so nothing is offered.
 * - `missing`: this build has no hook binary, and nothing is installed.
 * - `off`: not installed.
 * - `stale`: installed, but for another copy of Ottid; installing again
 *   points it at this one.
 * - `on`: installed for this copy.
 */
export type HookState = 'unreadable' | 'missing' | 'off' | 'stale' | 'on';

export function hookState(status: AgentHooksStatus): HookState {
	if (status.error !== null) return 'unreadable';
	if (status.installed) return status.current ? 'on' : 'stale';
	return status.hook_exe === null ? 'missing' : 'off';
}

/** Install (or point at this copy) is offered: there is a hook binary to point at. */
export function canInstall(status: AgentHooksStatus): boolean {
	const state = hookState(status);
	return (state === 'off' || state === 'stale') && status.hook_exe !== null;
}

/** Uninstall is offered. */
export function canUninstall(status: AgentHooksStatus): boolean {
	const state = hookState(status);
	return state === 'on' || state === 'stale';
}

// The shell's error codes the section explains; anything else is shown as
// the generic failure.
const KNOWN_ERRORS = new Set([
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
]);

/** The i18n key that explains a shell error code. */
export function errorKey(code: unknown): string {
	const text = typeof code === 'string' ? code : '';
	return KNOWN_ERRORS.has(text) ? `hub.agents.error.${text}` : 'hub.agents.error.other';
}
