<script lang="ts">
	// The Settings Hub (Milestone M4) — the "Hub" surface from
	// docs/design-system.md. It runs in its own frameless, transparent `hub`
	// window, declared hidden in tauri.conf.json and revealed by the Rust shell
	// from the tray. M4 fills three sections — General, Shortcuts, About; later
	// milestones add the rest. RTL-native, design tokens only.
	import { onMount } from 'svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { emit } from '@tauri-apps/api/event';
	import { invoke } from '@tauri-apps/api/core';
	import { getVersion } from '@tauri-apps/api/app';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { applyLanguage, LANGUAGES, t, type Lang } from '$lib/i18n';
	import { DEFAULTS, getSetting, setSetting } from '$lib/settings';
	import { type HardwareReport, type Tier } from '$lib/hardware';
	import HotkeyCapture from '$lib/components/HotkeyCapture.svelte';
	import TierSelect from '$lib/components/TierSelect.svelte';
	import Mark from '$lib/components/Mark.svelte';
	import RecipesSection from '$lib/recipes/RecipesSection.svelte';
	import VoiceCorrectionsSection from '$lib/voice/VoiceCorrectionsSection.svelte';

	type Section =
		| 'general'
		| 'shortcuts'
		| 'wakeword'
		| 'hardware'
		| 'llm'
		| 'recipes'
		| 'voice'
		| 'about';
	const SECTIONS: Section[] = [
		'general',
		'shortcuts',
		'wakeword',
		'hardware',
		'llm',
		'recipes',
		'voice',
		'about'
	];

	// English subtitles for the section headers — the Lamp design pairs each
	// Hebrew title with a small italic English sublabel ("כללי · General").
	// Hardcoded rather than re-resolved via the `en` locale so the UI doesn't
	// have to thrash the i18n store on every section switch.
	const SECTION_EN: Record<Section, string> = {
		general: 'General',
		shortcuts: 'Shortcuts',
		wakeword: 'Wake word',
		hardware: 'Hardware',
		llm: 'Language models',
		recipes: 'Recipes',
		voice: 'Voice corrections',
		about: 'About'
	};

	// M7 LLM provider mux. Two modes (command + chat) share the same provider
	// catalog but pick independently. ProviderMeta matches the Rust struct in
	// lashon-core::provider — keep the two in sync.
	type Confidence = 'None' | 'Basic' | 'Good' | 'Excellent';
	type LlmMode = 'command' | 'chat';
	const LLM_MODES: LlmMode[] = ['command', 'chat'];
	type ProviderMeta = {
		id: string;
		display_name_key: string;
		is_local: boolean;
		supports_hebrew: Confidence;
		has_api_key: boolean;
		default_model: string;
		available_models: string[];
		context_window: number;
		supports_tool_use: boolean;
		// `null` when the provider doesn't single out a "best" model.
		// When set, the matching entry in the model dropdown reads
		// "<name> — מומלץ / recommended".
		recommended_model: string | null;
	};
	type OllamaDetection = { running: boolean; models: string[] };
	// In-process local-LLM (docs/adr/0025). Mirrors the Rust types in
	// apps/desktop/src-tauri/src/llm.rs — keep the two in sync.
	type LocalLlmModelMeta = {
		id: string;
		display_name: string;
		description: string;
		license: string;
		source: string;
		context_window: number;
		bytes: number;
		installed: boolean;
	};
	type LocalLlmStatusReport = {
		runtime_available: boolean;
		active_model: string;
		active_installed: boolean;
		models: LocalLlmModelMeta[];
	};
	type LocalLlmProgress = {
		model_id: string;
		file: string;
		downloaded: number;
		total: number | null;
	};
	type LlmBaseUrlKey =
		| 'llm.anthropic.base_url'
		| 'llm.openai.base_url'
		| 'llm.groq.base_url'
		| 'llm.deepseek.base_url'
		| 'llm.mistral.base_url'
		| 'llm.together.base_url'
		| 'llm.openrouter.base_url'
		| 'llm.minimax.base_url'
		| 'llm.opencode-go.base_url'
		| 'llm.ollama-local.base_url'
		| 'llm.ollama-remote.base_url';
	function baseUrlKey(id: string): LlmBaseUrlKey | null {
		const key = `llm.${id}.base_url`;
		const VALID: readonly LlmBaseUrlKey[] = [
			'llm.anthropic.base_url',
			'llm.openai.base_url',
			'llm.groq.base_url',
			'llm.deepseek.base_url',
			'llm.mistral.base_url',
			'llm.together.base_url',
			'llm.openrouter.base_url',
			'llm.minimax.base_url',
			'llm.opencode-go.base_url',
			'llm.ollama-local.base_url',
			'llm.ollama-remote.base_url'
		];
		return VALID.includes(key as LlmBaseUrlKey) ? (key as LlmBaseUrlKey) : null;
	}

	// External links for the About section, opened in the system browser via
	// the opener plugin — the webview itself never navigates away.
	const LINKS = [
		{ key: 'hub.about.repo', url: 'https://github.com/bustrama/lashon', text: 'github.com/bustrama/lashon' },
		{ key: 'hub.about.links', url: 'https://www.bustrama.com/', text: 'bustrama.com' },
		{ key: 'hub.about.support', url: 'https://ko-fi.com/bustrama', text: 'ko-fi.com/bustrama' }
	];

	// Update-check state — driven from the Hub About section.
	// `status` mirrors the `updater:progress` event payload plus synthetic
	// values ('idle' / 'up-to-date' / 'error') that originate here.
	type UpdateStatus =
		| 'idle'
		| 'checking'
		| 'downloading'
		| 'installing'
		| 'installed'
		| 'up-to-date'
		| 'error';

	let updateStatus = $state<UpdateStatus>('idle');
	let updateVersion = $state<string | null>(null);
	let updatePercent = $state<number>(0);
	let updateError = $state<string | null>(null);

	// The bilingual label shown while the updater is running or after it settles.
	// Tests in this file drive Hebrew text paths so they are exercised at the
	// component layer (`.claude/rules/hebrew.md`).
	const updateStatusLabel = $derived((): string => {
		switch (updateStatus) {
			case 'idle':
				return $t('hub.about.checkUpdates');
			case 'checking':
				return $t('hub.about.checking');
			case 'downloading':
				return $t('hub.about.downloading').replace(
					'{version}',
					updateVersion ?? ''
				) + (updatePercent > 0 ? ` (${Math.round(updatePercent)}%)` : '');
			case 'installing':
				return $t('hub.about.installing');
			case 'installed':
				return $t('hub.about.installed');
			case 'up-to-date':
				return $t('hub.about.upToDate');
			case 'error':
				return `${$t('hub.about.updateError')}: ${updateError ?? ''}`;
		}
	});

	const updateBusy = $derived(
		updateStatus === 'checking' ||
		updateStatus === 'downloading' ||
		updateStatus === 'installing'
	);

	// Invoke the Rust `check_for_updates` command and follow the
	// `updater:progress` events the command emits into the Hub status display.
	// The button is disabled while any update operation is in progress.
	async function checkForUpdates(): Promise<void> {
		import('@tauri-apps/api/event').then(({ listen }) => {
			const unlisten = listen<{
				status?: string;
				version?: string;
				current_version?: string;
				percent?: number;
			}>('updater:progress', (event) => {
				const { status, version, percent } = event.payload;
				if (version) updateVersion = version;
				if (status === 'downloading') {
					updateStatus = 'downloading';
					updatePercent = percent ?? 0;
				} else if (status === 'installing') {
					updateStatus = 'installing';
				} else if (status === 'installed') {
					updateStatus = 'installed';
					// Unlisten once the terminal state is reached.
					unlisten.then((fn) => fn());
				}
			});
		});

		updateStatus = 'checking';
		updateError = null;
		updateVersion = null;
		updatePercent = 0;

		try {
			const result = await invoke<string>('check_for_updates');
			if (result === 'up-to-date') {
				updateStatus = 'up-to-date';
			}
			// 'installed' is set via the event listener above.
		} catch (err) {
			updateStatus = 'error';
			updateError = String(err);
			console.error('hub: check_for_updates failed', err);
		}
	}

	let section = $state<Section>('general');
	let language = $state<Lang>('he');
	let hotkey = $state('Control+Space');
	// M8 Command-mode hotkey, configurable from the Shortcuts section.
	// Initial value is the same default `settings.ts` ships — the live
	// value loads from the store on mount and may differ.
	let commandHotkey = $state(DEFAULTS['hotkeys.command']);
	let version = $state('');
	// The Hardware section — the persisted tier, the latest detection report,
	// and whether a detection run is in flight.
	let tier = $state<Tier | null>(null);
	// The tier the STT sidecar was started with — a change to `tier` needs an
	// app restart before it takes effect.
	let initialTier = $state<Tier | null>(null);
	let hardware = $state<HardwareReport | null>(null);
	let hwDetecting = $state(false);
	// The Wake-word section hosts two independent slots:
	//
	// - Dictation slot: fires the existing dictation flow (transcript
	//   types into the focused field).
	// - Command slot: fires M8 Command mode (transcript runs as a tool
	//   chain on the M8 dispatcher).
	//
	// Each slot has its own enable / sensitivity / model triplet. The
	// two slots must name DIFFERENT classifiers — a single utterance
	// can't be classified as both intents. The model picker below
	// enforces that by hiding the other slot's pick from each
	// dropdown.
	let dictationEnabled = $state(false);
	let dictationSensitivity = $state(0.7);
	let dictationModel = $state('hey_lashon');
	let commandEnabled = $state(false);
	let commandSensitivity = $state(0.7);
	let commandModel = $state('');
	let wakeModels = $state<string[]>([]);
	// Opt-in CC-BY-NC wake-word classifiers the user may download from the
	// Hub. The list comes from models/manifests/wake-classifiers.json; nothing
	// is fetched until the user confirms the licence.
	type AvailableWake = {
		id: string;
		display_name: string;
		license: string;
		source: string;
		bytes: number;
		installed: boolean;
	};
	let wakeAvailable = $state<AvailableWake[]>([]);
	let wakeInstalling = $state<string | null>(null);
	let wakeInstallError = $state<string | null>(null);

	// LLM section state. The provider catalog comes from the Rust side
	// (`get_llm_providers`); the active provider + model + base URL are
	// persisted in settings.json. API keys live in the OS keychain and never
	// touch the frontend except as the masked input flowing into save_api_key.
	let llmProviders = $state<ProviderMeta[]>([]);
	let llmActive = $state<Record<LlmMode, string>>({ command: 'none', chat: 'none' });

	// Lamp redesign: the LLM page shows ONE mode's settings at a time via
	// a tab-strip at the top — the design source's three-column "tabs"
	// pattern, in our case two (`command` / `chat`; `cleanup` is M9 work
	// and unimplemented). Defaults to `command` because M8 — Command mode
	// — is what most users will configure first.
	let llmActiveTab = $state<LlmMode>('command');
	const LLM_TAB_TINT: Record<LlmMode, string> = {
		command: 'var(--garnet)',
		chat: 'var(--indigo)'
	};
	const LLM_TAB_EN: Record<LlmMode, string> = {
		command: 'Command',
		chat: 'Chat'
	};
	let llmModel = $state<Record<LlmMode, string>>({ command: '', chat: '' });
	let llmBaseUrl = $state<Record<string, string>>({});
	let llmKeyInput = $state<Record<string, string>>({});
	let llmKeySaving = $state<string | null>(null);
	let llmTestPrompt = $state<Record<LlmMode, string>>({ command: 'שלום', chat: 'שלום' });
	let llmTestResult = $state<Record<LlmMode, string>>({ command: '', chat: '' });
	let llmTestError = $state<Record<LlmMode, string>>({ command: '', chat: '' });
	let llmTesting = $state<LlmMode | null>(null);
	let ollama = $state<OllamaDetection>({ running: false, models: [] });
	let ollamaProbing = $state(false);

	// Per-provider remote model lists, populated by `fetch_provider_models`
	// when an API key is saved (or the user clicks Refresh). The Hub uses
	// the remote list (if present) in preference to the static
	// `available_models` from the catalogue so brand-new release models
	// show up without a Lashon update. The list arrives filtered (chat-
	// capable only) + sorted (newest first) + capped at the Rust-side
	// REMOTE_MODELS_CAP (30); `total_count` carries the pre-cap count so
	// the UI can render "30 of 78".
	type ProviderModelsResult = {
		models: string[];
		source: 'remote' | 'fallback';
		total_count: number;
		error: string | null;
	};
	let llmRemoteModels = $state<Record<string, ProviderModelsResult>>({});
	let llmModelsFetching = $state<string | null>(null);

	// docs/adr/0025 — in-process local LLM. The status report drives the
	// "Download required" / "Ready" copy under the chip; while a download
	// runs, the per-model percentage and byte count come from the
	// `local_llm:progress` Tauri event.
	let localLlmStatus = $state<LocalLlmStatusReport>({
		runtime_available: true,
		active_model: '',
		active_installed: false,
		models: []
	});
	let localLlmInstalling = $state<string | null>(null);
	let localLlmDeleting = $state<string | null>(null);
	let localLlmError = $state<string | null>(null);
	let localLlmDownloaded = $state<Record<string, number>>({});
	let localLlmTotal = $state<Record<string, number | null>>({});
	function localLlmModel(id: string): LocalLlmModelMeta | undefined {
		return localLlmStatus.models.find((m) => m.id === id);
	}
	function localLlmPercent(id: string): number | null {
		const total = localLlmTotal[id];
		const downloaded = localLlmDownloaded[id] ?? 0;
		if (!total || total === 0) return null;
		return Math.min(100, Math.round((downloaded / total) * 100));
	}
	function formatBytes(n: number): string {
		if (!n) return '';
		const mb = n / (1024 * 1024);
		if (mb < 1024) return `${mb.toFixed(0)} MB`;
		return `${(mb / 1024).toFixed(2)} GB`;
	}

	// Whether the picked provider for `mode` is the cloud kind that requires
	// an API key. Drives the inline reveal of the key input.
	function isCloud(id: string): boolean {
		return llmProviders.find((p) => p.id === id)?.is_local === false;
	}
	function meta(id: string): ProviderMeta | undefined {
		return llmProviders.find((p) => p.id === id);
	}
	function activeMeta(mode: LlmMode): ProviderMeta | undefined {
		return meta(llmActive[mode]);
	}

	// Human label for a wake-word model id (the filename stem). The project's
	// own "Hey Lashon" is hard-coded; opt-in classifiers come labelled from
	// their manifest; everything else falls back to a title-cased rendering of
	// the filename so a user-trained "my_dragon" reads as "My Dragon".
	const KNOWN_NICKNAMES: Record<string, string> = {
		hey_lashon: 'Hey Lashon'
	};
	function nicknameFor(id: string): string {
		if (KNOWN_NICKNAMES[id]) return KNOWN_NICKNAMES[id];
		const available = wakeAvailable.find((m) => m.id === id);
		if (available) return available.display_name;
		return id
			.split(/[_-]+/)
			.filter((part) => part.length > 0)
			.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
			.join(' ');
	}

	onMount(() => {
		void getSetting('ui.language').then((lang) => (language = lang));
		void getSetting('hotkeys.dictation').then((chord) => (hotkey = chord));
		void getSetting('hotkeys.command').then((chord) => (commandHotkey = chord));
		void getSetting('hardware.tier').then((value) => {
			tier = value;
			initialTier = value;
		});
		// The wakeword.dictation.* / wakeword.command.* schema is the
		// post-PR layout. The Tauri side applies a one-shot migration
		// from the legacy flat `wakeword.*` keys (see wakeword.rs); the
		// UI reads only the new keys.
		void getSetting('wakeword.dictation.enabled').then((value) => (dictationEnabled = value ?? false));
		void getSetting('wakeword.dictation.sensitivity').then(
			(value) => (dictationSensitivity = value ?? 0.7),
		);
		void getSetting('wakeword.dictation.model').then(
			(value) => (dictationModel = value ?? 'hey_lashon'),
		);
		void getSetting('wakeword.command.enabled').then((value) => (commandEnabled = value ?? false));
		void getSetting('wakeword.command.sensitivity').then(
			(value) => (commandSensitivity = value ?? 0.7),
		);
		void getSetting('wakeword.command.model').then((value) => (commandModel = value ?? ''));
		void refreshWakeModels();
		void getVersion()
			.then((value) => (version = value))
			.catch(() => {});
		void loadLlmState();

		// Subscribe to the in-process LLM's download progress events
		// (docs/adr/0025). The Tauri shell emits one per integer-percent
		// crossing during the GGUF download — the Hub chip uses it to
		// fill a progress bar instead of sitting silent for the ~1 GB
		// pull. Unsubscribe on unmount.
		let unlistenLocalLlm: (() => void) | null = null;
		import('@tauri-apps/api/event').then(({ listen }) => {
			listen<LocalLlmProgress>('local_llm:progress', (event) => {
				const p = event.payload;
				localLlmDownloaded[p.model_id] = p.downloaded;
				localLlmTotal[p.model_id] = p.total;
			}).then((fn) => {
				unlistenLocalLlm = fn;
			});
		});
		return () => {
			if (unlistenLocalLlm) unlistenLocalLlm();
		};
	});

	// Load the LLM catalog from Rust + the persisted per-mode active
	// provider/model + the per-provider base-URL overrides.
	async function loadLlmState(): Promise<void> {
		try {
			const list = await invoke<ProviderMeta[]>('get_llm_providers', { mode: 'command' });
			llmProviders = list;
		} catch (err) {
			console.error('hub: get_llm_providers failed', err);
			return;
		}
		void getSetting('llm.command.provider').then((value) => (llmActive.command = value));
		void getSetting('llm.command.model').then((value) => (llmModel.command = value));
		void getSetting('llm.chat.provider').then((value) => (llmActive.chat = value));
		void getSetting('llm.chat.model').then((value) => (llmModel.chat = value));
		for (const meta of llmProviders) {
			const key = baseUrlKey(meta.id);
			if (!key) continue;
			void getSetting(key).then((value) => {
				llmBaseUrl[meta.id] = value;
			});
		}
		void refreshLocalLlmStatus();
	}

	// docs/adr/0025 — refresh the in-process LLM card so the Hub knows
	// whether to render "Download required" or "Ready".
	async function refreshLocalLlmStatus(): Promise<void> {
		try {
			const report = await invoke<LocalLlmStatusReport>('local_llm_status');
			localLlmStatus = report;
		} catch (err) {
			console.error('hub: local_llm_status failed', err);
		}
	}

	// Trigger the GGUF download for `modelId`. While in flight, the
	// `local_llm:progress` event listener updates the per-model
	// downloaded/total state below.
	async function installLocalLlm(modelId: string): Promise<void> {
		if (localLlmInstalling) return;
		localLlmInstalling = modelId;
		localLlmError = null;
		localLlmDownloaded[modelId] = 0;
		localLlmTotal[modelId] = localLlmModel(modelId)?.bytes || null;
		try {
			await invoke<string>('install_local_llm', { modelId });
			await refreshLocalLlmStatus();
		} catch (err) {
			localLlmError = String(err);
			console.error('hub: install_local_llm failed', err);
		} finally {
			localLlmInstalling = null;
		}
	}

	async function deleteLocalLlm(modelId: string): Promise<void> {
		if (localLlmDeleting) return;
		localLlmDeleting = modelId;
		try {
			await invoke<number>('delete_local_llm', { modelId });
			await refreshLocalLlmStatus();
		} catch (err) {
			localLlmError = String(err);
			console.error('hub: delete_local_llm failed', err);
		} finally {
			localLlmDeleting = null;
		}
	}

	// Switch the active provider for a mode. Empty model so the next picker
	// load reads the provider's default. Cloud is never silently active —
	// the user must store a key before a `test` call will succeed.
	async function selectLlmProvider(mode: LlmMode, id: string): Promise<void> {
		llmActive[mode] = id;
		llmTestResult[mode] = '';
		llmTestError[mode] = '';
		await setSetting(`llm.${mode}.provider`, id);
		// Default the model to the provider's default if none is configured
		// yet for this mode; the user can override below.
		if (id !== 'none') {
			const picked = meta(id);
			if (picked && (llmModel[mode] === '' || !picked.available_models.includes(llmModel[mode]))) {
				llmModel[mode] = picked.default_model;
				await setSetting(`llm.${mode}.model`, picked.default_model);
			}
		}
		void invoke('set_llm_provider', { mode, id }).catch((err) =>
			console.error('hub: set_llm_provider failed', err)
		);
		if (id === 'ollama-local' && !ollama.running) {
			void probeOllama();
		}
	}

	async function selectLlmModel(mode: LlmMode, model: string): Promise<void> {
		llmModel[mode] = model;
		await setSetting(`llm.${mode}.model`, model);
	}

	async function saveLlmBaseUrl(id: string, url: string): Promise<void> {
		llmBaseUrl[id] = url;
		const key = baseUrlKey(id);
		if (!key) return;
		await setSetting(key, url);
	}

	// Save an API key to the OS keychain via the Tauri command. The key is
	// scrubbed from the input immediately afterwards — the only way back is
	// to re-enter it. There is no "get key" command (docs/adr/0020).
	async function saveApiKey(id: string): Promise<void> {
		const secret = (llmKeyInput[id] ?? '').trim();
		if (secret.length === 0) return;
		llmKeySaving = id;
		try {
			await invoke('save_api_key', { stage: 'llm', provider: id, secret });
			llmKeyInput[id] = '';
			// Refresh the catalog so `has_api_key` flips to true.
			await loadLlmState();
			// Pull the live model list now that a key is available — the
			// model dropdown switches from the static catalogue to whatever
			// the vendor actually serves.
			void refreshProviderModels(id);
		} catch (err) {
			console.error('hub: save_api_key failed', err);
		} finally {
			llmKeySaving = null;
		}
	}

	/// Hit `/v1/models` on the provider and replace the per-provider
	/// model dropdown with the live list. Falls back silently to the
	/// static `available_models` when the remote call errors — the
	/// error string is shown next to the picker so the user can debug
	/// a bad key / rate-limited endpoint without guessing.
	async function refreshProviderModels(id: string): Promise<void> {
		if (llmModelsFetching === id) return;
		llmModelsFetching = id;
		try {
			const result = await invoke<ProviderModelsResult>('fetch_provider_models', {
				providerId: id
			});
			llmRemoteModels[id] = result;
		} catch (err) {
			console.error('hub: fetch_provider_models failed', err);
			llmRemoteModels[id] = {
				models: [],
				source: 'fallback',
				total_count: 0,
				error: String(err)
			};
		} finally {
			llmModelsFetching = null;
		}
	}

	/// Return the model list the dropdown should render for a provider.
	/// Prefers the live remote list over the static catalogue; falls
	/// back to the catalogue when no remote fetch has happened yet.
	function modelOptionsFor(meta: ProviderMeta): string[] {
		const remote = llmRemoteModels[meta.id];
		if (remote && remote.models.length > 0) return remote.models;
		return meta.available_models;
	}

	async function clearApiKey(id: string): Promise<void> {
		try {
			await invoke('delete_api_key', { stage: 'llm', provider: id });
			await loadLlmState();
		} catch (err) {
			console.error('hub: delete_api_key failed', err);
		}
	}

	async function probeOllama(): Promise<void> {
		ollamaProbing = true;
		try {
			ollama = await invoke<OllamaDetection>('detect_ollama');
		} catch (err) {
			console.error('hub: detect_ollama failed', err);
			ollama = { running: false, models: [] };
		} finally {
			ollamaProbing = false;
		}
	}

	async function runLlmTest(mode: LlmMode): Promise<void> {
		const text = (llmTestPrompt[mode] ?? '').trim();
		if (text.length === 0) return;
		llmTesting = mode;
		llmTestError[mode] = '';
		llmTestResult[mode] = '';
		try {
			const reply = await invoke<string>('test_llm_prompt', { mode, text });
			llmTestResult[mode] = reply;
		} catch (err) {
			llmTestError[mode] = String(err);
			console.error('hub: test_llm_prompt failed', err);
		} finally {
			llmTesting = null;
		}
	}

	$effect(() => {
		if (section === 'llm') {
			void probeOllama();
		}
	});

	// The detected RAM / GPU readout, shown above the tier picker.
	const hwReadings = $derived(
		hardware === null
			? ''
			: `${$t('hardware.ramLabel')}: ${Math.round(hardware.probe.ram_gb)} GB · ` +
				(hardware.probe.cuda
					? `${$t('hardware.gpuNvidia')} · ${hardware.probe.vram_gb.toFixed(1)} GB`
					: hardware.probe.vulkan
						? $t('hardware.gpuVulkan')
						: $t('hardware.gpuNone'))
	);

	// Whether the selected tier differs from the one the STT sidecar was
	// started with — a tier change needs an app restart to take effect.
	const tierChanged = $derived(tier !== initialTier);

	// What the selected tier means for speech recognition: the GPU-probing
	// path (tiers A/B) or CPU-only (tiers C/D).
	const sttDeviceNote = $derived(
		tier === 'A' || tier === 'B'
			? $t('hub.hardware.sttDeviceGpu')
			: tier === 'C' || tier === 'D'
				? $t('hub.hardware.sttDeviceCpu')
				: ''
	);

	// Detect the hardware tier. Adopts and persists the detected tier only
	// when none is saved yet — an existing onboarding choice is left intact.
	async function detectHardware(): Promise<void> {
		hwDetecting = true;
		try {
			const report = await invoke<HardwareReport>('detect_hardware');
			hardware = report;
			if (tier === null) {
				tier = report.tier;
				await setSetting('hardware.tier', report.tier);
			}
		} catch (err) {
			console.error('hub: hardware detection failed', err);
		} finally {
			hwDetecting = false;
		}
	}

	// Persist an explicit tier override from the picker.
	async function saveTier(next: Tier): Promise<void> {
		tier = next;
		await setSetting('hardware.tier', next);
	}

	// Relaunch the app so a tier change takes effect now — the STT sidecar
	// reads its device only at startup.
	function restartApp(): void {
		void invoke('restart_app').catch((err) =>
			console.error('hub: restart failed', err)
		);
	}

	// Detect on first view of the Hardware section, so its readings are fresh
	// without a detection run on every Hub open.
	$effect(() => {
		if (section === 'hardware' && hardware === null && !hwDetecting) {
			void detectHardware();
		}
	});

	// Persist the UI language and broadcast it. `applyLanguage` updates this
	// window at once; the `settings:changed` event carries the switch to the
	// tongue and tutorial windows (their +layout.svelte listens for it).
	async function selectLanguage(lang: Lang): Promise<void> {
		if (lang === language) return;
		language = lang;
		await setSetting('ui.language', lang);
		applyLanguage(lang);
		void emit('settings:changed', { key: 'ui.language' }).catch(() => {});
	}

	// Persist a rebound dictation hotkey and tell the tongue window to
	// re-register it. HotkeyCapture has already validated the chord.
	async function saveHotkey(accelerator: string): Promise<void> {
		hotkey = accelerator;
		await setSetting('hotkeys.dictation', accelerator);
		void emit('settings:changed', { key: 'hotkeys.dictation' }).catch(() => {});
	}

	// Persist a rebound Command-mode hotkey (M8). Same broadcast pattern as
	// the dictation one — the tongue listens for `settings:changed` and
	// re-registers the chord live.
	async function saveCommandHotkey(accelerator: string): Promise<void> {
		commandHotkey = accelerator;
		await setSetting('hotkeys.command', accelerator);
		void emit('settings:changed', { key: 'hotkeys.command' }).catch(() => {});
	}

	// Persist a wake-word slot setting and broadcast the change. The
	// wake-word worker listens for any `wakeword.*` event and live-
	// reloads — no app restart.

	async function saveDictationEnabled(value: boolean): Promise<void> {
		dictationEnabled = value;
		await setSetting('wakeword.dictation.enabled', value);
		void emit('settings:changed', { key: 'wakeword.dictation.enabled' }).catch(() => {});
	}

	async function saveDictationSensitivity(value: number): Promise<void> {
		dictationSensitivity = value;
		await setSetting('wakeword.dictation.sensitivity', value);
		void emit('settings:changed', { key: 'wakeword.dictation.sensitivity' }).catch(() => {});
	}

	async function saveDictationModel(value: string): Promise<void> {
		// Defensive: the dropdown already filters out the Command
		// slot's pick, but a stale value could land here. Swap the
		// other slot if the user picks its current model.
		if (value && value === commandModel) {
			commandModel = '';
			await setSetting('wakeword.command.model', '');
			void emit('settings:changed', { key: 'wakeword.command.model' }).catch(() => {});
		}
		dictationModel = value;
		await setSetting('wakeword.dictation.model', value);
		void emit('settings:changed', { key: 'wakeword.dictation.model' }).catch(() => {});
	}

	async function saveCommandEnabled(value: boolean): Promise<void> {
		commandEnabled = value;
		await setSetting('wakeword.command.enabled', value);
		void emit('settings:changed', { key: 'wakeword.command.enabled' }).catch(() => {});
	}

	async function saveCommandSensitivity(value: number): Promise<void> {
		commandSensitivity = value;
		await setSetting('wakeword.command.sensitivity', value);
		void emit('settings:changed', { key: 'wakeword.command.sensitivity' }).catch(() => {});
	}

	async function saveCommandModel(value: string): Promise<void> {
		if (value && value === dictationModel) {
			dictationModel = '';
			await setSetting('wakeword.dictation.model', '');
			void emit('settings:changed', { key: 'wakeword.dictation.model' }).catch(() => {});
		}
		commandModel = value;
		await setSetting('wakeword.command.model', value);
		void emit('settings:changed', { key: 'wakeword.command.model' }).catch(() => {});
	}

	// Pull the list of installed classifiers and the opt-in catalog together so
	// the picker and the download section stay in sync after an install.
	async function refreshWakeModels(): Promise<void> {
		try {
			wakeModels = await invoke<string[]>('list_wake_models');
		} catch (err) {
			console.error('hub: list_wake_models failed', err);
		}
		try {
			wakeAvailable = await invoke<AvailableWake[]>('available_wake_models');
		} catch (err) {
			console.error('hub: available_wake_models failed', err);
		}
	}

	// Confirm the licence with the user and then download a CC-BY-NC wake-word
	// classifier from its manifest URL. The Rust side SHA-256-verifies every
	// byte before writing the file.
	async function installWakeModel(entry: AvailableWake): Promise<void> {
		const confirmMessage = $t('hub.wakeword.installConfirm')
			.replace('{name}', entry.display_name)
			.replace('{license}', entry.license);
		if (!confirm(confirmMessage)) return;

		wakeInstalling = entry.id;
		wakeInstallError = null;
		try {
			const installed = await invoke<string>('install_wake_model', { id: entry.id });
			await refreshWakeModels();
			// Auto-assign the freshly installed classifier to the first
			// empty slot so the user can try it without an extra click.
			// Dictation has precedence (legacy default). When both slots
			// already have a model picked, leave the install alone — the
			// user has to deliberately swap.
			if (!dictationModel) {
				await saveDictationModel(installed);
			} else if (!commandModel) {
				await saveCommandModel(installed);
			}
		} catch (err) {
			wakeInstallError = String(err);
			console.error('hub: install_wake_model failed', err);
		} finally {
			wakeInstalling = null;
		}
	}

	function close(): void {
		void getCurrentWindow().hide();
	}

	function openExternal(url: string): void {
		void openUrl(url).catch((error) => console.error('could not open URL', error));
	}

	// The window is frameless: the header is the drag surface. Clicks on
	// buttons are excluded so the close control keeps working.
	function draggable(node: HTMLElement) {
		function onMouseDown(event: MouseEvent) {
			if (event.buttons !== 1) return;
			if ((event.target as HTMLElement).closest('button')) return;
			void getCurrentWindow().startDragging();
		}
		node.addEventListener('mousedown', onMouseDown);
		return {
			destroy() {
				node.removeEventListener('mousedown', onMouseDown);
			}
		};
	}
