<!--
	Hub "Coding agents" section (docs/adr/0050): connects Claude Code's
	permission requests to Ottid's approval card by adding Ottid's hook to
	Claude Code's user settings, and takes it out again.

	Nothing changes without the user seeing it first: each action shows a
	preview of exactly what is added or removed, in which file, and the
	change is made only on confirmation, and only to the file as it was
	previewed (the fingerprint). The shell backs the file up first.
-->
<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { onMount, tick } from 'svelte';
	import { t } from '$lib/i18n';
	import { fill, pieces } from '$lib/overlay/approval';
	import {
		canInstall,
		canUninstall,
		errorKey,
		hookState,
		type AgentHooksApplied,
		type AgentHooksPreview,
		type AgentHooksStatus,
		type HookAction
	} from './claudeCode';

	let { agent = 'claude' }: { agent?: 'claude' | 'codex' } = $props();
	const agentName = $derived(agent === 'codex' ? 'Codex' : 'Claude Code');
	const BUILD_HOOK = $derived(`cargo build -p ottid-core --bin ${agent === 'codex' ? 'ottid-codex-hook' : 'ottid-hook'}`);
	function agentText(key: string): string {
		if (agent === 'codex' && key === 'hub.agents.requirements') return $t('hub.agents.codexRequirements');
		if (agent === 'codex' && key === 'hub.agents.scope') return $t('hub.agents.codexScope');
		if (agent === 'codex' && key === 'hub.agents.hooksDisabled') return $t('hub.agents.codexDisabled');
		const value = $t(key);
		return agent === 'codex' ? value.replaceAll('Claude Code', 'Codex') : value;
	}

	let status = $state<AgentHooksStatus | null>(null);
	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let preview = $state<{ action: HookAction; data: AgentHooksPreview } | null>(null);
	let busy = $state(false);
	// An i18n key.
	let actionError = $state<string | null>(null);
	let toast = $state<{ kind: 'success' | 'warn'; lines: string[] } | null>(null);
	let toastTimer: ReturnType<typeof setTimeout> | null = null;
	let previewHeading = $state<HTMLHeadingElement | null>(null);
	let actions = $state<HTMLDivElement | null>(null);

	const hook = $derived(status ? hookState(status) : null);

	function flashToast(kind: 'success' | 'warn', lines: string[]) {
		toast = { kind, lines };
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => (toast = null), 6_000);
	}

	onMount(() => {
		void load();
		return () => {
			if (toastTimer) clearTimeout(toastTimer);
		};
	});

	async function load() {
		loadError = null;
		try {
			status = await invoke<AgentHooksStatus>('agent_hooks_status', { agent });
		} catch (err) {
			loadError = errorKey(err);
		} finally {
			loading = false;
		}
	}

	async function ask(action: HookAction) {
		busy = true;
		actionError = null;
		try {
			const data = await invoke<AgentHooksPreview>('agent_hooks_preview', { action, agent });
			preview = { action, data };
			await tick();
			previewHeading?.focus();
		} catch (err) {
			actionError = errorKey(err);
		} finally {
			busy = false;
		}
	}

	async function confirm() {
		if (!preview) return;
		const { action, data } = preview;
		busy = true;
		actionError = null;
		let done = false;
		try {
			const applied = await invoke<AgentHooksApplied>('agent_hooks_apply', {
				agent,
				action,
				fingerprint: data.fingerprint
			});
			preview = null;
			const lines = [agentText(`hub.agents.toast.${action === 'install' ? 'installed' : 'uninstalled'}`)];
			if (applied.backup) lines.push(fill(agentText('hub.agents.toast.backup'), { path: applied.backup }));
			flashToast('success', lines);
			await load();
			done = true;
		} catch (err) {
			if (err === 'changed') {
				// The file changed since the preview: show the change again,
				// against the file as it is now.
				await ask(action);
			}
			actionError = errorKey(err);
		} finally {
			busy = false;
		}
		if (done) await focusActions();
	}

	async function cancel() {
		preview = null;
		actionError = null;
		await focusActions();
	}

	// Back to the section's buttons once the preview closes.
	async function focusActions() {
		await tick();
		actions?.querySelector('button')?.focus();
	}
</script>

