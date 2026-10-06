# Session titles and project labels

Activity cards show the local Codex thread title instead of a numeric label when
available. The heading is `project • session title`, with each name isolated for
mixed Hebrew/English text. Missing project labels omit the bullet; missing titles
retain the numeric fallback. A smaller line identifies the agent.
Claude cards also receive the working-folder label; their numeric heading remains.

The Codex hook reads only the last 2 MiB of `session_index.jsonl`, beside its
`hooks.json` (honouring `CODEX_HOME`). It matches the native session ID and selects
the latest valid `thread_name`. It never opens a rollout or transcript, starts a
model, writes Codex settings, or logs title/path values. The full working path is
discarded before activity crosses the authenticated bridge.

Optional labels preserve compatibility with older hook payloads. Missing files,
malformed/partial index records and invalid display labels fall back to the
existing heading. Later lifecycle events refresh a renamed title while preserving
card identity; transient missing metadata retains the previous label. There is no
background index watcher. Old sessions outside the bounded index tail retain the
numeric fallback. Project labels reflect the folder name, rather than a custom
Codex sidebar project name.

Validation: core tests cover title selection, partial appends, missing IDs,
Windows/Unix paths, display bounds and identity preservation. The existing
authenticated bridge regression and frontend suite remain required. Manual UI
check: start two real Codex sessions in different folders, verify their titles
and project labels, then rename one and generate another lifecycle event.

Worktree: `codex-session-labels`, branch `feat/session-labels`. This change is
isolated from the concurrent branding work in `codex-codex-bridge`.
