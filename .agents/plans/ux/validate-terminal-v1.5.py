#!/usr/bin/env python3
"""Pure fixed-grid ANSI/text/SVG/PNG assertions for 20 V1.5 visual fixtures."""
from __future__ import annotations
from pathlib import Path
import struct
import unicodedata

ROOT=Path(__file__).resolve().parent / "terminal-previews"
SIZES={
 "v1.5-minimal-headerless-80x24":(80,24),
 "v1.5-minimal-headerless-120x40":(120,40),
 "v1.5-workspace-review-160x45":(160,45),
 "v1.5-workspace-review-200x60":(200,60),
 "v1.5-agent-max-120x40":(120,40),
 "v1.5-files-contained-160x45":(160,45),
 "v1.5-files-editor-max-120x40":(120,40),
 "v1.5-file-tab-160x45":(160,45),
 "v1.5-file-tab-max-120x40":(120,40),
 "v1.5-git-contained-160x45":(160,45),
 "v1.5-git-diff-max-120x40":(120,40),
 "v1.5-diff-tab-160x45":(160,45),
 "v1.5-diff-tab-max-120x40":(120,40),
 "v1.5-terminal-max-120x40":(120,40),
 "v1.5-review-light-160x45":(160,45),
 "v1.5-review-mono-100x30":(100,30),
 "v1.5-palette-workspace-120x40":(120,40),
 "v1.5-palette-agents-100x30":(100,30),
 "v1.5-palette-files-120x40":(120,40),
 "v1.5-palette-minimal-80x24":(80,24),
}
count=0
def chk(value,message):
 global count
 assert value,message
 count+=1
for name,(cols,rows) in SIZES.items():
 p=ROOT/name
 for ext in (".ansi",".txt",".svg",".png"):
  chk(Path(str(p)+ext).is_file(),f"{name}: missing {ext}")
 text=Path(str(p)+".txt").read_text()
 lines=text.rstrip("\n").split("\n")
 chk(len(lines)==rows,f"{name}: wrong terminal height: {len(lines)}")
 for line in lines:
  cells=sum(2 if unicodedata.east_asian_width(ch) in ("F","W") else 1 for ch in line)
  chk(cells<=cols,f"{name}: overflow {cells} > {cols}")
 if "palette" in name:
  chk("Command Palette" in text,f"{name}: unified palette is missing")
  chk("! Windows" in text and "@ Files" in text and "# Sessions" in text,
      f"{name}: existing aw palette categories absent")
  chk("& Agents" in text,f"{name}: proposed Agents category absent")
 if name in ("v1.5-file-tab-160x45","v1.5-diff-tab-160x45"):
  chk("standalone" not in text and "aw/main" in text,
      f"{name}: duplicated standalone pane title")
 if "ascii" in name or "mono" in name:
  chk(all(ord(ch)<128 for ch in text),f"{name}: ASCII fallback contains Unicode")
 ansi=Path(str(p)+".ansi").read_text()
 chk("\x1b[H" in ansi and "\x1b[38;2;" in ansi and ";48;2;" in ansi,
     f"{name}: no truecolor terminal escape sequence")
 svg=Path(str(p)+".svg").read_text()
 chk(f'width="{cols*8}" height="{rows*18}"' in svg,f"{name}: wrong SVG cell dimensions")
 chk("<text " in svg and "<rect " in svg,f"{name}: empty SVG")
 b=Path(str(p)+".png").read_bytes()
 chk(b[:8]==b"\x89PNG\r\n\x1a\n",f"{name}: bad PNG signature")
 chk(struct.unpack(">II",b[16:24])==(cols*8,rows*18),f"{name}: wrong PNG dimensions")
 chk(len(b)>10_000,f"{name}: blank screenshot {len(b)}")
 if "minimal-headerless" in name:
  chk(not lines[0].strip(),f"{name}: minimal retained a top header")
  chk("GPT-6" in lines[-1] and "main" in lines[-1],
      f"{name}: minimal bottom status missing")
 if name=="v1.5-agent-max-120x40":
  chk("coordinator" in text and "Diff · tool.rs" not in text,
      f"{name}: Agent zoom did not hide other main panes")
 if name=="v1.5-file-tab-max-120x40":
  chk("tool.rs" in text and "projects" not in text and "Git changes" not in text,
      f"{name}: immersive File still shows navigation")
 if name=="v1.5-diff-tab-max-120x40":
  chk("Diff" in text and "projects" not in text,
      f"{name}: immersive Diff still shows navigation")
print(f"V1.5 ANSI/cell/SVG/PNG validation: {count} assertions in {len(SIZES)} fixtures")