</script>

<div class="hub">
	<div class="card">
		<!-- Title bar — draggable, with the brand string centered and a
		     functional close X on the trailing edge. The Lamp design source
		     uses mac-style traffic lights here; on a cross-platform window
		     they'd feel out of place, so we ship just the close control
		     and let the brand string carry the chrome. -->
		<header class="title-bar" use:draggable>
			<button class="close" type="button" onclick={close} aria-label={$t('hub.close')}>
				✕
			</button>
			<div class="title-text">
				<span class="title-brand he-display">לָשׁוֹן</span>
				<span class="title-sep">·</span>
				<span class="title-section he-sans">{$t('hub.title')}</span>
			</div>
			<span class="title-spacer" aria-hidden="true"></span>
		</header>

		<div class="body">
			<aside class="sidebar" aria-label={$t('hub.title')}>
				<!-- Brand block — mark + Hebrew wordmark + version + local badge. -->
				<div class="sidebar-brand">
					<Mark size={28} color="var(--ink-text)" />
					<div class="sidebar-brand-text">
						<div class="he-display sidebar-brand-name">לָשׁוֹן</div>
						<div class="mono sidebar-brand-meta">
							{version ? `v${version}` : ''}<span class="dot">·</span>local
						</div>
					</div>
				</div>
				<nav class="nav">
					{#each SECTIONS as id}
						<button
							type="button"
							class="nav-item"
							class:active={section === id}
							aria-current={section === id ? 'page' : undefined}
							onclick={() => (section = id)}
						>
							<span class="nav-rail" aria-hidden="true"></span>
							<span class="nav-he he-sans">{$t(`hub.nav.${id}`)}</span>
							<span class="nav-en mono">{SECTION_EN[id]}</span>
						</button>
					{/each}
				</nav>
			</aside>

			<div class="detail">
				{#if section === 'general'}
					<section aria-live="polite">
						<h2 class="section-head">
						<span class="section-title he-display">{$t('hub.general.title')}</span>
						<span class="section-en lat">· General</span>
					</h2>
						<div class="field">
							<span class="field-label">{$t('hub.general.language')}</span>
							<p class="field-hint">{$t('hub.general.languageHint')}</p>
							<div class="segmented" role="group" aria-label={$t('hub.general.language')}>
								{#each LANGUAGES as lang}
									<button
										type="button"
										class="segment"
										class:selected={language === lang}
										aria-pressed={language === lang}
										onclick={() => void selectLanguage(lang)}
									>
										{$t(lang === 'he' ? 'hub.general.langHe' : 'hub.general.langEn')}
									</button>
								{/each}
							</div>
						</div>
					</section>
				{:else if section === 'shortcuts'}
					<section aria-live="polite">
						<h2 class="section-head">
						<span class="section-title he-display">{$t('hub.shortcuts.title')}</span>
						<span class="section-en lat">· Shortcuts</span>
					</h2>
						<div class="field">
							<span class="field-label">{$t('hub.shortcuts.dictation')}</span>
							<p class="field-hint">{$t('hub.shortcuts.dictationHint')}</p>
							<HotkeyCapture value={hotkey} onchange={saveHotkey} />
						</div>
						<div class="field" style="margin-block-start: 24px;">
							<span class="field-label">{$t('hub.shortcuts.command')}</span>
							<p class="field-hint">{$t('hub.shortcuts.commandHint')}</p>
							<HotkeyCapture value={commandHotkey} onchange={saveCommandHotkey} />
						</div>
					</section>
				{:else if section === 'wakeword'}
						<section aria-live="polite">
							<h2 class="section-head">
							<span class="section-title he-display">{$t('hub.wakeword.title')}</span>
							<span class="section-en lat">· Wake words</span>
						</h2>
							<p class="field-hint">{$t('hub.wakeword.intro')}</p>

							{#snippet slotCard(
								slotKey: 'dictation' | 'command',
								enabled: boolean,
								sensitivity: number,
								model: string,
								setEnabled: (v: boolean) => void,
								setSensitivity: (v: number) => void,
								setModel: (v: string) => void,
								otherModel: string,
							)}
								<div class="wake-slot wake-slot--{slotKey}">
									<h3 class="slot-title slot-title--{slotKey}">
										{$t(`hub.wakeword.${slotKey}.title`)}
									</h3>
									<p class="field-hint">{$t(`hub.wakeword.${slotKey}.hint`)}</p>
									<div class="field">
										<span class="field-label">{$t('hub.wakeword.enable')}</span>
										<div class="segmented" role="group" aria-label={$t('hub.wakeword.enable')}>
											<button
												type="button"
												class="segment"
												class:selected={!enabled}
												aria-pressed={!enabled}
												onclick={() => void setEnabled(false)}
											>
												{$t('hub.wakeword.off')}
											</button>
											<button
												type="button"
												class="segment"
												class:selected={enabled}
												aria-pressed={enabled}
												onclick={() => void setEnabled(true)}
											>
												{$t('hub.wakeword.on')}
											</button>
										</div>
									</div>
									<div class="field wake-field">
										<span class="field-label">{$t('hub.wakeword.model')}</span>
										<p class="field-hint">{$t('hub.wakeword.modelHint')}</p>
										{#if wakeModels.length === 0}
											<p class="readings" dir="auto">{$t('hub.wakeword.noModels')}</p>
										{:else}
											<select
												class="select"
												aria-label={$t('hub.wakeword.model')}
												value={model}
												onchange={(event) => void setModel(event.currentTarget.value)}
											>
												{#if !model || !wakeModels.includes(model)}
													<option value="">{$t('hub.wakeword.pickModel')}</option>
												{/if}
												{#each wakeModels.filter((m) => m === model || m !== otherModel) as candidate}
													<option value={candidate}>{nicknameFor(candidate)}</option>
												{/each}
											</select>
										{/if}
									</div>
									<div class="field wake-field">
										<span class="field-label">{$t('hub.wakeword.sensitivity')}</span>
										<p class="field-hint">{$t('hub.wakeword.sensitivityHint')}</p>
										<input
											class="slider"
											type="range"
											min="0"
											max="1"
											step="0.05"
											value={sensitivity}
											aria-label={$t('hub.wakeword.sensitivity')}
											onchange={(event) => void setSensitivity(Number(event.currentTarget.value))}
										/>
									</div>
								</div>
							{/snippet}

							<div class="wake-slot-row">
								{@render slotCard(
									'dictation',
									dictationEnabled,
									dictationSensitivity,
									dictationModel,
									saveDictationEnabled,
									saveDictationSensitivity,
									saveDictationModel,
									commandModel,
								)}

								{@render slotCard(
									'command',
									commandEnabled,
									commandSensitivity,
									commandModel,
									saveCommandEnabled,
									saveCommandSensitivity,
									saveCommandModel,
									dictationModel,
								)}
							</div>

							<!-- Shared library: download more classifiers + train-your-own link.
							     Applies to both slots; lives once at the bottom of the section. -->
							<div class="wake-library">
								{#if wakeAvailable.some((m) => !m.installed)}
									<details class="wake-downloads">
										<summary>{$t('hub.wakeword.moreOptions')}</summary>
										<p class="field-hint">{$t('hub.wakeword.moreOptionsHint')}</p>
										{#each wakeAvailable.filter((m) => !m.installed) as available}
											<div class="download-row">
												<div class="download-info">
													<span class="download-name" dir="auto">{available.display_name}</span>
													<span class="badge nc" title={$t('hub.wakeword.ncTooltip')}>
														{$t('hub.wakeword.ncBadge')}
													</span>
												</div>
												<button
													type="button"
													class="install-btn"
													disabled={wakeInstalling !== null}
													onclick={() => void installWakeModel(available)}
												>
													{wakeInstalling === available.id
														? $t('hub.wakeword.installing')
														: `${$t('hub.wakeword.install')} · ${Math.round(available.bytes / 1024)} KB`}
												</button>
											</div>
										{/each}
										{#if wakeInstallError}
											<p class="install-error" dir="auto" role="alert">
												{$t('hub.wakeword.installFailed')}: {wakeInstallError}
											</p>
										{/if}
										<p class="field-hint">
											{$t('hub.wakeword.libraryHint')}
											<button
												type="button"
												class="train-link inline"
												onclick={() => openExternal('https://openwakeword.com/library')}
											>
												openwakeword.com/library →
											</button>
										</p>
									</details>
								{/if}
								<button
									class="train-link"
									type="button"
									onclick={() => openExternal('https://bustrama.github.io/lashon/wake-word-training/')}
								>
									{$t('hub.wakeword.trainGuide')} →
								</button>
							</div>
						</section>
					{:else if section === 'hardware'}
					<section aria-live="polite">
						<h2 class="section-head">
						<span class="section-title he-display">{$t('hub.hardware.title')}</span>
						<span class="section-en lat">· Hardware</span>
					</h2>
						<div class="field">
							<span class="field-label">{$t('hardware.label')}</span>
							<p class="field-hint">{$t('hub.hardware.intro')}</p>
							<p class="readings" dir="auto">
								{hardware === null ? $t('hub.hardware.detecting') : hwReadings}
							</p>
							<TierSelect value={tier} detected={hardware?.tier ?? null} onchange={saveTier} />
							{#if sttDeviceNote}
								<p class="field-hint" dir="auto">{sttDeviceNote}</p>
							{/if}
							{#if tierChanged}
								<p class="restart-hint" dir="auto">
									{$t('hub.hardware.restartToApply')}
								</p>
								<button
									class="restart-btn"
									type="button"
									onclick={restartApp}
								>
									{$t('hub.hardware.restartButton')}
								</button>
							{/if}
							<button
								class="detect"
								type="button"
								onclick={() => void detectHardware()}
								disabled={hwDetecting}
							>
								{$t('hub.hardware.detect')}
							</button>
						</div>
					</section>
								{:else if section === 'llm'}
					<section aria-live="polite" class="llm-section">
						<h2 class="section-head">
							<span class="section-title he-display">{$t('hub.llm.title')}</span>
							<span class="section-en lat">· Language models</span>
						</h2>
						<p class="field-hint llm-intro">{$t('hub.llm.intro')}</p>

						<!-- Mode tabs. The active tab's dot carries the mode color
						     (garnet for command, indigo for chat) so the LLM page maps
						     to the tongue's halo when a take is in flight. -->
						<div class="llm-tabs" role="tablist" aria-label={$t('hub.llm.title')}>
							{#each LLM_MODES as mode}
								<button
									type="button"
									class="llm-tab"
									class:active={llmActiveTab === mode}
									role="tab"
									aria-selected={llmActiveTab === mode}
									onclick={() => (llmActiveTab = mode)}
								>
									{#if llmActiveTab === mode}
										<span
											class="llm-tab-dot"
											style="background: {LLM_TAB_TINT[mode]};"
											aria-hidden="true"
										></span>
									{/if}
									<span class="he-sans">{$t(`hub.llm.${mode}`)}</span>
									<span class="lat llm-tab-en">{LLM_TAB_EN[mode]}</span>
								</button>
							{/each}
						</div>

						<p class="field-hint llm-mode-hint">{$t(`hub.llm.${llmActiveTab}Hint`)}</p>

						<div class="llm-eyebrow he-sans">{$t('hub.llm.pickProvider')}</div>
						<div
							class="provider-grid"
							role="group"
							aria-label={$t(`hub.llm.${llmActiveTab}`)}
						>
							<button
								type="button"
								class="provider-chip provider-chip-none"
								class:active={llmActive[llmActiveTab] === 'none'}
								aria-pressed={llmActive[llmActiveTab] === 'none'}
								onclick={() => void selectLlmProvider(llmActiveTab, 'none')}
							>
								<span class="chip-dot" aria-hidden="true"></span>
								<span class="chip-name he-sans">{$t('hub.llm.none')}</span>
							</button>
							{#each llmProviders as provider}
								<button
									type="button"
									class="provider-chip"
									class:active={llmActive[llmActiveTab] === provider.id}
									aria-pressed={llmActive[llmActiveTab] === provider.id}
									onclick={() => void selectLlmProvider(llmActiveTab, provider.id)}
								>
									<span class="chip-dot" aria-hidden="true"></span>
									<span class="chip-name he-sans" dir="auto">
										{$t(provider.display_name_key)}
									</span>
									{#if provider.supports_hebrew === 'Good' || provider.supports_hebrew === 'Excellent'}
										<span class="chip-hebrew" title={$t('hub.llm.hebrewOkTooltip')}>עברית</span>
									{:else if provider.supports_hebrew === 'Basic'}
										<span class="chip-hebrew unverified" title={$t('hub.llm.hebrewBasicTooltip')}>עברית~</span>
									{/if}
									{#if provider.default_model}
										<span class="chip-sub lat">{provider.default_model}</span>
									{/if}
									<span class="chip-kind">
										{#if provider.is_local}
											<span class="kind kind-local"><span class="kind-dot"></span>מקומי</span>
										{:else}
											<span class="kind kind-cloud">
												<svg width="10" height="8" viewBox="0 0 10 8" fill="none" aria-hidden="true">
													<path d="M2.4 7C1.2 7 .3 6.1.3 4.9c0-1.1.8-2 1.9-2.1C2.5 1.7 3.6 1 4.9 1c1.4 0 2.6.9 3 2.1.1 0 .2 0 .3 0 .9 0 1.5.7 1.5 1.5 0 .8-.6 1.4-1.5 1.4H2.4z" stroke="currentColor" stroke-width="0.7" fill="none"/>
												</svg>
												ענן
											</span>
										{/if}
									</span>
								</button>
							{/each}
						</div>

						{#if llmActive[llmActiveTab] !== 'none' && activeMeta(llmActiveTab)}
							{@const m = activeMeta(llmActiveTab)!}
							<div class="llm-detail">
								{#if isCloud(m.id) && m.id !== 'ollama-local'}
									<div class="llm-row">
										<div class="llm-row-label">
											<div class="he-sans llm-row-title">{$t('hub.llm.apiKey')}</div>
											<div class="he-sans llm-row-hint">{$t('hub.llm.cloudBadgeTooltip')}</div>
										</div>
										<div class="llm-row-control">
											{#if m.has_api_key}
												<p class="key-saved" dir="auto">
													●●●●●●●● ✓ {$t('hub.llm.apiKeySaved')}
													<button type="button" class="key-clear" onclick={() => void clearApiKey(m.id)}>
														{$t('hub.llm.apiKeyClear')}
													</button>
												</p>
											{:else}
												<div class="key-input-row">
													<input
														type="password"
														class="key-input"
														placeholder={$t('hub.llm.apiKeyPlaceholder')}
														bind:value={llmKeyInput[m.id]}
														autocomplete="off"
														aria-label={$t('hub.llm.apiKey')}
													/>
													<button
														type="button"
														class="key-save"
														disabled={llmKeySaving === m.id || !(llmKeyInput[m.id] ?? '').trim()}
														onclick={() => void saveApiKey(m.id)}
													>
														{$t('hub.llm.apiKeySave')}
													</button>
												</div>
											{/if}
										</div>
									</div>
								{/if}

								{#if m.id === 'ollama-local' && !ollama.running && !ollamaProbing}
									<p class="ollama-hint" dir="auto" role="status">
										{$t('hub.llm.ollamaNotRunning')}
										<button type="button" class="ollama-connect" onclick={() => void probeOllama()}>
											{$t('hub.llm.ollamaConnect')}
										</button>
									</p>
								{/if}

								{#if m.id === 'local-llm'}
									<!-- docs/adr/0025 — in-process local LLM: no daemon, no
									     loopback HTTP, just a GGUF on disk loaded via mistralrs.
									     The chip shows the model card, the download/ready state,
									     and the active model picker. -->
									{#if !localLlmStatus.runtime_available}
										<p class="ollama-hint" dir="auto" role="status">
											{$t('hub.llm.localLlm.runtimeMissing')}
										</p>
									{/if}
									{#each localLlmStatus.models as model (model.id)}
										{@const downloaded = localLlmDownloaded[model.id] ?? 0}
										{@const percent = localLlmPercent(model.id)}
										{@const isInstalling = localLlmInstalling === model.id}
										{@const isDeleting = localLlmDeleting === model.id}
										<div class="local-llm-card" dir="auto">
											<div class="local-llm-head">
												<div class="local-llm-title he-sans">{model.display_name}</div>
												<div class="local-llm-meta lat">
													{$t('hub.llm.localLlm.license').replace('{license}', model.license)}
													{#if model.bytes > 0}
														· {formatBytes(model.bytes)}
													{/if}
												</div>
											</div>
											<p class="local-llm-desc he-sans" dir="auto">{model.description}</p>
											<div class="local-llm-actions">
												{#if model.installed}
													<span class="local-llm-ready">
														✓ {model.bytes > 0
															? $t('hub.llm.localLlm.ready').replace('{size}', formatBytes(model.bytes))
															: $t('hub.llm.localLlm.sizeUnknown')}
													</span>
													<button
														type="button"
														class="key-clear"
														disabled={isDeleting}
														onclick={() => void deleteLocalLlm(model.id)}
													>
														{isDeleting
															? $t('hub.llm.localLlm.deleting')
															: $t('hub.llm.localLlm.delete')}
													</button>
												{:else if isInstalling}
													<span class="local-llm-progress">
														{#if percent !== null}
															{$t('hub.llm.localLlm.downloading').replace('{percent}', String(percent))}
															· {formatBytes(downloaded)}
														{:else}
															{$t('hub.llm.localLlm.installing')}
															{#if downloaded > 0}
																· {formatBytes(downloaded)}
															{/if}
														{/if}
													</span>
												{:else}
													<p class="local-llm-hint he-sans">
														{$t('hub.llm.localLlm.downloadRequired')}
													</p>
													<button
														type="button"
														class="key-save"
														disabled={!localLlmStatus.runtime_available || localLlmInstalling !== null}
														onclick={() => void installLocalLlm(model.id)}
													>
														{model.bytes > 0
															? $t('hub.llm.localLlm.download').replace('{bytes}', formatBytes(model.bytes))
															: $t('hub.llm.localLlm.downloadIndeterminate')}
													</button>
												{/if}
											</div>
											{#if isInstalling && percent !== null}
												<div
													class="local-llm-bar"
													role="progressbar"
													aria-valuenow={percent}
													aria-valuemin="0"
													aria-valuemax="100"
												>
													<div class="local-llm-bar-fill" style="width: {percent}%"></div>
												</div>
											{/if}
										</div>
									{/each}
									{#if localLlmError}
										<p class="ollama-hint" dir="auto" role="alert">{localLlmError}</p>
									{/if}
								{/if}

								<div class="llm-row">
									<div class="llm-row-label">
										<div class="he-sans llm-row-title">{$t('hub.llm.model')}</div>
									</div>
									<div class="llm-row-control">
										<div class="model-picker-row">
											<select
												class="select"
												aria-label={$t('hub.llm.model')}
												value={llmModel[llmActiveTab] || m.default_model}
												onchange={(event) =>
													void selectLlmModel(llmActiveTab, event.currentTarget.value)}
											>
												{#if m.id === 'ollama-local' && ollama.running && ollama.models.length > 0}
													{#each ollama.models as model}
														<option value={model}>{model}</option>
													{/each}
												{:else}
													{#each modelOptionsFor(m) as model}
														<option value={model}>
															{model}{m.recommended_model === model ? ` — ${$t('hub.llm.recommended')}` : ''}
														</option>
													{/each}
												{/if}
											</select>
											{#if m.id !== 'local-llm' && m.id !== 'ollama-local'}
												<button
													type="button"
													class="model-refresh-btn he-sans"
													aria-label={$t('hub.llm.refreshModels')}
													title={$t('hub.llm.refreshModels')}
													disabled={llmModelsFetching === m.id || !m.has_api_key}
													onclick={() => void refreshProviderModels(m.id)}
												>
													{llmModelsFetching === m.id
														? $t('hub.llm.refreshing')
														: $t('hub.llm.refreshModels')}
												</button>
											{/if}
										</div>
										{#if m.recommended_model}
											<p class="recommended-hint" dir="auto">
												{$t('hub.llm.recommendedHint').replace('{model}', m.recommended_model)}
											</p>
										{/if}
										{#if llmRemoteModels[m.id]}
											{@const result = llmRemoteModels[m.id]}
											{#if result.source === 'remote'}
												<p class="model-source-hint model-source-hint-remote" dir="auto">
													{result.total_count > result.models.length
														? $t('hub.llm.modelsShowingOf')
																.replace('{shown}', String(result.models.length))
																.replace('{total}', String(result.total_count))
														: $t('hub.llm.modelsFromProvider').replace('{count}', String(result.models.length))}
												</p>
											{:else if result.error}
												<p class="model-source-hint model-source-hint-fallback" dir="auto">
													{$t('hub.llm.modelsFallback')}{': '}{result.error}
												</p>
											{/if}
										{/if}
									</div>
								</div>

								{#if m.id !== 'local-llm'}
									<div class="llm-row">
										<div class="llm-row-label">
											<div class="he-sans llm-row-title">{$t('hub.llm.baseUrl')}</div>
											<div class="he-sans llm-row-hint">{$t('hub.llm.baseUrlHint')}</div>
										</div>
										<div class="llm-row-control">
											<input
												type="text"
												class="base-url-input"
												dir="ltr"
												placeholder={m.id === 'ollama-local' ? 'http://127.0.0.1:11434/v1' : ''}
												value={llmBaseUrl[m.id] ?? ''}
												aria-label={$t('hub.llm.baseUrl')}
												onchange={(event) =>
													void saveLlmBaseUrl(m.id, event.currentTarget.value.trim())}
											/>
										</div>
									</div>
								{/if}

								<div class="llm-row">
									<div class="llm-row-label">
										<div class="he-sans llm-row-title">{$t('hub.llm.testPrompt')}</div>
									</div>
									<div class="llm-row-control">
										<div class="test-prompt-row">
											<input
												type="text"
												class="test-input"
												dir="auto"
												placeholder={$t('hub.llm.testPromptPlaceholder')}
												bind:value={llmTestPrompt[llmActiveTab]}
												aria-label={$t('hub.llm.testPrompt')}
											/>
											<button
												type="button"
												class="test-send"
												disabled={llmTesting === llmActiveTab || llmActive[llmActiveTab] === 'none'}
												onclick={() => void runLlmTest(llmActiveTab)}
											>
												{llmTesting === llmActiveTab ? $t('hub.llm.testPromptSending') : $t('hub.llm.testPromptSend')}
											</button>
										</div>
										{#if llmTestResult[llmActiveTab]}
											<div class="test-result" dir="auto" aria-live="polite">
												<span class="test-result-label">{$t('hub.llm.testPromptResult')}</span>
												<p class="test-result-body" dir="auto">{llmTestResult[llmActiveTab]}</p>
											</div>
										{/if}
										{#if llmTestError[llmActiveTab]}
											<p class="test-error" dir="auto" role="alert">
												{$t('hub.llm.testPromptError')}: {llmTestError[llmActiveTab]}
											</p>
										{/if}
									</div>
								</div>
							</div>
						{/if}
					</section>
				{:else if section === 'recipes'}
					<RecipesSection onopenmcp={() => openExternal('https://bustrama.github.io/lashon/')} />
				{:else if section === 'voice'}
					<VoiceCorrectionsSection />
				{:else}
					<section aria-live="polite">
						<h2 class="section-head">
						<span class="section-title he-display">{$t('hub.about.title')}</span>
						<span class="section-en lat">· About</span>
					</h2>
						<div class="about">
							<img class="about-mark" src="/lashon-mark.svg" alt="" draggable="false" />
							<p class="tagline">{$t('hub.about.tagline')}</p>
							{#if version}
								<p class="version">{$t('hub.about.version')} {version}</p>
							{/if}
							<p class="local-first">{$t('hub.about.localFirst')}</p>

							<!-- Auto-update control — the user drives the flow, no OS dialog. -->
							<div class="update-row">
								<button
									class="update-btn"
									class:update-busy={updateBusy}
									class:update-installed={updateStatus === 'installed'}
									class:update-error={updateStatus === 'error'}
									type="button"
									disabled={updateBusy}
									onclick={() => {
										if (updateStatus === 'installed') {
											restartApp();
										} else {
											void checkForUpdates();
										}
									}}
									aria-busy={updateBusy}
								>
									{#if updateStatus === 'installed'}
										{$t('hub.about.restartToFinish')}
									{:else}
										{updateStatusLabel()}
									{/if}
								</button>
								{#if updateStatus === 'up-to-date' || updateStatus === 'error'}
									<button
										class="update-retry"
										type="button"
										onclick={() => void checkForUpdates()}
									>
										{$t('hub.about.checkUpdates')}
									</button>
								{/if}
							</div>

							<div class="links">
								{#each LINKS as link}
									<button
										class="link"
										type="button"
										onclick={() => openExternal(link.url)}
									>
										<span class="link-label">{$t(link.key)}</span>
										<span class="link-url" dir="ltr">{link.text}</span>
									</button>
								{/each}
							</div>
						</div>
					</section>
				{/if}
			</div>
		</div>
	</div>
</div>

<style>
	/* ── REDESIGN — "Lamp" Hub ────────────────────────────────────────────
	   Cool slate study; the peach mark in the sidebar is the brand's single
	   warm note. The window itself is transparent + borderless; everything
	   visible is the .card slab inside it. Mirrors the redesigned tongue's
	   token palette so the two surfaces feel like one product, not two. */

	/* Local Hebrew display / sans / mono utility classes — these mirror the
	   global ones used in the Tongue but are duplicated locally because
	   Svelte CSS is scoped by default; we want them addressable from the
	   hub markup without needing :global() throughout. */
	.he-display {
		font-family: var(--font-he-display);
		direction: rtl;
	}
	.he-sans {
		font-family: var(--font-he-sans);
		direction: rtl;
	}
	.lat {
		font-family: var(--font-lat-sans);
		direction: ltr;
	}
	.mono {
		font-family: var(--font-mono);
		direction: ltr;
	}

	/* The hub window is frameless and transparent — only the .card slab is
	   painted, with see-through padding around it for the drop-shadow. */
	.hub {
		position: fixed;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 18px;
		box-sizing: border-box;
		background: transparent;
	}

	.card {
		display: flex;
		flex-direction: column;
		width: 100%;
		max-width: 960px;
		height: 100%;
		box-sizing: border-box;
		border-radius: 12px;
		overflow: hidden;
		background: var(--ink);
		color: var(--ink-text);
		direction: rtl;
		font-family: var(--font-he-sans);
		box-shadow: 0 24px 80px rgba(0, 0, 0, 0.5);
	}

	/* ── Title bar ─────────────────────────────────────────────────────── */
	.title-bar {
		flex: 0 0 auto;
		height: 36px;
		background: var(--ink-2);
		border-bottom: 1px solid var(--ink-line);
		display: flex;
		align-items: center;
		padding: 0 12px;
		cursor: grab;
		/* LTR so the close-X sits on the visual left like a mac control,
		   matching the redesign source's traffic-light placement without
		   adopting the cosmetic dots. */
		direction: ltr;
	}
	.title-bar:active {
		cursor: grabbing;
	}

	.close {
		flex: 0 0 auto;
		width: 24px;
		height: 22px;
		display: flex;
		align-items: center;
		justify-content: center;
		font-size: 11px;
		font-family: inherit;
		color: var(--ink-faint);
		background: transparent;
		border: none;
		border-radius: 5px;
		cursor: pointer;
		transition:
			background 0.15s ease,
			color 0.15s ease;
	}
	.close:hover {
		background: var(--state-error);
		color: #fff;
	}
	.close:focus-visible {
		outline: 2px solid var(--garnet);
		outline-offset: 1px;
	}

	.title-text {
		flex: 1;
		text-align: center;
		font-size: 12.5px;
		font-weight: 500;
		color: var(--ink-mute);
		display: inline-flex;
		justify-content: center;
		align-items: baseline;
		gap: 0;
	}
	.title-sep {
		margin: 0 6px;
		opacity: 0.4;
	}
	.title-section {
		font-weight: 500;
	}
	.title-spacer {
		width: 24px;
		height: 22px;
		flex: 0 0 auto;
	}

	/* ── Body: sidebar + detail ───────────────────────────────────────── */
	.body {
		flex: 1 1 auto;
		display: flex;
		min-height: 0;
		overflow: hidden;
	}

	/* Sidebar lives on the trailing edge in RTL — Hebrew reading direction
	   puts navigation on the right. The 240px width matches the redesign
	   source; the inner padding hugs the brand + nav with breathing room. */
	.sidebar {
		flex: 0 0 240px;
		display: flex;
		flex-direction: column;
		gap: 0;
		padding: 20px 12px;
		background: var(--ink-2);
		border-inline-start: 1px solid var(--ink-line);
		overflow: auto;
	}

	.sidebar-brand {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 0 8px 14px;
		border-bottom: 1px solid var(--ink-line);
		margin-bottom: 12px;
	}
	.sidebar-brand-text {
		display: flex;
		flex-direction: column;
	}
	.sidebar-brand-name {
		font-size: 16px;
		font-weight: 500;
		letter-spacing: 0.5px;
		color: var(--ink-text);
	}
	.sidebar-brand-meta {
		font-size: 9.5px;
		color: var(--ink-faint);
		direction: ltr;
		text-transform: lowercase;
	}
	.sidebar-brand-meta .dot {
		margin: 0 4px;
		opacity: 0.5;
	}

	.nav {
		display: flex;
		flex-direction: column;
		gap: 1px;
	}

	.nav-item {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		border-radius: 7px;
		background: transparent;
		border: none;
		color: var(--ink-mute);
		font-weight: 500;
		cursor: pointer;
		font-family: inherit;
		text-align: start;
		transition:
			background 0.12s ease,
			color 0.12s ease;
	}
	.nav-item:hover {
		background: rgba(221, 228, 233, 0.04);
		color: var(--ink-text);
	}
	.nav-item.active {
		background: color-mix(in srgb, var(--saffron) 12%, transparent);
		color: var(--saffron);
		font-weight: 600;
	}
	.nav-item:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	.nav-rail {
		width: 3px;
		height: 16px;
		border-radius: 2px;
		background: transparent;
		flex: 0 0 auto;
		transition: background 0.12s ease;
	}
	.nav-item.active .nav-rail {
		background: var(--saffron);
	}
	.nav-he {
		font-size: 13.5px;
		flex: 1;
	}
	.nav-en {
		font-size: 9px;
		opacity: 0.5;
		direction: ltr;
	}
	.nav-item.active .nav-en {
		opacity: 0.85;
	}

	.detail {
		flex: 1 1 auto;
		padding: 32px 36px;
		overflow-y: auto;
		background: var(--ink);
		scrollbar-width: thin;
		scrollbar-color: var(--ink-line-2) transparent;
	}

	/* ── Section header — Hebrew title + English italic sublabel ──────── */
	.section-head {
		margin: 0 0 28px;
		padding: 0 0 18px;
		border-bottom: 1px solid var(--ink-line);
		display: flex;
		align-items: baseline;
		gap: 10px;
		font-weight: 500;
	}
	.section-title {
		font-size: 26px;
		font-weight: 500;
		letter-spacing: 0.3px;
		color: var(--ink-text);
	}
	.section-en {
		font-size: 12px;
		font-weight: 400;
		color: var(--ink-faint);
		font-style: italic;
	}

	/* Any other h2 inside the detail pane (legacy use, if any) inherits
	   reasonable defaults so it doesn't fall back to UA stark-big-bold. */
	h2 {
		margin: 0 0 20px;
		font-size: 20px;
		font-weight: 600;
		color: var(--ink-text);
		font-family: var(--font-he-sans);
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 6px;
		max-width: 460px;
	}

	.field-label {
		font-size: 14px;
		font-weight: 700;
		color: var(--text-primary);
	}

	.field-hint {
		margin: 0 0 8px;
		font-size: 13px;
		line-height: 1.6;
		color: var(--text-muted);
	}

	/* The "we recommend X for fast + accurate Command-mode usage" hint
	   below the model dropdown. Citron accent so it reads as a nudge,
	   not an error. */
	.recommended-hint {
		margin: 6px 0 0;
		font-size: 12px;
		line-height: 1.5;
		color: var(--accent-citron);
	}

	/* The model picker row: select + Refresh button side-by-side. */
	.model-picker-row {
		display: flex;
		gap: 8px;
		align-items: center;
		min-width: 0;
		max-width: 100%;
	}
	.model-picker-row .select {
		/* flex: 1 1 0 + min-width: 0 lets the select shrink below its
		   intrinsic content width — without these, a long option id
		   like `ft:gpt-4o:org:custom:2026-05-25:xyz123` makes the
		   select widen and overflow the Hub column horizontally. */
		flex: 1 1 0;
		min-width: 0;
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.model-picker-row .select option {
		max-width: 100%;
	}
	.model-refresh-btn {
		flex: 0 0 auto;
		padding: 6px 10px;
		border-radius: 8px;
		border: 1px solid var(--stroke-subtle);
		background: var(--bg-deep);
		color: var(--text-secondary);
		font-size: 12px;
		cursor: pointer;
		white-space: nowrap;
	}
	.model-refresh-btn:hover:not(:disabled) {
		background: var(--bg-elevated, rgba(255, 255, 255, 0.04));
		color: var(--text-primary, var(--ink-text));
	}
	.model-refresh-btn:disabled {
		opacity: 0.45;
		cursor: not-allowed;
	}
	.model-source-hint {
		margin: 6px 0 0;
		font-size: 11px;
		line-height: 1.5;
	}
	.model-source-hint-remote {
		color: var(--text-tertiary, var(--ink-faint));
		opacity: 0.7;
	}
	.model-source-hint-fallback {
		color: var(--state-error);
	}

	/* The detected RAM / GPU readout in the Hardware section. */
	.readings {
		margin: 0 0 4px;
		padding: 9px 13px;
		border-radius: 11px;
		background: var(--bg-deep);
		border: 1px solid var(--stroke-subtle);
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
	}

	/* Shown when the tier is changed — the STT device only switches on the
	   next launch, so a change is acknowledged rather than silent. */
	.restart-hint {
		margin: 2px 0 0;
		font-size: 13px;
		font-weight: 700;
		line-height: 1.6;
		color: var(--accent-citron);
	}

	/* The one-click relaunch — the call to action when a tier is changed. */
	.restart-btn {
		align-self: flex-start;
		margin-top: 8px;
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: #1a1709;
		background: var(--accent-citron);
		border: none;
		border-radius: 9px;
		padding: 8px 18px;
		cursor: pointer;
		transition: filter 0.15s ease;
	}
	.restart-btn:hover {
		filter: brightness(1.08);
	}
	.restart-btn:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	/* "Detect again" — a quiet secondary button under the tier picker. */
	.detect {
		align-self: flex-start;
		margin-top: 4px;
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-strong);
		border-radius: 9px;
		padding: 8px 16px;
		cursor: pointer;
		transition:
			border-color 0.15s ease,
			color 0.15s ease;
	}
	.detect:hover {
		border-color: var(--text-muted);
		color: var(--text-primary);
	}
	.detect:disabled {
		cursor: default;
		color: var(--text-muted);
	}
	.detect:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	/* Two wake slots (Dictation + Command) sit side-by-side. Equal-width
	   columns so the alternative pickers line up — easier to compare the
	   two slot configs at a glance than the stacked layout. Collapses to
	   a single column under ~560 px (the Hub itself is 960 px wide, but
	   the embedded webview is occasionally resized for screenshots and
	   the smaller layout costs nothing). */
	.wake-slot-row {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 16px;
		margin-top: 12px;
	}
	@media (max-width: 560px) {
		.wake-slot-row {
			grid-template-columns: 1fr;
		}
	}

	/* Each slot card gets a subtle outlined panel so the two configs read
	   as parallel units, not as a long flat scroll of fields. Reuses the
	   same ink-line border + ink-2 background the Hub uses for other
	   panels (search for `background: var(--ink-2)` for siblings). */
	.wake-slot {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 14px 16px 16px;
		border: 1px solid var(--ink-line);
		border-radius: 8px;
		background: var(--ink-2);
		min-width: 0; /* let the field children shrink instead of overflowing */
	}

	/* Slot title — centered, pill-shaped highlight whose colour mirrors
	   the mode the slot triggers (matching the design-system convention:
	   citron for dictation, aqua for command). The pill is a soft tint
	   so it reads as a chip rather than a full-bleed banner. Slight
	   negative top margin so it visually tucks against the card's top
	   edge while the inner content keeps its breathing room.

	   `color-mix` gives us a 14-18 % tint of the accent colour so the
	   pill is unmistakably citron / aqua without overwhelming the
	   card's neutral background. Falls back gracefully on browsers
	   without color-mix (the text colour still carries the signal). */
	.slot-title {
		margin: -2px auto 8px;
		padding: 4px 14px;
		font-size: 13px;
		font-weight: 700;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		text-align: center;
		align-self: center;
		border-radius: 999px;
		color: var(--ink-text);
		background: color-mix(in srgb, var(--ink-line) 60%, transparent);
		border: 1px solid var(--ink-line);
	}
	.slot-title--dictation {
		color: var(--accent-citron);
		background: color-mix(in srgb, var(--accent-citron) 14%, transparent);
		border-color: color-mix(in srgb, var(--accent-citron) 40%, transparent);
	}
	.slot-title--command {
		color: var(--accent-aqua);
		background: color-mix(in srgb, var(--accent-aqua) 14%, transparent);
		border-color: color-mix(in srgb, var(--accent-aqua) 40%, transparent);
	}

	/* The wake-word sensitivity field, spaced off the enable control above it. */
	.wake-field {
		margin-top: 20px;
	}

	/* The wake-word sensitivity slider. */
	.slider {
		align-self: flex-start;
		width: 240px;
		accent-color: var(--accent-citron);
		cursor: pointer;
	}
	.slider:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 4px;
	}

	/* The wake-phrase picker dropdown. The native browser chevron renders on
	   the wrong side in RTL mode and looks misaligned next to mixed-script
	   labels like "Hey Lashon", so we draw our own and pin it to the trailing
	   edge — right side in LTR, left in RTL — with logical padding. */
	.select {
		align-self: flex-start;
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--text-primary);
		background: var(--bg-deep);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
		padding: 8px 12px;
		padding-inline-end: 32px;
		cursor: pointer;
		appearance: none;
		-webkit-appearance: none;
		background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 8' fill='none' stroke='%23eae7da' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='M1 1.5l5 5 5-5'/></svg>");
		background-repeat: no-repeat;
		background-size: 10px 7px;
		background-position: right 12px center;
	}
	.select:dir(rtl) {
		background-position: left 12px center;
	}
	.select:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	/* "How to train your own" link in the Wake-word section. */
	.train-link {
		align-self: flex-start;
		margin-top: 4px;
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--accent-aqua);
		background: transparent;
		border: none;
		padding: 4px 0;
		cursor: pointer;
		text-decoration: underline;
		text-decoration-color: rgba(63, 203, 192, 0.4);
		text-underline-offset: 3px;
		transition: text-decoration-color 0.15s ease;
	}
	.train-link:hover {
		text-decoration-color: var(--accent-aqua);
	}
	.train-link:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}
	.train-link.inline {
		display: inline;
		margin: 0;
		font-size: inherit;
	}

	/* Collapsible "More wake words" section under the picker. */
	.wake-downloads {
		margin-top: 8px;
		align-self: stretch;
		max-width: 480px;
		padding: 10px 14px;
		border-radius: 10px;
		background: var(--bg-deep);
		border: 1px solid var(--stroke-subtle);
	}
	.wake-downloads summary {
		cursor: pointer;
		font-size: 13px;
		color: var(--text-primary);
		list-style: none;
		padding: 4px 0;
	}
	.wake-downloads summary::-webkit-details-marker {
		display: none;
	}
	.wake-downloads summary::before {
		content: '▸';
		display: inline-block;
		margin-inline-end: 6px;
		font-size: 10px;
		transition: transform 0.15s ease;
	}
	.wake-downloads[open] summary::before {
		transform: rotate(90deg);
	}
	.download-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding: 8px 0;
		border-top: 1px solid var(--stroke-subtle);
	}
	.download-info {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.download-name {
		font-size: 13px;
		color: var(--text-primary);
	}
	.badge {
		font-size: 10px;
		padding: 2px 7px;
		border-radius: 6px;
		font-family: inherit;
		letter-spacing: 0.02em;
		white-space: nowrap;
	}
	.badge.nc {
		background: rgba(220, 130, 50, 0.18);
		color: #f3a861;
		border: 1px solid rgba(220, 130, 50, 0.4);
	}
	.install-btn {
		font-family: inherit;
		font-size: 12px;
		color: var(--text-primary);
		background: transparent;
		border: 1px solid var(--stroke-subtle);
		border-radius: 8px;
		padding: 6px 12px;
		cursor: pointer;
		white-space: nowrap;
		transition: background 0.15s ease, border-color 0.15s ease;
	}
	.install-btn:hover:not(:disabled) {
		background: var(--bg-elevated);
		border-color: var(--accent-aqua);
	}
	.install-btn:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}
	.install-btn:disabled {
		opacity: 0.6;
		cursor: progress;
	}
	.install-error {
		font-size: 12px;
		color: #f37070;
		margin-top: 8px;
	}

	/* Segmented picker. The Lamp design uses a SUBTLE selected state
	   (slate-4 on slate-2 with ink-text) rather than a punchy saffron
	   fill — the saffron accent is reserved for nav-item active and
	   the brand-recognised primary buttons (restart-btn etc). */
	.segmented {
		display: inline-flex;
		align-self: flex-start;
		padding: 3px;
		gap: 0;
		border-radius: 8px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		border: none;
	}

	.segment {
		font-family: inherit;
		font-size: 12.5px;
		font-weight: 600;
		color: var(--ink-mute);
		background: transparent;
		border: none;
		border-radius: 6px;
		padding: 5px 14px;
		cursor: pointer;
		transition:
			background 0.15s ease,
			color 0.15s ease;
	}
	.segment:hover {
		color: var(--ink-text);
	}
	.segment.selected {
		background: var(--ink-4);
		color: var(--ink-text);
		box-shadow: 0 1px 0 rgba(255, 255, 255, 0.05) inset;
	}
	.segment:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	.about {
		display: flex;
		flex-direction: column;
		gap: 12px;
		max-width: 460px;
	}

	.about-mark {
		width: 64px;
		height: 64px;
		filter: drop-shadow(0 4px 14px rgba(0, 0, 0, 0.5));
	}

	.tagline {
		margin: 0;
		font-size: 16px;
		font-weight: 500;
		color: var(--text-primary);
	}

	.version {
		margin: 0;
		font-size: 14px;
		font-weight: 700;
		color: var(--accent-citron);
	}

	.local-first {
		margin: 0;
		font-size: 13px;
		line-height: 1.7;
		color: var(--text-secondary);
	}

	.links {
		display: flex;
		flex-direction: column;
		gap: 6px;
		margin-top: 4px;
	}

	.link {
		display: flex;
		gap: 10px;
		align-items: baseline;
		font-family: inherit;
		text-align: start;
		background: transparent;
		border: 1px solid transparent;
		border-radius: 9px;
		padding: 7px 10px;
		cursor: pointer;
		transition:
			background 0.15s ease,
			border-color 0.15s ease;
	}
	.link:hover {
		background: var(--bg-glass);
		border-color: var(--stroke-subtle);
	}
	.link:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	.link-label {
		flex: 0 0 auto;
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
	}

	.link-url {
		font-size: 13px;
		color: var(--accent-aqua);
	}

	/* Update-check row in the About section. */
	.update-row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		margin-block-start: 4px;
	}

	/* The primary "Check for updates / Restart" button. Shares the secondary
	   detect-button aesthetic but goes citron when an install is ready. */
	.update-btn {
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-strong);
		border-radius: 9px;
		padding: 8px 16px;
		cursor: pointer;
		transition:
			border-color 0.15s ease,
			color 0.15s ease,
			background 0.15s ease;
	}
	.update-btn:hover:not(:disabled) {
		border-color: var(--text-muted);
		color: var(--text-primary);
	}
	.update-btn.update-busy {
		cursor: progress;
		opacity: 0.7;
	}
	.update-btn.update-installed {
		background: var(--accent-citron);
		color: #1a1709;
		border-color: transparent;
	}
	.update-btn.update-installed:hover {
		filter: brightness(1.08);
	}
	.update-btn.update-error {
		border-color: rgba(243, 112, 112, 0.5);
		color: #f37070;
	}
	.update-btn:disabled {
		cursor: progress;
	}
	.update-btn:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	/* "Check again" link shown when the state is up-to-date or error. */
	.update-retry {
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--accent-aqua);
		background: transparent;
		border: none;
		padding: 4px 0;
		cursor: pointer;
		text-decoration: underline;
		text-decoration-color: rgba(63, 203, 192, 0.4);
		text-underline-offset: 3px;
	}
	.update-retry:hover {
		text-decoration-color: var(--accent-aqua);
	}
	.update-retry:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	/* LLM section. Two sub-pickers (command + chat); each one is a chip grid
	   of providers plus the inline reveal for the active provider's key,
	   model, base URL, and test-prompt controls. RTL-native — chips flow
	   from start to end via flex; the cloud/Hebrew badges use logical
	   margin-inline so they sit on the right edge in he and the left in en. */
	/* M0 / M7 legacy LLM styles (`.llm-mode`, `.chip*`, `.badge.*`) removed —
	   superseded by the Lamp `.llm-tabs` / `.provider-chip` / `.chip-kind`
	   structure defined below. */

	/* The API-key entry input — masked password field, full-width, with a
	   "Save" button that disables when the input is empty. */
	.key-input-row {
		display: flex;
		gap: 8px;
		align-items: center;
	}
	.key-input {
		flex: 1 1 auto;
		font-family: inherit;
		font-size: 13px;
		color: var(--text-primary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
		padding: 8px 12px;
	}
	.key-input:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
		border-color: var(--accent-aqua);
	}
	.key-save {
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: #1a1709;
		background: var(--accent-citron);
		border: none;
		border-radius: 9px;
		padding: 8px 16px;
		cursor: pointer;
	}
	.key-save:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.key-saved {
		margin: 0;
		font-size: 13px;
		color: var(--text-secondary);
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.key-clear {
		font-family: inherit;
		font-size: 12px;
		color: var(--accent-aqua);
		background: transparent;
		border: none;
		padding: 4px 0;
		cursor: pointer;
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	.ollama-hint {
		margin: 0;
		padding: 10px 14px;
		font-size: 13px;
		color: var(--text-secondary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
	}
	.ollama-connect {
		font-family: inherit;
		font-size: 12px;
		color: var(--accent-aqua);
		background: transparent;
		border: none;
		padding: 4px 0;
		margin-inline-start: 8px;
		cursor: pointer;
		text-decoration: underline;
		text-underline-offset: 3px;
	}

	/* docs/adr/0025 — in-process local-LLM model card. Sits inside the
	   provider's llm-detail block, one card per model offered by the
	   manifest. The "Ready" / "Download" / "Downloading…" state is a
	   pill on the right; the progress bar is full-width beneath. */
	.local-llm-card {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 12px 14px;
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
		margin-block-end: 8px;
	}
	.local-llm-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 12px;
	}
	.local-llm-title {
		font-size: 14px;
		font-weight: 600;
		color: var(--text-primary);
	}
	.local-llm-meta {
		font-size: 12px;
		color: var(--text-muted);
	}
	.local-llm-desc {
		margin: 0;
		font-size: 12px;
		color: var(--text-secondary);
		line-height: 1.45;
	}
	.local-llm-actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		margin-block-start: 4px;
	}
	.local-llm-ready {
		font-size: 12px;
		color: var(--accent-aqua);
		font-weight: 500;
	}
	.local-llm-progress {
		font-size: 12px;
		color: var(--text-secondary);
		font-family: ui-monospace, monospace;
	}
	.local-llm-hint {
		margin: 0;
		font-size: 12px;
		color: var(--text-secondary);
		flex: 1 1 auto;
	}
	.local-llm-bar {
		width: 100%;
		height: 4px;
		background: var(--bg-deep);
		border-radius: 2px;
		overflow: hidden;
		margin-block-start: 6px;
	}
	.local-llm-bar-fill {
		height: 100%;
		background: var(--accent-aqua);
		transition: width 0.15s ease-out;
	}

	/* The base URL is now its own llm-row (no longer collapsed inside a
	   <details>) — the related .base-url / .base-url[open] styles are
	   removed; .base-url-input below is still used as the row's input. */
	.base-url-input {
		width: 100%;
		font-family: ui-monospace, monospace;
		font-size: 12px;
		color: var(--text-primary);
		background: var(--bg-deep);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
		padding: 8px 12px;
		margin-block-start: 6px;
		box-sizing: border-box;
	}
	.base-url-input:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
		border-color: var(--accent-aqua);
	}

	.test-prompt-row {
		display: flex;
		gap: 8px;
	}
	.test-input {
		flex: 1 1 auto;
		font-family: inherit;
		font-size: 13px;
		color: var(--text-primary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-subtle);
		border-radius: 9px;
		padding: 8px 12px;
	}
	.test-input:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
		border-color: var(--accent-aqua);
	}
	.test-send {
		font-family: inherit;
		font-size: 13px;
		font-weight: 700;
		color: var(--text-secondary);
		background: var(--bg-elevated);
		border: 1px solid var(--stroke-strong);
		border-radius: 9px;
		padding: 8px 16px;
		cursor: pointer;
	}
	.test-send:hover:not(:disabled) {
		border-color: var(--text-muted);
		color: var(--text-primary);
	}
	.test-send:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.test-send:focus-visible {
		outline: 3px solid var(--accent-aqua);
		outline-offset: 2px;
	}

	.test-result {
		margin-block-start: 8px;
		padding: 10px 14px;
		border-radius: 9px;
		background: var(--bg-elevated);
		border: 1px solid rgba(63, 203, 192, 0.3);
	}
	.test-result-label {
		font-size: 11px;
		font-weight: 700;
		color: var(--accent-aqua);
		text-transform: uppercase;
		letter-spacing: 0.05em;
	}
	.test-result-body {
		margin: 4px 0 0;
		font-size: 13px;
		line-height: 1.7;
		color: var(--text-primary);
		white-space: pre-wrap;
	}
	.test-error {
		margin-block-start: 6px;
		font-size: 12px;
		color: #f37070;
	}

	@media (prefers-reduced-motion: reduce) {
		.close,
		.nav-item,
		.segment,
		.link,
		.detect,
		.restart-btn,
		.train-link,
		.update-btn,
		.update-retry,
		.provider-chip,
		.llm-tab {
			transition: none;
		}
	}

	/* ── REDESIGN — LLM section ──────────────────────────────────────────
	   Mode tabs at the top → eyebrow → provider grid → settings rows.
	   Matches the Lamp design source. The previous M7/M8 chip-grid layout
	   is replaced; the .chip / .badge / .base-url styles above are now
	   dead code (kept for one PR cycle in case anything else references
	   them, but the svelte-check warns about them — clean follow-up). */

	.llm-intro {
		margin-bottom: 18px;
	}

	/* Mode tabs — `--ink-2` capsule with `--ink-4` selected segment. The
	   dot on the active tab carries the mode color (garnet for command,
	   indigo for chat). */
	.llm-tabs {
		display: inline-flex;
		gap: 4px;
		padding: 4px;
		border-radius: 10px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		margin: 0 0 14px;
	}
	.llm-tab {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		padding: 7px 16px;
		border-radius: 7px;
		background: transparent;
		border: none;
		color: var(--ink-mute);
		font-family: inherit;
		font-weight: 600;
		font-size: 13px;
		cursor: pointer;
		transition:
			background 0.12s ease,
			color 0.12s ease;
	}
	.llm-tab:hover {
		color: var(--ink-text);
	}
	.llm-tab.active {
		background: var(--ink-4);
		color: var(--ink-text);
		box-shadow: 0 1px 0 rgba(255, 255, 255, 0.05) inset;
	}
	.llm-tab:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}
	.llm-tab-dot {
		width: 6px;
		height: 6px;
		border-radius: 999px;
		flex: 0 0 auto;
	}
	.llm-tab-en {
		font-size: 10.5px;
		opacity: 0.55;
	}

	.llm-mode-hint {
		margin: 0 0 22px;
	}

	.llm-eyebrow {
		font-size: 11.5px;
		color: var(--ink-faint);
		text-transform: uppercase;
		letter-spacing: 1px;
		margin-bottom: 10px;
		font-weight: 700;
	}

	/* Provider grid — 2 columns of provider-chip cards. */
	.provider-grid {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: 10px;
		margin-bottom: 28px;
		max-width: 720px;
	}

	.provider-chip {
		position: relative;
		display: grid;
		grid-template-columns: auto 1fr auto;
		align-items: center;
		gap: 10px 12px;
		padding: 12px 14px;
		border-radius: 10px;
		background: var(--ink-2);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
		color: var(--ink-text);
		font-family: inherit;
		text-align: start;
		cursor: pointer;
		transition:
			background 0.12s ease,
			box-shadow 0.12s ease;
	}
	.provider-chip:hover {
		background: var(--ink-3);
	}
	.provider-chip.active {
		background: color-mix(in srgb, var(--saffron) 10%, var(--ink-2));
		box-shadow: inset 0 0 0 1.5px var(--saffron);
	}
	.provider-chip:focus-visible {
		outline: 2px solid var(--saffron);
		outline-offset: 2px;
	}

	.provider-chip-none {
		grid-template-columns: auto 1fr;
	}

	/* Radio dot at the start of each chip. */
	.chip-dot {
		grid-row: 1 / span 2;
		width: 10px;
		height: 10px;
		border-radius: 999px;
		flex: 0 0 auto;
		background: transparent;
		box-shadow: inset 0 0 0 1.5px var(--ink-faint);
		align-self: start;
		margin-top: 5px;
	}
	.provider-chip.active .chip-dot {
		background: var(--saffron);
		box-shadow: none;
	}

	/* Name + Hebrew badge on the first row of the chip. */
	.chip-name {
		font-size: 14px;
		font-weight: 600;
		color: var(--ink-text);
	}

	/* Hebrew badge — green pill (state-success) so it reads as "this
	   provider speaks Hebrew well", not as a state alert. */
	.chip-hebrew {
		display: inline-block;
		margin-inline-start: 6px;
		padding: 1px 6px;
		border-radius: 999px;
		font-family: var(--font-he-sans);
		font-size: 9.5px;
		font-weight: 700;
		letter-spacing: 0.3px;
		background: color-mix(in srgb, var(--state-success) 22%, transparent);
		color: var(--state-success);
	}
	.chip-hebrew.unverified {
		background: color-mix(in srgb, var(--ink-faint) 22%, transparent);
		color: var(--ink-mute);
	}

	/* Subtitle (default model id) — small lat, faint. */
	.chip-sub {
		grid-column: 2;
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--ink-faint);
		text-align: start;
	}

	/* Cloud / local badge on the trailing edge. */
	.chip-kind {
		grid-row: 1 / span 2;
		grid-column: 3;
		align-self: start;
		margin-top: 2px;
	}
	.kind {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 2px 8px;
		border-radius: 999px;
		font-family: var(--font-he-sans);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.2px;
	}
	.kind-cloud {
		color: var(--ink-text);
		background: rgba(221, 228, 233, 0.06);
		box-shadow: inset 0 0 0 1px var(--ink-line-2);
	}
	.kind-local {
		color: var(--saffron);
		background: color-mix(in srgb, var(--saffron) 10%, transparent);
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--saffron) 35%, transparent);
	}
	.kind-dot {
		width: 5px;
		height: 5px;
		border-radius: 999px;
		background: var(--saffron);
		display: inline-block;
	}

	/* ── LLM settings rows — label on the trailing edge (RTL), control
	     on the leading edge. Same pattern the design source uses. ── */
	.llm-detail {
		display: flex;
		flex-direction: column;
		max-width: 720px;
	}
	.llm-row {
		display: flex;
		align-items: flex-start;
		gap: 20px;
		padding: 14px 0;
		border-bottom: 1px solid var(--ink-line);
	}
	.llm-row:last-child {
		border-bottom: none;
	}
	.llm-row-label {
		flex: 0 0 200px;
	}
	.llm-row-title {
		font-size: 13.5px;
		font-weight: 600;
		color: var(--ink-text);
	}
	.llm-row-hint {
		font-size: 11.5px;
		color: var(--ink-faint);
		margin-top: 3px;
		line-height: 1.4;
	}
	.llm-row-control {
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 8px;
		align-items: flex-start;
	}
</style>
