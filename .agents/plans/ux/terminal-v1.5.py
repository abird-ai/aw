#!/usr/bin/env python3
"""V1.5 terminal-cell UX study. Extends (but does not rewrite) V1.4 painter.

Real 24-bit ANSI escapes and fixed cells, *not* aw ratatui or a live PTY.
"""
from __future__ import annotations
import argparse
from pathlib import Path
import runpy
import sys

V14=runpy.run_path(str(Path(__file__).with_name("terminal-v1.4.py")))
Canvas=V14["Canvas"]
Cell=V14["Cell"]
PALETTES={"dark":V14["DARK"],"light":V14["LIGHT"],"mono":V14["MONO"]}

def clear_region(c,x,y,w,h,bg="bg"):
    for row in range(max(y,0),min(c.h,y+h)):
        for col in range(max(x,0),min(c.w,x+w)):
            c.cells[row][col]=Cell(" ","text",bg)

def footer(c,mode):
    c.fill(0,c.h-1,c.w,1,"side")
    if mode=="minimal":
        c.inline(1,c.h-1,[("muted","aw / aw "),("purple","◆ main "),("amber","M4")],min(35,c.w-24))
        c.inline(max(1,c.w-37),c.h-1,
                 [("muted","GPT-6 · ctx 18% · "),("amber","◐1 attention")],37)
    else:
        c.inline(1,c.h-1,[("muted","abird / aw "),("purple","◆ main "),("amber","M 4")],min(38,c.w-22))
        c.inline(max(1,c.w-32),c.h-1,
                 [("blue","●"),("muted"," 3 working "),("amber","◐"),("muted"," 1 attention")],32)

def minimal(c):
    # No global top header, no faux top metadata: conversation begins at row 0.
    x=2; width=c.w-4
    c.put(x,2,"› Review the runtime authorization changes.","text",limit=width)
    c.wrap(x,5,"I'll check source and epoch grants without touching other Agent work.",width,2)
    c.fill(x,9,max(0,width),1,"greentint")
    c.put(x+1,9,"✓ read · tool.rs","green",limit=width-2)
    c.fill(x,10,max(0,width),1,"greentint")
    c.put(x+1,10,"✓ tests · 47 passed","green",limit=width-2)
    c.wrap(x,14,"The grants are intact. One reviewer is waiting for approval.",width,3)
    c.line(x,c.h-4,width,"line")
    c.put(x,c.h-3,"› Ask anything…","purple",limit=width)
    footer(c,"minimal")

def make_header(c,title,standalone=False):
    if standalone:
        return 0
    c.fill(0,0,c.w,1,"side")
    c.inline(1,0,[("text","aw"),("muted"," / "),("text","abird"),("purple"," ▾"),
                  ("muted"," / "),("text","aw"),("purple"," ◆ main")],max(10,c.w-22))
    c.inline(c.w-17,0,[("green","●"),("muted"," x2 "),("green","●"),
                      ("muted"," a1 "),("purple","▾")],17)
    return 1

def draw_file(c,x,y,w,h,title,focus=True,immersive=False):
    if w<20 or h<6:return
    c.box(x,y,w,h,title,focus)
    c.put(x+w-4,y,"❐", "purple",limit=1)
    code=[
        " 71  pub fn authorize(tool: Tool) {",
        " 72    let source = tool.source;",
        " 73    let effect = tool.effect;",
        " 74    envelope.authorize(",
        " 75        source, effect)?;",
        " 76    journal.intent(..)?;",
        " 77    dispatch(tool)",
        " 78  }",
    ]
    c.put(x+2,y+2,("tool.rs · " if immersive else "")+"aw/main · pvl-x2 · read-only", "muted",limit=w-4)
    for i,line in enumerate(code):
        if y+5+i>=y+h-2:break
        c.put(x+2,y+5+i,line,"text",limit=w-4)
    if h>21:
        c.put(x+2,y+16,"↗ breaks out; ⛶ maximizes; opening is never a write.","muted",limit=w-4)

