#!/usr/bin/env bash
# Terminal-cell snapshots: real ANSI SGR streams plus fixed-cell SVG/PNG proof.
# Visual study only; never invokes aw, providers, shells as Agents, or a PTY.
set -euo pipefail
UX="$(cd -- "$(dirname -- "$0")" && pwd)"
OUT="$UX/terminal-previews"
mkdir -p "$OUT"
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT
CHROME=""
for exe in google-chrome chromium chromium-browser; do
  if command -v "$exe" >/dev/null 2>&1; then CHROME="$(command -v "$exe")";break;fi
done
if [[ -n "$CHROME" ]] && command -v fc-list >/dev/null 2>&1; then
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
while IFS='|' read -r name cols rows mode tab nav dock theme glyphs; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  count=$((count+1))
  prefix="$OUT/$name"
  python3 "$UX/terminal-v1.4.py" \
    --width "$cols" --height "$rows" --mode "$mode" \
    --tab "$tab" --nav "$nav" --dock "$dock" \
    --theme "$theme" --glyphs "$glyphs" \
    --output "$prefix"
  if [[ -n "$CHROME" ]]; then
    W=$((cols*8))
    H=$((rows*18))
    # SVG geometry is produced from ANSI cell paint operations and shared palette.
    # Headless screenshot is a VIEW of the cell output, not an HTML layout model.
    timeout 35 "$CHROME" --headless --no-sandbox --disable-gpu \
      --disable-dev-shm-usage --hide-scrollbars --virtual-time-budget=950 \
      --user-data-dir="$TMP/chrome-$count" \
      --window-size="$W,$H" \
      --screenshot="$prefix.png" "file://$prefix.svg" > "$TMP/chrome.log" 2>&1 ||
      { echo "PNG visual export failed for $name; ANSI/SVG remain available." >&2;tail -12 "$TMP/chrome.log" >&2;exit 1; }
  fi
done <<'SCREENS'
v1.4-review-160x45|160|45|workspace|review|projects|activity|dark|unicode
v1.4-review-200x60|200|60|workspace|review|projects|activity|dark|unicode
v1.4-mini-git-120x40|120|40|workspace|review|projects|git|dark|unicode
v1.4-agents-100x30|100|30|agents|review|recents|activity|dark|unicode
v1.4-minimal-ascii-80x24|80|24|minimal|agent|projects|activity|dark|ascii
v1.4-full-git-light-160x45|160|45|workspace|git|projects|activity|light|unicode
v1.4-terminals-160x45|160|45|workspace|terminals|projects|files|dark|unicode
v1.4-review-narrow-100x30|100|30|workspace|review|projects|activity|dark|unicode
v1.4-monochrome-120x40|120|40|workspace|review|projects|files|mono|ascii
SCREENS
echo "$count terminal-cell scenarios rendered as ANSI / plain / SVG / PNG."
