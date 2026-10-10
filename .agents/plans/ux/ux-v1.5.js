/* V1.5 visual proposal: one Workspace with maximizable panes and native file/diff tabs.
 * Historical versions use the unchanged legacy view functions. No OS/Agent/Git actions. */
(function () {
'use strict';
const U=window.AwUX, s=U.state;
const A=(action,label,cls,value,tooltip)=>U.action(action,label,cls,value,tooltip);
const e=U.safe;
const tabKind=()=>s.v15Tabs.find(t=>t.key===s.v15Tab);
function activeZoom(){return s.v15ZoomByTab[s.v15Tab]||'';}
function buttonZoom(paneId){
 const isMax=activeZoom()===paneId;
 return A('v15-zoom',isMax?'❐':'⛶','v15-pane-control '+(isMax?'on':''),paneId,
          (isMax?'Restore ':'Maximize ')+paneId+' pane');
}
function breakout(kind,path){
 return A(kind==='file'?'v15-open-file-tab':'v15-open-diff-tab','↗','v15-pane-control',
    path,'Open '+path+' '+(kind==='file'?'file':'diff')+' in its own tab');
}
function pane(paneId,label,body,{extra='',side='',focus=false}={}){
 return '<section class="pane v15-pane '+(focus?'focused':'')+'" data-pane="'+e(paneId)+'">'+
  '<header class="pane-head"><div class="v15-pane-title">'+
    A('v15-focus-pane',label,'v15-pane-heading',paneId,'Focus '+label)+'</div>'+
    '<div class="v15-pane-actions">'+
      (side?'<span class="v15-pane-side faint">'+side+'</span>':'')+
      extra+buttonZoom(paneId)+'</div></header>'+
  '<div class="pane-inner">'+body+'</div></section>';
}
function demoAgentBody(name='coordinator'){
 if(name!=='coordinator'){
   const hooks=name==='hooks';
   return '<div class="muted" style="margin-bottom:12px">'+
     (hooks?'aw/feat-hooks · pvl-a1 · delegated by coordinator':
            'aw/main · pvl-x2 · delegated by coordinator')+'</div>'+
     '<div class="chat-user">'+(hooks?'Review typed hooks and request approval before changes.':
       'Inspect runtime contracts independently and validate tests.')+'</div>'+
     '<div class="chat-answer">'+(hooks?'The proposed hook edit requires owner approval.':
       'The runtime generation and effect checks are under review.')+'</div>'+
     (hooks?'<div class="notice"><span class="warn">◐</span> Awaiting approval · hooks.rs</div>':
       '<div class="tool-strip"><span class="ok">✓</span> tests · 47 passed</div>')+
     '<div class="v15-pane-composer"><span class="accent">❯</span> Message '+e(name)+'…</div>';
 }
 return '<div class="muted" style="margin-bottom:12px">context 4 files · tools 8 · 2 child Agents</div>'+
    '<div class="chat-user">Review the authorization boundary without changing the other Agent work.</div>'+
    '<div class="chat-answer">The visible tool source and exact Actor Envelope are checked at each effect boundary.</div>'+
    '<div class="tool-strip"><span class="ok">✓</span> read <span class="muted">src/tool.rs</span></div>'+
    '<div class="tool-strip"><span class="ok">✓</span> tests <span class="muted">47 passed</span></div>'+
    '<div class="notice"><span class="warn">◐</span> hooks lead · awaiting review <span class="faint">· a1</span></div>'+
    '<div class="v15-subagents"><div><span class="info">●</span> runtime <span class="faint">· x2</span></div>'+
    '<div class="muted">　└─ <span class="ok">✓</span> checks complete</div>'+
    '<div><span class="warn">◐</span> hooks lead <span class="faint">· feat/hooks</span></div></div>'+
    '<div class="v15-pane-composer"><span class="accent">❯</span> Message coordinator…</div>';
}
function codeBody(file){
 return '<div class="codeview">'+
 '<span class="linenum"> 71</span> pub fn authorize(tool: Tool) {\n'+
 '<span class="linenum"> 72</span>   let source = tool.source;\n'+
 '<span class="linenum"> 73</span>   let effect = tool.effect;\n'+
 '<span class="linenum"> 74</span>   envelope.check(\n'+
 '<span class="linenum"> 75</span>       source, effect)?;\n'+
 '<span class="linenum"> 76</span>   journal.intent(..)?;\n'+
 '<span class="linenum"> 77</span>   dispatch(tool)\n'+
 '<span class="linenum"> 78</span> }</div>'+
 '<div class="v15-pane-note muted">Read-only preview · open another file through the aw command palette (Ctrl+Shift+P / F1).</div>';
}
function diffBody(file,withContext=true){
 return (withContext?'<div class="v15-diff-context muted">Unstaged · aw/main · pvl-x2</div>':'')+
 '<div class="diff-code">'+
 '<span class="context">@@ -72,4 +72,8 @@</span>'+
 '<span class="context"> fn authorize(..) {</span>'+
 '<span class="minus">- allow_by_name(tool)?;</span>'+
 '<span class="plus">+ envelope.check(</span>'+
 '<span class="plus">+   tool.source,</span>'+
 '<span class="plus">+   tool.effect)?;</span>'+
 '<span class="context">  dispatch(tool)</span>'+
 '<span class="context"> }</span></div>'+
 '<div class="v15-pane-note muted">hunk 1/3 · read-only until an explicit Git action</div>';
}
function terminalBody(name){
 return '<div class="term-output"><span class="muted">'+e(name)+' · pvl-a1 · viewer</span><br>'+
 '$ cargo test --locked -p aw-agent-core<br><span class="ok">✓ 47 passed</span><br>$ <span class="accent">▍</span></div>';
}
function layoutTabs(){
 return '<div class="main-tabs v15-tabs">'+s.v15Tabs.map(t=>{
   const selected=s.v15Tab===t.key;
   return '<span class="v15-tab-entry '+(selected?'selected':'')+'">'+
     A('v15-select-tab',e(t.label),'v15-tab-label',t.key,'Open '+t.label+' tab')+
     (t.kind==='file'||t.kind==='diff'||t.kind==='scratch'?
        A('v15-close-tab','×','v15-tab-close',t.key,'Close '+t.label+' visual tab (does not terminate anything)'):'')+
     '</span>';
 }).join('')+A('v15-new-tab','＋','v15-plus',null,'Open an empty Workspace tab')+'</div>';
}
function review(){
 const zoom=activeZoom();
 const agent=pane('agent','<span class="dot info"></span><span class="strong">coordinator</span> · Agent',
      demoAgentBody(),{side:'pvl-x2',focus:!zoom||zoom==='agent'});
 const diff=pane('diff','Diff · '+e(s.selectedFile),diffBody(s.selectedFile),
      {extra:breakout('diff',s.selectedFile),side:'M · x2',focus:zoom==='diff'});
 const term=pane('terminal','<span class="dot ok"></span>tests · real PTY',
      terminalBody('tests'),{side:'viewer · a1',focus:zoom==='terminal'});
 if(zoom==='agent'||zoom==='diff'||zoom==='terminal'){
   return '<div class="v15-focus-wrap">'+({agent,diff,terminal:term}[zoom])+'</div>';
 }
 return '<div class="v15-review-tiles" data-layout="'+e(s.layout)+'">'+
 '<div class="v15-review-upper">'+agent+diff+'</div><div class="v15-review-lower">'+term+'</div></div>';
}
function agent(){
 return '<div class="v15-single-pane">'+
  pane('agent','<span class="dot '+(s.v15SelectedAgent==='hooks'?'warn':'info')+'"></span>'+
     '<span class="strong">'+e(s.v15SelectedAgent)+'</span> · native Agent',
  demoAgentBody(s.v15SelectedAgent),
  {side:(s.v15SelectedAgent==='hooks'?'feat/hooks · pvl-a1':'aw/main · pvl-x2'),focus:true})+'</div>';
}
function files(){
 const zoom=activeZoom();
 let tree='<div class="v15-file-context muted">aw/main · x2 · lazy</div>'+
 '<div class="file-row">▾ crates</div><div class="file-row">　▾ aw-agent-core</div>'+
 '<div class="file-row">　　▾ src</div>';
 for(const file of ['tool.rs','hooks.rs','agent.rs','message.rs']){
  tree+=A('v15-preview-file','<span>　　 '+(file==='tool.rs'||file==='hooks.rs'?'<span class="warn">M</span> ':'')+e(file)+'</span>',
    'file-row '+(s.v15FileWithinFiles===file?'selected':''),file,'Preview '+file+' within Files');
 }
 tree+='<div class="file-row">　▸ aw-agent-kernel</div><div class="file-row">▸ .agents</div>'+
 '<div class="v15-pane-note muted">Enter previews within Files. <span class="accent">↗</span> opens an independent tab.</div>';
 const left=pane('tree','<span class="strong">Explorer</span> · Files',tree,{side:'aw/main',focus:zoom==='tree'});
 const file=s.v15FileWithinFiles;
 const right=pane('editor','<span class="strong">'+e(file)+'</span>',
      codeBody(file),{extra:breakout('file',file),side:'x2/main · readonly',focus:zoom==='editor'||!zoom});
 return '<div class="v15-file-layout '+(zoom?'v15-focus-wrap':'')+'">'+
    (zoom==='editor'?'':left)+(zoom==='tree'?'':right)+'</div>';
}
function git(){
 const zoom=activeZoom();
 let list='<div class="muted">aw/main · x2 · 0 staged</div>'+
 '<div class="git-row">▸ Staged · 0</div><div class="git-row">▾ Unstaged · 4</div>';
 for(const f of U.files){
  list+=A('v15-preview-diff','<span>'+U.badge(f.type)+' '+e(f.path)+'</span>'+
     '<span class="muted">+'+f.adds+' / −'+f.dels+'</span>',
     'git-row '+(s.v15DiffWithinGit===f.path?'selected':''),f.path,
     'Preview '+f.path+' diff within Git');
 }
 list+='<div class="v15-pane-note muted">Selection is read-only; staging remains an explicit operation.</div>';
 const left=pane('gitlist','<span class="strong">Changes</span> · staged/unstaged',
     list,{side:'M 4',focus:zoom==='gitlist'});
 const diffFile=s.v15DiffWithinGit;
 const right=pane('gitdiff','<span class="strong">Diff</span> · '+e(diffFile),
     diffBody(diffFile),{extra:breakout('diff',diffFile),side:'unstaged · x2',focus:zoom==='gitdiff'||!zoom});
 return '<div class="v15-git-layout '+(zoom?'v15-focus-wrap':'')+'">'+
   (zoom==='gitdiff'?'':left)+(zoom==='gitlist'?'':right)+'</div>';
}
function ptyGrid(){
 const cells=[
  ['build','pvl-x2','owner','$ cargo check<br><span class="ok">✓ Finished dev profile</span><br>$ <span class="accent">▍</span>'],
  ['tests','pvl-a1','viewer','$ cargo test<br><span class="ok">46 passed</span> · 1 running'],
  ['debug','pvl-x2','paused','$ journalctl -f<br><span class="warn">◐ waiting</span>'],
  ['shell','pvl-a1','idle','~/abird/src/aw<br>$ <span class="accent">▍</span>']
 ];
 const zoom=activeZoom();
 const entries=cells.filter(c=>!zoom||zoom==='pty:'+c[0]);
 const panes=entries.map(c=>pane('pty:'+c[0],
     '<span class="dot '+(c[0]==='build'?'ok':c[0]==='tests'?'info':c[0]==='debug'?'warn':'')+'"></span>'+e(c[0]),
     '<div class="term-output">'+c[3]+'</div>',
     {side:e(c[1]+' · '+c[2]),focus:zoom==='pty:'+c[0]}));
 return '<div class="'+(zoom?'v15-focus-wrap':'terminal-grid')+'">'+panes.join('')+'</div>';
}
function standalone(t){
 const isDiff=t.kind==='diff',file=t.file||s.selectedFile;
 const name=isDiff?'Diff':'File',paneId=isDiff?'standaloneDiff':'file';
 const immersive=activeZoom()===paneId;
 // One title only: the tab names the document. No second pane header,
 // no duplicated filename beneath it. The tiny floating action is always
 // reachable, including when immersive zoom hides the outer tab strip.
 return '<div class="v15-single-pane v15-standalone">'+
   '<section class="v15-document-pane" data-pane="'+e(paneId)+'" aria-label="'+name+' '+e(file)+'">'+
   '<div class="v15-document-actions">'+
     (immersive?'<span class="v15-document-immersive-name">'+name+' · '+e(file)+'</span>':'')+
     buttonZoom(paneId)+'</div>'+
   '<div class="v15-document-body">'+
     '<div class="v15-document-context muted">'+
       (isDiff?'Unstaged':'Read-only')+' · aw/main · pvl-x2</div>'+
     (isDiff?diffBody(file,false):codeBody(file))+
   '</div></section></div>';
}
function scratch(){
 return '<div class="v15-single-pane">'+pane('scratch','<span class="strong">New Workspace tab</span>',
  '<div class="muted">Empty layout. Choose a pane using the palette:</div>'+
  A('v15-open-file-tab','▤ Open tool.rs in tab','v15-open-suggestion','tool.rs','Open a standalone file')+
  A('v15-open-diff-tab','± Open hooks.rs diff in tab','v15-open-suggestion','hooks.rs','Open standalone diff')+
  A('v15-select-tab','◉ Select native Agent','v15-open-suggestion','agent','Open Agent tab'),
  {side:'layout draft',focus:true})+'</div>';
}
function management(kind){
 const rows=kind==='spaces'?
 ['abird · Fleet · x2+a1','Local · x2 · Home ~','Local · a1 · Home ~']:
 ['● pvl-x2 · connected','● pvl-a1 · connected','Attach machine · intentional action'];
 return '<div class="management"><div class="strong">'+(kind==='spaces'?'Spaces':'Machines / Fleet')+
  '</div><div class="muted">Current context and exact host identity are preserved on navigation.</div>'+
  rows.map(item=>'<div class="manage-row">'+e(item)+'</div>').join('')+
  '<div class="manage-hint">These are design-only management views. Attaching hosts never merges credentials or execution permissions.</div></div>';
}
function main(){
 const t=tabKind()||s.v15Tabs[0];
 let body;
 switch(t.kind){
  case 'file':case 'diff':body=standalone(t);break;
  case 'scratch':body=scratch();break;
  default:
   if(t.key==='review')body=review();
   else if(t.key==='agent')body=agent();
   else if(t.key==='files')body=files();
   else if(t.key==='git')body=git();
   else if(t.key==='terminals')body=ptyGrid();
   else if(t.key==='spaces'||t.key==='fleet')body=management(t.key);
   else body=review();
 }
 const immersive=(t.kind==='file'||t.kind==='diff'||t.kind==='scratch')&&!!activeZoom();
 return '<main class="main v15-shell '+(immersive?'v15-immersive':'')+'">'+
     layoutTabs()+'<div class="main-content v15-content">'+body+'</div></main>';
}
function minimal(){
 // Intentionally NO topbar, sidebar, saved layout tabs or faux header metadata.
 // The single footer status is the only global chrome in Minimal.
 return '<main class="main v15-minimal"><div class="chatpane">'+
  '<div class="chat-contents v15-minimal-transcript">'+
  '<div class="chat-user">Review the current runtime architecture and identify authorization regressions.</div>'+
  '<div class="chat-answer">I will inspect the relevant boundaries and run the focused tests.</div>'+
  '<div class="tool-strip"><span class="ok">✓</span> read <span class="muted">crates/aw-agent-core/src/tool.rs</span></div>'+
  '<div class="tool-strip"><span class="ok">✓</span> tests <span class="muted">47 passed · 2.4s</span></div>'+
  '<div class="chat-answer">The source grants remain exact. A child reviewer is still awaiting a decision.</div>'+
  '</div><div class="compose"><span class="accent">❯</span> Ask anything… <span class="faint">/ commands</span></div>'+
  '</div></main>';
}
function footer(){
 const left=s.mode==='minimal'?
  '<span class="muted">~/abird/aw</span> <span class="accent">◆ main</span> <span class="warn">M 4</span>':
  '<span class="muted">'+e(U.metadata())+'</span> / aw <span class="accent">◆ main</span> <span class="warn">M 4</span>';
 return '<footer class="footer v15-footer"><span>'+left+'</span><span>'+
  (s.mode==='minimal'?'GPT-6 · ctx 18% · ':'')+
  '<span class="dot info"></span> 3 working · <span class="warn">◐</span> 1 attention</span></footer>';
}
function popover(){
 // ONE palette using the existing aw six categories + proposed '& Agents'.
 // No second Quick Switch overlay; this remains a UI-only mock.
 if(s.popover!=='palette'&&s.popover!=='quick')return '';
 if(s.popover==='quick')s.popover='palette';
 return U.v15Palette.markup();
}
U.v15={main,minimal,footer,popover,pane};
})();