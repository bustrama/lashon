# Short agent activity and side docking

Implemented on `feat/agent-activity-side-docking`, stacked on Codex support (#36).
Review and merge pending.

Ottid shows one short status for local Claude Code / Codex sessions, using native
UserPromptSubmit, PreToolUse, PostToolUse, Stop and SessionEnd hooks. Codex adds
Interrupt; Claude adds PostToolUseFailure and StopFailure. The existing hook
executables and authenticated bridge carry these events: no model call, generated
summary, transcript reader or additional background service.

Only session identity, event and validated tool name survive input parsing. Prompt,
tool arguments, tool output and assistant messages are discarded. Lifecycle hooks
return `{}` regardless of bridge success and never return permission or continuation
decisions. Their connection/handshake limits are short; the acknowledgement wait is
one second. Approval hooks retain their existing native fallback behavior.

Codex approval cards are now opt-in (`agents.codexApprovalCards`, default false).
Codex runs PermissionRequest before user or automatic review, and its hook input
does not expose `approvals_reviewer`; permission_mode does not distinguish the two.
With cards off, Ottid immediately returns Ask (no hook decision), so Codex retains
its normal approval/reviewer flow. Lifecycle activity still reaches the tracker.
The Hub explains that cards should stay off for Approve for me. Claude cards keep
their existing behavior. The setting does not change any Codex configuration or
automatically allow a request. Store read failure also leaves review with Codex.

The core tracker keeps at most 64 sessions. It prefers active sessions over finished
ones, shows a small count for concurrent sessions, expires terminal status after
four seconds and drops silent sessions after ten minutes. A dropped session is not
claimed to have completed. The Hub's single `ui.agentActivity` toggle hides both
the bubble and activity pose immediately and persists across launches. Approval,
dictation and command feedback have priority. No expanded activity list is added.

Existing connections show as needing reinstall so the user previews all added hook
definitions. Codex definitions require renewed native trust through `/hooks`.
Uninstall removes only Ottid's lifecycle handlers and preserves unrelated hooks.

Left and Right are persisted placements alongside Taskbar, Float and Ceiling.
Dragging to an edge snaps there; selection through the Hub or context menu uses
the same geometry. Side placement preserves vertical position, moves the creature
toward the wall, keeps the window inside the work area and leaves room above or
below for the approval card. Switching to Float lifts away from the side.

Validation: bridge and lifecycle settings tests, overlay geometry tests including
negative monitor origins and 100/125/200% scale, existing frontend creature tests,
Svelte diagnostics and full Windows debug build. Manual visual checks and real
trusted Codex/Claude sessions remain necessary; no Linux/macOS runtime verification.
