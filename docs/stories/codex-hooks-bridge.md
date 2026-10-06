# Codex hooks bridge

Implemented on top of PR #35; review and merge pending. Decision: ADR-0051.

The full edition provides separate Claude Code and Codex connections in the
Hub. Each previews and edits only the user's appropriate JSON file. Codex
uses `ottid-codex-hook`; it shares the authenticated pipe/socket exchange,
bounded input and fallback behavior with Claude's client. The complete Codex
request and authenticated agent label reach the existing approval card.

Validation on Windows: 77 bridge tests, 123 frontend tests, clean Svelte check,
full embedded-frontend debug build, both hook binaries built. Windows shell
invocation tested with Hebrew JSON and no reachable bridge: exit 0, no decision.

Manual checks: connect Codex from the Hub, review trust via `/hooks` in a new
real Codex session, approve/deny, Hebrew keyboard, timeout and quit mid-request.
Connecting/disconnecting either agent must leave the other's opt-in intact.
Linux/macOS, packaged install and release signing are not verified locally.
No real user hook/trust files or approval policies are changed by development.
