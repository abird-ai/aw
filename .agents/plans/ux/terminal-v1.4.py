#!/usr/bin/env python3
"""Standalone V1.4 terminal-CELL visual study (not aw application code).

Builds exact rows*cols character cells and writes genuine ANSI truecolor
sequences, plain-text cell snapshots, and optional deterministic per-cell SVG
snapshots. No ratatui dependency, provider, Git, filesystem scanning or PTY.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from html import escape
from pathlib import Path
import sys
import textwrap
import unicodedata

DARK={
    "bg":"#14151c","side":"#1b1d27","raised":"#252734",
    "line":"#363846","text":"#dadde8","muted":"#9a9cae",
    "faint":"#74788b","purple":"#b9a0e8","blue":"#90b5f2",
    "green":"#87c99c","amber":"#e6bc76","red":"#eb929c",
    "select":"#302e43","greentint":"#202a29","redtint":"#302127",
    "ambertint":"#2c2923",
}
LIGHT={
    "bg":"#fbfbfd","side":"#f0f1f6","raised":"#e9eaf2",
    "line":"#d6d9e4","text":"#303347","muted":"#6f7386",
    "faint":"#9ca1b1","purple":"#7557ad","blue":"#3f78c3",
    "green":"#337d4d","amber":"#9b6c1d","red":"#bd5260",
    "select":"#e4e4f0","greentint":"#edf5ee","redtint":"#f9e9eb",
    "ambertint":"#f6f1e7",
}
MONO={k:("#111111" if k not in ("bg","side","raised","select") else
         "#ffffff" if k=="bg" else "#f0f0f0") for k in DARK}
CELL_W=8
CELL_H=18

def cell_width(ch:str)->int:
    if unicodedata.combining(ch):
        return 0
    return 2 if unicodedata.east_asian_width(ch) in ("W","F") else 1

@dataclass
class Cell:
    ch:str=" "
    fg:str="text"
    bg:str="bg"

class Canvas:
    def __init__(self,width:int,height:int,palette:dict[str,str],ascii_only:bool):
        self.w=width;self.h=height;self.p=palette;self.ascii=ascii_only
        self.cells=[[Cell() for _ in range(width)] for _ in range(height)]
        self.clip_errors=0
    def fill(self,x:int,y:int,w:int,h:int,bg:str)->None:
        for yy in range(max(0,y),min(self.h,y+h)):
            for xx in range(max(0,x),min(self.w,x+w)):
                self.cells[yy][xx].bg=bg
    def glyph(self,ch:str)->str:
        if not self.ascii:return ch
        mapping={"┌":"+","┐":"+","└":"+","┘":"+","│":"|","─":"-",
                 "├":"+","┤":"+","┬":"+","┴":"+","┼":"+","▾":"v",
                 "▸":">","◆":"*","◉":"@","●":"o","◐":"o","✓":"v",
                 "▤":"#","◷":"~","▦":"#","▍":"|","±":"+","→":">",
                 "…":".","›":">","·":".","↻":"~","—":"-","–":"-"}
        return mapping.get(ch,ch if ord(ch)<128 else "?")
    def put(self,x:int,y:int,text:str,fg:str="text",bg:str|None=None,limit:int|None=None)->None:
        if y<0 or y>=self.h:return
        if limit is None:limit=self.w-x
        xx=x;remaining=limit
        for raw in text:
            ch=self.glyph(raw)
            cw=cell_width(ch)
            if cw==0:continue
            if remaining<cw:break
            if 0<=xx<self.w and xx+cw<=self.w:
                self.cells[y][xx]=Cell(ch,fg,bg or self.cells[y][xx].bg)
                if cw==2:self.cells[y][xx+1]=Cell(" ",fg,bg or self.cells[y][xx+1].bg)
            else:self.clip_errors+=1
            xx+=cw;remaining-=cw
    def line(self,x:int,y:int,w:int,fg:str="line")->None:
        self.put(x,y,"─"*max(0,w),fg,limit=w)
    def box(self,x:int,y:int,w:int,h:int,title:str="",focus:bool=False)->None:
        if w<4 or h<3:return
        c="purple" if focus else "line"
        self.put(x,y,"┌"+"─"*(w-2)+"┐",c,limit=w)
        for yy in range(y+1,y+h-1):
            self.put(x,yy,"│",c)
            self.put(x+w-1,yy,"│",c)
        self.put(x,y+h-1,"└"+"─"*(w-2)+"┘",c,limit=w)
        if title:self.put(x+2,y,title,"text",limit=w-4)
    def inline(self,x:int,y:int,parts:list[tuple[str,str]],width:int)->None:
        for color,text in parts:
            if width<=0:break
            self.put(x,y,text,color,limit=width)
            used=sum(cell_width(self.glyph(c)) for c in text)
            x+=used;width-=used
    def wrap(self,x:int,y:int,text:str,width:int,max_rows:int,color:str="text")->None:
        if width<3:return
        lines=textwrap.wrap(text,width=width,break_long_words=False,break_on_hyphens=False)
        for idx,line in enumerate(lines[:max_rows]):
            self.put(x,y+idx,line,color,limit=width)
    def plain(self)->str:
        return "\n".join("".join(c.ch for c in row).rstrip() for row in self.cells)+"\n"
    def ansi(self)->str:
        def rgb(v:str)->tuple[int,int,int]:
            return tuple(int(v[n:n+2],16) for n in (1,3,5))
        lines=[]
        for row in self.cells:
            bits=[];current=None
            for cell in row:
                style=(cell.fg,cell.bg)
                if style!=current:
                    fr,fg,fb=rgb(self.p[style[0]])
                    br,bg,bb=rgb(self.p[style[1]])
                    bits.append(f"\x1b[38;2;{fr};{fg};{fb};48;2;{br};{bg};{bb}m")
                    current=style
                bits.append(cell.ch)
            lines.append("".join(bits)+"\x1b[0m")
        return "\x1b[H"+"\n".join(lines)+"\x1b[0m\n"
    def svg(self)->str:
        items=[f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w*CELL_W}" height="{self.h*CELL_H}" viewBox="0 0 {self.w*CELL_W} {self.h*CELL_H}">',
               '<desc>Terminal cell-by-cell rendering of aw UX V1.4; ANSI source remains authoritative</desc>',
               '<rect width="100%" height="100%" fill="'+self.p["bg"]+'"/>']
        for yy,row in enumerate(self.cells):
            start=0
            while start<self.w:
                bg=row[start].bg;end=start+1
                while end<self.w and row[end].bg==bg:end+=1
                if bg!="bg":
                    items.append(f'<rect x="{start*CELL_W}" y="{yy*CELL_H}" width="{(end-start)*CELL_W}" height="{CELL_H}" fill="{self.p[bg]}"/>')
                start=end
            for xx,c in enumerate(row):
                if c.ch==" ":continue
                items.append(f'<text x="{xx*CELL_W+1}" y="{yy*CELL_H+13}" fill="{self.p[c.fg]}" font-size="13" font-family="DejaVu Sans Mono, Liberation Mono, monospace">{escape(c.ch)}</text>')
        items.append("</svg>")
        return "\n".join(items)+"\n"

def render(w:int,h:int,mode:str,tab:str,nav:str,dock:str,theme:str,glyphs:str)->Canvas:
    p=DARK if theme=="dark" else LIGHT if theme=="light" else MONO
    c=Canvas(w,h,p,glyphs=="ascii")
    # header/footer are ONE row each, entirely real terminal cells.
    c.fill(0,0,w,1,"side")
    c.fill(0,h-1,w,1,"side")
    c.inline(1,0,[("text","aw"),("muted"," / "),("text","abird"),("purple"," ▾"),
                  ("muted"," / "),("text","aw"),("purple"," ◆ main")],w-21)
    c.inline(w-16,0,[("green","●"),("muted"," x2 "),("green","●"),("muted"," a1 "),
                     ("purple","▾")],16)
    c.inline(1,h-1,[("muted","abird / aw "),("purple","◆ main "),("amber","M 4")],min(w-27,32))
    c.inline(max(0,w-33),h-1,[("blue","●"),("muted"," 3 working "),("amber","◐"),("muted"," 1 attention")],32)

    sidebar=mode!="minimal" and w>=100
    sidew=min(34,max(26,w//5)) if sidebar else 0
    if sidebar:
        c.fill(0,1,sidew,h-2,"side")
        for yy in range(1,h-1):c.put(sidew,yy,"│","line")
        c.put(1,1,"projects" if nav=="projects" else "recents","muted",limit=sidew-12)
        c.inline(sidew-9,1,[("purple","[#] " if nav=="projects" else " # "),("muted","[~]")],8)
        if nav=="projects":
            entries=[
                ("▾ aw          M 4",0,"text"),
                ("◆ main · x2",1,"purple"),
                ("◆ feat/hooks · a1",1,"muted"),
                ("▸ dotlink",0,"text"),
                ("▸ abird-health",0,"text"),
            ]
        else:
            entries=[
                ("▤ Review",0,"text"),("◉ coordinator",0,"text"),
                ("* dotlink",0,"text"),("# tests · a1",0,"text"),
                ("# tool.rs",0,"text")
            ]
        for i,(value,depth,color) in enumerate(entries):
            if 3+i>=h-15:break
            c.put(2+2*depth,3+i,value,color,limit=sidew-4-2*depth)
        docktop=max(13,int((h-2)*0.61))
        c.line(0,docktop,sidew,"line")
        c.inline(1,docktop+1,[("muted",dock),("amber"," o1" if dock=="activity" else " M4")],sidew-11)
        c.inline(sidew-11,docktop+1,[("purple","[o] " if dock=="activity" else " o  "),("muted","[+] [#]")],11)
        if dock=="activity":
            rows=["▾ Recents         3","  # Review","  @ coordinator",
                  "  * dotlink","▸ Now             3","▸ Automations      2"]
        elif dock=="git":
            rows=["aw/main · x2","v Unstaged        4",
                  "  M tool.rs     +52","  M hooks.rs     +47",
                  "  M message.rs   +27","▸ Staged          0"]
        else:
            rows=["aw/main · x2","v crates","  v aw-agent-core",
                  "    v src","      M tool.rs","      M hooks.rs"]
        for i,value in enumerate(rows):
            if docktop+3+i>=h-1:break
            color=("green" if value.startswith("  M") else "muted")
            c.put(2,docktop+3+i,value,color,limit=sidew-4)
    xmin=sidew+1 if sidebar else 0
    avail=w-xmin
    if mode=="minimal":
        draw_agent(c,xmin+1,1,avail-2,h-2,True,unboxed=True)
    elif mode=="agents":
        draw_agent(c,xmin+1,1,avail-2,h-2,True,children=True,unboxed=True)
    else:
        c.fill(xmin,1,avail,1,"side")
        c.put(xmin+1,1,"[Review] [Agent] [Git] [Files] [Build] [+]","purple",limit=avail-2)
        c.put(xmin+1,2,"Review · aw * main · pvl-x2","muted",limit=avail-2)
        left=xmin+1
        top=3
        innerw=max(8,w-left-1)
        innerh=max(8,h-top-2)
        if tab=="terminals":
            draw_terminal_grid(c,left,top,innerw,innerh)
        elif tab=="git":
            draw_git(c,left,top,innerw,innerh)
        elif tab=="files":
            draw_files(c,left,top,innerw,innerh)
        elif tab in ("spaces","fleet"):
            draw_fleet(c,left,top,innerw,innerh,tab)
        elif tab=="agent":
            draw_agent(c,left,top,innerw,innerh,True)
        else:
            if w>=112 and innerh>=22:
                bottom=max(8,int(innerh*.28))
                topheight=innerh-bottom
                split=max(38,int(innerw*.54))
                draw_agent(c,left,top,split,topheight,False)
                draw_diff(c,left+split,top,innerw-split,topheight)
                draw_terminal(c,left,top+topheight,innerw,bottom,"tests · pvl-a1",True)
            else:
                draw_agent(c,left,top,innerw,innerh,True)
    return c

def draw_agent(c:Canvas,x:int,y:int,w:int,h:int,focused:bool,
               children:bool=False,unboxed:bool=False)->None:
    if w<14 or h<8:return
    if unboxed:
        inner=w-4
        c.put(x+2,y+1,"context 4 files · 3 skills · 8 granted tools","muted",limit=inner)
        c.put(x+2,y+4,"› Review the runtime authorization changes.","text",limit=inner)
        c.wrap(x+2,y+7,"I'll inspect exact source/epoch grants and run the relevant tests.",inner,2)
        c.put(x+2,y+10,"✓ read · tool.rs","green",limit=inner)
        c.put(x+2,y+11,"✓ tests · 47 passed","green",limit=inner)
        if children:
            c.put(x+2,y+14,"● runtime lead · working","blue",limit=inner)
            c.put(x+2,y+15,"+-- v tests complete","green",limit=inner)
            c.put(x+2,y+17,"◐ hooks lead · awaiting review","amber",limit=inner)
        else:
            c.wrap(x+2,y+14,"The remaining concern is revocation while a child waits for approval.",inner,3)
        c.line(x+2,y+h-4,max(0,w-4),"line")
        c.put(x+2,y+h-3,"› Ask anything ...","purple",limit=inner)
        return
    c.box(x,y,w,h,"● coordinator · Agent · x2",focus=focused)
    inner=w-4
    c.put(x+2,y+2,"context 4 files · 3 skills · 8 granted tools","muted",limit=inner)
    c.fill(x+2,y+4,max(0,w-4),2,"raised")
    c.wrap(x+3,y+4,"Review the runtime authorization changes.",max(1,w-6),2)
    c.wrap(x+2,y+8,"The exact grant path was inspected, and model-visible tools are source-bound.",inner,3)
    c.fill(x+2,y+12,max(0,w-4),1,"greentint")
    c.put(x+3,y+12,"✓ tests · 47 passed", "green",limit=w-6)
    if children:
        c.put(x+2,y+15,"● runtime lead · working","blue",limit=inner)
        c.put(x+4,y+16,"+-- v tests complete","green",limit=inner-2)
        c.put(x+2,y+18,"o hooks lead · awaiting approval","amber",limit=inner)
    else:
        if h>24:
            c.fill(x+2,y+15,max(0,w-4),1,"ambertint")
            c.put(x+3,y+15,"◐ hooks lead · approval requested","amber",limit=w-6)
    c.line(x+2,y+h-4,max(0,w-4),"line")
    c.put(x+2,y+h-3,"› Ask anything ...","purple",limit=inner)

def draw_diff(c:Canvas,x:int,y:int,w:int,h:int)->None:
    if w<18 or h<8:return
    c.box(x,y,w,h,"Diff · tool.rs",False)
    lines=[
        ("@@ -72,4 +72,8 @@","muted","side"),
        ("fn authorize(tool) {","text","bg"),
        ("- allow_by_name(tool);","red","redtint"),
        ("+ envelope.check(","green","greentint"),
        ("+   source, effect,","green","greentint"),
        ("+   actor, site)?;","green","greentint"),
        ("  dispatch(tool)","muted","bg"),
        ("}","muted","bg"),
    ]
    for i,(text,color,bg) in enumerate(lines):
        if y+2+i>=y+h-2:break
        if bg!="bg":c.fill(x+1,y+2+i,max(0,w-2),1,bg)
        c.put(x+2,y+2+i,text,color,limit=w-4)

def draw_terminal(c:Canvas,x:int,y:int,w:int,h:int,title:str,viewer:bool=False)->None:
    if w<16 or h<5:return
    c.box(x,y,w,h,title)
    c.put(x+2,y+2,"$ cargo test --locked -p aw-agent-core","text",limit=w-4)
    c.put(x+2,y+3,"✓ 47 passed · 0 failed","green",limit=w-4)
    if h>=7:c.put(x+2,y+5,"$ |","purple",limit=w-4)
    if viewer:c.put(x+w-12,y,"viewer","muted",limit=8)

def draw_terminal_grid(c:Canvas,x:int,y:int,w:int,h:int)->None:
    halfw=w//2;halfh=h//2
    for i,title in enumerate(("build · x2","tests · a1","debug · x2","shell · a1")):
        col=i%2;row=i//2
        draw_terminal(c,x+col*halfw,y+row*halfh,
                      halfw if col==0 else w-halfw,
                      halfh if row==0 else h-halfh,title,i==1)

def draw_git(c:Canvas,x:int,y:int,w:int,h:int)->None:
    half=max(27,w*43//100)
    c.box(x,y,half,h,"Git changes · aw/main",True)
    for i,txt in enumerate(("Staged         0","Unstaged       4",
                            "M tool.rs     +52","M hooks.rs    +47",
                            "M message.rs +27","D legacy.rs   -31")):
        c.put(x+2,y+2+i,txt,"amber" if txt.startswith("M") else "text",limit=half-4)
    draw_diff(c,x+half,y,w-half,h)

def draw_files(c:Canvas,x:int,y:int,w:int,h:int)->None:
    half=max(25,w*39//100)
    c.box(x,y,half,h,"Files · aw/main",False)
    for i,txt in enumerate(("v crates","  v aw-agent-core","    v src",
                            "      M tool.rs","      M hooks.rs","      agent.rs",
                            "  > aw-agent-kernel","> .agents")):
        c.put(x+2,y+2+i,txt,"amber" if " M " in txt else "text",limit=half-4)
    rx=x+half
    c.box(rx,y,w-half,h,"tool.rs · Rust · read only",True)
    for i,txt in enumerate(("71 pub fn authorize(..) {","72   let source = tool.source;",
                            "73   let effect = tool.effect;","74   envelope.authorize(",
                            "75     source, effect)?;","76   journal.intent(..)?;",
                            "77   dispatch(..)","78 }")):
        c.put(rx+2,y+2+i,txt,"text",limit=w-half-4)

def draw_fleet(c:Canvas,x:int,y:int,w:int,h:int,tab:str)->None:
    c.box(x,y,w,h,"Spaces" if tab=="spaces" else "Machines / Fleet",True)
    for i,txt in enumerate(("abird · fleet · x2+a1","Local x2 · Home ~","Local a1 · Home ~")
                           if tab=="spaces" else
                           ("o pvl-x2 · connected","o pvl-a1 · connected","Attach machine · explicit grant")):
        c.put(x+3,y+3+i*2,txt,"green" if txt.startswith("o") else "text",limit=w-6)

def main()->None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--width",type=int,default=160)
    parser.add_argument("--height",type=int,default=45)
    parser.add_argument("--mode",choices=["minimal","agents","workspace"],default="workspace")
    parser.add_argument("--tab",choices=["review","agent","git","files","terminals","spaces","fleet"],default="review")
    parser.add_argument("--nav",choices=["projects","recents"],default="projects")
    parser.add_argument("--dock",choices=["activity","git","files"],default="activity")
    parser.add_argument("--theme",choices=["dark","light","mono"],default="dark")
    parser.add_argument("--glyphs",choices=["unicode","ascii"],default="unicode")
    parser.add_argument("--output",type=Path,help="Write matching .ansi, .txt and .svg files instead of stdout")
    args=parser.parse_args()
    if not 50<=args.width<=240 or not 20<=args.height<=80:
        parser.error("design fixture must have 50..240 columns and 20..80 rows")
    canvas=render(args.width,args.height,args.mode,args.tab,args.nav,args.dock,args.theme,args.glyphs)
    assert len(canvas.cells)==args.height
    assert all(len(row)==args.width for row in canvas.cells)
    assert canvas.clip_errors==0,f"unexpected cell clipping: {canvas.clip_errors}"
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True)
        Path(str(args.output)+".ansi").write_text(canvas.ansi())
        Path(str(args.output)+".txt").write_text(canvas.plain())
        Path(str(args.output)+".svg").write_text(canvas.svg())
        print(f"Terminal cells: {args.width}x{args.height}, {args.theme} / {args.glyphs} / {args.mode}; {args.output}")
    else:
        sys.stdout.write(canvas.ansi())

if __name__=="__main__":
    main()