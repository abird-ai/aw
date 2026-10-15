import {initialLayout,reduceLayout,groupRects,sessionTarget,sessionMetrics} from '/workspace-model.js';
import {createWorkspaceSidebars} from '/workspace-sidebars.js';
const $=(tag,cls,text)=>{const e=document.createElement(tag);if(cls)e.className=cls;if(text!==undefined)e.textContent=text;return e;};
const button=(label,fn,cls='desk-button')=>{const b=$('button',cls,label);b.type='button';b.addEventListener('click',fn);return b;};
const bytes=s=>new TextEncoder().encode(s).length;
export function mountWorkspace(bridge){
  if(!document.getElementById('aw-workspace-style')){const style=document.createElement('link');style.id='aw-workspace-style';style.rel='stylesheet';style.href='/workspace.css';document.head.append(style);}
  const caps=bridge.config.capabilities;let layout=initialLayout(),alive=true,shown=true,maximize=false,raf=0;
  let gitContext='root',focusedView='v1';
  const surfaces=new Map(),controllers=new Map(),clientId=crypto.randomUUID();
  const bookmark=document.createComment('aw-chat-home');bridge.chat.before(bookmark);
  const shell=$('section','desktop-shell');shell.id='desktop-shell';shell.setAttribute('aria-label','Browser workspace');
  const toolbar=$('div','desk-toolbar');const title=$('strong','','aw');const scope=$('span','desk-scope',caps.root.label);
  const content=$('div','desk-content'),left=$('aside','desk-side desk-left'),stage=$('div','desk-stage'),right=$('aside','desk-side desk-right'),chrome=$('div','desk-chrome'),layer=$('div','desk-layer');
  const leftHandle=$('div','desk-outer-resizer'),rightHandle=$('div','desk-outer-resizer');
  leftHandle.setAttribute('role','separator');leftHandle.setAttribute('aria-label','Resize left region');leftHandle.setAttribute('aria-orientation','vertical');
  rightHandle.setAttribute('role','separator');rightHandle.setAttribute('aria-label','Resize right region');rightHandle.setAttribute('aria-orientation','vertical');
  const toggleSide=(panel,handle,compactClass)=>{
    const compact=window.matchMedia(compactClass).matches;
    if(compact){panel.hidden=false;panel.classList.toggle('forced-visible');handle.hidden=true;}
    else{panel.classList.remove('forced-visible');const hidden=!panel.hidden;panel.hidden=hidden;handle.hidden=hidden;}
    schedulePaint();
  };
  const leftToggle=button('Left',()=>toggleSide(left,leftHandle,'(max-width: 720px)'));
  const rightToggle=button('Right',()=>toggleSide(right,rightHandle,'(max-width: 980px)'));
  toolbar.append(title,scope,leftToggle,button('Groups',()=>showMenu()),button('Maximize',()=>{maximize=!maximize;paint();}),rightToggle);
  if(bridge.settings)toolbar.append(button('Settings',bridge.settings));
  toolbar.append(button('Agent only',()=>toggle()));
  if(!bridge.localSingleUser)toolbar.append(button('Lock',bridge.lock));
  stage.append(chrome,layer);content.append(left,leftHandle,stage,rightHandle,right);
  const notice=$('div','desk-notice');notice.setAttribute('role','status');notice.textContent='Local views only · center tabs are independent · model context changes only through explicit conversation actions.';
  shell.append(toolbar,content,notice);document.querySelector('main').append(shell);document.body.classList.add('desktop-mode');
  function resizeRegion(handle,panel,edge){
    handle.tabIndex=0;
    handle.addEventListener('pointerdown',event=>{event.preventDefault();const start=event.clientX,startWidth=panel.getBoundingClientRect().width;
      const move=moveEvent=>{const delta=(moveEvent.clientX-start)*(edge==='left'?1:-1);panel.style.width=Math.max(180,Math.min(520,startWidth+delta))+'px';schedulePaint();};
      const done=()=>{document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',done);};
      document.addEventListener('pointermove',move);document.addEventListener('pointerup',done);
    });
  }
  resizeRegion(leftHandle,left,'left');resizeRegion(rightHandle,right,'right');
  const chatPanel=$('section','desk-surface chat-surface');chatPanel.setAttribute('role','tabpanel');chatPanel.id='surface-chat';chatPanel.append(bridge.chat);layer.append(chatPanel);
  const chatSurface={node:chatPanel,kind:'conversation',dispose:()=>{chatPanel.hidden=true;},visible:false};surfaces.set('v1',chatSurface);
  const dialog=$('dialog','desk-dialog');dialog.setAttribute('aria-label','Workspace actions');document.body.append(dialog);
  function notify(text){if(alive)notice.textContent=text;}
  async function request(path,body){if(!alive)throw Error('Workspace closed');try{const result=await bridge.request(path,body);if(!alive)throw Error('Workspace closed');return result;}catch(e){if(alive){notify(e.message);if(e.status===401)dispose();}throw e;}}
  const command=body=>request('/api/workspace/action',body);
  function act(action){try{layout=reduceLayout(layout,action);paint();}catch(e){notify(e.message);}}
  function open(target,group){act({type:'open',target,group});}
  function activeGroup(){return layout.groups[layout.focused];}
  function activeView(){return layout.views[activeGroup().active];}
  const sidebars=createWorkspaceSidebars({
    left,right,request,command,open,
    openSession:bridge.openSession,
    getSnapshot:bridge.getSnapshot,
    notify,
    rootLabel:caps.root.label,
    onGitContext:value=>{gitContext=value;},
  });
  function addOption(title,fn){dialog.append(button(title,()=>{dialog.close();fn();},'desk-choice'));}
  function showMenu(group=layout.focused){
    act({type:'focus',group});dialog.replaceChildren($('h2','','Open a view or arrange a group'));
    for(const gid of Object.keys(layout.groups))addOption('Focus group '+gid,()=>act({type:'focus',group:gid}));
    addOption('Conversation',()=>open({kind:'conversation',title:'Conversation'}));
    addOption('Session tree',()=>open({kind:'tree',title:'Session tree'}));
    addOption('Activity log',()=>open({kind:'logs',title:'Activity log'}));
    addOption('Session metrics',()=>open({kind:'metrics',title:'Session metrics'}));
    addOption('Help and limits',()=>open({kind:'help',title:'Help'}));
    addOption(caps.terminals?'New terminal — trusted OS shell':'Terminals disabled by owner',()=>{if(caps.terminals)startTerminal();else notify('Restart with --web-workspace --allow-web-terminals only after reviewing shell authority.');});
    addOption('Existing terminals',()=>listTerminals());
    for(const site of caps.sites||[])addOption('Website: '+site.label,()=>open({kind:'site',title:site.label,resource:site.id}));
    addOption('Split group right',()=>act({type:'split',axis:'columns'}));addOption('Split group below',()=>act({type:'split',axis:'rows'}));
    for(const g of Object.keys(layout.groups)){if(g!==group)addOption('Move active tab to '+g,()=>act({type:'move',group:g,view:activeGroup().active}));}
    addOption('Close active view',()=>closeView(group,activeGroup().active));addOption('Merge this group into another',()=>act({type:'close_group'}));
    dialog.append(button('Cancel',()=>dialog.close()));dialog.showModal();
  }
  function closeView(group,id){const target=layout.views[id];if(!target)return;
    if(target.kind==='site'&&!confirm('Close this embedded page? Unknown form state may be lost.'))return;
    if(target.kind==='conversation'&&bridge.chat.querySelector('#prompt')?.value&&!confirm('Hide this conversation? The unsent draft remains in this browser only.'))return;
    const s=surfaces.get(id);if(s&&target.kind!=='conversation'){s.dispose?.();s.node.remove();surfaces.delete(id);}else if(s){s.node.hidden=true;surfaces.delete(id);}
    act({type:'close',group,view:id});notify(target.kind==='terminal'?'Terminal view hidden; its process is still running. Reopen through Existing terminals.':'View closed; no agent or pipeline was stopped.');
  }
  function viewShell(view){const node=$('section','desk-surface');node.setAttribute('role','tabpanel');const bar=$('div','surface-toolbar'),body=$('div','surface-body'),status=$('div','surface-status');bar.append($('strong','',view.title));node.append(bar,body,status);layer.append(node);return {node,bar,body,status,kind:view.kind,visible:false,disposed:false,dispose(){this.disposed=true;}};}
  function attachFeedback(text,target){if(target!==sessionTarget(bridge.getSnapshot())){notify('Conversation target changed while review was loading. Inspect and attach again.');return;}if(bytes(text+(bridge.chat.querySelector('#prompt')?.value||''))>128*1024){notify('Combined draft would exceed its byte limit.');return;}bridge.feedback(text);open({kind:'conversation',title:'Conversation'});notify('Feedback added to the unsent composer. No model call was started.');}
  function makeSurface(view,id){
    if(view.kind==='conversation'){chatPanel.hidden=false;surfaces.set(id,chatSurface);return chatSurface;}
    const s=viewShell(view);s.node.id='surface-'+id;
    const fail=e=>{if(alive&&!s.disposed)s.status.textContent=e.message;};
    if(view.kind==='file'){
      s.status.textContent='Read-only • owner root • not model context';command({op:'file',path:view.resource}).then(data=>{if(s.disposed)return;const pre=$('pre','file-text',data.text);s.body.replaceChildren(pre);}).catch(fail);
    }else if(view.kind==='diff'){
      s.status.textContent='Loading exact review snapshot…';command({op:'review',review_id:view.resource}).then(data=>{
        if(s.disposed)return;s.status.textContent=`${data.basis} • ${data.root} • read only${data.truncated?' • display truncated':''}`;
        const lines=$('div','diff-lines');for(const [index,line] of data.lines.entries()){const row=$('button','diff-row '+line.kind);row.type='button';row.title='Add comment to row '+(index+1);row.append($('span','line-number',line.old??''),$('span','line-number',line.new??''),$('code','',line.text));row.onclick=async()=>{
          if(!data.same_agent_root){notify('This review belongs to another checkout; the current agent has not moved.');return;}
          const text=prompt('Comment on this exact diff row (feedback is not sent automatically):');if(!text?.trim())return;const target=sessionTarget(bridge.getSnapshot());
          try{const r=await command({op:'review_feedback',review_id:data.id,row:index,text});if(!s.disposed)attachFeedback(r.feedback,target);}catch(e){fail(e);}
        };lines.append(row);}s.body.replaceChildren(lines);
      }).catch(fail);
    }else if(view.kind==='tree'){
      s.bar.append(button('Refresh',()=>loadTree()));async function loadTree(){try{const data=await request('/api/tree');if(s.disposed)return;const list=$('div','tree-records');const depths=new Map();for(const n of data.nodes||[]){const d=n.parent_id?(depths.get(n.parent_id)||0)+1:0;depths.set(n.id,d);const row=$('div','tree-record');row.style.paddingLeft=(10+Math.min(d,10)*12)+'px';row.textContent=`${n.kind} · ${n.actor} · ${n.id.slice(-8)}`;list.append(row);}s.body.replaceChildren(list);s.status.textContent='Immutable conversation nodes, not Git branches. Use the conversation controls to fork.';}catch(e){fail(e);}}loadTree();
    }else if(view.kind==='logs'){
      let paused=false,filter='';const search=$('input');search.placeholder='Filter observed activity';search.setAttribute('aria-label','Filter activity');search.oninput=()=>{filter=search.value;render();};
      const follow=button('Pause follow',()=>{paused=!paused;follow.textContent=paused?'Resume follow':'Pause follow';if(!paused)render();});s.bar.append(search,follow);
      function render(){if(paused||s.disposed)return;const snap=bridge.getSnapshot(),rows=[];for(const turn of (snap?.turns||[]).slice(-32)){rows.push(`Turn ${turn.id} · ${turn.outcome} · ${turn.profile||''}`);for(const r of turn.tool_records||[])rows.push(`  ${r.call?.name} · ${r.status} · ${r.execution_site}`);}if(snap?.pending){rows.push(`Run ${snap.pending.run_id} · in progress`);for(const r of snap.pending.tool_records||[])rows.push(`  ${r.call?.name} · ${r.status}`);}const selected=rows.filter(r=>r.toLowerCase().includes(filter.toLowerCase())).slice(-256);s.body.replaceChildren($('pre','log-text',selected.join('\n')||'No observed activity in this view.'));s.status.textContent='Current owner projection • bounded recent records • not a complete durable event log';if(!paused)s.body.scrollTop=s.body.scrollHeight;}
      s.update=render;render();
    }else if(view.kind==='metrics'){
      function render(){if(s.disposed)return;const m=sessionMetrics(bridge.getSnapshot());const tiles=$('div','metric-grid');for(const [label,key] of [['Completed turns','completed'],['Failed turns','failed'],['Cancelled turns','cancelled'],['Running now','active'],['Observed invocations','observed_invocations']]){const tile=$('div','metric-card');tile.append($('strong','',m[key]),$('span','',label));tiles.append(tile);}s.body.replaceChildren(tiles,$('p','',m.scope),$('p','fineprint','No infrastructure datasource, cost estimate, latency chart or task percentage is fabricated.'));s.status.textContent='Derived from the same authorized session snapshot; no extra model request or telemetry fetch.';}
      s.update=render;render();
    }else if(view.kind==='site'){
      const site=(caps.sites||[]).find(x=>x.id===view.resource);if(!site){s.body.textContent='Site descriptor unavailable.';return s;}
      let frame=null;const address=$('span','site-address',site.url),outside=$('a','desk-button','Open in browser');outside.href=site.url;outside.target='_blank';outside.rel='noopener noreferrer';outside.referrerPolicy='no-referrer';
      function load(){if(frame){frame.src=site.url;return;}frame=$('iframe','site-frame');frame.title=site.label;frame.setAttribute('sandbox','allow-scripts');frame.referrerPolicy='no-referrer';frame.setAttribute('allow',"camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'; clipboard-write 'none'");frame.src=site.url;s.body.replaceChildren(frame);s.status.textContent='Opened address only. Page state unverified; some sites refuse embedding. No aw bridge or credential sharing.';}
      s.bar.append(outside,button('Reload opened address',()=>{if(confirm('Reload this website? Unsaved page state may be lost.'))load();}),button('Suspend / unload',()=>{if(frame&&confirm('Unload this page? Its unsaved state may be lost.')){frame.remove();frame=null;s.body.replaceChildren(button('Load page again',load));}}));s.bar.append(address);s.dispose=()=>{s.disposed=true;frame?.remove();frame=null;};load();
    }else if(view.kind==='terminal'){
      terminalSurface(s,view.resource);
    }else {
      s.body.append($('h2','','One group, any supported tab'),$('p','','Use + to open conversations, terminal views, files, reviews, activity, session metrics and registered websites. Split right/below, then move a tab with its menu or drag it to another strip. Your current agent/root/account do not move.'),$('p','','Ctrl+Shift+P or F1 opens workspace actions; Ctrl+Shift+E/G focuses Explorer/Source Control; Ctrl+Tab cycles center tabs. A terminal owns ordinary keys. External frames may capture their own shortcuts; use the surrounding toolbar or browser to leave them.'),$('p','','This slice has one current conversation owner, one switchable navigator, four content groups and 24 views. Website frames have limited embedding/login support. Shells are unsandboxed. Native attach, multi-user identities, local-device inference and multiplayer remain separate integrations.'));
    }
    return s;
  }
  function ansi(value){if(!value)return '';if(/^#[0-9a-f]{6}$/i.test(value))return value;const m=/^ansi-(\d{1,3})$/.exec(value);if(!m)return '';const n=Number(m[1]);if(n>255)return '';const palette=['#10141a','#ba3b46','#469a6a','#b7963d','#4c79b8','#9464b5','#338d99','#d2d5db','#687583','#ee606c','#73cc93','#edd077','#80a9df','#c097e2','#69c5cd','#ffffff'];if(n<16)return palette[n];if(n>=232){const v=8+(n-232)*10;return `rgb(${v},${v},${v})`;}const c=n-16,scale=[0,95,135,175,215,255];return `rgb(${scale[Math.floor(c/36)]},${scale[Math.floor(c/6)%6]},${scale[c%6]})`;}
  function controller(id){if(controllers.has(id))return controllers.get(id);const c={id,lease:null,next:1,queue:[],pending:null,uncertain:false,busy:false,snapshot:null,reading:false,lastRead:0,scrollback:0,error:'',views:new Set()};controllers.set(id,c);return c;}
  async function claim(c){if(c.uncertain){notify('Resolve the exact pending input before changing control. Inspect the terminal; do not replay blindly.');return;}try{const r=await command({op:'terminal_claim',terminal_id:c.id,client_id:clientId});c.lease=r.lease;c.next=r.next_sequence;c.error='';c.views.forEach(s=>s.update?.());}catch(e){c.error=e.message;notify(e.message);}}
  async function flush(c){if(!alive||c.busy||c.uncertain||!c.lease||(!c.pending&&!c.queue.length))return;
    if(!c.pending)c.pending={op:'terminal_input',terminal_id:c.id,client_id:clientId,lease:c.lease,sequence:c.next,input:c.queue.shift()};
    c.busy=true;try{const r=await command(c.pending);if(!alive)return;c.next=r.accepted_sequence+1;c.pending=null;if(c.error)notify('Exact terminal input acknowledged as queued; this is not a process-completion receipt.');c.error='';}catch(e){c.uncertain=true;c.error='Input unconfirmed. Retry only the exact pending packet, or inspect and discard unsent input.';notify(c.error);}finally{c.busy=false;if(alive){c.views.forEach(s=>s.update?.());if(!c.uncertain)flush(c);}}
  }
  function enqueue(c,input){if(!c.lease){notify('Claim terminal control before typing.');return;}if(bytes(JSON.stringify(c.queue))+bytes(JSON.stringify(input))>65536){notify('Terminal input queue full; input was not accepted.');return;}const last=c.queue.at(-1);if(last?.kind==='text'&&input.kind==='text'&&bytes(last.text+input.text)<=4096)last.text+=input.text;else c.queue.push(input);flush(c);}
  function terminalSurface(s,id){const c=controller(id);c.views.add(s);const screen=$('pre','terminal-screen');screen.setAttribute('role','log');screen.setAttribute('aria-live','off');screen.setAttribute('aria-label','Terminal screen');const input=$('textarea','terminal-input');input.rows=1;input.placeholder='Claim control to type into this owner-held shell';input.setAttribute('aria-label','Terminal keyboard input');input.autocomplete='off';input.spellcheck=false;
    const claimButton=button('Claim control',()=>claim(c));const release=button('Release control',async()=>{if(!c.lease)return;if((c.pending||c.queue.length)&&!confirm('Discard locally pending input? Already queued effects cannot be undone.'))return;try{await command({op:'terminal_release',terminal_id:id,client_id:clientId,lease:c.lease});}catch(e){notify(e.message);}c.lease=null;c.queue=[];c.pending=null;c.uncertain=false;s.update();});
    const retry=button('Retry exact input',()=>{c.uncertain=false;flush(c);});
    const discard=button('Discard unconfirmed input',()=>{if(c.busy)return;if(!confirm('After inspecting the shell, discard only the locally pending input? Some text may already have been queued or executed; this does not undo it.'))return;c.pending=null;c.queue=[];c.uncertain=false;c.lease=null;c.error='Local pending input discarded after explicit inspection; reclaim control before typing.';s.update();notify(c.error);});const stop=button('Terminate',async()=>{if(confirm('Terminate this UNSANDBOXED owner-held terminal? Completed effects remain.'))try{await command({op:'terminal_stop',terminal_id:id,confirmed:true});}catch(e){notify(e.message);}});
    const fit=button('Fit active group',async()=>{if(!c.lease){notify('Only the controlling client can resize.');return;}const r=s.body.getBoundingClientRect();const rows=Math.max(2,Math.min(60,Math.floor((r.height-55)/18))),cols=Math.max(4,Math.min(160,Math.floor((r.width-22)/8)));try{await command({op:'terminal_resize',terminal_id:id,client_id:clientId,lease:c.lease,rows,cols});}catch(e){notify(e.message);}});
    s.bar.append(claimButton,release,fit,retry,discard,stop,button('Older output',()=>{c.scrollback=Math.min(1000,c.scrollback+20);c.lastRead=0;startPolling();}),button('Live screen',()=>{c.scrollback=0;c.lastRead=0;startPolling();}));s.body.append(screen,input);
    screen.onclick=()=>input.focus();let composing=false;input.oncompositionstart=()=>{composing=true;};input.oncompositionend=()=>{composing=false;};
    input.addEventListener('input',()=>{if(composing)return;const text=input.value;input.value='';if(text)enqueue(c,{kind:'text',text});});
    input.addEventListener('compositionend',()=>{queueMicrotask(()=>{const text=input.value;input.value='';if(text)enqueue(c,{kind:'text',text});});});
    input.addEventListener('paste',e=>{e.preventDefault();const text=e.clipboardData.getData('text/plain');enqueue(c,{kind:'paste',text});});
    input.addEventListener('keydown',e=>{if(e.isComposing||composing)return;if(e.key===']'&&e.ctrlKey)return;if(prefix)return;
      if(e.metaKey)return; // Browser/OS reserved chords remain outside this emulator.
      const special=e.key.length>1&&!['Shift','Control','Alt','Meta','Dead','Unidentified'].includes(e.key);
      if(special||e.ctrlKey||e.altKey){e.preventDefault();e.stopPropagation();enqueue(c,{kind:'key',key:e.key,ctrl:e.ctrlKey,alt:e.altKey,shift:e.shiftKey});}
    });
    let last='';s.update=()=>{if(s.disposed)return;const snap=c.snapshot;input.disabled=!c.lease||c.uncertain;claimButton.disabled=Boolean(c.lease);release.disabled=!c.lease;retry.hidden=!c.uncertain;discard.hidden=!c.uncertain;discard.disabled=c.busy;const detail=snap?`${snap.status} • ${snap.cols}×${snap.rows} • ${snap.root}`:'Waiting for screen';s.status.textContent=`${detail} • UNSANDBOXED owner shell • ${c.lease?'you control':'read-only view'}${snap?.scrollback?' • viewing scrollback':''}${c.error?' • '+c.error:''}`;
      const screenKey=snap?`${snap.version}:${snap.scrollback}:${snap.rows}:${snap.cols}`:'';if(snap&&screenKey!==last){last=screenKey;const lines=[];for(const runs of snap.lines||[]){const row=$('div','terminal-row');for(const run of runs){const span=$('span','',run.text);const v=run.style||{};if(ansi(v.fg))span.style.color=ansi(v.fg);if(ansi(v.bg))span.style.backgroundColor=ansi(v.bg);if(v.bold)span.style.fontWeight='bold';if(v.dim)span.style.opacity='.7';if(v.italic)span.style.fontStyle='italic';if(v.underline)span.style.textDecoration='underline';if(v.inverse)span.classList.add('terminal-inverse');row.append(span);}lines.push(row);}screen.replaceChildren(...lines);if(snap.cursor_visible){const cursor=$('span','terminal-cursor');const canvas=document.createElement('canvas'),ctx=canvas.getContext('2d');let width=7.83;if(ctx){ctx.font=getComputedStyle(screen).font;width=ctx.measureText('M').width;}cursor.style.left=(10+snap.cursor[1]*width)+'px';cursor.style.top=(10+snap.cursor[0]*18)+'px';screen.append(cursor);}}
    };
    s.dispose=()=>{s.disposed=true;c.views.delete(s);if(!c.views.size&&c.lease){const token=c.lease;c.lease=null;command({op:'terminal_release',terminal_id:id,client_id:clientId,lease:token}).catch(()=>{});}};s.update();
  }
  async function pollTerminals(){if(!alive||!shown||document.hidden)return;for(const c of controllers.values()){
    if(![...c.views].some(s=>s.visible)||c.reading||Date.now()-c.lastRead<100)continue;c.reading=true;c.lastRead=Date.now();
    const body={op:'terminal_screen',terminal_id:c.id,scrollback:c.scrollback};if(c.lease){body.client_id=clientId;body.lease=c.lease;}
    command(body).then(snap=>{if(!alive)return;c.snapshot=snap;c.views.forEach(s=>s.update?.());}).catch(e=>{if(e.status===409){c.lease=null;c.uncertain=Boolean(c.pending||c.queue.length);}c.error=e.message;c.views.forEach(s=>s.update?.());}).finally(()=>{c.reading=false;});
  }}
  async function startTerminal(){if(!caps.terminals){notify('Owner did not allow browser terminals.');return;}if(!confirm('Launch an UNSANDBOXED shell on the aw OWNER machine? It can access files and credentials available to that OS user. This is not your browser device automatically.'))return;const body={op:'terminal_start',request_id:crypto.randomUUID(),context:gitContext,confirmed:true,rows:24,cols:80};try{const r=await command(body);open({kind:'terminal',resource:r.terminal_id,title:'Terminal'});}catch(e){notify('Launch result: '+e.message+'. No automatic retry. Check Existing terminals before another launch.');}}
  async function listTerminals(){try{const list=await command({op:'terminals'});dialog.replaceChildren($('h2','','Owner-held terminals'));for(const t of list){addOption(t.id.slice(-9)+' · '+t.status,()=>open({kind:'terminal',resource:t.id,title:'Terminal '+t.id.slice(-5)}));if(!['Running','Stopping'].includes(t.status))addOption('Forget exited '+t.id.slice(-9),async()=>{try{await command({op:'terminal_forget',terminal_id:t.id});notify('Exited resource forgotten. Old tab references cannot restart a process.');}catch(e){notify(e.message);}});}if(!list.length)dialog.append($('p','','No terminals started.'));dialog.append(button('Close',()=>dialog.close()));dialog.showModal();}catch(e){notify(e.message);}}
  function paint(){if(!alive)return;document.body.classList.toggle('desktop-mode',shown);shell.hidden=!shown;if(!shown)return;
    content.className='desk-content';const r=stage.getBoundingClientRect();const geo=groupRects(layout,r.width,r.height,maximize);chrome.replaceChildren();for(const surface of surfaces.values()){surface.node.hidden=true;surface.visible=false;}
    for(const [id,rect] of Object.entries(geo.groups)){
      const group=$('section','desk-group');group.setAttribute('aria-label','Content group '+id);group.style.cssText=`left:${rect.x}px;top:${rect.y}px;width:${rect.width}px;height:${rect.height}px;`;
      const strip=$('div','desk-tabstrip');strip.setAttribute('role','tablist');strip.setAttribute('aria-label','Content tabs '+id);group.append(strip,$('div','desk-group-empty',layout.groups[id].tabs.length?'':'Open any view with +'));
      for(const viewId of layout.groups[id].tabs){const view=layout.views[viewId],b=button(view.title,()=>{focusedView=viewId;act({type:'select',group:id,view:viewId});},'desk-tab');b.id='tab-'+viewId;b.setAttribute('role','tab');b.setAttribute('aria-selected',String(layout.groups[id].active===viewId));b.setAttribute('aria-controls',view.kind==='conversation'?'surface-chat':'surface-'+viewId);b.draggable=true;
        b.ondragstart=e=>e.dataTransfer.setData('application/x-aw-tab',viewId);b.onkeydown=e=>{if(['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();const buttons=[...strip.querySelectorAll('[role=tab]')],n=buttons.indexOf(b),next=buttons[(n+(e.key==='ArrowRight'?1:buttons.length-1))%buttons.length];next?.focus();}};strip.append(b);
      }
      strip.append(button('+',()=>showMenu(id),'desk-add'));strip.ondragover=e=>{e.preventDefault();};strip.ondrop=e=>{e.preventDefault();const view=e.dataTransfer.getData('application/x-aw-tab');if(view&&layout.views[view])act({type:'move',group:id,view});};chrome.append(group);
      const active=layout.groups[id].active;if(active){let s=surfaces.get(active);if(!s){s=makeSurface(layout.views[active],active);surfaces.set(active,s);}s.node.hidden=false;s.visible=true;s.node.style.left=rect.x+'px';s.node.style.top=(rect.y+42)+'px';s.node.style.width=rect.width+'px';s.node.style.height=Math.max(0,rect.height-42)+'px';s.node.setAttribute('aria-labelledby','tab-'+active);s.node.classList.toggle('focused-surface',layout.focused===id);s.update?.();s.node.onpointerdown=()=>{if(layout.focused!==id){layout=reduceLayout(layout,{type:'focus',group:id});}};}
    }
    for(const divider of geo.dividers){const handle=$('div','desk-divider '+divider.axis);handle.setAttribute('role','separator');handle.setAttribute('aria-label','Resize content groups');handle.setAttribute('aria-orientation',divider.axis==='columns'?'vertical':'horizontal');handle.tabIndex=0;handle.style.cssText=`left:${divider.x}px;top:${divider.y}px;width:${divider.width}px;height:${divider.height}px;`;handle.onkeydown=e=>{if(['ArrowLeft','ArrowDown','ArrowUp','ArrowRight'].includes(e.key)){e.preventDefault();let node=layout.root;for(const p of divider.path)node=node[p];act({type:'resize',path:divider.path,ratio:node.ratio+(['ArrowRight','ArrowDown'].includes(e.key)?.025:-.025)});}};
      handle.onpointerdown=e=>{e.preventDefault();const start=stage.getBoundingClientRect();const move=ev=>{const position=divider.axis==='columns'?ev.clientX-start.left:ev.clientY-start.top;layout=reduceLayout(layout,{type:'resize',path:divider.path,ratio:(position-divider.origin)/divider.extent});schedulePaint();};const end=()=>{document.removeEventListener('pointermove',move);document.removeEventListener('pointerup',end);};document.addEventListener('pointermove',move);document.addEventListener('pointerup',end);};chrome.append(handle);}
    startPolling();
    if(geo.collapsed)notify('Compact viewport: showing the focused group. Other groups remain in Groups; desired layout is preserved.');
  }
  function schedulePaint(){if(!raf&&alive)raf=requestAnimationFrame(()=>{raf=0;paint();});}
  function toggle(){shown=!shown;if(shown){chatPanel.append(bridge.chat);shell.hidden=false;}else{bookmark.after(bridge.chat);shell.hidden=true;}document.body.classList.toggle('desktop-mode',shown);schedulePaint();}
  function sessionChanged(){sidebars.refresh();for(const s of surfaces.values())if(s.visible)s.update?.();}
  function keyboard(e){
    if(!alive||!shown)return;
    const target=e.target;
    if(target?.closest?.('iframe,.terminal-input'))return;
    const ctrl=e.ctrlKey||e.metaKey;
    if((ctrl&&e.shiftKey&&e.key.toLowerCase()==='p')||e.key==='F1'){
      e.preventDefault();e.stopPropagation();showMenu();return;
    }
    if(ctrl&&e.shiftKey&&e.key.toLowerCase()==='e'){
      e.preventDefault();e.stopPropagation();left.hidden=false;left.classList.add('forced-visible');sidebars.focus('files');schedulePaint();return;
    }
    if(ctrl&&e.shiftKey&&e.key.toLowerCase()==='g'){
      e.preventDefault();e.stopPropagation();left.hidden=false;left.classList.add('forced-visible');sidebars.focus('git');schedulePaint();return;
    }
    if(ctrl&&e.key==='Tab'){
      const group=activeGroup(),tabs=group?.tabs||[];
      if(tabs.length>1){e.preventDefault();e.stopPropagation();const at=tabs.indexOf(group.active),delta=e.shiftKey?-1:1,next=tabs[(at+delta+tabs.length)%tabs.length];focusedView=next;act({type:'select',group:layout.focused,view:next});}
      return;
    }
    if(ctrl&&e.key==='`'){
      e.preventDefault();e.stopPropagation();listTerminals();return;
    }
  }

  const observer=new ResizeObserver(schedulePaint);observer.observe(stage);document.addEventListener('keydown',keyboard,true);
  let timer=null;function startPolling(){if(timer)clearTimeout(timer);timer=null;if(!alive||!shown||document.hidden||![...controllers.values()].some(c=>[...c.views].some(s=>s.visible)))return;pollTerminals();timer=setTimeout(startPolling,120);}
  const visibility=startPolling;document.addEventListener('visibilitychange',visibility);
  function dispose(){if(!alive)return;alive=false;clearTimeout(timer);cancelAnimationFrame(raf);observer.disconnect();document.removeEventListener('keydown',keyboard,true);document.removeEventListener('visibilitychange',visibility);for(const s of surfaces.values())if(s.kind!=='conversation')s.dispose?.();sidebars.dispose();bookmark.after(bridge.chat);bookmark.remove();shell.remove();dialog.remove();document.body.classList.remove('desktop-mode');controllers.clear();surfaces.clear();}
  paint();sidebars.refresh();
  return {toggle,dispose,sessionChanged};
}
