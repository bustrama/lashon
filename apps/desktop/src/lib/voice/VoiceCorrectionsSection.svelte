<!--
	Hub "Voice corrections" section — manages the user's
	`stt.word_aliases` map. After STT produces a transcript, the
	Tauri shell applies these substitutions before either the recipe
	cascade or the LLM planner sees the text. Single source of truth
	for "when Lashon hears X, treat it as Y" — fixes Whisper's
	persistent "claude → cloud" misrecognition, contact-name
	homonyms, Hebrew transliteration drift, etc.

	State is an *array* of `{from, to}` rows even though storage is a
	map — the array preserves edit order during a Hub session. On
	save we convert to a map (later rows win on duplicate `from`).
-->
<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { t } from '$lib/i18n';

	type Row = { from: string; to: string };

	let rows = $state<Row[]>([]);
	let loading = $state(true);
	let saving = $state(false);
	let loadError = $state<string | null>(null);
	let toast = $state<{ kind: 'success' | 'warn'; text: string } | null>(null);
	let toastTimer: ReturnType<typeof setTimeout> | null = null;

	function flashToast(kind: 'success' | 'warn', text: string) {
		toast = { kind, text };
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => (toast = null), 3_500);
	}

	onMount(() => {
		void load();
		return () => {
			if (toastTimer) clearTimeout(toastTimer);
		};
	});

	async function load() {
		loading = true;
		loadError = null;
		try {
			const map = await invoke<Record<string, string>>('get_word_aliases');
			rows = Object.entries(map)
				.sort(([a], [b]) => a.localeCompare(b))
				.map(([from, to]) => ({ from, to }));
			// Always leave one empty row so the user has a "type here" entry.
			if (rows.length === 0) rows = [{ from: '', to: '' }];
		} catch (err) {
			loadError = String(err);
		} finally {
			loading = false;
		}
	}

	async function save() {
		saving = true;
		try {
			// Convert to a map; later entries win on duplicate `from`.
			// Drop rows where either side is empty after trim.
			const map: Record<string, string> = {};
			for (const r of rows) {
				const f = r.from.trim();
				const t = r.to.trim();
				if (f.length > 0 && t.length > 0) {
					map[f] = t;
				}
			}
			await invoke('set_word_aliases', { aliases: map });
			flashToast('success', $t('hub.voice.savedToast').replace('{n}', String(Object.keys(map).length)));
		} catch (err) {
			flashToast('warn', `${$t('hub.voice.errorSave')}: ${err}`);
		} finally {
			saving = false;
		}
	}

	function addRow() {
		rows = [...rows, { from: '', to: '' }];
	}

	function removeRow(idx: number) {
		rows = rows.filter((_, i) => i !== idx);
		if (rows.length === 0) rows = [{ from: '', to: '' }];
	}

	const nonEmptyCount = $derived(
		rows.filter((r) => r.from.trim().length > 0 && r.to.trim().length > 0).length
	);
</script>

