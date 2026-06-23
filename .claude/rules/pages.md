# GitHub Pages

The Lashon public website at <https://bustrama.github.io/lashon/> is **not**
served from `main/docs/`. It is served from the dedicated **`gh-pages`**
branch, root path, with `.nojekyll` (static HTML only — Jekyll is disabled).

Pages source is configured at the repo level via
`GET /repos/bustrama/lashon/pages → source = { branch: "gh-pages", path: "/" }`.
Do not switch it to `main/docs` — I tried that once; it took the marketing
site offline because `main/docs` is developer documentation and lacks the
handcrafted `index.html` / `styles.css` / SVG assets the site needs.

## What lives where

| Branch | Path | What it is |
|---|---|---|
| `gh-pages` | `/` | The Hebrew RTL marketing landing page (`index.html`, `styles.css`, `lashon-mark.svg`, `.nojekyll`) and user-facing guides served at `/lashon/...`. |
| `gh-pages` | `/wake-word-training/index.html` | The wake-word training tutorial the in-app "How to train your own" button links to. Self-contained HTML reusing the marketing site's `styles.css` design tokens. |
| `main` | `/docs/*.md` | Developer documentation (architecture, providers, roadmap, ADRs, stories, training procedure source-of-truth). Never published. |
| `main` | `/docs/wake-word-training.md` | The Markdown source-of-truth for the tutorial. When the tutorial is updated, edit both — the HTML on `gh-pages` is a rendering of this content. |

## Working on the marketing site or tutorial

A worktree at `.claude/worktrees/website-gh-pages` is set up so `gh-pages`
can be checked out in parallel with `main`. **Use it**, do not `git checkout
gh-pages` in the main working tree — that leaks all of `main`'s untracked
build artifacts (`apps/`, `target/`, `node_modules/`, `.venv/`, …) into a
branch that has no `.gitignore`, and GitHub Desktop shows tens of thousands
of phantom "changes". The marketing site is deliberately bare, and
`gh-pages` has no `.gitignore` for that reason.

```sh
# Edit the marketing site or tutorial — from the worktree:
cd .claude/worktrees/website-gh-pages
# …make changes, commit, push…
```

## PRs against `gh-pages`

CI is `main`-only (no checks run against `gh-pages`), so a Pages PR is fast
to merge — `gh pr merge <N> --squash --delete-branch` lands it the moment
you open it. Pages rebuilds automatically and the site is live in ~1 min.

## Why this matters

The `docs/` tree on `main` looks like a Jekyll source candidate (it has
Markdown files with front-matter-style headings). It is not. It is
internal documentation, kept current with the code. Treat the two
publishing surfaces — `gh-pages` for users, `docs/` for developers — as
strictly separate.
