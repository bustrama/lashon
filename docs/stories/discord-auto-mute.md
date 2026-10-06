# Discord suppression during dictation — experimental Windows adapter

Implemented on `feat/discord-auto-mute`, stacked on #37. Review/merge pending.

Use Discord desktop's native **Push to Mute** in Voice Activity mode, bound by
the user to reserved F24. Ottid holds this key before microphone capture begins
and releases it after final transcription/injection, also for command-mode and
wake-triggered takes. No Toggle Mute, device mute, default-device change, Discord
account token, browser injection or virtual audio driver is used.

The feature is off by default. General settings offer a delayed F24 tap for
Discord's keybind recorder, a six-second hold test, and an explicit confirmation
that automatic release and preservation of existing manual mute were both tested.
The enable control requires that confirmation. Rebinding resets verification and
disables the option. Leave Discord's Keybinds page before testing: its shortcuts
are disabled while editing. Do not assign F24 to any other action/application.

The guard's acknowledgement confirms input synthesis only, **not Discord mute**.
Discord has no mute acknowledgement on this shortcut interface. Consequently the
UI describes the adapter as experimental and never claims verified suppression.
Recheck after changing bindings, input mode or process privilege levels. Browser
Discord and non-Windows platforms are unsupported. RPC offers a richer interface
but requires an app registration and authorized scopes; it is not configured here.

A helper mode in the existing Ottid executable holds the key. The parent owns its
stdin pipe: completion closes it, and an Ottid abort/exit closes it through the OS.
The helper releases on EOF, failed acknowledgement, unwinding, or a ten-minute
deadline. It runs hidden with CREATE_NO_WINDOW and skips Tauri initialization.
Normal parent cleanup waits briefly for exit, kills a stuck helper and sends a
fallback key-up. A single-process lease prevents overlapping test/recording holds.
This is not a system service and needs no separate bundled binary.

If the enabled guard fails before capture, recording is blocked with visible
feedback. Capture failure, a short/discarded take, transcription error and normal
completion all drop the lease. Shortcut misconfiguration cannot be detected and
does not provide a privacy guarantee. Forced termination of the helper itself or
input-system failure also needs manual recovery; the UI must not hide this limit.

Validation: fake-key tests cover successful release, partial press failure,
unwind, parent EOF, deadline and handshake failure without touching real keys;
Svelte diagnostics and existing frontend tests pass. Windows app build and live
Discord setup/test are the next manual validation surface. No real microphone
capture or Discord transmission test has been performed by the agent.

Source: [Discord keybind documentation](https://support.discord.com/hc/en-us/articles/217083547-How-do-I-add-different-Keybinds).
