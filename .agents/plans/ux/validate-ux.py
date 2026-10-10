#!/usr/bin/env python3
"""Offline, stdlib-only design archive integrity tests (no live aw tools)."""
from __future__ import annotations

import argparse
from html.parser import HTMLParser
from pathlib import Path
import re
import os
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent
VERSIONS = ("v0", "v1", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5")
DOCS = (
    "README.md", "V0-BASELINE.md", "V1-TILING.md",
    "V1.1-RECENTS-ACTIVITY.md", "V1.2-SPACE-MACHINE-HEADER.md",
    "V1.3-INTEGRATED-NAV.md", "V1.4-CANONICAL.md",
    "V1.5-UNIFIED-FOCUS-AND-TABS.md", "TERMINAL-V1.5-CELL-STUDY.md",
    "VISUAL-SPEC.md", "NAVIGATION-AND-STATE.md",
    "INTERACTIONS-AND-ACCEPTANCE.md", "TERMINAL-PROTOTYPE-NEXT.md",
    "REFERENCES-AND-VISUAL-INVENTORY.md", "VISUAL-GALLERY.md",
)
ASSETS = ("ux.css", "ux-data.js", "ux-views.js", "ux.js", "ux-v1.5.css", "ux-v1.5.js",
          "ux-palette-v1.5.css", "ux-palette-v1.5.js")
CAPTURES = (
    "v0-minimal", "v0-workspace", "v1-review", "v1.1-workspace",
    "v1.2-space-switcher", "v1.3-workspace", "v1.4-review",
    "v1.4-mini-git", "v1.4-mini-files", "v1.4-full-git",
    "v1.4-full-files", "v1.4-terminals", "v1.4-agents",
    "v1.4-recents", "v1.4-automations", "v1.4-local-x2",
    "v1.4-light", "v1.4-focus", "v1.4-split",
    "v1.5-minimal", "v1.5-review", "v1.5-agent-max",
    "v1.5-files-contained", "v1.5-files-editor-max", "v1.5-file-tab",
    "v1.5-file-tab-max", "v1.5-git-contained", "v1.5-git-diff-max",
    "v1.5-diff-tab", "v1.5-diff-tab-max", "v1.5-terminals",
    "v1.5-terminal-max",
    "v1.5-palette", "v1.5-palette-agents",
    "v1.5-palette-files", "v1.5-palette-minimal",
)

class HTMLRefs(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.refs: list[str] = []
    def handle_starttag(self, tag: str, attrs: list[tuple[str,str|None]]) -> None:
        for key, value in attrs:
            if key in {"src","href"} and value:
                self.refs.append(value)

def ensure(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)

def static_checks() -> int:
    count=0
    required = ("index.html", "test-interactions.html","test-ux-interactions.js",
                "test-v1.5.html", "test-ux-v1.5.js",
                "render-previews.sh", "render-terminal-v1.5.sh",
                "terminal-v1.5.py", "validate-terminal-v1.5.py",
                "validate-ux.py") + DOCS + ASSETS + tuple(v+".html" for v in VERSIONS)
    for name in required:
        ensure((ROOT/name).is_file(), f"Missing archive asset: {name}")
        count += 1
    ensure("CANONICAL" in (ROOT/"V1.4-CANONICAL.md").read_text(), "V1.4 baseline must remain archived")
    ensure("CURRENT" in (ROOT/"V1.5-UNIFIED-FOCUS-AND-TABS.md").read_text(), "V1.5 must be current")
    ensure("data-start=\"v1.5\"" in (ROOT/"index.html").read_text(), "index must open V1.5")
    ensure("frozen" in (ROOT/"V0-BASELINE.md").read_text().lower(), "V0 must be preserved")

    for doc in DOCS:
        text=(ROOT/doc).read_text()
        ensure(len(text)>450,f"Too short design document: {doc}")
        for link in re.findall(r"\]\(([^)]+)\)",text):
            destination=link.split("#",1)[0]
            if not destination or destination.startswith(("http://","https://","mailto:")):
                continue
            ensure((ROOT/destination).exists(),f"Broken relative doc link in {doc}: {destination}")
            count+=1
    for name in ("index.html", "test-interactions.html", "test-v1.5.html") + tuple(v+".html" for v in VERSIONS):
        parser=HTMLRefs()
        text=(ROOT/name).read_text()
        parser.feed(text)
        ensure("ux-data.js" in text and "ux-views.js" in text and "ux.js" in text,
               f"{name} lacks full offline visual switcher")
        if name in ("index.html","v1.5.html","test-v1.5.html"):
            ensure("ux-v1.5.js" in text and "ux-v1.5.css" in text and
                   "ux-palette-v1.5.js" in text and "ux-palette-v1.5.css" in text,
                   f"{name} missing V1.5 shared palette controls")
        for link in parser.refs:
            if link.startswith(("http:","https:")):
                raise AssertionError(f"External runtime asset in {name}: {link}")
            ensure((ROOT/link.split("?",1)[0]).is_file(),
                   f"Missing relative runtime asset: {name}: {link}")
            count+=1
        count+=1

    for script in ("ux-data.js", "ux-views.js", "ux.js", "ux-v1.5.js",
                   "ux-palette-v1.5.js", "test-ux-interactions.js", "test-ux-v1.5.js"):
        ensure("fetch(" not in (ROOT/script).read_text(),f"Unexpected network access: {script}")
        if shutil.which("node"):
            subprocess.run(["node","--check",str(ROOT/script)],check=True,capture_output=True)
        count+=1

    import hashlib
    digests=[]
    for name in CAPTURES:
        p=ROOT/"previews"/(name+".png")
        ensure(p.is_file(),f"Missing image: {p}")
        data=p.read_bytes()
        ensure(data.startswith(b"\x89PNG\r\n\x1a\n"),f"Not PNG: {name}")
        ensure(struct.unpack(">II",data[16:24])==(1440,790),f"Wrong pixels: {name}")
        ensure(len(data)>20_000,f"Unreadable/blank screenshot: {name}, {len(data)} bytes")
        digests.append(hashlib.sha256(data).digest())
        count+=1
    ensure(len(set(digests))==len(digests),"Different scenarios produced duplicate screenshots")
    return count

def browser_checks() -> int:
    chrome=next((shutil.which(x) for x in ("google-chrome","chromium","chromium-browser") if shutil.which(x)),None)
    ensure(bool(chrome),"Chrome/Chromium is needed for browser interaction proof")
    cases=[
        ("v0.html?mode=minimal",["Pi","Agent"]),
        ("v0.html?mode=workspace&tab=agent",["class=\"inspector\"","coordinator"]),
        ("v1.html?mode=workspace&tab=review",["review-tiles","native Agent"]),
        ("v1.1.html?mode=workspace",["recent","background"]),
        ("v1.2.html?mode=workspace&popover=spaces",["popover spaces","Local"]),
        ("v1.3.html?mode=workspace",["text-tab","activity"]),
        ("v1.4.html?mode=workspace&dock=git",["side-lower","Unstaged"]),
        ("v1.4.html?mode=agents&group=now",["coordinator","runtime lead"]),
        ("test-interactions.html",["data-status=\"passed\"","data-failed=\"0\""]),
        ("v1.5.html?mode=minimal",["v15-minimal","v15-footer"]),
        ("v1.5.html?mode=workspace&tab=review",["v15-review-tiles","v15-pane-control"]),
        ("v1.5.html?mode=workspace&tab=files",["v15-file-layout","Explorer"]),
        ("v1.5.html?mode=workspace&tab=file:abird:tool.rs",["standalone","v15-tab-close"]),
        ("v1.5.html?mode=workspace&tab=diff:abird:hooks.rs&zoom=standaloneDiff",["v15-app-immersive","standaloneDiff"]),
        ("v1.5.html?mode=workspace&popover=palette",
         ["v15-palette-dialog","v15-palette-categories","v15-palette-row-kind"]),
        ("v1.5.html?mode=minimal&popover=palette&query=%26hooks",
          ["v15-minimal","v15-palette-dialog","Command Palette"]),
        ("v1.5.html?mode=workspace&popover=palette&query=%40tool",
          ["v15-palette-dialog","tool.rs · Diff"]),
        ("test-v1.5.html",["data-status=\"passed\"","data-failed=\"0\""]),
    ]
    count=0
    with tempfile.TemporaryDirectory(prefix="aw-ux-check-") as temp:
        env=os.environ.copy()
        # Nix/bubblewrap sometimes lacks a default fontconfig configuration.
        # Use an installed local font in a temporary config; ship no font.
        if shutil.which("fc-list"):
            match=subprocess.run(["fc-list","-f","%{file}\\n"],capture_output=True,text=True)
            font=next((line.strip() for line in match.stdout.splitlines()
                       if line.strip() and Path(line.strip()).is_file()),None)
            if font:
                config=Path(temp)/"fonts.conf"
                config.write_text('<fontconfig><dir>'+str(Path(font).parent)+
                    '</dir><cachedir>'+temp+'/font-cache</cachedir></fontconfig>')
                env["FONTCONFIG_FILE"]=str(config)
        for i,(url,expected) in enumerate(cases):
            result=subprocess.run([
                chrome,"--headless","--no-sandbox","--disable-gpu",
                "--disable-dev-shm-usage","--virtual-time-budget=850",
                "--user-data-dir="+str(Path(temp)/("chrome-"+str(i))),
                "--dump-dom",(ROOT/url).as_uri() if "?" not in url else
                    (ROOT/url.split("?",1)[0]).as_uri()+"?"+url.split("?",1)[1]
            ],check=True,capture_output=True,text=True,timeout=35,env=env)
            dom=result.stdout
            for token in expected:
                ensure(token in dom,f"Rendered {url} missing {token}")
                count+=1
            print("  Browser PASS:",url)
    return count

if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--browser",action="store_true",help="also render every revision and execute UI interaction suite")
    args=parser.parse_args()
    n=static_checks()
    print(f"UX static + gallery + JS validation: {n} assertions passed")
    if args.browser:
        n=browser_checks()
        print(f"UX real headless browser assertions: {n} passed")