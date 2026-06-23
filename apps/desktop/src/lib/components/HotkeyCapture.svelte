<script lang="ts">
	// A chord-capture control for the Settings Hub. Clicking it starts capture;
	// the *focused button itself* receives every keydown — a reliable surface,
	// unlike a window listener — shows the modifiers building up live, and on a
	// complete combination validates it through the `validate_hotkey` command
	// (the rule lives in lashon-core). A valid chord is handed back via
	// `onchange`; an invalid one shows a localized reason and capture stays open.
	import { t } from '$lib/i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { eventToAccelerator, formatAccelerator } from '$lib/hotkey';

	let { value, onchange }: { value: string; onchange: (accelerator: string) => void } = $props();

	let capturing = $state(false);
	let preview = $state('');
	let errorCode = $state<string | null>(null);
	let recorder = $state<HTMLButtonElement>();

	function startCapture(): void {
		capturing = true;
		preview = '';
		errorCode = null;
		// Keep focus on the button so its keydown handler receives the chord.
		recorder?.focus();
	}

	function stopCapture(): void {
		capturing = false;
		preview = '';
	}

	async function onKeydown(event: KeyboardEvent): Promise<void> {
		if (!capturing) return;
		event.preventDefault();
		event.stopPropagation();
		if (event.key === 'Escape') {
			stopCapture();
			return;
		}
		const accelerator = eventToAccelerator(event);
		if (!accelerator) {
			// Only modifiers held so far — echo them back so the control
			// visibly responds to every key while the chord is still forming.
			const held: string[] = [];
			if (event.ctrlKey) held.push('Ctrl');
			if (event.altKey) held.push('Alt');
			if (event.shiftKey) held.push('Shift');
			if (event.metaKey) held.push('Win');
			preview = held.length > 0 ? `${held.join(' + ')} + …` : '…';
			return;
		}
		try {
			await invoke('validate_hotkey', { accelerator });
			errorCode = null;
			capturing = false;
			preview = '';
			onchange(accelerator);
		} catch (code) {
			// `validate_hotkey` rejects with a HotkeyError code string
			// (lashon-core::hotkey). A non-string rejection would be a
			// framework failure, not a bad chord — fall back to a generic
			// reason. Capture stays open so the user can try another chord.
			errorCode = typeof code === 'string' ? code : 'malformed';
			preview = formatAccelerator(accelerator);
		}
	}
</script>

<div class="capture">
	{#if !capturing}
		<kbd class="chord" dir="ltr">{formatAccelerator(value)}</kbd>
	{/if}
	<button
		bind:this={recorder}
		class="recorder"
		class:capturing
		type="button"
		onclick={startCapture}
		onkeydown={onKeydown}
		onblur={stopCapture}
	>
		{#if capturing}
			<span class="prompt" dir="auto">{preview || $t('hub.shortcuts.capturing')}</span>
		{:else}
			{$t('hub.shortcuts.rebind')}
		{/if}
	</button>
	{#if capturing}
		<button class="cancel" type="button" onclick={stopCapture}>
			{$t('hub.shortcuts.cancel')}
		</button>
	{/if}
	{#if errorCode}
		<span class="error" role="alert">{$t(`hub.shortcuts.invalid.${errorCode}`)}</span>
	{/if}
</div>

<style>
	.capture {
		display: flex;
		align-items: center;
		gap: 12px;
		flex-wrap: wrap;
	}

	.chord {
		font-family: inherit;
		font-size: 15px;
		font-weight: 700;
		color: var(--text-primary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-strong);
		border-bottom-width: 3px;
		border-radius: 8px;
		padding: 8px 16px;
	}

	button {
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		border-radius: 9px;
		padding: 8px 16px;
		cursor: pointer;
		border: 1px solid transparent;
		transition:
			background 0.15s ease,
			border-color 0.15s ease,
			color 0.15s ease;
	}

	button:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	.recorder {
		background: var(--bg-elevated);
		color: var(--text-primary);
		border-color: var(--stroke-strong);
	}
	.recorder:hover {
		border-color: var(--text-muted);
	}
	/* Capturing — the control reads as "live": a citron ring around the chord
	   building up inside it. */
	.recorder.capturing {
		min-width: 210px;
		color: var(--accent-citron);
		border-color: var(--accent-citron);
	}

	.prompt {
		font-weight: 700;
	}

	.cancel {
		background: transparent;
		color: var(--text-muted);
	}
	.cancel:hover {
		color: var(--text-secondary);
	}

	.error {
		font-size: 13px;
		color: var(--state-recording);
	}

	@media (prefers-reduced-motion: reduce) {
		button {
			transition: none;
		}
	}
</style>
