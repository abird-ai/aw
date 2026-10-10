/* Interactive offline preview controller. UI-only actions: no actual aw work. */
(function(){
'use strict';
const U=window.AwUX,s=U.state,V=U.views;
const esc=U.safe;
function opts(items,selected) {
  return items.map(it=>'<option value="'+esc(it[0])+'"'+(selected===it[0]?' selected':'')+'>'+esc(it[1])+'</option>').join('');
}
function select(id,label,options,value){
  return '<div class="control"><label for="u-'+id+'">'+label+'</label>'+
    '<select id="u-'+id+'" data-control="'+id+'">'+opts(options,value)+'</select></div>';
}
function segment(id,label,items,value){
 return '<div class="control"><label>'+label+'</label><div class="inline">'+items.map(it=>
   '<button type="button" data-action="set-'+id+'" data-value="'+esc(it[0])+'" '+
   'class="'+(it[0]===value?'on':'')+'">'+esc(it[1])+'</button>').join('')+'</div></div>';
}
function toggle(id,label,checked){
 return '<div class="control"><label>'+label+'</label><button type="button" data-action="toggle-'+id+'" '+
 'class="'+(checked?'on':'')+'">'+(checked?'✓ On':'○ Off')+'</button></div>';
}
function renderToolbar(){
 let h=select('version','Version',U.VERSIONS.map(v=>[v,U.INFO[v].title]),s.version);
 h+=segment('mode','Interface', s.version==='v1.5'?[['minimal','Minimal'],['workspace','Workspace']]:[['minimal','Minimal'],['agents','Agents'],['workspace','Workspace']],s.mode);
 h+=select('theme','Theme',[['dark','Dark'],['light','Light']],s.theme);
 if(U.feature('v1.2'))h+=select('scope','Space context',[['abird','abird · Fleet'],['x2','Local · x2'],['a1','Local · a1']],s.scope);
 if(s.mode==='workspace' && U.feature('v1')){
    h+=segment('layout','Review layout',[['focus','Focus'],['split','Split'],['grid','Grid']],s.layout);
    h+=toggle('inspector','Inspector',s.inspector);
    h+=toggle('zoom','Zoom pane',s.zoom!=='');
 }
 if(U.feature('v1.3')&&s.mode!=='minimal'){
    h+=select('nav','Navigator',[['projects','Projects'],['recents','Recents']],s.nav);
    h+=select('group','Activity group',[['recents','Recents'],['now','Now'],['automations','Automations'],['none','None']],s.group);
 }
 if(U.feature('v1.4')&&s.mode!=='minimal'){
    h+=select('dock','Mini dock',[['activity','Activity'],['git','Git'],['files','Files']],s.dock);
    h+=toggle('collapsed','Dock',!s.collapsed);
 }
 if(U.feature('v1.1')&&s.mode!=='minimal')h+=toggle('compact','Compact',s.compact);
 if(s.version==='v1.5')h+='<div class="control"><label>aw command palette</label>'+
   '<button type="button" data-action="v15-palette-open">Ctrl+Shift+P / F1</button></div>';
 h+='<div class="control note">All values and operations are illustrative. Use header and TUI tabs inside the preview as well.</div>';
 document.getElementById('toolbar').innerHTML='<section class="controlbar" aria-label="Preview controls">'+h+'</section>';
}
function render(){
 document.body.classList.toggle('light',s.theme==='light');
 document.body.classList.toggle('capture-mode',U.capture);
 renderToolbar();
 document.getElementById('lab-header').innerHTML=
   '<div class="masthead"><div><h1>aw TUI — Design history / '+esc(s.version)+'</h1>'+
     '<p>'+esc(U.INFO[s.version].detail)+'</p></div>'+
     '<span class="version-pill">'+(s.version==='v1.5'?'Current UX target':'Archived historical proposal')+'</span></div>';
 let inspector=s.mode==='workspace' && (s.version==='v0' || (U.feature('v1') && s.inspector));
 let view='<div class="ux-frame '+(s.version==='v1.5' && ['file','diff','scratch'].includes((s.v15Tabs.find(t=>t.key===s.v15Tab)||{}).kind) && !!s.v15ZoomByTab[s.v15Tab]?'v15-app-immersive':'')+
      '" data-version="'+esc(s.version)+'" role="application" aria-label="AW design-only TUI simulator">'+
    '<div class="screen">'+V.header()+
    '<div class="screenbody">'+V.sidebar()+V.main()+
    (inspector?V.inspector():'')+'</div>'+V.footer()+
    V.popover()+(s.toast?'<div class="tooltip">'+esc(s.toast)+'</div>':'')+
    '</div></div>';
 document.getElementById('demo').innerHTML=view;
 if(s.version==='v1.5'&&s.popover==='palette'){
   document.querySelector('[data-palette-input]')?.focus({preventScroll:true});
 }
 let notes='<div class="version-summary"><span>Visual-only preview · no repo state, tools, provider calls or terminal commands are used.</span>'+
    '<span><a href="'+esc(s.version==='v1.5'?'V1.5-UNIFIED-FOCUS-AND-TABS.md':s.version==='v1.4'?'V1.4-CANONICAL.md':'README.md')+'">Read design specs ↗</a></span></div>';
 document.getElementById('notes').innerHTML=notes;
}
function normalize(){
 if(!U.VERSIONS.includes(s.version))s.version='v1.5';
 if(!['minimal','agents','workspace'].includes(s.mode))s.mode='workspace';
 if(!['projects','recents'].includes(s.nav))s.nav='projects';
 if(!['activity','git','files'].includes(s.dock))s.dock='activity';
 if(!['recents','now','automations','none'].includes(s.group))s.group='recents';
 if(!['focus','split','grid'].includes(s.layout))s.layout='grid';
 if(!['dark','light'].includes(s.theme))s.theme='dark';
 if(!['abird','x2','a1'].includes(s.scope))s.scope='abird';
 if(s.version==='v0'&&s.main==='review')s.main='agent';
 if(s.version==='v1.5'){
   if(s.mode==='agents'){
     s.mode='workspace';s.v15Tab='agent';s.v15ZoomByTab.agent='agent';
   }
   if(!s.v15Tabs.some(t=>t.key===s.v15Tab))s.v15Tab='review';
 }
 if(s.mode==='minimal'&&s.version!=='v1.5')s.popover='';
}
function changeVersion(version){
 if(!U.VERSIONS.includes(version))return;
 const url=new URL(version+'.html',location.href);
 for(const [k,v] of Object.entries({
   mode:s.mode,space:s.scope,theme:s.theme,nav:s.nav,dock:s.dock,group:s.group,
   layout:s.layout,inspector:s.inspector?'1':'0',collapsed:s.collapsed?'1':'0',compact:s.compact?'1':'0',
   tab:s.version==='v1.5'?s.v15Tab:s.main
 })){url.searchParams.set(k,v);}
 if(U.capture)url.searchParams.set('capture','1');
 location.assign(url.href);
}
function setMode(mode){
 if(s.version==='v1.5'){
    if(mode==='agents'){mode='workspace';s.v15Tab='agent';s.v15ZoomByTab.agent='agent';}
    s.mode=mode;s.popover='';s.toast='';
    return;
 }
 s.mode=mode;
 if(mode==='agents')s.group='now';
 if(mode==='workspace'&&s.version==='v0')s.main='agent';
 if(mode==='workspace'&&s.version!=='v0'&&s.main==='agent')s.main='review';
 s.popover='';s.zoom='';s.toast='';
}
function action(name,value){
 s.toast='';
 // Only V1.5 owns the standalone File/Diff tab and pane zoom vocabulary.
 if(s.version==='v1.5' && name.startsWith('v15-')){
   const t=s.v15Tabs.find(item=>item.key===s.v15Tab);
   const ensureTab=(kind,file)=>{
     const key=kind+':'+s.scope+':'+file;
     const existing=s.v15Tabs.find(item=>item.key===key);
     if(!existing){
       s.v15Tabs.push({key,kind,file,label:(kind==='diff'?'± ':'▤ ')+file,
         returnTo:s.v15Tab});
     } else if(existing.key!==s.v15Tab){
       existing.returnTo=s.v15Tab;
     }
     s.v15Tab=key;s.mode='workspace';s.selectedFile=file;s.popover='';
   };
   switch(name){
    case 'v15-palette-open':
      s.popover='palette';s.paletteSelected=0;break;
    case 'v15-palette-close':
      s.popover='';break;
    case 'v15-palette-category':
      s.paletteQuery=U.v15Palette.rewrite(s.paletteQuery,value);
      s.paletteSelected=0;break;
    case 'v15-palette-run':{
      const item=U.v15Palette.results(s.paletteQuery).find(x=>x.id===value);
      if(!item)break;
      s.popover='';
      action(item.action,item.value||'');
      return;
    }
    case 'v15-palette-command':
      s.toast=value+' is an existing aw command; design preview did not execute it.';
      break;
    case 'v15-palette-max':{
      const target=document.querySelector('.v15-content [data-pane="'+s.focusedPane+'"]') ||
        document.querySelector('.v15-content [data-pane]');
      if(target){
        s.v15ZoomByTab[s.v15Tab]=target.dataset.pane;
        s.focusedPane=target.dataset.pane;
      } else {
        s.toast='No visible pane available to maximize.';
      }
      break;
    }
    case 'v15-palette-pty':
      s.v15Tab='terminals';
      s.focusedPane='pty:'+value;s.mode='workspace';break;
    case 'v15-select-tab':
      if(s.v15Tabs.some(item=>item.key===value)){s.v15Tab=value;s.popover='';}
      break;
    case 'v15-open-file-tab':ensureTab('file',value);break;
    case 'v15-open-diff-tab':ensureTab('diff',value);break;
    case 'v15-preview-file':s.v15FileWithinFiles=value;break;
    case 'v15-preview-diff':s.v15DiffWithinGit=value;break;
    case 'v15-zoom':
      s.v15ZoomByTab[s.v15Tab]=s.v15ZoomByTab[s.v15Tab]===value?'':value;
      s.focusedPane=value;
      break;
    case 'v15-focus-pane':s.focusedPane=value;break;
    case 'v15-show-agent':
      s.v15SelectedAgent=['coordinator','runtime','hooks'].includes(value)?value:'coordinator';
      s.v15Tab='agent';s.mode='workspace';s.group='now';
      s.v15ZoomByTab.agent='agent';s.popover='';
      break;
    case 'v15-close-tab':{
      const index=s.v15Tabs.findIndex(item=>item.key===value);
      if(index>=0 && s.v15Tabs[index].kind!=='layout'){
        const origin=s.v15Tabs[index].returnTo;
        s.v15Tabs.splice(index,1);
        delete s.v15ZoomByTab[value];
        if(s.v15Tab===value){
          s.v15Tab=(origin&&s.v15Tabs.some(t=>t.key===origin))?origin:
            (s.v15Tabs[Math.max(0,index-1)]?.key||'review');
        }
      }
      break;
    }
    case 'v15-new-tab':{
      const index=s.v15Tabs.filter(item=>item.kind==='scratch').length+1;
      const key='scratch:'+index;
      s.v15Tabs.push({key,kind:'scratch',label:'New '+index});
      s.v15Tab=key;
      break;
    }
    default:return;
   }
   normalize();render();return;
 }

 switch(name){
   case 'noop':return;
   case 'set-version':changeVersion(value);return;
   case 'set-mode':setMode(value);break;
   case 'set-nav':s.nav=value;break;
   case 'set-dock':s.dock=value;s.collapsed=false;break;
   case 'set-group':s.group=s.group===value?'none':value;break;
   case 'set-main':
     if(s.version==='v1.5'){
       if(!s.v15Tabs.some(t=>t.key===value))s.v15Tabs.push({
         key:value,kind:'layout',label:value[0].toUpperCase()+value.slice(1)
       });
       s.v15Tab=value;
     }
     s.main=value;s.popover='';s.zoom='';break;
   case 'set-layout':s.layout=value;s.zoom='';break;
   case 'set-scope':
     s.scope=value;s.main=s.version==='v0'?'agent':'review';
     if(s.version==='v1.5')s.v15Tab='review';
     s.popover='';s.zoom='';break;
   case 'open-spaces':
     if(s.version==='v1.5'){
       if(!s.v15Tabs.some(t=>t.key==='spaces'))s.v15Tabs.push({key:'spaces',kind:'layout',label:'Spaces'});
       s.v15Tab='spaces';
     }
     s.main='spaces';s.mode='workspace';s.popover='';break;
   case 'open-fleet':
     if(s.version==='v1.5'){
       if(!s.v15Tabs.some(t=>t.key==='fleet'))s.v15Tabs.push({key:'fleet',kind:'layout',label:'Fleet'});
       s.v15Tab='fleet';
     }
     s.main='fleet';s.mode='workspace';s.popover='';break;
   case 'toggle-space-menu':s.popover=s.popover==='spaces'?'':'spaces';break;
   case 'toggle-machine-menu':s.popover=s.popover==='machines'?'':'machines';break;
   case 'show-quick':
     if(s.version==='v1.5'){
       s.popover=s.popover==='palette'?'':'palette';
       s.paletteSelected=0;
     } else s.popover=s.popover==='quick'?'':'quick';
     break;
   case 'toggle-dock':s.collapsed=!s.collapsed;break;
   case 'toggle-inspector':s.inspector=!s.inspector;break;
   case 'toggle-zoom':s.zoom=s.zoom?'':s.focusedPane;break;
   case 'toggle-compact':s.compact=!s.compact;break;
   case 'focus-pane':s.focusedPane=value;break;
   case 'open-agent':
     if(s.version==='v1.5'){action('v15-show-agent','');return;}
     s.mode='agents';s.group='now';break;
   case 'open-git-file':
     if(s.version==='v1.5'){action('v15-open-diff-tab',value);return;}
     s.selectedFile=value;
     if(s.main!=='review')s.main='git';
     break;
   case 'open-file':
     if(s.version==='v1.5'){action('v15-open-file-tab',value);return;}
     s.selectedFile=value;s.main='files';s.mode='workspace';break;
   case 'open-recent':
     s.selectedRecent=value;
     if(s.version==='v1.5'){
        const item=U.recents.find(i=>i.id===value);
        if(item?.kind==='agent'){action('v15-show-agent','');return;}
        if(item?.kind==='file'){action('v15-open-file-tab',item.title);return;}
        if(item?.kind==='terminal'){s.v15Tab='terminals';s.mode='workspace';s.popover='';render();return;}
        if(item?.kind==='workspace'){
          s.v15Tab=item.id==='build'?'terminals':'review';
          s.mode='workspace';s.popover='';render();return;
        }
     }
     const item=U.recents.find(r=>r.id===value);
     if(item){
       if(item.kind==='agent')s.mode='agents';
       else if(item.kind==='file'){s.main='files';s.mode='workspace';s.selectedFile=item.title;}
       else if(item.kind==='terminal'){s.main='terminals';s.mode='workspace';}
       else if(item.kind==='workspace'){s.main=item.id==='build'?'terminals':'review';s.mode='workspace';}
       else if(item.kind==='project'){s.nav='projects';s.mode='workspace';}
     }
     s.popover='';break;
   case 'select-project':
     s.nav='projects';s.mode='workspace';s.main='review';
     if(s.version==='v1.5')s.v15Tab='review';
     break;
   case 'design-info':s.toast=(value==='machine'?'Machine pairing':value==='space'?'Space creation':'Layout creation')+
       ' would open an authenticated workflow; preview does not execute.';break;
   default:return;
 }
 normalize();render();
}
function handleClick(ev){
 const button=ev.target.closest('[data-action]');
 if(button){
   ev.preventDefault();
   action(button.dataset.action,button.dataset.value||'');return;
 }
}
function handleChange(ev){
 const elt=ev.target.closest('[data-control]');
 if(!elt)return;
 const key=elt.dataset.control,val=elt.value;
 if(key==='version'){changeVersion(val);return;}
 if(key==='mode')setMode(val);
 else if(key==='scope')action('set-scope',val);
 else if(key==='theme'||key==='nav'||key==='dock'||key==='group'||key==='layout'){
   s[key]=val;
   if(key==='nav'&&val==='recents'&&s.group==='recents'){} /* suppressed, not deleted */
   if(key==='dock')s.collapsed=false;
   if(key==='layout')s.zoom='';
 }
 normalize();render();
}
function handleInput(e){
 if(s.version!=='v1.5'||!e.target.matches?.('[data-palette-input]'))return;
 const index=e.target.selectionStart;
 s.paletteQuery=e.target.value;
 s.paletteSelected=0;
 render();
 const input=document.querySelector('[data-palette-input]');
 if(input){
   input.focus({preventScroll:true});
   if(index!=null)input.setSelectionRange(index,index);
 }
}
function handleKeys(e){
 // Mirror aw's real command palette: Ctrl+Shift+P or F1.
 // Ctrl+P belongs to editor/composer history or a focused PTY; never grab it
 // as an unconditional global quick-switch chord in V1.5.
 if(s.version==='v1.5'){
   const chord=e.key==='F1'||
     (e.ctrlKey&&e.shiftKey&&e.key.toLowerCase()==='p');
   if(chord){
     e.preventDefault();
     action(s.popover==='palette'?'v15-palette-close':'v15-palette-open','');
     return;
   }
   if(s.popover==='palette'){
     if(e.key==='Escape'){
       e.preventDefault();action('v15-palette-close','');return;
     }
     const matches=U.v15Palette.results(s.paletteQuery);
     if(e.key==='ArrowDown'||e.key==='ArrowUp'){
       e.preventDefault();
       if(matches.length){
         const delta=e.key==='ArrowDown'?1:-1;
         s.paletteSelected=(s.paletteSelected+delta+matches.length)%matches.length;
         render();
       }
       return;
     }
     if(e.key==='Enter'){
       e.preventDefault();
       if(matches.length){
         const index=Math.min(s.paletteSelected,matches.length-1);
         action('v15-palette-run',matches[index].id);
       }
       return;
     }
     // All other keys are normal editable palette input, including Ctrl+P
     // which is NOT substituted for Ctrl+Shift+P.
   }
 }
 const tag=e.target&&e.target.tagName;
 if(tag==='INPUT'||tag==='TEXTAREA'||tag==='SELECT')return;
 if(e.key==='Escape'&&s.popover){s.popover='';render();return;}
 if(s.version!=='v1.5'&&(e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='p'){
   e.preventDefault();action('show-quick','');return;
 }
 if(e.key==='F2'){
   e.preventDefault();
   const modes=s.version==='v1.5'?['minimal','workspace']:U.modes;
   setMode(modes[(modes.indexOf(s.mode)+1)%modes.length]);render();return;
 }
 if(e.target&&e.target.matches&&e.target.matches('[data-resize]')&&['ArrowUp','ArrowDown'].includes(e.key)){
   e.preventDefault();
   s.dockHeight=Math.min(375,Math.max(83,s.dockHeight+(e.key==='ArrowUp'?-15:15)));
   render();
 }
}
let dragging=null;
function startDrag(e){
 if(!e.target.closest('[data-resize]'))return;
 dragging={startY:e.clientY,height:s.dockHeight};
 e.preventDefault();
}
function moveDrag(e){
 if(!dragging)return;
 const height=Math.max(83,Math.min(375,dragging.height+dragging.startY-e.clientY));
 const dock=document.querySelector('.side-lower');
 if(dock){dock.style.height=height+'px';dock.style.flexBasis=height+'px';}
 s.dockHeight=height;
}
function finishDrag(){
 if(!dragging)return;
 dragging=null;render();
}
function paneDoubleClick(e){
 const pane=e.target.closest('[data-pane]');
 if(!pane)return;
 const id=pane.dataset.pane;
 if(s.version==='v1.5'){
   // Double-click text must select text, not zoom. Only the pane header
   // (or its explicit small maximize control) may change pane layout.
   if(!e.target.closest('.pane-head'))return;
   action('v15-zoom',id);return;
 }
 if(['agent','diff','terminal'].includes(id)){
   s.focusedPane=id;
   s.zoom=s.zoom===id?'':id;
   render();
 }
}
function attach(){
 document.addEventListener('click',handleClick);
 document.addEventListener('change',handleChange);
 document.addEventListener('input',handleInput);
 document.addEventListener('keydown',handleKeys);
 document.addEventListener('pointerdown',startDrag);
 document.addEventListener('pointermove',moveDrag);
 document.addEventListener('pointerup',finishDrag);
 document.addEventListener('dblclick',paneDoubleClick);
}
normalize();attach();render();
window.AwUX.render=render;
})();