<!-- Paths and the added entry read left to right, each non-ASCII word in
     its own isolate, hidden characters by code point (`pieces`). -->
{#snippet text(value: string)}{#each pieces(value) as piece, i (i)}{#if piece.kind === 'text'}{piece.text}{:else if piece.kind === 'isolate'}<bdi>{piece.text}</bdi>{:else}<span class="hidden-char" title={fill(agentText('approval.hiddenCharTitle'), { code: piece.code })}>{piece.code}</span>{/if}{/each}{/snippet}

<section>
	<h2 class="section-head">
		<span class="section-title he-display">{agentText('hub.agents.title')}</span>
		<span class="section-en lat">· Coding agents</span>
	</h2>
	<p class="he-sans intro" dir="auto">{agentText('hub.agents.intro')}</p>

	<div class="agent">
		<div class="agent-head">
			<span
				class="lamp"
				class:live={hook === 'on' && status?.listening}
				aria-hidden="true"
			></span>
			<span class="agent-name lat">{agentName}</span>
		</div>

		{#if loading}
			<p class="he-sans muted">{agentText('hub.agents.loading')}</p>
		{:else if loadError || !status || !hook}
			<p class="he-sans error" role="alert">{agentText(loadError ?? 'hub.agents.error.other')}</p>
		{:else}
			<p class="he-sans state" role="status">{agentText(`hub.agents.state.${hook}`)}</p>
			{#if hook === 'unreadable'}
				<p class="he-sans error">{text(errorKey(status.error))}</p>
			{/if}
			{#if hook === 'missing'}
				<p class="he-sans hint">
					{agentText('hub.agents.missingHint')}
					<code class="mono" dir="ltr">{BUILD_HOOK}</code>
				</p>
			{/if}
			{#if hook === 'on' && !status.listening}
				<p class="he-sans warn">{agentText('hub.agents.notListening')}</p>
			{/if}
			{#if status.hooks_disabled}
				<p class="he-sans warn">{agentText('hub.agents.hooksDisabled')}</p>
			{/if}

			<dl class="facts">
				<dt class="he-sans">{agentText('hub.agents.settingsFile')}</dt>
				<dd><code class="mono path" dir="ltr">{@render text(status.settings_path)}</code></dd>
			</dl>
			<ul class="notes he-sans">
				<li>{agentText('hub.agents.scope')}</li>
				<li>{agentText('hub.agents.requirements')}</li>
				<li>{agentText('hub.agents.once')}</li>
				<li>{agentText('hub.agents.privacy')}</li>
			</ul>

			{#if !preview}
				<div class="actions" bind:this={actions}>
					{#if canInstall(status)}
						<button
							type="button"
							class="primary he-sans"
							onclick={() => void ask('install')}
							disabled={busy}
						>
							{text(hook === 'stale' ? 'hub.agents.reinstall' : 'hub.agents.install')}
						</button>
					{/if}
					{#if canUninstall(status)}
						<button
							type="button"
							class="secondary he-sans"
							onclick={() => void ask('uninstall')}
							disabled={busy}
						>
							{agentText('hub.agents.uninstall')}
						</button>
					{/if}
				</div>
			{/if}

			{#if actionError}
				<p class="he-sans error" role="alert">{agentText(actionError)}</p>
			{/if}

			{#if preview}
				<div class="preview" role="region" aria-labelledby={`agents-preview-title-${agent}`}>
					<h3 id={`agents-preview-title-${agent}`} class="he-sans" tabindex="-1" bind:this={previewHeading}>
						{agentText(`hub.agents.preview.${preview.action}`)}
					</h3>
					{#if !preview.data.changes}
						<p class="he-sans">{agentText('hub.agents.preview.noChange')}</p>
					{:else}
						<p class="he-sans">
							{agentText('hub.agents.preview.file')}
							<code class="mono path" dir="ltr">{@render text(preview.data.settings_path)}</code>
						</p>
						{#if preview.data.added !== null}
							<p class="he-sans">
								{agentText('hub.agents.preview.where')}
								<code class="mono" dir="ltr">hooks.PermissionRequest</code>:
							</p>
							<pre class="mono added" dir="ltr">{@render text(preview.data.added)}</pre>
						{/if}
						{#if preview.data.removed > 0}
							<p class="he-sans">
								{fill(
									agentText(
										preview.action === 'install'
											? 'hub.agents.preview.replaces'
											: 'hub.agents.preview.removes'
									),
									{ n: preview.data.removed }
								)}
							</p>
						{/if}
						<p class="he-sans">
							{text(preview.data.exists ? 'hub.agents.preview.backup' : 'hub.agents.preview.newFile')}
						</p>
						<p class="he-sans">{agentText('hub.agents.preview.nothingElse')}</p>
					{/if}
					<div class="actions">
						{#if preview.data.changes}
							<button
								type="button"
								class="primary he-sans"
								onclick={() => void confirm()}
								disabled={busy}
							>
								{busy
									? agentText('hub.agents.preview.applying')
									: text(
											preview.action === 'install'
												? 'hub.agents.preview.confirmInstall'
												: 'hub.agents.preview.confirmUninstall'
										)}
							</button>
						{/if}
						<button
							type="button"
							class="secondary he-sans"
							onclick={() => void cancel()}
							disabled={busy}
						>
							{agentText('hub.agents.preview.cancel')}
						</button>
					</div>
				</div>
			{/if}
		{/if}
	</div>

	{#if toast}
		<div class="toast {toast.kind}" role="status">
			{#each toast.lines as line, i (i)}
				<span class="he-sans" dir="auto">{line}</span>
			{/each}
		</div>
	{/if}
</section>

<style>
	.section-head {
		display: flex;
		align-items: baseline;
		gap: 10px;
		margin: 0 0 6px;
	}
	.section-title {
		font-size: 24px;
		font-weight: 500;
		color: var(--ink-text);
	}
	.section-en {
		font-size: 13px;
		color: var(--ink-faint);
		font-style: italic;
	}
	.intro {
		font-size: 13.5px;
		color: var(--ink-mute);
		line-height: 1.55;
		max-width: 640px;
		margin: 0 0 18px;
	}
	.agent {
		max-width: 720px;
		padding: 16px 18px;
		border-radius: 10px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.agent-head {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-bottom: 8px;
	}
	.lamp {
		width: 9px;
		height: 9px;
		border-radius: 50%;
		background: var(--ink-faint);
	}
	.lamp.live {
		background: var(--aqua);
		box-shadow: 0 0 8px var(--aqua-glow);
	}
	.agent-name {
		font-size: 15px;
		font-weight: 600;
		color: var(--ink-text);
	}
	.state {
		font-size: 13.5px;
		color: var(--ink-text);
		margin: 0 0 8px;
	}
	.muted {
		color: var(--ink-faint);
		font-size: 13px;
	}
	.error {
		color: var(--state-error);
		font-size: 13px;
		margin: 8px 0;
	}
	.warn {
		color: var(--saffron);
		font-size: 13px;
		line-height: 1.5;
		margin: 8px 0;
	}
	.hint {
		font-size: 12.5px;
		color: var(--ink-mute);
		margin: 8px 0;
	}
	.facts {
		margin: 12px 0;
	}
	.facts dt {
		font-size: 10.5px;
		font-weight: 700;
		letter-spacing: 1px;
		text-transform: uppercase;
		color: var(--ink-faint);
		margin-bottom: 4px;
	}
	.facts dd {
		margin: 0;
	}
	code {
		font-family: var(--font-mono);
		font-size: 12.5px;
		color: var(--ink-text);
		background: var(--ink-3);
		padding: 1px 6px;
		border-radius: 4px;
		unicode-bidi: isolate;
	}
	.path {
		word-break: break-all;
	}
	.notes {
		margin: 0 0 14px;
		padding-inline-start: 18px;
		font-size: 12.5px;
		color: var(--ink-mute);
		line-height: 1.55;
	}
	.notes li + li {
		margin-top: 4px;
	}
	.actions {
		display: flex;
		gap: 10px;
		align-items: center;
		margin-top: 10px;
	}
	.primary {
		padding: 8px 16px;
		border-radius: 7px;
		border: none;
		background: var(--aqua);
		color: var(--ink);
		font-weight: 700;
		font-size: 12.5px;
		cursor: pointer;
	}
	.secondary {
		padding: 7px 12px;
		border-radius: 7px;
		border: 1px solid var(--ink-line-2);
		background: transparent;
		color: var(--ink-text);
		font-weight: 600;
		font-size: 12.5px;
		cursor: pointer;
	}
	.primary:disabled,
	.secondary:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}
	.primary:focus-visible,
	.secondary:focus-visible {
		outline: 2px solid var(--aqua);
		outline-offset: 2px;
	}
	.preview {
		margin-top: 14px;
		padding: 14px 16px;
		border-radius: 8px;
		background: var(--ink-3);
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--aqua) 40%, transparent);
	}
	.preview h3 {
		font-size: 14px;
		font-weight: 600;
		color: var(--ink-text);
		margin: 0 0 10px;
	}
	.preview h3:focus-visible {
		outline: 2px solid var(--aqua);
		outline-offset: 2px;
	}
	.preview p {
		font-size: 12.5px;
		color: var(--ink-mute);
		line-height: 1.55;
		margin: 6px 0;
	}
	.added {
		margin: 6px 0 10px;
		padding: 10px 12px;
		border-radius: 6px;
		background: var(--ink);
		color: var(--ink-text);
		font-size: 12px;
		line-height: 1.5;
		text-align: left;
		unicode-bidi: isolate;
		overflow-x: auto;
		white-space: pre;
	}
	/* A character that would not show for what it is. */
	.hidden-char {
		unicode-bidi: isolate;
		display: inline-block;
		margin: 0 1px;
		padding: 0 3px;
		border-radius: 3px;
		font-size: 0.85em;
		color: var(--ink);
		background: var(--saffron);
	}
	.toast {
		position: fixed;
		bottom: 24px;
		inset-inline-end: 24px;
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 10px 14px;
		border-radius: 8px;
		background: var(--ink-2);
		font-size: 12.5px;
		max-width: 360px;
		word-break: break-all;
	}
	.toast.success {
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--state-success) 45%, transparent);
		color: var(--state-success);
	}
	.toast.warn {
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--state-error) 45%, transparent);
		color: var(--state-error);
	}
</style>