def draw_diff(c,x,y,w,h,title,immersive=False):
    if w<20 or h<6:return
    c.box(x,y,w,h,title,True)
    c.put(x+w-4,y,"❐","purple",limit=1)
    c.put(x+2,y+2,("Diff · hooks.rs · " if immersive else "")+"unstaged · aw/main · pvl-x2","muted",limit=w-4)
    lines=[
        ("@@ -72,4 +72,8 @@","muted","side"),
        ("fn authorize(..) {","muted","bg"),
        ("- allow_by_name(tool)?;","red","redtint"),
        ("+ envelope.check(","green","greentint"),
        ("+   source, effect,","green","greentint"),
        ("+   actor, site)?;","green","greentint"),
        ("  dispatch(..)","muted","bg"),
        ("}","muted","bg")]
    for i,(txt,color,bg) in enumerate(lines):
        row=y+5+i
        if row>=y+h-2:break
        if bg!="bg":c.fill(x+1,row,w-2,1,bg)
        c.put(x+2,row,txt,color,limit=w-4)
    if h>21:c.put(x+2,y+18,"Read-only until explicit staging or revert.","muted",limit=w-4)

def draw_agent(c,x,y,w,h):
    if w<24 or h<10:return
    c.box(x,y,w,h,"● coordinator · native Agent · aw/main",True)
    c.put(x+w-4,y,"❐","purple",limit=1)
    c.put(x+2,y+2,"context · tools 8 · 2 children","muted",limit=w-4)
    c.fill(x+2,y+4,w-4,1,"raised")
    c.put(x+3,y+4,"Review the authorization boundaries.","text",limit=w-6)
    c.wrap(x+2,y+7,"The current provider and Actor Envelope must both authorize the selected tool.",w-4,2)
    c.fill(x+2,y+11,w-4,1,"greentint")
    c.put(x+3,y+11,"✓ tests · 47 passed","green",limit=w-6)
    c.put(x+2,y+14,"├─ ● runtime · working","blue",limit=w-4)
    c.put(x+2,y+15,"└─ ◐ hooks · awaiting approval","amber",limit=w-4)
    c.line(x+2,y+h-4,w-4,"line")
    c.put(x+2,y+h-3,"❯ Message coordinator…","purple",limit=w-4)

def draw_terminal(c,x,y,w,h):
    if w<20 or h<7:return
    c.box(x,y,w,h,"● tests · real PTY · pvl-a1",True)
    c.put(x+w-4,y,"❐","purple",limit=1)
    c.put(x+2,y+3,"$ cargo test --locked -p aw-agent-core","text",limit=w-4)
    c.put(x+2,y+4,"✓ 47 passed · 0 failed","green",limit=w-4)
    c.put(x+2,y+6,"$ ▍","purple",limit=w-4)
    c.put(x+2,y+10,"viewer (read only) · resize requires actual PTY owner","muted",limit=w-4)

def draw_zoom(c,x,y,w,h,kind):
    if kind=="agent":
        draw_agent(c,x,y,w,h)
    elif kind in ("file","editor"):
        draw_file(c,x,y,w,h,"" if kind=="file" else "tool.rs",immersive=kind=="file")
    elif kind in ("diff","gitdiff"):
        draw_diff(c,x,y,w,h,"" if kind=="diff" else "Diff · hooks.rs",immersive=kind=="diff")
    elif kind=="terminal":
        draw_terminal(c,x,y,w,h)

