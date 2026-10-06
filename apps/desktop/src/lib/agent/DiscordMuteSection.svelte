<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { t } from '$lib/i18n';
    import { getSetting, setSetting } from '$lib/settings';
    let supported = $state(false);
    let enabled = $state(false);
    let verified = $state(false);
    let busy = $state(false);
    let message = $state<string | null>(null);
    onMount(() => {
        void invoke<boolean>('discord_mute_supported').then((v) => (supported = v)).catch(() => {});
        void getSetting('discordMute.enabled').then((v) => (enabled = v));
        void getSetting('discordMute.verified').then((v) => (verified = v));
    });
    async function verify(value: boolean) {
        verified = value;
        await setSetting('discordMute.verified', value);
        if (!value) await enable(false);
    }
    async function enable(value: boolean) {
        enabled = value && verified;
        await setSetting('discordMute.enabled', enabled);
    }
    async function action(kind: 'setup' | 'test') {
        busy = true;
        message = kind === 'setup' ? 'hub.discord.settingUp' : 'hub.discord.testing';
        try {
            if (kind === 'setup') await verify(false);
            await invoke(kind === 'setup' ? 'discord_setup_key' : 'discord_test_mute');
            message = 'hub.discord.checkResult';
        } catch { message = 'hub.discord.failed'; }
        finally { busy = false; }
    }
</script>

<div class="discord-section">
    <h3 class="he-sans">{$t('hub.discord.title')}</h3>
    <p class="he-sans">{$t('hub.discord.intro')}</p>
    {#if supported}
        <ol class="he-sans">
            <li>{$t('hub.discord.step1')}</li>
            <li>{$t('hub.discord.step2')}</li>
            <li>{$t('hub.discord.step3')}</li>
        </ol>
        <div class="actions">
            <button type="button" disabled={busy} onclick={() => void action('setup')}>{$t('hub.discord.setup')}</button>
            <button type="button" disabled={busy} onclick={() => void action('test')}>{$t('hub.discord.test')}</button>
        </div>
        {#if message}<p class="he-sans" role="status">{$t(message)}</p>{/if}
        <label class="he-sans"><input type="checkbox" checked={verified} disabled={busy} onchange={(event) => void verify(event.currentTarget.checked)} /> {$t('hub.discord.verified')}</label>
        <div class="activation he-sans">
            <span id="discord-enable-label" dir="auto">{$t('hub.discord.enable')}</span>
            <button class="activation-switch" class:enabled type="button" role="switch" aria-checked={enabled} aria-labelledby="discord-enable-label" aria-describedby="discord-enable-hint" disabled={!verified || busy} onclick={() => void enable(!enabled)}>
                <span class="switch-track" aria-hidden="true"><span class="switch-knob"></span></span>
                <span>{$t(enabled ? 'hub.discord.active' : 'hub.discord.off')}</span>
            </button>
        </div>
        <p id="discord-enable-hint" class="he-sans">{$t('hub.discord.enableHint')}</p>
        <p class="he-sans note">{$t('hub.discord.limit')}</p>
    {:else}<p class="he-sans">{$t('hub.discord.windowsOnly')}</p>{/if}
</div>

<style>
    .discord-section { margin-top: 22px; padding-top: 16px; border-top: 1px solid var(--ink-line-2); max-width: 640px; }
    h3 { font-size: 16px; color: var(--ink-text); }
    p, ol, label { font-size: 13px; line-height: 1.6; color: var(--ink-mute); }
    label { display: block; margin-block: 10px; }
    .actions { display: flex; gap: 10px; flex-wrap: wrap; }
    button { font: inherit; color: var(--ink-text); background: var(--ink-2); border: 1px solid var(--ink-line-2); border-radius: 7px; padding: 8px 12px; cursor: pointer; }
    button:disabled { opacity: .5; cursor: default; }
    button:focus-visible { outline: 2px solid var(--aqua); outline-offset: 2px; }
    .note { color: var(--ink-faint); }
    .activation { display: flex; direction: ltr; align-items: center; justify-content: space-between; gap: 16px; margin-block: 16px 8px; font-size: 13px; color: var(--ink-text); }
    .activation-switch { display: flex; align-items: center; gap: 8px; min-width: 104px; flex-shrink: 0; }
    .switch-track { display: flex; align-items: center; justify-content: flex-start; width: 36px; height: 22px; padding: 3px; box-sizing: border-box; border-radius: 12px; background: var(--ink-line-2); }
    .switch-knob { width: 16px; height: 16px; border-radius: 50%; background: var(--ink-text); }
    .enabled .switch-track { justify-content: flex-end; background: var(--aqua); }
    .enabled .switch-knob { background: var(--ink-2); }
</style>
