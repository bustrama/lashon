# 51. Codex permission hooks

## Status

Accepted — 2026-10-06. Extends ADR-0050; implementation depends on the
bridge hardening in PR #35.

## Decision

Use Codex's documented `PermissionRequest` hook, not an app-server proxy.
The current hook input and allow/deny output match the bridge's contract.
Validated against the locally installed Codex CLI 0.142.5; this is a tested
version, not a claim about the earliest supported release.

The full edition includes `ottid-codex-hook`. It shares the bounded stdin,
HMAC exchange and fail-to-native-prompt behavior of `ottid-hook`. The sealed
Ask carries an enum agent identity; absent identity is legacy Claude Code.
Unknown identities are refused. Old listeners refuse Codex frames rather
than showing them as Claude Code. The card shows the complete shell command,
patch or MCP input and names Codex.

The Hub previews, installs and removes only Ottid's own entry in the user's
`hooks.json`, under Ottid's `CODEX_HOME` or `~/.codex`. The shared raw JSON
editor preserves unrelated values, backs up and syncs before replacement,
and refuses symlinked files. It never edits `config.toml`, permissions,
trust records, project hooks or managed policies.

Codex invokes command hooks through a shell. Unix commands use a quoted
absolute executable path. `commandWindows` uses PowerShell EncodedCommand
with a single-quoted literal executable path and UTF-8 stdin forwarding,
so spaces, apostrophes and shell metacharacters remain data. Requests are
never interpolated into the command. Only the decision goes to stdout.

Users must inspect and trust the installed definition through `/hooks` in
Codex, and start a new local session. Ottid never bypasses this requirement
and cannot infer its trust state. The Hub's installed status refers to the
displayed file, not a guarantee that a particular Codex session invokes it.
`features.hooks=false`, managed restrictions, cloud orchestration, a different
`CODEX_HOME`, or an older runtime can prevent invocation.

The listener runs while either agent is installed. Disconnecting one leaves
the other working; each request is checked against its own installation.
No standing permissions, rewritten input or approval-policy changes are
returned. Failures and timeouts print no decision and preserve native flow.

## Validation and limits

Core tests cover identity compatibility, JSON preservation, install/remove
idempotence and quoting. Local smoke tests exercise the real client binary
without starting a model or executing the displayed tool. A real Codex
session and trust review still require the user's manual check. The native
hook contract is local-only; WSL and remote/cloud sessions are unsupported.

Source: [official hooks documentation](https://learn.chatgpt.com/docs/hooks),
including PermissionRequest, commandWindows, CODEX_HOME, and hook trust.