def draw(c,mode,tab,nav,dock):
    if mode=="minimal":
        minimal(c);return
    # Reuse V1.4's exact Space header + independent mini dock; do not change
    # the frozen old V1.4 fixtures/source when evaluating this new iteration.
    base=V14["render"](c.w,c.h,"workspace","review",nav,dock,
                       "dark" if c.p is PALETTES["dark"] else "light" if c.p is PALETTES["light"] else "mono",
                       "ascii" if c.ascii else "unicode")
    c.cells=base.cells
    sidew=min(34,max(26,c.w//5)) if c.w>=100 else 0
    x=sidew+1 if sidew else 0
    contentw=c.w-x
    clear_region(c,x,1,contentw,c.h-2)
    c.fill(x,1,contentw,1,"side")
    labels={
      "review":"[Review] [Agent] [Files] [Git] [Build] [+]",
      "agent-max":"[Review] [Agent] [Files] [Git] [Build] [+]",
      "files":"[Review] [Agent] [Files] [Git] [Build] [+]",
      "files-editor-max":"[Review] [Agent] [Files] [Git] [Build] [+]",
      "file-tab":"[Review] [Agent] [Files] [Git] [▤ tool.rs ×] [+]",
      "file-tab-max":"",
      "git":"[Review] [Agent] [Files] [Git] [Build] [+]",
      "diff-tab":"[Review] [Agent] [Files] [Git] [± hooks.rs ×] [+]",
      "diff-tab-max":"",
      "terminal-max":"[Review] [Agent] [Files] [Git] [Build] [+]",
    }
    c.put(x+1,1,labels.get(tab,"[Review] [Agent] [+]"),"purple",limit=max(0,contentw-2))
    # The selected TAB is the title. A second generic context/title bar
    # duplicates that information and is intentionally absent in V1.5.
    cx=x+1;cy=2;cw=c.w-cx-1;ch=c.h-cy-2
    if tab=="review":
        if c.w>=115:
            bottom=max(9,ch//4); top=ch-bottom
            left=max(40,int(cw*.55))
            draw_agent(c,cx,cy,left,top)
            draw_diff(c,cx+left,cy,cw-left,top,"Diff · tool.rs ↗ ⛶")
            draw_terminal(c,cx,cy+top,cw,bottom)
        else:draw_agent(c,cx,cy,cw,ch)
    elif tab=="files":
        V14["draw_files"](c,cx,cy,cw,ch)
        c.put(c.w-5,cy,"↗ ⛶","purple",limit=3)
    elif tab=="git":
        V14["draw_git"](c,cx,cy,cw,ch)
        c.put(c.w-5,cy,"↗ ⛶","purple",limit=3)
    elif tab=="file-tab":
        draw_file(c,cx,cy,cw,ch,"")
    elif tab=="diff-tab":
        draw_diff(c,cx,cy,cw,ch,"")
    elif tab=="file-tab-max" or tab=="diff-tab-max":
        # Immersive standalone tab => no global header, no sidebar or tabbar.
        c.cells=Canvas(c.w,c.h,c.p,c.ascii).cells
        draw_zoom(c,1,0,c.w-2,c.h-1,"file" if tab=="file-tab-max" else "diff")
    else:
        draw_zoom(c,cx,cy,cw,ch,"agent" if tab=="agent-max" else
                  "editor" if tab=="files-editor-max" else
                  "gitdiff" if tab=="git-diff-max" else "terminal")
    footer(c,"workspace")

def draw_palette(c:Canvas,category:str)->None:
    """One centered aw command palette with authentic existing prefix contract.

    This is a static visual fixture; keyboard/search behavior lives in the
    offline HTML mock and the future real aw palette implementation.
    """
    width=min(c.w-6,101);height=min(c.h-4,19)
    x=(c.w-width)//2;y=(c.h-height)//2
    # Modal must fully paint its area. Changing background colors alone
    # would leave stray Agent/Diff glyphs visible THROUGH the palette.
    clear_region(c,x,y,width,height,"side")
    c.box(x,y,width,height,"Command Palette")
    # Existing aw categories: > Actions, ! Windows, @ Files,
    # # Sessions, $ Terminals, / Commands; proposed: & Agents.
    categories="> Actions  ·  ! Windows  ·  @ Files  ·  # Sessions  ·  $ Terminals"
    extra="/ Commands  ·  & Agents"
    query={
      "all":("› Search actions, files, sessions, agents…" if width<85 else
             "› Search actions, windows, files, sessions, terminals, commands, agents"),
      "agents":"› &hooks",
      "files":"› @tool",
    }.get(category,"› ")
    c.fill(x+2,y+2,width-4,1,"raised")
    c.put(x+3,y+2,query,"purple",limit=width-6)
    c.put(x+2,y+4,categories,"muted",limit=width-4)
    c.put(x+2,y+5,extra,"purple" if category=="agents" else "muted",limit=width-4)
    c.line(x+1,y+6,width-2,"line")
    if category=="agents":
        entries=[
          ("Agents","hooks lead","◐ review needed · aw/feat-hooks · a1"),
        ]
    elif category=="files":
        entries=[
          ("Files","tool.rs","File · aw-agent-core/src · x2"),
          ("Files","tool.rs · Diff","Unstaged · aw/main · x2"),
        ]
    else:
        entries=[
          ("Actions","New workspace tab","visual layout only"),
          ("Windows","Review","saved Workspace layout"),
          ("Files","tool.rs","File · aw-agent-core/src · x2"),
          ("Sessions","aw/main","durable conversation"),
          ("Terminals","tests","PTY · a1 · viewer"),
          ("Commands","/think","Agent slash command"),
          ("Agents","runtime lead","working · child · x2"),
        ]
    for index,(kind,name,detail) in enumerate(entries):
        yy=y+7+index
        if yy>=y+height-2:break
        if index==0:c.fill(x+1,yy,width-2,1,"select")
        col={"Actions":"blue","Windows":"amber","Files":"green",
             "Sessions":"purple","Terminals":"blue","Commands":"red","Agents":"purple"}[kind]
        c.put(x+2,yy,kind,col,limit=11)
        c.put(x+15,yy,name,"text",limit=min(22,width-19))
        if width>65:c.put(x+39,yy,detail,"muted",limit=width-43)
    c.line(x+1,y+height-3,width-2,"line")
    c.put(x+2,y+height-2,"↑ ↓ select  ·  Enter open  ·  Esc close",
          "muted",limit=width-4)


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--width",type=int,default=160)
    ap.add_argument("--height",type=int,default=45)
    ap.add_argument("--mode",choices=("minimal","workspace"),default="workspace")
    ap.add_argument("--tab",choices=("review","agent-max","files","files-editor-max",
                 "file-tab","file-tab-max","git","git-diff-max","diff-tab",
                 "diff-tab-max","terminal-max"),default="review")
    ap.add_argument("--nav",choices=("projects","recents"),default="projects")
    ap.add_argument("--dock",choices=("activity","git","files"),default="activity")
    ap.add_argument("--theme",choices=("dark","light","mono"),default="dark")
    ap.add_argument("--glyphs",choices=("unicode","ascii"),default="unicode")
    ap.add_argument("--palette",choices=("none","all","agents","files"),default="none",
                    help="overlay unified aw command palette for terminal-cell visual proof")
    ap.add_argument("--output",type=Path)
    a=ap.parse_args()
    if a.width<50 or a.height<20 or a.width>240 or a.height>80:
        ap.error("fixture requires 50..240 columns, 20..80 rows")
    c=Canvas(a.width,a.height,PALETTES[a.theme],a.glyphs=="ascii")
    draw(c,a.mode,a.tab,a.nav,a.dock)
    if a.palette!="none":
        draw_palette(c,a.palette)
    assert len(c.cells)==a.height and all(len(row)==a.width for row in c.cells)
    assert c.clip_errors==0,("cell clipping",c.clip_errors)
    if a.output:
        a.output.parent.mkdir(parents=True,exist_ok=True)
        for extension,value in ((".ansi",c.ansi()),(".txt",c.plain()),(".svg",c.svg())):
            Path(str(a.output)+extension).write_text(value)
        print(f"V1.5 terminal cells: {a.width}x{a.height} {a.mode} {a.tab} {a.theme}")
    else:sys.stdout.write(c.ansi())

if __name__=="__main__":main()