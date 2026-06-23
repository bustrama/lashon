# 31. `Step::RunShell.dry_run` bypasses the confirmation gate

## Status

Accepted — landed in PR #81 alongside the Steps panel work (the
designer-mocked dry-run annotation needed a real schema field).

## Context

The recipe runtime gates every `run_shell` step on the
`ConfirmHandler` trait (`AlwaysAllow`, `AlwaysDeny`, or the
Tauri-shell `EventBasedConfirm` that emits `recipe:confirm`). This
matches the M8 Command-mode `run_command` tool's confirmation policy:
no shell command executes without user approval.

The designer's Steps panel mockup surfaced a small rose-italic
annotation **"· dry-run בלבד — לא מתבצע שינוי"** on shell cards
where the recipe author had marked the step as a preview. The
implication: an author can declare a `run_shell` step that *renders
the command* but doesn't execute, useful for:

- Authoring a new recipe and wanting to see the interpolated command
  in the Steps panel without side effects
- Building a recipe that's deliberately a preview (e.g.
  `batch-rename-files` with `dry_run: true` echoes the rename plan
  without touching the disk)
- Documentation / teaching recipes

Adding the field is trivial; the question was the **runtime
semantics** — what does `dry_run: true` skip?

## Decision

Add `dry_run: bool` as a `#[serde(default)]` additive field on
`Step::RunShell`. When `true`, the runtime:

1. **Skips the actual subprocess spawn.** No powershell, no `/bin/sh`,
   no command lands on the OS.
2. **Bypasses the confirmation gate.** The runtime does **not** call
   `ConfirmHandler::confirm` for a dry-run step.
3. **Binds `capture_into` (when set) to the sentinel string
   `"(dry-run)"`** so a later step that interpolates
   `{{ captured_var }}` doesn't trip `UnknownInterpolation`.
4. **Logs at INFO** with `command_len` only (never the command text —
   security rule `.claude/rules/security.md`).

The bypass on confirmation is the load-bearing part of this
decision.

## Why bypass confirmation on dry-run

A confirmation modal exists to gate a *side effect* the user might
not want. If the runtime guarantees no spawn happens, the modal has
nothing to gate — popping it would be a friction tax with no safety
benefit. Concretely:

- **The author already opted in.** They set `dry_run: true`
  explicitly in the YAML; they wrote the command knowing it would
  preview. A confirmation prompt at run time tells the user nothing
  they don't already see in the Steps panel's rendered code block.
- **`AlwaysDeny` for testing.** Without the bypass, every dry-run
  test under `AlwaysDeny` aborts before getting to assert the
  capture sentinel binds. The bypass is what makes
  `dry_run_shell_step_skips_execution_and_binds_capture` a clean,
  no-confirmation-handler-required test.
- **The Steps panel already surfaces the indicator.** The user can
  see at a glance — without the modal — which shell steps are
  preview-only.

## Why NOT also make `dry_run` skip the permission declaration check

The validator still requires `shell.run` in `permissions:` when a
`Step::RunShell` appears (regardless of `dry_run`). The permission
list is a **descriptive** signal for the Hub's badge row + the
future M11+ sandboxing decision; dropping it for dry-run steps
would make the per-recipe permission summary lie about the recipe's
shape ("this recipe contains shell logic but the badge doesn't
show it because half are dry-runs"). The validator stays strict.

## Consequences

- **Test ergonomics improve.** The runtime's dry-run path is
  testable without instantiating a confirmation handler that
  happens to allow shell — the bypass is the test.
- **Authors get a preview tool.** `dry_run: true` is the cheap way
  to inspect what a complex shell pipeline would interpolate to,
  without touching the disk. Re-runs of the same recipe are
  side-effect-free until the field is flipped to `false`.
- **No security loss.** The Steps panel renders the rose-italic
  "dry-run" annotation prominently; the field defaults to `false`
  on every existing recipe (additive `#[serde(default)]`); a
  malicious recipe author who flips `dry_run: false` after the
  user inspected the recipe in the Steps panel would still hit
  the confirmation modal on the actual run.
- **`Step::RunShell` documentation in the schema rustdoc** spells
  the bypass out so a future author / reader sees it without
  having to read the runtime impl.

## Migration

None. The field is additive with `#[serde(default = false)]`; every
existing recipe parses unchanged.

## Notes

The sentinel string `"(dry-run)"` for `capture_into` was picked over
the alternatives:

- `""` (empty string) — would be indistinguishable from a real
  shell command that produced no output
- `serde_yaml_ng::Null` — typing complication (the var map is
  `HashMap<String, String>`)
- A magic flag with separate per-step "is dry-run output" tracking —
  pulls weight that the sentinel string already carries

A recipe author who *needs* to distinguish dry-run captured output
from real captured output should check for the literal `"(dry-run)"`
prefix in a later step. Documented in the schema rustdoc.
