#!/usr/bin/env bash
# Optional offline browser screenshot generator for UX gallery.
# No network and no font files included in Git.
set -euo pipefail
UX_DIR="$(cd -- "$(dirname -- "$0")" && pwd)"
OUT_DIR="$UX_DIR/previews"
mkdir -p "$OUT_DIR"
if command -v google-chrome >/dev/null 2>&1; then
  CHROME="$(command -v google-chrome)"
elif command -v chromium >/dev/null 2>&1; then
  CHROME="$(command -v chromium)"
elif command -v chromium-browser >/dev/null 2>&1; then
  CHROME="$(command -v chromium-browser)"
else
  echo 'Chrome/Chromium unavailable; use HTML pages directly.' >&2
  exit 2
fi
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT
# Headless Nix/bwrap may have no globally configured fontconfig. Configure an
# installed font directory in a private temporary config. Ship NO font files.
if command -v fc-list >/dev/null 2>&1; then
  FONT_FILE="$(fc-list -f '%{file}\n' 2>/dev/null | head -n 1 || true)"
  if [[ -n "$FONT_FILE" && -f "$FONT_FILE" ]]; then
    FONT_DIR="$(dirname -- "$FONT_FILE")"
    cat > "$TMP/fonts.conf" <<EOF
<?xml version="1.0"?>
<fontconfig><dir>$FONT_DIR</dir><cachedir>$TMP/font-cache</cachedir></fontconfig>
EOF
    export FONTCONFIG_FILE="$TMP/fonts.conf"
  fi
fi
count=0
while IFS='|' read -r name version options; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  url="file://$UX_DIR/$version.html?capture=1&$options"
  count=$((count+1))
  output="$OUT_DIR/$name.png"
  if ! timeout 35 "$CHROME" --headless --no-sandbox --disable-gpu \
      --disable-dev-shm-usage --hide-scrollbars \
      --window-size=1440,790 --virtual-time-budget=900 \
      --user-data-dir="$TMP/chrome-$count" \
      --screenshot="$output" "$url" > "$TMP/chrome.log" 2>&1; then
    echo "Screenshot FAILED: $name" >&2
    tail -30 "$TMP/chrome.log" >&2
    exit 1
  fi
  python3 - "$output" <<'PY'
from pathlib import Path
import struct, sys
p=Path(sys.argv[1])
b=p.read_bytes()
assert b[:8]==b'\x89PNG\r\n\x1a\n',p
assert struct.unpack('>II',b[16:24])==(1440,790),p
assert len(b)>20000,(p,len(b))
print(f'  {p.name}: PNG 1440x790, {len(b)} bytes')
PY
done <<'SCREENS'
v0-minimal|v0|mode=minimal
v0-workspace|v0|mode=workspace&tab=agent
v1-review|v1|mode=workspace&tab=review&layout=grid
v1.1-workspace|v1.1|mode=workspace&tab=review
v1.2-space-switcher|v1.2|mode=workspace&tab=review&popover=spaces
v1.3-workspace|v1.3|mode=workspace&tab=review&nav=projects&group=now
v1.4-review|v1.4|mode=workspace&tab=review&dock=activity&group=recents
v1.4-mini-git|v1.4|mode=workspace&tab=review&dock=git
v1.4-mini-files|v1.4|mode=workspace&tab=review&dock=files
v1.4-full-git|v1.4|mode=workspace&tab=git&dock=git
v1.4-full-files|v1.4|mode=workspace&tab=files&dock=files
v1.4-terminals|v1.4|mode=workspace&tab=terminals&dock=activity
v1.4-agents|v1.4|mode=agents&group=now
v1.4-recents|v1.4|mode=workspace&tab=review&nav=recents&group=recents
v1.4-automations|v1.4|mode=workspace&tab=activity&dock=activity&group=automations
v1.4-local-x2|v1.4|mode=workspace&tab=files&space=x2&dock=files
v1.4-light|v1.4|mode=workspace&tab=review&theme=light
v1.4-focus|v1.4|mode=workspace&tab=review&layout=focus&dock=activity
v1.4-split|v1.4|mode=workspace&tab=review&layout=split&dock=activity
v1.5-minimal|v1.5|mode=minimal
v1.5-review|v1.5|mode=workspace&tab=review&dock=activity&layout=grid
v1.5-agent-max|v1.5|mode=workspace&tab=agent&zoom=agent
v1.5-files-contained|v1.5|mode=workspace&tab=files&dock=files
v1.5-files-editor-max|v1.5|mode=workspace&tab=files&zoom=editor&dock=files
v1.5-file-tab|v1.5|mode=workspace&tab=file:abird:tool.rs&dock=files
v1.5-file-tab-max|v1.5|mode=workspace&tab=file:abird:tool.rs&zoom=file
v1.5-git-contained|v1.5|mode=workspace&tab=git&dock=git
v1.5-git-diff-max|v1.5|mode=workspace&tab=git&zoom=gitdiff&dock=git
v1.5-diff-tab|v1.5|mode=workspace&tab=diff:abird:hooks.rs&dock=git
v1.5-diff-tab-max|v1.5|mode=workspace&tab=diff:abird:hooks.rs&zoom=standaloneDiff
v1.5-terminals|v1.5|mode=workspace&tab=terminals&dock=activity
v1.5-terminal-max|v1.5|mode=workspace&tab=terminals&zoom=pty:tests
v1.5-palette|v1.5|mode=workspace&tab=review&popover=palette
v1.5-palette-agents|v1.5|mode=workspace&tab=agent&popover=palette&query=%26hooks
v1.5-palette-files|v1.5|mode=workspace&tab=file:abird:tool.rs&popover=palette&query=%40tool
v1.5-palette-minimal|v1.5|mode=minimal&popover=palette&query=%26runtime
SCREENS
echo "Generated $count design-only concept screenshots."