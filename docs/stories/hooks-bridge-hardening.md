# Hooks bridge hardening

Status: implemented; independent review and merge pending.

Follow-up to [ADR-0050](../adr/0050-claude-code-hooks-bridge.md) and the low-priority
security findings from PR #33.

- Sync settings before replacement; create staging files exclusively, preserve
  Windows DACLs, restrict Unix settings and backups to owner read/write, and
  refuse symlinked settings files.
- Resolve Windows bridge storage through the OS known folder, check the opened
  bridge file's owner, and verify the connected server's user before handshaking.
- State the Claude Code 2.1.139 minimum and explain that connection status refers
  to Ottid's displayed settings file and its own `CLAUDE_CONFIG_DIR` environment.

Validation on Windows: 73 bridge unit tests pass; Svelte check reports zero
errors and warnings. New Unix-specific permission and symlink tests are included
but have not been executed locally. Cross-platform testing remains a review item.

No change is applied to the user's actual Claude Code settings during development;
settings tests use disposable directories. Failed identity checks retain the
existing fallback to Claude Code's own prompt.
