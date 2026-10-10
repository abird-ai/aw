/* aw UX mock data + stable UI state: no runtime, network or filesystem actions. */
(function () {
  'use strict';
  const VERSIONS = ['v0','v1','v1.1','v1.2','v1.3','v1.4','v1.5'];
  const INFO = {
    'v0': {title:'V0 · Original', detail:'Pi-like minimal Agent, hierarchical Agents and Workspace with inspector', short:'Frozen original'},
    'v1': {title:'V1 · Tiled workspace', detail:'Real PTY-style panes, saved layouts, zoom and optional inspector', short:'Herdr-inspired panes'},
    'v1.1': {title:'V1.1 · Quiet activity', detail:'Compact Recents, attention and autonomous/background work', short:'Recents and activity'},
    'v1.2': {title:'V1.2 · Context header', detail:'Space and machine switchers above a complete Workspace', short:'Local and fleet'},
    'v1.3': {title:'V1.3 · Integrated navigation', detail:'Projects/Recents textual tabs and one Activity tree', short:'Full TUI + activity'},
    'v1.4': {title:'V1.4 · Dock foundation', detail:'Tiny upper navigator icons plus independent resizable Activity/Git/Files dock', short:'Preserved baseline'},
    'v1.5': {title:'V1.5 · Unified focus', detail:'Headerless Minimal and one Workspace with maximizable panes, standalone file/diff tabs and no separate Agents mode',short:'Canonical target'}
  };
  const recents = [
    {id:'review', icon:'▤', title:'Review',meta:'Workspace tab · aw/main · x2',kind:'workspace'},
    {id:'coordinator', icon:'◉', title:'coordinator',meta:'Agent · aw/main · x2',kind:'agent'},
    {id:'dotlink',icon:'◆',title:'dotlink',meta:'Project · main · x2',kind:'project'},
    {id:'tests',icon:'▣',title:'tests',meta:'Terminal · aw/main · a1',kind:'terminal'},
    {id:'tool.rs',icon:'▤',title:'tool.rs',meta:'File · aw/main · x2',kind:'file'},
    {id:'health',icon:'◉',title:'seed reviewer',meta:'Agent · abird-health · a1',kind:'agent'},
    {id:'build',icon:'▤',title:'Build',meta:'Workspace tab · x2',kind:'workspace'}
  ];
  const files = [
    {path:'tool.rs',type:'M',adds:52,dels:9},
    {path:'hooks.rs',type:'M',adds:47,dels:4},
    {path:'message.rs',type:'M',adds:27,dels:0},
    {path:'legacy.rs',type:'D',adds:0,dels:31}
  ];
  const modes=['minimal','agents','workspace']; // Historical V0–V1.4. V1.5 offers Minimal and Workspace only.
  const spaceMap = {abird:'abird',x2:'Local · x2',a1:'Local · a1'};
  const query = new URLSearchParams(location.search);
  const raw = document.documentElement.getAttribute('data-start') || 'v1.5';
  const version=VERSIONS.includes(query.get('version'))?query.get('version'):(VERSIONS.includes(raw)?raw:'v1.5');
  const inSet=(value,valid,fallback)=>valid.includes(value)?value:fallback;
  const capture=query.get('capture')==='1';
  const initial = {
    version:version,
    mode:inSet(query.get('mode'),modes,version==='v0'?'minimal':'workspace'),
    nav:inSet(query.get('nav'),['projects','recents'],'projects'),
    dock:inSet(query.get('dock'),['activity','git','files'],'activity'),
    group:inSet(query.get('group'),['recents','now','automations','none'],'recents'),
    main:inSet(query.get('tab')||query.get('main'),['review','agent','git','files','terminals','spaces','fleet','activity'],'review'),
    scope:inSet(query.get('space'),['abird','x2','a1'],'abird'),
    layout:inSet(query.get('layout'),['focus','split','grid'],'grid'),
    theme:inSet(query.get('theme'),['dark','light'],'dark'),
    inspector:query.get('inspector')==='1',
    compact:query.get('compact')==='1',
    collapsed:query.get('collapsed')==='1',
    zoom:'', popover:query.get('popover')||'',
    paletteQuery:query.get('query')||'',
    paletteSelected:0,
    selectedFile:'tool.rs', focusedPane:'agent', selectedRecent:'review',
    v15Tab:query.get('v15Tab')||query.get('tab')||'review',
    v15Tabs:[
      {key:'review',kind:'layout',label:'Review'},
      {key:'agent',kind:'layout',label:'Agent'},
      {key:'files',kind:'layout',label:'Files'},
      {key:'git',kind:'layout',label:'Git'},
      {key:'terminals',kind:'layout',label:'Build'}
    ],
    v15ZoomByTab:{},
    v15FileWithinFiles:'tool.rs',
    v15DiffWithinGit:'tool.rs',
    v15ActivityEmphasis:'now',
    v15SelectedAgent:'coordinator',
    v15FocusByTab:{},
    dockHeight:220, toast:''
  };
  // Direct-linkable V1.5 tabs for fixed screenshot proofs and shareable
  // visual states; a production version would key them by full FileRef/DiffRef
  // including site, worktree and revision, not just mock filenames.
  if(version==='v1.5'){
    const id=initial.v15Tab;
    if(!initial.v15Tabs.some(t=>t.key===id)){
      const parts=id.split(':');
      if((parts[0]==='file'||parts[0]==='diff') && parts.length===3 &&
         parts[2].length>0 && parts[2].length<128){
        initial.v15Tabs.push({key:id,kind:parts[0],file:parts[2],
          label:(parts[0]==='diff'?'± ':'▤ ')+parts[2]});
        initial.selectedFile=parts[2];
      } else {initial.v15Tab='review';}
    }
    const zoom=query.get('zoom')||'';
    if(['agent','diff','terminal','editor','tree','gitlist','gitdiff',
        'file','standaloneDiff','pty:build','pty:tests','pty:debug','pty:shell'].includes(zoom)){
      initial.v15ZoomByTab[initial.v15Tab]=zoom;
    }
  }
  const U=window.AwUX={
    VERSIONS,INFO,recents,files,modes,spaceMap,capture,
    state:initial,
    feature(v) {return VERSIONS.indexOf(this.state.version)>=VERSIONS.indexOf(v);},
    safe(s) {return String(s).replace(/[&<>"']/g,ch=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[ch]));},
    attr(s) {return this.safe(s);},
    action(key,label,cls, val, title) {
      return '<button type="button" class="'+(cls||'')+'" data-action="'+this.attr(key)+'"'+
        (val==null?'':' data-value="'+this.attr(val)+'"')+
        (title?' title="'+this.attr(title)+'" aria-label="'+this.attr(title)+'"':'')+'>'+label+'</button>';
    },
    row(label,depth,cls,key,value) {
      return this.action(key||'noop',label,'tree-row '+(depth?'i'+depth:'')+' '+(cls||''),value);
    },
    recentRows(max) {
      return this.recents.slice(0,max||this.recents.length).map(item=>
        this.action('open-recent','<span class="kind">'+item.icon+'</span><span class="rc"><span class="rn">'+
        this.safe(item.title)+'</span><span class="rm">'+this.safe(item.meta)+'</span></span>',
        'recent '+(this.state.selectedRecent===item.id?'selected':''),item.id,
        'Open recent '+item.title)
      ).join('');
    },
    badge(type) {
      return type==='M'?'<span class="warn">M</span>':type==='D'?'<span class="error">D</span>':
        type==='A'?'<span class="ok">A</span>':'<span class="info">R</span>';
    },
    metadata() {return this.spaceMap[this.state.scope]||'abird';}
  };
})();