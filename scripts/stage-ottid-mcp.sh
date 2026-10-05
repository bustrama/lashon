#!/usr/bin/env bash
# Build ottid-mcp and stage it where the full edition's Tauri bundle expects it
# (docs/adr/0049): apps/desktop/src-tauri/binaries/ottid-mcp/, which
# tauri.conf.json lists in bundle.resources. Run it before `tauri build` of a
# full edition, and before scripts/sign-windows.ps1 -Tree when signing, so the
# binary is signed with the rest of the staged resources.
#
# A free edition (tauri.free.conf.json) does not bundle ottid-mcp: skip this.
#
# Honours CARGO_TARGET_DIR. Runs under Git Bash on Windows.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="$repo/apps/desktop/src-tauri/binaries/ottid-mcp"

exe="ottid-mcp"
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) exe="ottid-mcp.exe" ;;
esac

cd "$repo"
# The bin is built only with the `mcp-server` feature (`command-mode` includes it).
cargo build --release -p ottid-core --bin ottid-mcp --features mcp-server

target_dir="${CARGO_TARGET_DIR:-$repo/target}"
if command -v cygpath >/dev/null 2>&1; then
    target_dir="$(cygpath -u "$target_dir")"
fi
built="$target_dir/release/$exe"
[[ -f "$built" ]] || { echo "stage-ottid-mcp: $built was not built" >&2; exit 1; }

mkdir -p "$dest"
# Drop anything staged before, keeping .gitkeep, so a stale binary can't ship.
find "$dest" -mindepth 1 -maxdepth 1 ! -name .gitkeep -exec rm -rf {} +
cp "$built" "$dest/$exe"
echo "Staged $exe -> ${dest#"$repo"/}/"
