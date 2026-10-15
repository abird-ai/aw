// Lazy, plain-DOM context workflow. Opening/browsing never invokes a model.
// No provider credentials, arbitrary actor fields, automatic summarization or
// paid retries. Server snapshots and receipts remain authoritative.
export function uniqueSources(values) {
  const seen = new Set(); return values.filter(value => {
    const key = JSON.stringify([value.session_id, value.branch, value.head ?? null, value.selection]);
    if (seen.has(key)) return false; seen.add(key); return true;
  });
}
export function targetOf(snapshot) {
  const s = snapshot?.saved;
  if (!s || !Number.isSafeInteger(s.branch_revision)) throw new Error('Select a durable destination.');
  return {session_id:s.session_id, branch_id:s.branch_id, head:s.head, revision:s.branch_revision};
}
export function preparation(snapshot, sources, {compact = false, keep = 2, focus = '', target = null} = {}) {
  const chosen = uniqueSources(sources);
  if (!compact && !chosen.length) throw new Error('Select at least one source.');
  if (chosen.length > 16 || !Number.isInteger(keep) || keep < 0 || keep > 256) throw new Error('Selection outside bounds.');
  return {op:'prepare',request:{target:target||targetOf(snapshot),sources:compact?[]:chosen,
    kind:compact?{kind:'compact',keep_recent_turns:keep}:{kind:'import'}, focus,
    max_input_bytes:524288,max_output_bytes:32768,timeout_seconds:120}};
}
export function application(artifact, target, mode, requestId, name = '') {
  if (!artifact?.id || !target || !requestId) throw new Error('An artifact and exact target are required.');
  if (!['append','fork','new_session','next_prompt','compact'].includes(mode)) throw new Error('Unsupported target mode.');
  const kind={kind:mode};
  if (mode==='fork' || mode==='new_session') {
    if (!name.trim()) throw new Error('Name the new destination.');
    kind[mode==='fork'?'name':'title']=name.trim();
  }
  return {op:'apply',request:{artifact_id:artifact.id,target:{...target},mode:kind,request_id:requestId,confirmed_disclosure:true}};
}
export function orderedHistory(snapshot) {
  const out=[];const blocks=Array.isArray(snapshot?.context_blocks)?snapshot.context_blocks:[];
  const turns=Array.isArray(snapshot?.turns)?snapshot.turns:[];
  for (let index=0;index<=turns.length;index++) {
    for (const context of blocks.filter(b=>b.before_turn===index)) out.push({kind:'context',context});
    if(index<turns.length) out.push({kind:'turn',turn:turns[index]});
  }return out;
}
function node(tag, text = '', props = {}) {const el=document.createElement(tag);if(text)el.textContent=text;Object.assign(el,props);return el;}
function label(text, control) {const el=node('label',text);control.setAttribute('aria-label',text);el.append(control);return el;}
function choose(entries) {const el=node('select');for(const [value,title]of entries)el.append(node('option',title,{value}));return el;}
function uid(){return 'ctx-'+crypto.randomUUID();}
export function createContextFlow({api,getSnapshot,getChoice,refresh,guardTarget}) {
  let dialog=null,plan=null,digest='',artifact=null,sources=[],request=null,epoch=0,pollTimer=null,busy=false;
  function clearTimer(){if(pollTimer!==null)clearTimeout(pollTimer);pollTimer=null;}
  function close(){epoch++;clearTimer();dialog?.close();dialog?.remove();dialog=null;plan=null;artifact=null;sources=[];request=null;busy=false;}
  async function open() {
    close();const generation=epoch;
    const snapshot=getSnapshot();if(!snapshot?.saved)throw new Error('Open a saved destination first.');
    const root=node('dialog','',{className:'context-flow',id:'context-flow-dialog'});dialog=root;
    const title=node('h2','Bring knowledge between branches');
    const intro=node('p','Choose exact source snapshots, prepare without inference, then generate or write a summary. Applying adds context; only explicit compaction replaces an older span.');
    const status=node('p','No model call has been made.',{role:'status',className:'context-status'});
    const error=node('p','',{role:'alert',className:'error-box',hidden:true});
    const session=choose([]),branch=choose([]),scope=choose([['divergent','Since common ancestor'],['standalone','Standalone active path'],['range','Explicit range']]);
    const from=node('input','',{placeholder:'Start node ID (exclusive, range only)'}),head=node('input','',{placeholder:'End node ID (optional; otherwise pinned current head)'});
    const targetSession=choose([]),targetBranch=choose([]);
    const selected=node('ul','',{className:'context-selected'});
    const focus=node('textarea','',{rows:2,placeholder:'Focus: decisions, test findings, unresolved disagreements…',maxLength:4096});
    const compact=node('input','',{type:'checkbox'}),keep=node('input','',{type:'number',min:'0',max:'256',value:'2'});
    const evidence=node('pre','',{className:'context-evidence'}),summary=node('textarea','',{rows:9,placeholder:'Generated or human-authored candidate. Editing creates a new artifact, never changes the old one.',maxLength:65536});
    const mode=choose([['append','Add to selected branch'],['fork','Create fork + import'],['new_session','New private session + import'],['next_prompt','Only for the next prompt'],['compact','Apply prepared compaction']]);
    const destName=node('input','',{placeholder:'New branch/session name'}),reuse=node('input','',{placeholder:'Existing artifact ID (optional reuse)'}),lookup=node('input','',{placeholder:'Operation/request ID for recovery'});
    const applyLookup=node('input','',{placeholder:'Attachment request ID for lookup-only recovery'});
    const button=(text,fn,id)=>{const b=node('button',text,{type:'button',className:'quiet',id:id||''});b.onclick=()=>run(fn);return b;};
    async function call(command) {const current=getSnapshot();if(!current?.saved||!live())throw new Error('Destination is unavailable.');const value=await api('/api/context/action',{expected_revision:current.revision,command});if(!live())throw new Error('View closed; look up the retained operation before retrying.');return value;}
    function live(){return dialog===root&&generation===epoch;}
    async function run(fn) {if(busy)return;error.hidden=true;try{busy=true;await fn();}catch(e){if(live()){error.textContent=e.message;error.hidden=false;}}finally{if(live())busy=false;}}
    function selectedRows(){selected.replaceChildren();sources.forEach((s,i)=>{const row=node('li',`${s.session_id} / ${s.branch} · ${s.selection.kind}`);row.append(button('Remove',()=>{sources.splice(i,1);selectedRows();}));selected.append(row);});}
    function showPlan(value){plan=value.plan;digest=value.input_sha256;artifact=null;summary.value='';
      evidence.textContent=`Pinned ${plan.evidence.length} evidence records; ${plan.duplicate_nodes_omitted} exact duplicates omitted.\nDevice consent SHA-256: ${digest}\n\n`+JSON.stringify({target:plan.target,sources:plan.sources,evidence:plan.evidence},null,2);
      mode.value=plan.kind.kind==='compact'?'compact':'append';status.textContent='Prepared. Review exact evidence, destination and account. No model invocation yet.';
    }
    function showArtifact(value){artifact=value;plan=value.plan;summary.value=value.text;status.textContent=`Candidate ${value.id} · ${value.method}${value.imported_unverified?' · unverified external import':''}${value.identity?' · '+value.identity.account_id+' / '+value.identity.model:''}. Source snapshots stay immutable.`;}
    async function loadBranches(){const values=await call({op:'branches',session_id:session.value});if(!live())return;branch.replaceChildren();for(const b of values)branch.append(node('option',`${b.name} · ${b.head}`,{value:b.id}));}
    async function inspectOperation(id){const op=await call({op:'operation',id});if(!live())return;
      status.textContent=`Summary ${op.state} · ${id}`;
      if(op.state==='ready'&&op.artifact_id){clearTimer();showArtifact(await call({op:'artifact',artifact_id:op.artifact_id}));await refresh();}
      else if(['failed','cancelled','incomplete','interrupted'].includes(op.state)){clearTimer();throw new Error(op.error||`Summary ${op.state}; no automatic retry.`);}
      else {pollTimer=setTimeout(()=>{if(live())run(()=>inspectOperation(id));},800);}
    }
    async function loadTargetBranches(){const values=await call({op:'branches',session_id:targetSession.value});targetBranch.replaceChildren();for(const b of values)targetBranch.append(node('option',`${b.name} · ${b.head}`,{value:b.id}));if(targetSession.value===snapshot.saved.session_id)targetBranch.value=snapshot.saved.branch_id;}
    async function selectedTarget(){return call({op:'target_at',session_id:targetSession.value,branch:targetBranch.value});}
    session.onchange=()=>run(loadBranches);targetSession.onchange=()=>run(loadTargetBranches);
    const add=button('Add source',()=>{if(!session.value||!branch.value)throw new Error('Choose a source session and branch.');sources=uniqueSources([...sources,{session_id:session.value,branch:branch.value,head:head.value.trim()||null,selection:scope.value==='range'?{kind:'range',start_exclusive:from.value.trim()}:{kind:scope.value}}]);selectedRows();},'context-add-source');
    const prepare=button('1 · Prepare',async()=>{showPlan(await call(preparation(getSnapshot(),sources,{compact:compact.checked,keep:Number(keep.value),focus:focus.value,target:await selectedTarget()})));},'context-prepare');
    const generate=button('2 · Generate summary',async()=>{
      if(!plan)throw new Error('Prepare and review the sources first.');
      const choice=getChoice();if(!choice)throw new Error('Select your own permitted account/model.');
      if(!confirm(`Disclose the prepared source evidence to ${choice.account_id} / ${choice.model}? This is a separate model invocation. Device accounts also require local consent for digest ${digest}.`))return;
      request=uid();lookup.value=request;
      await call({op:'generate',plan_id:plan.id,request_id:request,account_id:choice.account_id,model:choice.model,confirmed_disclosure:true});
      await inspectOperation(request);
    },'context-generate');
    const manual=button('Save human candidate',async()=>{if(!plan)throw new Error('Prepare a source plan first.');showArtifact(await call({op:'manual',plan_id:plan.id,request_id:uid(),text:summary.value,from:artifact?.id||null}));},'context-manual');
    const apply=button('3 · Apply reviewed candidate',async()=>{
      if(!artifact)throw new Error('Generate or save a candidate first.');
      if(summary.value!==artifact.text)throw new Error('Save the edited text as a new human candidate before applying.');
      if(!guardTarget())return;
      if(!confirm('Apply this exact candidate to its pinned destination? No model call will be made. Files, accounts and tool grants do not move.'))return;
      const applyId=uid();applyLookup.value=applyId;const result=await call(application(artifact,plan.target,mode.value,applyId,destName.value));
      status.textContent=result.reused?'Existing import reused; no duplicate generation.':result.pending_id?'Attached once to the next exact prompt.':'Summary applied; original source and destination history remain stored.';await refresh();
    },'context-apply');
    const cancel=button('Cancel generation',async()=>{if(request)await api('/api/context/cancel',{request_id:request});},'context-cancel');
    const recover=button('Look up operation',()=>{clearTimer();return inspectOperation(lookup.value.trim());});
    const load=button('Load existing artifact',async()=>{showArtifact(await call({op:'artifact',artifact_id:reuse.value.trim()}));});
    const retarget=button('Use selected destination',async()=>{if(!plan)throw new Error('Load a candidate first.');if(!confirm('Explicitly retarget this existing candidate to the explicitly selected saved branch head? No regeneration occurs.'))return;plan={...plan,target:await selectedTarget()};status.textContent='New target selected explicitly. Source candidate unchanged; apply rechecks permissions.';});
    const chooser=node('div','',{className:'context-grid'});chooser.append(label('Source session',session),label('Branch',branch),label('Scope',scope),label('From node',from),label('Through node',head),add);
    const actions=node('div','',{className:'dialog-actions'});actions.append(prepare,generate,manual,cancel);
    const applyRow=node('div','',{className:'dialog-actions'});applyRow.append(mode,destName,apply,retarget);
    const recovery=node('details');recovery.append(node('summary','Reuse and recovery'),reuse,load,lookup,recover,applyLookup,button('Look up attachment',async()=>{const receipt=await call({op:'application',request_id:applyLookup.value.trim()});status.textContent=`Attachment already recorded in ${receipt.session.id} / ${receipt.branch.name}. No mutation repeated.`;}));
    const sharing=node('details');sharing.append(node('summary','Share a source with a named participant'));
    const people=choose([]),accounts=node('input','',{placeholder:'Recipient account IDs allowed for summarization (comma separated)'});
    sharing.append(label('Participant',people),accounts,
      button('Grant read + selected summary accounts',async()=>{
        if(!people.value||!session.value)throw new Error('Select a source session and enabled recipient.');
        if(!confirm('Grant this person read access to this source and its future updates? Only the owner can grant. This does not grant source append or any credential.'))return;
        await call({op:'grant_read',session_id:session.value,principal:people.value,allow:true,summary_accounts:accounts.value.split(',').map(x=>x.trim()).filter(Boolean)});status.textContent='Source grant recorded. Empty account list gives read only, not summary use.';
      }),button('Revoke source grant',async()=>{await call({op:'grant_read',session_id:session.value,principal:people.value,allow:false,summary_accounts:[]});status.textContent='Future source and derived use revoked; already delivered content cannot be recalled.';}));
    const exits=node('div','',{className:'dialog-actions'});exits.append(button('Close',close,'context-close'));
    const targetChooser=node('div','',{className:'context-grid'});targetChooser.append(label('Destination session',targetSession),label('Destination branch',targetBranch));
    root.append(title,intro,targetChooser,chooser,selected,label('Focus',focus),label('Compact this destination instead',compact),label('Recent complete-attempt tail to keep',keep),actions,status,error,evidence,label('Candidate text',summary),applyRow,recovery,sharing,exits);
    root.addEventListener('cancel',()=>close());document.body.append(root);root.showModal();
    try {
      const sessions=await call({op:'sources'});if(!live())return;
      for(const s of sessions)session.append(node('option',`${s.title} · ${s.owner}`,{value:s.id}));
      const targets=await call({op:'targets'});for(const t of targets)targetSession.append(node('option',t.title,{value:t.id}));targetSession.value=snapshot.saved.session_id;if(targetSession.options.length)await loadTargetBranches();
      const recipients=await api('/api/context/recipients');if(!live())return;
      for(const [id,name]of Object.entries(recipients))people.append(node('option',`${name} · ${id}`,{value:id}));
      sharing.hidden=!people.options.length;
      if(session.options.length)await loadBranches();
    }catch(e){if(live()){error.textContent=e.message;error.hidden=false;}}
  }
  return {open,dispose:close};
}
