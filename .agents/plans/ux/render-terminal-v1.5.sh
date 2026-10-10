#!/usr/bin/env bash
# V1.5 exact terminal character-grid proof; never invokes live aw runtime.
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
<fontconfig><dir>$FONT_DIR</dir><cachedir>$TMP/cache</cachedir></fontconfig>
EOF
    export FONTCONFIG_FILE="$TMP/fonts.conf"
  fi
fi
count=0
while IFS='|' read -r name cols rows mode tab nav dock theme glyphs palette; do
  [[ -z "$name" || "$name" == \#* ]] && continue
  count=$((count+1))
  prefix="$OUT/$name"
  python3 "$UX/terminal-v1.5.py" --width "$cols" --height "$rows" \
    --mode "$mode" --tab "$tab" --nav "$nav" --dock "$dock" \
    --theme "$theme" --glyphs "$glyphs" --palette "${palette:-none}" --output "$prefix"
  if [[ -n "$CHROME" ]]; then
    W=$((cols*8)); H=$((rows*18))
    timeout 35 "$CHROME" --headless --no-sandbox --disable-gpu \
      --disable-dev-shm-usage --hide-scrollbars --virtual-time-budget=900 \
      --user-data-dir="$TMP/chrome-$count" --window-size="$W,$H" \
      --screenshot="$prefix.png" "file://$prefix.svg" >"$TMP/chrome.log" 2>&1 ||
      { echo "V1.5 SVG rasterization failed for $name" >&2;tail -12 "$TMP/chrome.log" >&2;exit 1; }
  fi
done <<'SCENARIOS'
v1.5-minimal-headerless-80x24|80|24|minimal|review|projects|activity|dark|ascii
v1.5-minimal-headerless-120x40|120|40|minimal|review|projects|activity|dark|unicode
v1.5-workspace-review-160x45|160|45|workspace|review|projects|activity|dark|unicode
v1.5-workspace-review-200x60|200|60|workspace|review|projects|activity|dark|unicode
v1.5-agent-max-120x40|120|40|workspace|agent-max|projects|activity|dark|unicode
v1.5-files-contained-160x45|160|45|workspace|files|projects|files|dark|unicode
v1.5-files-editor-max-120x40|120|40|workspace|files-editor-max|projects|files|dark|unicode
v1.5-file-tab-160x45|160|45|workspace|file-tab|projects|files|dark|unicode
v1.5-file-tab-max-120x40|120|40|workspace|file-tab-max|projects|files|dark|unicode
v1.5-git-contained-160x45|160|45|workspace|git|projects|git|dark|unicode
v1.5-git-diff-max-120x40|120|40|workspace|git-diff-max|projects|git|dark|unicode
v1.5-diff-tab-160x45|160|45|workspace|diff-tab|projects|git|dark|unicode
v1.5-diff-tab-max-120x40|120|40|workspace|diff-tab-max|projects|git|dark|unicode
v1.5-terminal-max-120x40|120|40|workspace|terminal-max|projects|activity|dark|unicode
v1.5-review-light-160x45|160|45|workspace|review|projects|activity|light|unicode
v1.5-review-mono-100x30|100|30|workspace|review|recents|activity|mono|ascii
v1.5-palette-workspace-120x40|120|40|workspace|review|projects|activity|dark|unicode|all
v1.5-palette-agents-100x30|100|30|workspace|agent-max|projects|activity|dark|unicode|agents
v1.5-palette-files-120x40|120|40|workspace|file-tab|recents|files|dark|unicode|files
v1.5-palette-minimal-80x24|80|24|minimal|review|projects|activity|dark|ascii|all
SCENARIOS
echo "$count V1.5 terminal-cell scenarios rendered; each has ANSI/Text/SVG/PNG."