<section aria-live="polite">
	<h2 class="section-head">
		<span class="section-title he-display">{$t('hub.voice.title')}</span>
		<span class="section-en lat">· Voice corrections</span>
	</h2>
	<p class="he-sans intro" dir="auto">
		{$t('hub.voice.intro')}
	</p>

	{#if loading}
		<div class="he-sans muted">{$t('hub.voice.loading')}</div>
	{:else if loadError}
		<div class="he-sans error" role="alert">
			{$t('hub.voice.errorLoad')}: {loadError}
		</div>
	{:else}
		<div class="table-wrap">
			<div class="row head">
				<span class="he-sans col-label">{$t('hub.voice.colFrom')}</span>
				<span class="he-sans col-label">{$t('hub.voice.colTo')}</span>
				<span class="col-spacer" aria-hidden="true"></span>
			</div>
			{#each rows as row, i (i)}
				<div class="row">
					<input
						type="text"
						class="input mono"
						dir="auto"
						bind:value={row.from}
						placeholder={$t('hub.voice.fromPlaceholder')}
						aria-label={$t('hub.voice.colFrom')}
					/>
					<input
						type="text"
						class="input mono"
						dir="auto"
						bind:value={row.to}
						placeholder={$t('hub.voice.toPlaceholder')}
						aria-label={$t('hub.voice.colTo')}
					/>
					<button
						type="button"
						class="del"
						onclick={() => removeRow(i)}
						aria-label={$t('hub.voice.removeRow')}
						title={$t('hub.voice.removeRow')}
					>
						✕
					</button>
				</div>
			{/each}
			<div class="actions">
				<button type="button" class="add he-sans" onclick={addRow}>
					+ {$t('hub.voice.addRow')}
				</button>
				<button
					type="button"
					class="save he-sans"
					onclick={() => void save()}
					disabled={saving}
				>
					{saving ? $t('hub.voice.saving') : $t('hub.voice.save')}
				</button>
			</div>
			<p class="he-sans hint" dir="auto">
				{$t('hub.voice.count').replace('{n}', String(nonEmptyCount))}
			</p>
		</div>
	{/if}

	{#if toast}
		<div class="toast {toast.kind}" role="status">
			<span class="he-sans">{toast.text}</span>
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
	.muted {
		color: var(--ink-faint);
		font-size: 13px;
	}
	.error {
		color: var(--state-error);
		font-size: 13px;
	}
	.table-wrap {
		max-width: 720px;
	}
	.row {
		display: grid;
		grid-template-columns: 1fr 1fr 32px;
		gap: 10px;
		align-items: center;
		margin-bottom: 8px;
	}
	.row.head {
		margin-bottom: 6px;
	}
	.col-label {
		font-size: 10.5px;
		font-weight: 700;
		letter-spacing: 1px;
		text-transform: uppercase;
		color: var(--ink-faint);
	}
	.col-spacer {
		width: 32px;
	}
	.input {
		background: var(--ink-2);
		border: none;
		outline: none;
		padding: 8px 12px;
		border-radius: 8px;
		color: var(--ink-text);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		font-size: 13.5px;
		font-family: var(--font-mono);
		direction: ltr;
		transition: box-shadow 0.12s ease;
	}
	.input:focus-visible {
		box-shadow: inset 0 0 0 1.5px var(--saffron);
	}
	.del {
		width: 32px;
		height: 32px;
		border-radius: 6px;
		border: none;
		background: transparent;
		color: var(--ink-mute);
		cursor: pointer;
		font-size: 14px;
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.del:hover,
	.del:focus-visible {
		color: var(--state-error);
		outline: none;
	}
	.actions {
		display: flex;
		gap: 10px;
		margin-top: 6px;
		align-items: center;
	}
	.add {
		padding: 7px 12px;
		border-radius: 7px;
		border: 1px solid var(--ink-line-2);
		background: transparent;
		color: var(--ink-text);
		font-weight: 600;
		font-size: 12.5px;
		cursor: pointer;
	}
	.save {
		padding: 8px 16px;
		border-radius: 7px;
		border: none;
		background: var(--saffron);
		color: var(--ink);
		font-weight: 700;
		font-size: 12.5px;
		cursor: pointer;
	}
	.save:disabled {
		opacity: 0.55;
		cursor: not-allowed;
	}
	.hint {
		font-size: 11px;
		color: var(--ink-faint);
		margin: 12px 0 0;
	}
	.toast {
		position: fixed;
		bottom: 24px;
		inset-inline-end: 24px;
		padding: 10px 14px;
		border-radius: 8px;
		background: var(--ink-2);
		box-shadow:
			0 8px 24px rgba(0, 0, 0, 0.45),
			inset 0 0 0 1px var(--ink-line-2);
		font-size: 12.5px;
		max-width: 320px;
	}
	.toast.success {
		box-shadow:
			0 8px 24px rgba(0, 0, 0, 0.45),
			inset 0 0 0 1px rgba(95, 184, 135, 0.45);
		color: var(--state-success);
	}
	.toast.warn {
		box-shadow:
			0 8px 24px rgba(0, 0, 0, 0.45),
			inset 0 0 0 1px rgba(232, 98, 90, 0.45);
		color: var(--state-error);
	}
	@media (prefers-reduced-motion: reduce) {
		.input {
			transition: none;
		}
	}
</style>
