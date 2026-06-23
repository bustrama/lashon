<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { applyLanguage } from '$lib/i18n';
	import { getSetting } from '$lib/settings';
	import { listen } from '@tauri-apps/api/event';

	let { children } = $props();

	onMount(() => {
		// Apply the persisted UI language to this window, then keep it in sync:
		// the Hub broadcasts `settings:changed` whenever the user switches it.
		void getSetting('ui.language').then(applyLanguage);

		const unlisten = listen<{ key: string }>('settings:changed', (event) => {
			if (event.payload.key === 'ui.language') {
				void getSetting('ui.language').then(applyLanguage);
			}
		});
		return () => void unlisten.then((stop) => stop()).catch(() => {});
	});
</script>

{@render children()}
