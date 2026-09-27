#!/usr/bin/env bash
# Full reachable refs/history; never deliver an uncommitted workspace as a phase.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
cd "$root"
if [[ $# -ne 1 ]]; then echo 'usage: bash scripts/bundle.sh /outside/repo/aw-phase-N.bundle' >&2; exit 2; fi
if [[ -n $(git status --porcelain --untracked-files=all) ]]; then
  echo 'Commit or remove all reviewed changes/untracked files before bundling.' >&2; exit 1
fi
output=$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve())' "$1")
case "$output" in "$root"|"$root"/*) echo 'Bundle output must be outside the repository.' >&2; exit 1;; esac
[[ ! -e "$output" ]] || { echo 'Refusing to overwrite an existing phase bundle.' >&2; exit 1; }
tmp=$(mktemp "${output}.tmp.XXXXXX")
trap 'rm -f "$tmp"' EXIT
git bundle create "$tmp" --all
git bundle verify "$tmp"
mv "$tmp" "$output"
trap - EXIT
python3 - "$output" <<'PY'
from pathlib import Path
import hashlib,sys
p=Path(sys.argv[1]); h=hashlib.sha256()
with p.open('rb') as f:
    for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
p.with_suffix(p.suffix+'.sha256').write_text(h.hexdigest()+'  '+p.name+'\n')
print(p)
PY
