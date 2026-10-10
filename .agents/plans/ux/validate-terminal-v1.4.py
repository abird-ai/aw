#!/usr/bin/env python3
"""Checks real ANSI terminal-cell fixtures and matching SVG/PNG proof artifacts."""
from __future__ import annotations
from pathlib import Path
import re
import struct
import sys
import unicodedata

HERE=Path(__file__).resolve().parent
SETS={
 "v1.4-review-160x45":(160,45),
 "v1.4-review-200x60":(200,60),
 "v1.4-mini-git-120x40":(120,40),
 "v1.4-agents-100x30":(100,30),
 "v1.4-minimal-ascii-80x24":(80,24),
 "v1.4-full-git-light-160x45":(160,45),
 "v1.4-terminals-160x45":(160,45),
 "v1.4-review-narrow-100x30":(100,30),
 "v1.4-monochrome-120x40":(120,40),
}
def cw(ch):
 return 2 if unicodedata.east_asian_width(ch) in ("F","W") else 1
def ensure(test,msg):
 if not test: raise AssertionError(msg)
count=0
for name,(w,h) in SETS.items():
 p=HERE/"terminal-previews"/name
 for ext in (".ansi",".txt",".svg",".png"):
  ensure(Path(str(p)+ext).is_file(),name+ext+" absent")
  count+=1
 text=Path(str(p)+".txt").read_text()
 rows=text.rstrip("\n").split("\n")
 ensure(len(rows)==h,f"{name}: {len(rows)} rows expected {h}")
 for row in rows:
  ensure(sum(cw(c) for c in row)<=w,f"{name}: line overflows {w} cells: {row}")
  count+=1
 if "ascii" in name or "monochrome" in name:
  ensure(all(ord(ch)<128 for ch in text),f"{name}: non-ASCII fallback glyph")
 ansi=Path(str(p)+".ansi").read_text()
 ensure("\x1b[H" in ansi and "\x1b[38;2;" in ansi and ";48;2;" in ansi,
        f"{name}: no real SGR truecolor stream")
 svg=Path(str(p)+".svg").read_text()
 ensure(f'width="{w*8}" height="{h*18}"' in svg,f"{name}: SVG cell dimensions")
 ensure("<text " in svg and "<rect " in svg,f"{name}: no cell-derived vector")
 data=Path(str(p)+".png").read_bytes()
 ensure(data[:8]==b"\x89PNG\r\n\x1a\n",f"{name}: PNG magic")
 ensure(struct.unpack(">II",data[16:24])==(w*8,h*18),f"{name}: PNG pixels")
 ensure(len(data)>10_000,f"{name}: screenshot unexpectedly blank ({len(data)})")
 count+=7
print(f"Terminal V1.4 cell/ANSI/SVG/PNG assertions passed: {count} in {len(SETS)} fixtures")
