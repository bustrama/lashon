#!/usr/bin/env bash
# Reclaim disk space by deleting Lashon's build artifacts and (optionally)
# language environments and downloaded model weights.
#
# Three tiers, from safest to most aggressive:
#
#   light       Cargo target/ + empty orphan worktree dirs.
#               Cost of recovery: one cold cargo build.
#
#   medium      light + Python .venv, PyInstaller build/dist, npm
#               node_modules, .svelte-kit, Tauri-side bundled binaries
#               (regenerated at packaging time).
#               Cost of recovery: npm install, recreate venv, re-mirror
#               llama-server binaries from the ggml.llamacpp release.
#
#   aggressive  medium + models/stt + models/local-llm.
#               Cost of recovery: multi-GB downloads on next run.
#
# See .claude/rules/cleanup.md for the routine and tier policy.

set -euo pipefail

level="light"
dry_run=0

usage() {
    cat <<'EOF'
Usage: scripts/clean.sh [--level light|medium|aggressive] [--dry-run]

  --level     Cleanup tier (default: light).
  --dry-run   Show what would be deleted without touching anything.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --level)
            level="$2"
            shift 2
            ;;
        --level=*)
            level="${1#*=}"
            shift
            ;;
        --dry-run)
            dry_run=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage
            exit 1
            ;;
    esac
done

case "$level" in
    light|medium|aggressive) ;;
    *) echo "Invalid --level: $level" >&2; exit 1 ;;
esac

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

dir_size_mb() {
    local path="$1"
    [[ -e "$path" ]] || { echo 0; return; }
    du -sm "$path" 2>/dev/null | awk '{print $1}'
}

repo_total_mb() {
    du -sm "$repo_root" 2>/dev/null | awk '{print $1}'
}

reclaimed=0

remove_path() {
    local path="$1"
    local label="$2"
    if [[ ! -e "$path" ]]; then
        printf "  - %-55s (absent)\n" "$label"
        return
    fi
    local size; size="$(dir_size_mb "$path")"
    if [[ $dry_run -eq 1 ]]; then
        printf "  - %-55s %8s MB  (dry-run)\n" "$label" "$size"
    else
        rm -rf -- "$path"
        printf "  - %-55s %8s MB  deleted\n" "$label" "$size"
    fi
    reclaimed=$((reclaimed + size))
}

remove_orphan_worktree_dirs() {
    local wtdir="$repo_root/.claude/worktrees"
    [[ -d "$wtdir" ]] || return

    # Collect registered worktree paths into an array.
    local registered=()
    while IFS= read -r line; do
        [[ "$line" == worktree\ * ]] || continue
        registered+=("${line#worktree }")
    done < <(git worktree list --porcelain)

    for dir in "$wtdir"/*/; do
        [[ -d "$dir" ]] || continue
        local abs; abs="${dir%/}"
        local is_registered=0
        for r in "${registered[@]}"; do
            [[ "$r" == "$abs" ]] && is_registered=1 && break
        done
        [[ $is_registered -eq 1 ]] && continue
        local size; size="$(dir_size_mb "$abs")"
        local name; name="$(basename "$abs")"
        if [[ $dry_run -eq 1 ]]; then
            printf "  - orphan worktree %-39s %8s MB  (dry-run)\n" "$name" "$size"
        else
            rm -rf -- "$abs"
            printf "  - orphan worktree %-39s %8s MB  deleted\n" "$name" "$size"
        fi
        reclaimed=$((reclaimed + size))
    done

    [[ $dry_run -eq 0 ]] && git worktree prune >/dev/null
}

before_mb="$(repo_total_mb)"
printf "\nLashon repo cleanup -- tier: %s%s\n" "$level" "$([[ $dry_run -eq 1 ]] && echo ' (dry-run)')"
printf "Repo total before: %s MB\n\n" "$before_mb"

echo "Build artefacts:"
if [[ -d target ]]; then
    target_mb="$(dir_size_mb target)"
    if [[ $dry_run -eq 1 ]]; then
        printf "  - %-55s %8s MB  (dry-run)\n" "target/ (cargo clean)" "$target_mb"
    else
        cargo clean >/dev/null 2>&1 || rm -rf target
        printf "  - %-55s %8s MB  deleted\n" "target/ (cargo clean)" "$target_mb"
    fi
    reclaimed=$((reclaimed + target_mb))
else
    printf "  - %-55s (absent)\n" "target/ (cargo clean)"
fi
remove_orphan_worktree_dirs

if [[ "$level" == "medium" || "$level" == "aggressive" ]]; then
    echo ""
    echo "Language environments and regenerated bundles:"
    remove_path services/stt-sidecar/.venv "services/stt-sidecar/.venv"
    remove_path services/stt-sidecar/build "services/stt-sidecar/build"
    remove_path services/stt-sidecar/dist  "services/stt-sidecar/dist"
    remove_path apps/desktop/node_modules  "apps/desktop/node_modules"
    remove_path apps/desktop/.svelte-kit   "apps/desktop/.svelte-kit"
    remove_path apps/desktop/build         "apps/desktop/build"

    # Tauri-side bundled binaries — preserve .gitkeep (and README.md for llama).
    for bin_dir in \
        "apps/desktop/src-tauri/binaries/lashon-stt:.gitkeep" \
        "apps/desktop/src-tauri/binaries/llama-server:.gitkeep,README.md"; do
        path="${bin_dir%%:*}"
        keep="${bin_dir##*:}"
        [[ -d "$path" ]] || continue
        size_before="$(dir_size_mb "$path")"
        if [[ $dry_run -eq 1 ]]; then
            printf "  - %-55s %8s MB  (dry-run)\n" "$path/* (keep $keep)" "$size_before"
            reclaimed=$((reclaimed + size_before))
        else
            find "$path" -mindepth 1 -maxdepth 1 \
                $(echo "$keep" | tr ',' '\n' | sed 's/^/! -name /' | tr '\n' ' ') \
                -exec rm -rf {} +
            size_after="$(dir_size_mb "$path")"
            delta=$((size_before - size_after))
            printf "  - %-55s %8s MB  deleted\n" "$path/* (keep $keep)" "$delta"
            reclaimed=$((reclaimed + delta))
        fi
    done
fi

if [[ "$level" == "aggressive" ]]; then
    echo ""
    echo "Downloaded model weights (re-downloaded on next run):"
    remove_path models/stt       "models/stt"
    remove_path models/local-llm "models/local-llm"
fi

echo ""
if [[ $dry_run -eq 1 ]]; then
    printf "Projected reclaim: %s MB\n" "$reclaimed"
    echo "Re-run without --dry-run to apply."
else
    after_mb="$(repo_total_mb)"
    delta=$((before_mb - after_mb))
    printf "Repo total after:  %s MB\n" "$after_mb"
    printf "Reclaimed:         %s MB\n" "$delta"
fi
