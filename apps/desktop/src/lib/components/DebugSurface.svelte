<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { t } from '$lib/i18n';

	// Mirrors the Rust HealthReport struct returned by the lashon_healthcheck command.
	interface HealthReport {
		ok: boolean;
		model_ready: boolean;
		detail: string;
	}

	let status = $state<'checking' | 'ok' | 'error'>('checking');
	let detail = $state('');

	onMount(async () => {
		try {
			const report = await invoke<HealthReport>('lashon_healthcheck');
			status = report.ok ? 'ok' : 'error';
			detail = report.detail;
		} catch (error) {
			status = 'error';
			detail = error instanceof Error ? error.message : String(error);
		}
	});
</script>

<div class="debug" data-tauri-drag-region>
	<span class="title">{$t('debug.title')}</span>
	<span class="line {status}">{$t(`debug.${status}`)}</span>
	{#if detail}
		<span class="detail">{detail}</span>
	{/if}
</div>

<style>
	.debug {
		box-sizing: border-box;
		width: 100vw;
		height: 100vh;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 3px;
		padding: 0 14px;
		border-radius: 22px;
		background: var(--bg-glass);
		backdrop-filter: blur(24px) saturate(180%);
		-webkit-backdrop-filter: blur(24px) saturate(180%);
		border: 1px solid var(--stroke-subtle);
		box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45);
		overflow: hidden;
	}

	.title {
		pointer-events: none;
		font-size: 10px;
		font-weight: 500;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: var(--text-muted);
	}

	.line {
		pointer-events: none;
		font-size: 14px;
		font-weight: 700;
	}
	.line.checking {
		color: var(--text-secondary);
	}
	.line.ok {
		color: var(--state-success);
	}
	.line.error {
		color: var(--state-recording);
	}

	.detail {
		pointer-events: none;
		max-width: 100%;
		font-size: 9px;
		color: var(--text-muted);
		text-align: center;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
