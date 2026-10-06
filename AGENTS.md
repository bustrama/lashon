# Ottid: instructions for coding agents

[`CLAUDE.md`](CLAUDE.md) is the project context for **every** coding agent, not
only Claude Code: what Ottid is, where things live, the pinned stack and the run
commands. Read it first.

The working rules live in [`.claude/rules/`](.claude/rules/) as short files, one
per area. Read the ones for the files you touch:

| Rule | Covers |
|---|---|
| [`security.md`](.claude/rules/security.md) | Secrets, never logging transcripts, audio, PII or tool args, no cloud defaults, licensing bars (AGPL, CC-BY-NC) |
| [`architecture.md`](.claude/rules/architecture.md) | The provider seam, the `ottid-core` / Tauri boundary, the sidecar handshake |
| [`rust.md`](.claude/rules/rust.md) | Tests live in `ottid-core`, never in the Tauri crate; exact pins |
| [`frontend.md`](.claude/rules/frontend.md) | Svelte 5 runes, RTL, reduced motion, design tokens, the creature and its lamp |
| [`hebrew.md`](.claude/rules/hebrew.md) | Hebrew-first testing, injection and display |
| [`recipes.md`](.claude/rules/recipes.md) | Subprocess spawning (`CREATE_NO_WINDOW`), recipe storage and authoring |
| [`stt-sidecar.md`](.claude/rules/stt-sidecar.md) | The Python sidecar's transport, model integrity and CUDA pinning |
| [`pages.md`](.claude/rules/pages.md) | The public site is the `gh-pages` branch, edited from its own worktree |
| [`workflow.md`](.claude/rules/workflow.md) | ADRs, conventional commits, exact version pins |
| [`cleanup.md`](.claude/rules/cleanup.md) | What the clean scripts remove, and what they never touch |

Decisions are recorded in [`docs/adr/`](docs/adr/) and work units in
[`docs/stories/`](docs/stories/).

## Rules that are easy to miss

- **Work in your own git worktree.** Several agents work on this repository at
  once, so don't switch or create branches in the shared checkout. Use
  `git worktree add .claude/worktrees/<slug> -b <branch> origin/main`.
- **Never run `cargo fmt` across the tree.** The committed Rust has rustfmt
  drift, and CI doesn't gate formatting. Format only the files or hunks you
  wrote, with `rustfmt --edition 2021 <file>`.
- **Commit types:** `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`
  and `ci`. Never `build`. One concern per commit.
- **ADR numbers:** check open branches as well as `main` before you pick the next
  number. An open PR may have claimed it.
- **In Hebrew, Ottid (אוטיד) is grammatically masculine.**
