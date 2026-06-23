#!/usr/bin/env bash
# PostToolUse hook — auto-format the file Claude just wrote or edited.
#
# Best-effort by design: a missing or unconfigured formatter is skipped
# silently. The hook always exits 0 and never fails a session.

input=$(cat)

# Extract the edited file path from the tool input (Write uses file_path).
file_path=$(printf '%s' "$input" | jq -r '.tool_input.file_path // .tool_input.filePath // empty' 2>/dev/null)
[ -z "$file_path" ] && exit 0

cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
[ -f "$file_path" ] || exit 0

case "$file_path" in
  *.rs)
    # cargo fmt is edition-aware (reads each crate's Cargo.toml); rustfmt
    # ships with the pinned toolchain, so this is always available.
    command -v cargo >/dev/null 2>&1 && cargo fmt 2>/dev/null
    ;;
  *.py)
    # No Python formatter is pinned in the project; format only if one is
    # on PATH.
    if command -v ruff >/dev/null 2>&1; then
      ruff format "$file_path" 2>/dev/null
    elif command -v black >/dev/null 2>&1; then
      black --quiet "$file_path" 2>/dev/null
    fi
    ;;
esac

exit 0
