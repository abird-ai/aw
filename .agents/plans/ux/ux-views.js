/* Screen templates, never authoritative state or live process output. */
(function(){
'use strict';
const U=window.AwUX, s=U.state;
const A=(a,label,cls,val,title)=>U.action(a,label,cls,val,title);
const R=(label,level,cls,action,val)=>U.row(label,level,cls,action,val);
const e=U.safe;
function versionIndex(){return U.VERSIONS.indexOf(s.version);}
function header(){
  if(s.version==='v1.5' && s.mode==='minimal')return '';
  const old=versionIndex()<3;
  const left=old?
    '<span class="strong">aw</span><span class="muted">/</span><span class="context-title">aw <span class="accent">◆ main</span></span>':
    '<span class="strong">aw</span><span class="muted">/</span>'+
    A('open-spaces',e(U.spaceMap[s.scope]),'top-act strong',null,'Open Spaces management')+
    A('toggle-space-menu','▾','top-act accent',null,'Quick-switch Space')+
    '<span class="muted">/</span><span class="context-title">'+
      (s.scope==='abird'?'aw <span class="accent">◆ main</span>':'Home ~')+'</span>';
  const right=old?'<span class="dot info"></span><span class="muted">local · ? help</span>':
    A('open-fleet','<span class="dot ok"></span>x2'+(s.scope==='abird'?' <span class="dot ok"></span>a1':''),'top-act',null,'Manage machines and Fleet')+
    A('toggle-machine-menu','▾','top-act accent',null,'Quick-switch a machine Local Space');
  return '<header class="topbar"><div class="topbar-left">'+left+'</div><div class="topbar-right">'+right+'</div></header>';
}
function projectTree(){
  if(s.scope==='x2'){
    return R('▾ <span class="strong">Home</span> <span class="muted">~</span>',0,'selected','noop')+
       R('▸ aw <span class="warn">M 4</span>',1,'','select-project','aw')+
       R('▸ dotlink',1,'','select-project','dotlink')+
       R('▸ zerogravity',1,'','select-project','zerogravity');
  }
  if(s.scope==='a1'){
    return R('▾ <span class="strong">Home</span> <span class="muted">~</span>',0,'selected','noop')+
       R('▸ aw <span class="accent">◆ feat/hooks</span>',1,'','select-project','aw')+
       R('▸ inference',1,'','select-project','inference');
  }
  return R('▾ <span class="strong">aw</span> <span class="warn">M 4</span>',0,'selected','select-project','aw')+
    R('<span class="accent">◆</span> main <span class="faint">· x2</span>',1,'','select-project','aw')+
    R('<span class="accent">◆</span> feat/hooks <span class="faint">· a1</span>',1,'','select-project','aw')+
    R('▸ dotlink <span class="faint">· clean</span>',0,'','select-project','dotlink')+
    R('▸ abird-health <span class="warn">M 2</span>',0,'','select-project','health')+
    (s.compact?'':R('▸ zerogravity <span class="faint">· clean</span>',0,'','select-project','zerogravity'));
}
function machineTree(){
  return '<div class="side-label">machines</div>'+
    R('▾ local <span class="ok">●</span>',0,'')+
    R('pvl-x2 <span class="ok">●</span>',1,'','set-scope','x2')+
    R('pvl-a1 <span class="ok">●</span>',1,'','set-scope','a1');
}
function agentTree(){
  return '<div class="side-label">agents · grouped</div>'+
    R('▾ <span class="strong">aw</span> · main',0,'')+
    R('<span class="info">●</span> coordinator',1,'selected','open-agent')+
    R('├─ <span class="info">●</span> runtime lead',2,'','open-agent')+
    R('│　└─ <span class="ok">✓</span> tests',3,'','noop')+
    R('└─ <span class="warn">◐</span> hooks lead',2,'','open-agent')+
    R('▸ dotlink <span class="faint">○</span>',0,'','select-project','dotlink')+
    R('▸ abird-health <span class="ok">✓</span>',0,'','select-project','health');
}
function terminalTree(){
 return '<div class="side-label">terminals</div>'+
  R('<span class="dot ok"></span>build <span class="faint">· x2</span>',1,'','set-main','terminals')+
  R('<span class="dot info"></span>tests <span class="faint">· a1</span>',1,'','set-main','terminals');
}
function activityChildren(group){
 if(s.version==='v1.5'&&group==='now')return    A('v15-show-agent','<span class="info">●</span> coordinator <span class="faint">· aw</span>','activity-child','coordinator','Open coordinator Agent')+
   A('v15-show-agent','├─ <span class="info">●</span> runtime <span class="faint">bg</span>','activity-child','runtime','Open runtime child Agent')+
   A('v15-show-agent','└─ <span class="warn">◐</span> hooks <span class="faint">bg</span>','activity-child','hooks','Open hooks child Agent');
 if(group==='recents')return U.recents.slice(0,3).map(x=>
  '<div class="activity-child">'+x.icon+' '+e(x.title)+' <span class="faint">· '+e(x.kind)+'</span></div>').join('');
 if(group==='now')return '<div class="activity-child"><span class="info">●</span> coordinator <span class="faint">· aw</span></div>'+
    '<div class="activity-child">├─ <span class="info">●</span> runtime <span class="faint">bg</span></div>'+
    '<div class="activity-child">└─ <span class="warn">◐</span> hooks <span class="faint">bg</span></div>';
 if(group==='automations')return '<div class="activity-child"><span class="accent">↻</span> dependency updater</div>'+
    '<div class="activity-child"><span class="faint">◷</span> nightly checks</div>';
 return '';
}
function activityContent(simple){
 const active=(s.nav==='recents'&&s.group==='recents')?'none':s.group;
 let out='';
 for(const [group,label,count] of [['recents','Recents','3'],['now','Now','3 · ◐1'],['automations','Automations','2']]){
   out+=A('set-group','<span>'+(active===group?'▾ ':'▸ ')+label+'</span><span class="faint">'+count+'</span>',
     'activity-row '+(active===group?'selected':''),group,'Expand Activity '+label);
   if(!simple&&active===group)out+='<div class="activity-children">'+activityChildren(group)+'</div>';
 }
 return out;
}
function miniGit(){
 if(s.scope!=='abird' && s.scope!=='x2')return '<div class="side-small">No Git repository selected · Home ~</div>'+
   A('set-main','↗ Open full Files','openfull','files');
 let b='<div class="side-small">aw/main · pvl-x2 · 0 staged</div>'+
       A('noop','▸ Staged <span class="faint">0</span>','mini-list-row')+
       A('noop','▾ Unstaged <span class="faint">4</span>','mini-list-row');
 for(const f of U.files){
   b+=A(s.version==='v1.5'?'v15-open-diff-tab':'open-git-file','<span class="left">'+U.badge(f.type)+' '+e(f.path)+
      '</span><span class="faint">'+(f.adds?'+'+f.adds:'−'+f.dels)+'</span>',
      'mini-list-row '+(s.selectedFile===f.path?'selected':''),f.path,
      'Inspect diff for '+f.path);
 }
 return b+A(s.version==='v1.5'?'v15-select-tab':'set-main','↗ Open full Git','openfull','git','Open full Git');
}
function miniFiles(){
 const paths=['crates','aw-agent-core','src','tool.rs','hooks.rs','agent.rs','aw-agent-kernel','.agents'];
 let b='<div class="side-small">aw/main · pvl-x2 · lazy</div>';
 for(const p of paths){
  const depth=p==='crates'||p==='.agents'?0:p==='aw-agent-core'||p==='aw-agent-kernel'?1:p==='src'?2:3;
  let label=depth>=3?'<span class="'+(p==='tool.rs'||p==='hooks.rs'?'warn':'')+'">'+(p==='tool.rs'||p==='hooks.rs'?'M ':'')+'</span>'+p:
    (p==='.agents'||p==='aw-agent-kernel'?'▸ ':'▾ ')+p;
  b+=A(depth>=3?(s.version==='v1.5'?'v15-open-file-tab':'open-file'):'noop','<span class="left">'+label+'</span>',
        'mini-list-row '+(s.selectedFile===p?'selected':''),p,'Select '+p);
 }
 return b+A(s.version==='v1.5'?'v15-select-tab':'set-main','↗ Open full Files','openfull','files','Open full Files');
}
function lowerDock(){
 if(versionIndex()<5)return '';
 return '<div class="resizebar" data-resize tabindex="0" role="separator" aria-orientation="horizontal" title="Resize sidebar mini dock with drag or arrows"></div>'+
 '<div class="side-lower '+(s.collapsed?'collapsed':'')+'" style="height:'+s.dockHeight+'px;flex-basis:'+s.dockHeight+'px">'+
   '<div class="side-head"><span class="side-title">'+(s.dock==='git'?'git <span class="warn">M 4</span>':s.dock==='files'?'files':'activity <span class="warn">◐ 1</span>')+'</span>'+
   '<div class="icon-tabs">'+
   A('set-dock','●','icon-tab '+(s.dock==='activity'?'selected':''),'activity','Show Activity mini dock')+
   A('set-dock','±','icon-tab '+(s.dock==='git'?'selected':''),'git','Show Git mini dock')+
   A('set-dock','▤','icon-tab '+(s.dock==='files'?'selected':''),'files','Show Files mini dock')+
   A('toggle-dock',s.collapsed?'▴':'▾','icon-tab',null,'Collapse or expand dock')+
   '</div></div>'+
   '<div class="side-scroll">'+(s.dock==='activity'?activityContent(false):s.dock==='git'?miniGit():miniFiles())+'</div></div>';
}
function sideV0(){
 return '<div class="side-upper"><div class="side-scroll">'+machineTree()+'<div class="side-sep"></div>'+
  agentTree()+'<div class="side-sep"></div>'+terminalTree()+'</div></div>'+
  '<div class="side-bottom">● 3 working　◐ 1 waiting</div>';
}
function sideV1(){
 return '<div class="side-upper"><div class="side-scroll">'+
 '<div class="side-label">spaces</div>'+R('▾ abird · 2 machines',0,'','set-scope','abird')+
 R('◆ aw',1,'','select-project','aw')+
 '<div class="side-sep"></div><div class="side-label">projects</div>'+projectTree()+
 '<div class="side-sep"></div>'+agentTree()+
 '<div class="side-sep"></div>'+terminalTree()+'</div></div>'+
 '<div class="side-bottom">● 3 working　◐ 1 attention</div>';
}
function sideV11(){
 return '<div class="side-upper"><div class="side-scroll">'+
   '<div class="side-label">recents <span class="faint">· all ›</span></div>'+U.recentRows(s.compact?2:3)+
   '<div class="side-sep"></div><div class="side-label">projects</div>'+projectTree()+
   '</div></div><div class="side-bottom">'+
   '<div class="attention"><span class="warn">◐</span> hooks lead needs approval</div>'+
   A('set-main','<span class="info">●</span> 3 background · Activity ›','mini-list-row','activity')+
   '<div class="faint">◷ 2 automation definitions</div></div>';
}
function sideV12(){
 return '<div class="side-upper"><div class="side-scroll">'+
  '<div class="side-label">recents</div>'+U.recentRows(s.compact?2:3)+
  '<div class="side-sep"></div><div class="side-label">projects</div>'+projectTree()+
  '<div class="side-sep"></div><div class="side-label">activity <span class="warn">◐1</span></div>'+
  activityContent(true)+'</div></div>';
}
function sideV13(){
 return '<div class="side-upper"><div class="side-head"><div class="icon-tabs">'+
   A('set-nav','Projects','text-tab '+(s.nav==='projects'?'selected':''),'projects')+
   A('set-nav','Recents','text-tab '+(s.nav==='recents'?'selected':''),'recents')+
   '</div></div><div class="side-scroll">'+
  (s.nav==='projects'?projectTree():U.recentRows())+'</div></div>'+
  '<div class="side-lower" style="height:184px;flex-basis:184px">'+
    '<div class="side-head"><span class="side-title">activity <span class="warn">◐ 1</span></span></div>'+
    '<div class="side-scroll">'+activityContent(false)+'</div></div>';
}
function sideV14(){
 return '<div class="side-upper"><div class="side-head"><span class="side-title">'+e(s.nav)+'</span>'+
  '<div class="icon-tabs">'+
  A('set-nav','▦','icon-tab '+(s.nav==='projects'?'selected':''),'projects','Projects navigator')+
  A('set-nav','◷','icon-tab '+(s.nav==='recents'?'selected':''),'recents','Full Recents navigator')+'</div></div>'+
  '<div class="side-scroll">'+(s.nav==='projects'?projectTree():U.recentRows())+'</div></div>'+
   lowerDock();
}
function sidebar(){
 if(s.mode==='minimal')return '';
 let inside=[sideV0,sideV1,sideV11,sideV12,sideV13,sideV14][Math.min(5,versionIndex())]();
 return '<aside class="sidebar '+(s.compact?'compact':'')+'" aria-label="aw sidebar">'+inside+'</aside>';
}
function agentChat(brief){
 let body='<div class="muted">context 4 files · 3 skills · 8 granted tools</div>'+
    '<div class="chat-user">Review the runtime architecture and check source-bound authorization.</div>'+
    '<div class="chat-answer">I will inspect the relevant boundaries and run the focused native tests.</div>'+
    '<div class="tool-strip"><span class="ok">✓</span> read <span class="muted">crates/aw-agent-core/src/tool.rs</span> <span class="faint">· 32 ms</span></div>'+
    '<div class="tool-strip"><span class="ok">✓</span> tests <span class="muted">47 passed · 2.4s</span></div>'+
    '<div class="chat-answer">The exact source and epoch checks remain intact. One child reviewer is awaiting approval. No unverified effect is automatically replayed.</div>';
 if(brief)body+='<div class="notice"><span class="warn">◐</span> hooks lead · pending approval · feat/hooks · a1</div>';
 return '<div class="chatpane"><div class="chat-contents">'+body+'</div>'+
    '<div class="compose"><span class="accent">❯</span> Ask anything… <span class="faint">/ commands</span></div></div>';
}
function childSummary(){
 return '<div class="muted">Delegate runtime and hook review independently, preserving exact Agent ownership.</div>'+
 '<div class="chat-user">Review the two P6 workstreams in parallel.</div>'+
 '<div class="chat-answer">Two child Agents are working in separate work contexts.</div>'+
 '<div class="notice" style="border-left-color:var(--blue);background:var(--panel)">'+
   '<span class="info">●</span> <span class="strong">runtime lead</span> <span class="faint">· aw/main · x2</span><br>'+
   '<span class="faint">　└─</span> <span class="ok">✓</span> tests completed<br><br>'+
   '<span class="warn">◐</span> <span class="strong">hooks lead</span> <span class="faint">· feat/hooks · a1</span><br>'+
   '<span class="faint">　└─ approval requested: hooks.rs</span></div>'+
 '<div class="chat-answer">Child updates are factual notifications; they cannot silently grant their parent new capabilities.</div>';
}
function diffCode(){
 return '<div class="diff-code"><span class="context">@@ -72,4 +72,8 @@</span>'+
   '<span class="context">fn authorize(..) {</span>'+
   '<span class="minus">- allow_by_name(tool)?;</span>'+
   '<span class="plus">+ envelope.check(</span>'+
   '<span class="plus">+   source, effect,</span>'+
   '<span class="plus">+   actor, site)?;</span>'+
   '<span class="context">  dispatch(tool)</span><span class="context">}</span></div>';
}
function pane(id,title,body,cls,side){
 return '<section class="pane '+(cls||'')+' '+(s.focusedPane===id?'focused':'')+'" data-pane="'+id+'">'+
   '<header class="pane-head">'+
   A('focus-pane','<span>'+title+'</span>','',id,'Focus '+id+' pane')+
   '<span class="faint">'+(side||'')+'</span></header>'+
   '<div class="pane-inner">'+body+'</div></section>';
}
function reviewTiles(){
 const clazz=(s.zoom?'zoom-'+s.zoom:'');
 const inner='<div class="review-tiles '+clazz+'" data-layout="'+s.layout+'">'+
  '<div class="review-top">'+
    pane('agent','<span class="dot info"></span><span class="strong">coordinator</span> · native Agent',
        '<div class="muted">Reviewing tool source grants</div>'+
        '<div class="chat-user">Check exact source and epoch authorization.</div>'+
        '<div>The current ToolPolicy and Actor Envelope must both allow the selected tool.</div>'+
        '<div class="tool-strip"><span class="ok">✓</span> tests · 47 passed</div>'+
        '<div class="notice"><span class="warn">◐</span> hooks lead · approval requested</div>',
        'agentpane','pvl-x2')+
    pane('diff','<span class="strong">Diff</span> · '+e(s.selectedFile),diffCode()+
        '<div class="focushint">hunk 1/3 · unstaged · aw/main</div>','diffpane','M · pvl-x2')+
  '</div>'+
  '<div class="review-bottom">'+pane('terminal','<span class="dot ok"></span>tests · real PTY',
      '<div class="term-output">$ cargo test --locked -p aw-agent-core<br><span class="ok">✓ 47 passed</span> <span class="faint">· 0 failed</span><br>$ <span class="accent">▍</span></div>',
      '','pvl-a1 · viewer')+'</div></div>';
 return inner;
}
function fullGit(){
 let list='<div class="muted" style="margin-bottom:11px">Changes · aw/main · pvl-x2</div>'+
   '<div class="git-row">▸ Staged <span class="muted">0</span></div>'+
   '<div class="git-row">▾ Unstaged <span class="muted">4</span></div>';
 for(const f of U.files){
  list+=A('open-git-file','<span>'+U.badge(f.type)+' '+e(f.path)+'</span>'+
    '<span class="muted">+'+f.adds+' / −'+f.dels+'</span>',
    'git-row '+(f.path===s.selectedFile?'selected':''),f.path,'Inspect diff for '+f.path);
 }
 return '<div class="git-grid">'+
   pane('gitlist','Git changes · staged/unstaged',list,'','4 files')+
   pane('diff','Diff · '+e(s.selectedFile),diffCode()+
       '<div class="separator-note">Stage/revert/commit are separate explicit actions; selection is read-only.</div>',
       '','hunk 1/3')+'</div>';
}
function fullFiles(){
 let files='<div class="muted" style="margin-bottom:11px">Explorer · aw/main · pvl-x2</div>'+
   '<div class="file-row">▾ crates</div><div class="file-row">　▾ aw-agent-core</div><div class="file-row">　　▾ src</div>';
 for(const name of ['tool.rs','hooks.rs','agent.rs']){
   files+=A('open-file','<span>　　 '+(name==='tool.rs'||name==='hooks.rs'?'<span class="warn">M</span> ':'')+name+'</span>','file-row '+(s.selectedFile===name?'selected':''),name);
 }
 files+='<div class="file-row">　▸ aw-agent-kernel</div><div class="file-row">▸ .agents</div>';
 const code='<div class="codeview"><span class="linenum"> 71</span> pub fn authorize(..) {\n'+
 '<span class="linenum"> 72</span>   let source = tool.source;\n'+
 '<span class="linenum"> 73</span>   let effect = tool.effect;\n'+
 '<span class="linenum"> 74</span>   envelope.authorize(\n'+
 '<span class="linenum"> 75</span>      source, effect)?;\n'+
 '<span class="linenum"> 76</span>   journal.intent(..)?;\n'+
 '<span class="linenum"> 77</span>   dispatch(..)\n'+
 '<span class="linenum"> 78</span> }</div>';
 return '<div class="file-grid">'+
  pane('files','Files · lazy tree',files,'','aw/main')+
  pane('editor','File · '+e(s.selectedFile),code,'','Rust · read only')+'</div>';
}
function terminals(){
 const cells=[
  ['build · pvl-x2','owner','$ cargo check<br><span class="ok">✓ Finished dev profile</span><br>$ <span class="accent">▍</span>'],
  ['tests · pvl-a1','viewer','$ cargo test<br><span class="ok">46 passed</span> <span class="info">● 1 running</span>'],
  ['debug · pvl-x2','paused','$ journalctl -f<br><span class="warn">◐ waiting</span>'],
  ['shell · pvl-a1','idle','~/abird/src/aw<br>$ <span class="accent">▍</span>']
 ];
 return '<div class="terminal-grid">'+cells.map((x,i)=>
    pane('term'+i,'<span class="dot '+(i===0?'ok':i===1?'info':i===2?'warn':'')+'"></span>'+x[0],
    '<div class="term-output">'+x[2]+'</div>','',x[1])).join('')+'</div>';
}
function management(which){
 if(which==='spaces'){
  return '<div class="management"><div class="strong">Spaces</div>'+
  '<p class="muted">Built-in Local Homes and named single-machine / fleet Spaces.</p>'+
  '<div class="manage-row"><span>▾ <span class="strong">abird</span> <span class="accent">fleet</span></span><span class="muted">x2 + a1</span></div>'+
  '<div class="manage-row"><span>▸ Local · x2</span><span class="muted">built-in · Home ~</span></div>'+
  '<div class="manage-row"><span>▸ Local · a1</span><span class="muted">built-in · Home ~</span></div>'+
  '<div class="manage-row">'+A('design-info','＋ New Space <span class="faint">· design preview</span>','info','space','Explain create Space design')+'</div>'+
  '<div class="manage-hint">Header Space name opens this management tab; the adjacent chevron opens quick switching. Attaching machines changes membership without changing Space identity or moving active processes.</div></div>';
 }
 return '<div class="management"><div class="strong">Machines / Fleet</div>'+
 '<p class="muted">Connected machines and exact execution locations</p>'+
 '<div class="manage-row"><span><span class="dot ok"></span><span class="strong">pvl-x2</span></span><span class="muted">connected · local</span></div>'+
 '<div class="manage-row"><span><span class="dot ok"></span><span class="strong">pvl-a1</span></span><span class="muted">connected · remote</span></div>'+
 '<div class="manage-row">'+A('design-info','＋ Add / attach machine <span class="faint">· design preview</span>','info','machine','Explain attach machine design')+'</div>'+
 '<div class="manage-hint">Header machine names open this manager; chevron switches to an exact machine Local Space. No implicit rerouting, file sync or tool grant.</div></div>';
}
function fullActivity(){
 return '<div class="management"><div class="strong">Activity across abird</div>'+
  '<p class="muted">Now includes foreground, delegated and unattached background work. Every Agent has one actual identity.</p>'+
  '<div class="side-label">Needs you · 1</div>'+
  '<div class="notice"><span class="warn">◐</span> hooks lead · approval request · aw/feat-hooks · a1</div>'+
  '<div class="side-label">Now · 3</div>'+
  '<div class="manage-row"><span><span class="info">●</span> coordinator</span><span class="muted">working · x2</span></div>'+
  '<div class="manage-row"><span>　├─ <span class="info">●</span> runtime · bg</span><span class="muted">working</span></div>'+
  '<div class="manage-row"><span>　└─ <span class="warn">◐</span> hooks · bg</span><span class="muted">waiting</span></div>'+
  '<div class="side-label">Automation definitions · 2</div>'+
  '<div class="manage-row"><span><span class="accent">↻</span> dependency updater</span><span class="muted">Git trigger</span></div>'+
  '<div class="manage-row"><span>◷ nightly checks</span><span class="muted">next 02:00</span></div>'+
  '</div>';
}
function inspector(){
 return '<aside class="inspector"><div class="ins-label">Selection</div>'+
  '<div class="ins-row"><span class="strong">aw</span> <span class="accent">◆ main</span><br><span class="muted">pvl-x2</span></div>'+
  '<div class="ins-label">Git</div><div class="ins-row"><span class="warn">M 4</span> changed<br><span class="ok">+126</span> <span class="error">−44</span></div>'+
  '<div class="ins-label">Agents</div><div class="ins-row"><span class="info">●</span> runtime working<br><span class="warn">◐</span> hooks waiting</div>'+
  '<div class="ins-label">Activity</div><div class="ins-row"><span class="ok">✓</span> tests passed</div></aside>';
}
function workspace(){
 let tabs=['review','agent','git','files','terminals','spaces','fleet','activity'];
 if(versionIndex()===0)tabs=['agent','git','files','terminals'];
 else if(versionIndex()===1)tabs=['review','agent','git','files','terminals','fleet'];
 else if(versionIndex()===2)tabs=['review','agent','git','files','terminals','activity'];
 const current=tabs.includes(s.main)?s.main:tabs[0];
 const titles={review:'Review',agent:'Agent',git:'Git',files:'Files',terminals:'Build',spaces:'Spaces',fleet:'Fleet',activity:'Activity'};
 let content='';
 if(current==='review')content=reviewTiles();
 else if(current==='agent')content=agentChat(true);
 else if(current==='git')content=fullGit();
 else if(current==='files')content=fullFiles();
 else if(current==='terminals')content=terminals();
 else if(current==='spaces'||current==='fleet')content=management(current);
 else content=fullActivity();
 return '<div class="main-tabs">'+tabs.map(x=>A('set-main',titles[x],'main-tab '+(current===x?'selected':''),x,'Open '+titles[x]+' tab')).join('')+
    A('design-info','＋','main-tab','tab','Create saved layout tab (design preview)')+'</div>'+
    '<div class="scopebar"><span><span class="strong">'+e(titles[current])+'</span> <span class="muted">· aw</span> <span class="accent">◆ main</span></span>'+
    '<span class="muted">pvl-x2 · '+(current==='review'?'tiled layout':'Workspace')+'</span></div>'+
    '<div class="main-content">'+content+'</div>';
}
function main(){
 if(s.version==='v1.5')return s.mode==='minimal'?U.v15.minimal():U.v15.main();
 if(s.mode==='minimal')return '<main class="main">'+agentChat(false)+'</main>';
 if(s.mode==='agents')return '<main class="main"><div class="scopebar">'+
   '<span><span class="dot info"></span><span class="strong">coordinator</span> · working</span>'+
   '<span class="muted">aw/main · x2</span></div>'+
   '<div class="chatpane"><div class="chat-contents">'+childSummary()+'</div>'+
   '<div class="compose"><span class="accent">❯</span> Message coordinator…</div></div></main>';
 return '<main class="main">'+workspace()+'</main>';
}
function popover(){
 if(s.version==='v1.5' && (s.popover==='palette'||s.popover==='quick'))return U.v15.popover();
 if(s.popover==='spaces'){
  return '<div class="popover spaces"><div class="pop-label">Switch Space</div>'+
  [['abird','abird · Fleet','2 machines'],['x2','Local · x2','Home ~'],['a1','Local · a1','Home ~']].map(x=>
    A('set-scope','<span>'+e(x[1])+'<small>'+e(x[2])+'</small></span><span>'+(s.scope===x[0]?'✓':'')+'</span>',
      'pop-item',x[0],'Open '+x[1])).join('')+
    A('open-spaces','Manage all Spaces →','pop-item')+'</div>';
 }
 if(s.popover==='machines'){
  return '<div class="popover machines"><div class="pop-label">Switch to machine Local Space</div>'+
    [['x2','pvl-x2','connected'],['a1','pvl-a1','connected']].map(x=>
      A('set-scope','<span><span class="dot ok"></span>'+e(x[1])+'<small>Local Home · '+e(x[2])+'</small></span><span>↗</span>',
        'pop-item',x[0],'Open Local '+x[0])).join('')+
    A('open-fleet','Manage Machines / Fleet →','pop-item')+'</div>';
 }
 if(s.popover==='quick'){
   return '<div class="popover quick"><div class="pop-label">Quick switch destinations · Ctrl+P</div>'+
    U.recents.slice(0,5).map(x=>A('open-recent','<span>'+x.icon+' '+e(x.title)+'<small>'+e(x.meta)+'</small></span><span>↗</span>',
      'pop-item',x.id,'Open '+x.title)).join('')+'</div>';
 }
 return '';
}
function footer(){
 if(s.version==='v1.5')return U.v15.footer();
 return '<footer class="footer"><span>'+e(U.metadata())+
    ' / aw <span class="accent">◆ main</span> <span class="warn">M 4</span>'+
   '</span><span><span class="dot info"></span>3 running · <span class="warn">◐</span> 1 attention · sample data</span></footer>';
}
U.views={header,sidebar,main,footer,popover,workspace,agentChat,inspector};
})();