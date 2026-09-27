#!/usr/bin/env bash
set -euo pipefail

if (( $# > 1 )); then
  echo "usage: bash scripts/build-release-artifacts.sh [OUTPUT_DIRECTORY]" >&2
  exit 2
fi
out_dir=$(python3 -c 'import os, sys; print(os.path.abspath(sys.argv[1]))' "${1:-dist}")
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$root"
if [[ -L "$out_dir" || ( -e "$out_dir" && ! -d "$out_dir" ) ]]; then
  echo "Release output must be a directory, not a file or symlink: $out_dir" >&2
  exit 1
fi

nix_cmd=(nix --extra-experimental-features "nix-command flakes")

echo "Building all aw release binaries from the Nix release graph..."
release_out="$("${nix_cmd[@]}" build .#release-all --no-link --print-out-paths)"

mkdir -p -- "$(dirname -- "$out_dir")"
stage=$(mktemp -d "$(dirname -- "$out_dir")/.$(basename -- "$out_dir").tmp.XXXXXX")
trap 'rm -rf -- "$stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

cp "$release_out"/aw-* "$stage/"
cp "$release_out"/VERSION "$release_out"/PLATFORMS.txt \
  "$release_out"/release-manifest.json "$stage/"
cp install.sh install.ps1 "$stage/"

rm -rf -- "$out_dir"
mv -- "$stage" "$out_dir"
trap - EXIT

printf '\nRelease artifacts:\n'
ls -lh -- "$out_dir"
