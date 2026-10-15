import { acceptSnapshot, acceptDelta, canSend, isSubmitKey, submission, accountChoices, draftTargetChanged, submissionSettled, principalLabel } from '/model.js';
const $ = id => document.getElementById(id);
let state = null, source = null, posting = false, pendingSubmission = null, refresh = null, refreshAgain = false;
let chosenAccount = null, selectionMissing = false, draftTargetStale = false, branchesFor = null, retryOperation = null;
let liveText = null, viewEpoch = 0, refreshEpoch = -1, activeSubmission = null;
let desktop = null, contextFlow = null, settingsUI = null, localSingleUser = false;
function status(text, online = false) { $('connection').textContent = text; $('status-dot').className = 'status-dot' + (online ? ' online' : ''); }
function message(text, id = 'run-error') { $(id).textContent = text || ''; $(id).hidden = !text; }
function atBottom() { const el = $('transcript'); return el.scrollHeight - el.scrollTop - el.clientHeight < 80; }
function scrollBottom() { $('transcript').scrollTop = $('transcript').scrollHeight; }
function lockUI(text = '') {
  $('context-recovery').hidden=true;
  contextFlow?.dispose();contextFlow=null;
  desktop?.dispose(); desktop=null; $('open-workspace').hidden=true;
  chosenAccount = null; selectionMissing = false; draftTargetStale = false; branchesFor = null;
  $('session-dialog').close(); $('tree-dialog').close(); $('session-choice').replaceChildren(); $('node-choice').replaceChildren(); $('branch-choice').replaceChildren(); $('saved-title').textContent=''; $('draft-target').hidden=true; $('account-choice').replaceChildren();
  viewEpoch++; posting = false; activeSubmission = null;
  source?.close(); source = null; state = null; liveText = null; pendingSubmission = null;
  $('actor-label').textContent = '';
  $('transcript').replaceChildren(); $('prompt').value = ''; $('pair-code').value = '';
  $('uncertain').hidden = true; $('durable-recovery').hidden = true; $('durable-recovery').replaceChildren(); retryOperation = null;
  if (localSingleUser) {
    $('workspace').hidden = false;
    $('pairing').hidden = true;
    $('settings-button').hidden = false;
    $('lock').hidden = true;
    message(text || 'Local owner disconnected. Refresh after the owner is running again.');
    status('Disconnected');
  } else {
    $('settings-button').hidden = true;
    $('workspace').hidden = true;
    $('pairing').hidden = false;
    message(text, 'pair-error');
    status('Not paired');
  }
  controls();
}
function revokeContext() {
  lockUI('Source access changed. Cached derived text, candidates and views have been cleared.');
  $('context-recovery').hidden=false;
  status('Context withheld');
}
$('context-new-private').addEventListener('click',async()=>{
  const epoch=viewEpoch;
  if(!confirm('Start a new private conversation without the revoked source? This does not clear unresolved tool effects.'))return;
  try {
    const recovery=await api('/api/context/recovery');
    if(epoch!==viewEpoch)return;
    if(recovery.busy)throw new Error('Wait for active work to settle before creating another conversation.');
    const next=await api('/api/reset',{expected_revision:recovery.revision});
    if(epoch!==viewEpoch)return;
    $('context-recovery').hidden=true;renderSnapshot(next);connectEvents();
  }catch(e){if(epoch===viewEpoch)message(e.message,'pair-error');}
});

async function api(path, body) {
  const control = new AbortController(); const timer = setTimeout(() => control.abort(), 12000);
  try {
    const response = await fetch(path, { method: body === undefined ? 'GET' : 'POST', credentials: 'same-origin', cache: 'no-store',
      headers: body === undefined ? {} : { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: control.signal });
    const data = await response.json();
    if (!response.ok) { const e = new Error(typeof data.error === 'string' ? data.error : 'Request failed.'); e.status = response.status; throw e; }
    return data;
  } finally { clearTimeout(timer); }
}

async function ensureSettings(show = false) {
  if (!settingsUI) {
    try {
      const probe = await api('/api/settings');
      if (!probe?.local_single_user) return null;
    } catch (error) {
      if (error.status === 403) {
        localSingleUser = false;
        $('settings-button').hidden = true;
        $('lock').hidden = false;
        return null;
      }
      throw error;
    }
    const module = await import('/settings.js');
    settingsUI = module.createSettings({
      request: api,
      changed: async () => {
        await refreshSnapshot();
        desktop?.sessionChanged(state);
      },
    });
  }
  const config = await settingsUI.load(show);
  localSingleUser = Boolean(config?.local_single_user);
  if (localSingleUser) {
    $('settings-button').hidden = false;
    $('lock').hidden = true;
    $('pairing').hidden = true;
    $('workspace').hidden = false;
  }
  return config;
}

$('settings-button').addEventListener('click', () => {
  ensureSettings(true).catch(error => message(error.message));
});
function controls() {
  $('send').disabled = !canSend(state, posting, $('prompt').value) || Boolean(pendingSubmission) || selectionMissing || draftTargetStale;
  $('cancel').hidden = !state?.pending;
  $('new-chat').disabled = posting || Boolean(state?.pending);
  $('reconnect').disabled = posting;
  $('account-choice').disabled = posting || Boolean(state?.pending);
  $('reconcile').hidden = !state?.needs_reconciliation;
  $('acknowledge').disabled = posting || Boolean(state?.pending);
  for (const id of ['saved-sessions','switch-branch','fork-branch','session-tree','checkpoint','export-session','context-flow']) $(id).disabled = !state?.saved || posting || Boolean(state?.pending) || Boolean(pendingSubmission);
  $('draft-target').hidden = !draftTargetStale;
  $('run-state').textContent = state?.pending ? 'Response streaming…' : state?.needs_reconciliation ? 'Inspect interrupted effects' : selectionMissing ? 'Select a permitted account' : state ? 'Ready' : '';
}
function block(role, text, outcome = '') {
  const article = document.createElement('article'); article.className = `message ${role} ${outcome}`;
  const label = document.createElement('div'); label.className = 'message-label'; label.textContent = role === 'user' ? 'You' : 'aw';
  const meta = document.createElement('span'); meta.className = 'outcome'; meta.textContent = outcome; label.append(meta);
  const body = document.createElement('div'); body.className = 'message-body'; const node = document.createTextNode(text); body.append(node);
  article.append(label, body); $('transcript').append(article); return node;
}
function renderSnapshot(next) {
  const accepted = acceptSnapshot(state, next); if (accepted === state) return;
  const follow = atBottom() || !state; const oldScroll = $('transcript').scrollTop;
  if (draftTargetChanged(state, accepted, $('prompt').value)) draftTargetStale = true;
  state = accepted; renderAccounts(); renderSaved(); $('pairing').hidden = true; $('workspace').hidden = false; $('open-workspace').hidden=false;
  desktop?.sessionChanged(state);
  $('actor-label').textContent = principalLabel(state);
  $('profile-label').textContent = state.profile; $('model-label').textContent = state.model;
  renderRecoveryHints();
  $('transcript').replaceChildren(); liveText = null;
  for (let i=0;i<=state.turns.length;i++) {
    for(const context of (state.context_blocks||[]).filter(c=>c.before_turn===i)) {
      const box=document.createElement('details');box.className='context-import';
      const title=document.createElement('summary');title.textContent=`${context.kind==='compaction'?'Compacted':'Imported'} context · ${context.artifact_id} · ${(context.sources||[]).length} source references`;
      const text=document.createElement('pre');text.textContent=context.text;box.append(title,text);$('transcript').append(box);
    }
    const turn=state.turns[i];if(turn){block('user',turn.prompt);block('assistant',turn.answer,turn.outcome);renderTools(turn.tool_records);renderUsage(turn.invocations);}
  }
  if (state.pending) { block('user', state.pending.prompt); liveText = block('assistant', state.pending.answer, 'streaming'); renderTools(state.pending.tool_records); }
  if (!state.turns.length && !state.pending && !state.context_blocks?.length) {
    const empty = document.createElement('div'); empty.className = 'welcome';
    const mark = document.createElement('span'); mark.className = 'welcome-mark'; mark.textContent = 'aw';
    const title = document.createElement('h2'); title.textContent = 'What shall we work through?';
    const note = document.createElement('p'); note.textContent = state.identity
      ? 'A simple conversation with your configured model. Saved branches retain history; current tool and account grants still control every request.'
      : 'No model is configured yet. Open Settings to add a local or compatible provider; no model call is made until you send a prompt.';
    empty.append(mark, title, note); $('transcript').append(empty);
  }
  message(state.last_error); controls();
  if (follow) scrollBottom(); else $('transcript').scrollTop = oldScroll;
}
function refreshSnapshot() {
  const epoch = viewEpoch;
  if (refresh && refreshEpoch === epoch) { refreshAgain = true; return refresh; }
  refreshEpoch = epoch;
  const task = (async () => {
    do {
      refreshAgain = false; const next = await api('/api/session');
      if (epoch !== viewEpoch) return;
      renderSnapshot(next);
    } while (refreshAgain);
  })().catch(e => {
    if (epoch !== viewEpoch) return;
    if (e.status === 401) lockUI('Pair this browser to the running owner.');
    else if(e.status===403){revokeContext();}
    else { status('Disconnected'); message(e.message); }
  }).finally(() => { if (refresh === task) refresh = null; });
  refresh = task; return task;
}
function connectEvents() {
  const epoch = viewEpoch;
  source?.close(); source = new EventSource('/api/events');
  source.onopen = () => { if (epoch === viewEpoch) status('Owner connected', true); };
  source.onerror = () => { if (epoch === viewEpoch) status('Reconnecting…'); };
  source.addEventListener('context_revoked',()=>{if(epoch===viewEpoch)revokeContext();});
  source.addEventListener('auth_expired', () => { if (epoch === viewEpoch) lockUI('Session locked or expired. Ask the owner for a new invitation after restart.'); });
  source.addEventListener('snapshot', event => {
    if (epoch !== viewEpoch) return;
    try { renderSnapshot(JSON.parse(event.data)); status('Owner connected', true); }
    catch (e) { message(e.message); refreshSnapshot(); }
  });
  source.addEventListener('changed', () => { if (epoch === viewEpoch) refreshSnapshot(); });
  source.addEventListener('delta', event => {
    if (epoch !== viewEpoch) return;
    try {
      const follow = atBottom(); const delta = JSON.parse(event.data); const next = acceptDelta(state, Number(event.lastEventId), delta);
      if (next.resync) { refreshSnapshot(); return; }
      if (next.applied) { state = next.snapshot; if (liveText) liveText.appendData(delta.text); else { refreshSnapshot(); return; } if (follow) scrollBottom(); }
    } catch (e) { message(e.message); refreshSnapshot(); }
  });
}
async function restore() {
  desktop?.dispose(); desktop = null;
  if (posting && activeSubmission) { pendingSubmission = activeSubmission; $('uncertain').hidden = false; }
  posting = false; activeSubmission = null;
  const epoch = ++viewEpoch;
  source?.close(); source = null;
  try {
    const next = await api('/api/session');
    if (epoch !== viewEpoch) return;
    renderSnapshot(next); connectEvents();
    const localSettings = await ensureSettings(false);
    if (epoch !== viewEpoch) return;
    if (localSettings) {
      await ensureWorkspace(false);
      if (epoch !== viewEpoch) return;
      if (!state?.identity) await settingsUI.open();
    }
  } catch (e) {
    if (epoch !== viewEpoch) return;
    if (e.status === 401) lockUI();
    else if(e.status===403)revokeContext();
    else lockUI('Cannot reach the local owner. Start the intended aw owner and refresh.');
  }
}
$('pair-form').addEventListener('submit', async event => {
  event.preventDefault(); $('pair-button').disabled = true; message('', 'pair-error');
  try { await api('/api/pair', { code: $('pair-code').value.trim() }); $('pair-code').value = ''; await restore(); }
  catch (e) { message(e.message, 'pair-error'); }
  finally { $('pair-button').disabled = false; }
});
async function send(body) {
  if (posting) return; const epoch = viewEpoch; posting = true; activeSubmission = body; controls(); message('');
  try {
    const receipt = await api('/api/prompts', body); if (epoch !== viewEpoch) return; pendingSubmission = null; $('uncertain').hidden = true;
    // Typing a different next draft while the POST is in-flight is not erased.
    if ($('prompt').value === body.prompt) $('prompt').value = '';
    await refreshSnapshot();
    if (submissionSettled(receipt)) message(`Recovered prior submission: ${receipt.state}. No new model request was started.`);
  } catch (e) {
    if (epoch !== viewEpoch) return;
    if (e.status === 401) { lockUI(e.message); }
    else if (e.status) { pendingSubmission = null; $('uncertain').hidden = true; await refreshSnapshot(); if (epoch === viewEpoch) message(e.message); }
    else { pendingSubmission = body; $('uncertain').hidden = false; message('Connection failed before the result was confirmed. No automatic retry was made.'); }
  } finally { if (epoch === viewEpoch) { posting = false; activeSubmission = null; controls(); } }
}
$('compose').addEventListener('submit', event => {
  event.preventDefault(); if (!canSend(state, posting, $('prompt').value) || pendingSubmission || selectionMissing || draftTargetStale) return;
  const body=submission(state, $('prompt').value, crypto.randomUUID(), chosenAccount); if(retryOperation) body.retry_operation_id=retryOperation; send(body); retryOperation=null;
});
$('prompt').addEventListener('keydown', event => { if (isSubmitKey(event)) { event.preventDefault(); $('compose').requestSubmit(); } });
$('prompt').addEventListener('input', controls);
$('retry-submit').addEventListener('click', () => { if (pendingSubmission) send(pendingSubmission); });
$('cancel').addEventListener('click', async () => {
  if (!state?.pending) return; const epoch = viewEpoch;
  try { await api('/api/cancel', { run_id: state.pending.run_id }); if (epoch !== viewEpoch) return; $('run-state').textContent = 'Cancellation requested; remote billing may continue.'; }
  catch (e) { message(e.message); }
});
$('new-chat').addEventListener('click', async () => {
  if (!state || posting || state.pending || !confirm(state.saved ? 'Create a new saved conversation? Previous history remains. Unsent draft text is not saved.' : 'Discard this memory-only conversation? Nothing is saved to disk.')) return;
  const epoch = viewEpoch;
  try { const next = await api('/api/reset', { expected_revision: state.revision }); if (epoch !== viewEpoch) return; pendingSubmission = null; $('uncertain').hidden = true; renderSnapshot(next); }
  catch (e) { message(e.message); }
});
$('lock').addEventListener('click', async () => {
  if (!confirm('Lock this browser session and cancel any active response? A fresh invitation requires restarting the owner.')) return;
  try { await api('/api/logout', {}); lockUI('Session locked. Ask the owner for a new invitation after restart.'); }
  catch (e) { message(e.message); }
});
$('reconnect').addEventListener('click', restore);
window.addEventListener('pagehide', () => { viewEpoch++; source?.close(); });
window.addEventListener('pageshow', event => { if (event.persisted) restore(); });
controls(); restore();

function renderRecoveryHints() {
  const host=$('durable-recovery');host.replaceChildren();
  const hints=Array.isArray(state?.recovery_hints)?state.recovery_hints:[];
  if(!hints.length){host.hidden=true;return;} host.hidden=false;
  for(const h of hints){
    const row=document.createElement('div');row.className='recovery-hint';
    const text=document.createElement('span');text.textContent=h.message;
    row.append(text);
    const actions=h.actions||{};
    const names=Array.isArray(actions.actions)?actions.actions:[];
    if(names.includes('retry_same_context')&&typeof actions.prompt==='string'){
      const b=document.createElement('button');b.type='button';b.className='quiet';b.textContent='Retry same context';
      b.addEventListener('click',async()=>{
        try {
          if(!state?.saved||typeof actions.origin_head!=='string')throw new Error('Original retry snapshot is unavailable.');
          if(state.saved.head!==actions.origin_head){
            const name=`retry-${h.operation_id.slice(-8)}`;
            const next=await api('/api/fork',{expected_revision:state.revision,name,at_node:actions.origin_head});renderSnapshot(next);
          }
          retryOperation=h.operation_id;$('prompt').value=actions.prompt;controls();$('prompt').focus();
        }catch(e){message(e.message);}
      });row.append(b);
    }
    if(names.includes('retry_from_current_branch')&&typeof actions.prompt==='string'){
      const b=document.createElement('button');b.type='button';b.className='quiet';b.textContent='Retry from current branch';
      b.addEventListener('click',()=>{retryOperation=null;$('prompt').value=actions.prompt;controls();$('prompt').focus();});row.append(b);
    }
    if(names.includes('inspect_effects')){
      const b=document.createElement('button');b.type='button';b.className='quiet';b.textContent='Inspect effects';b.addEventListener('click',()=>$('reconcile').scrollIntoView({behavior:'smooth'}));row.append(b);
    }
    host.append(row);
  }
}

function renderAccounts() {
  const choices = accountChoices(state); const select = $('account-choice'); select.replaceChildren();
  const stillAllowed = choices.some(c => c.account_id === chosenAccount?.account_id && c.model === chosenAccount?.model);
  if (chosenAccount && !stillAllowed) { chosenAccount = null; selectionMissing = true; }
  const serverDefault = choices.find(c => c.account_id === state.identity?.account_id && c.model === state.identity?.model) || null;
  // A newly configured explicit server default is not an implicit fallback: it is
  // the owner's just-saved selection. Adopt it when no valid local choice exists.
  if (!chosenAccount && serverDefault) {
    chosenAccount = serverDefault;
    selectionMissing = false;
  } else if (!chosenAccount && !choices.length) {
    selectionMissing = true;
  }
  if (selectionMissing) {
    const empty = document.createElement('option'); empty.value = ''; empty.textContent = 'Select a permitted account — no fallback'; empty.selected = true; select.append(empty);
  }
  for (const c of choices) {
    const option = document.createElement('option'); option.value = JSON.stringify([c.account_id, c.model]);
    option.textContent = `${c.account_id} / ${c.model} · ${c.account_kind} · ${c.inference_executor} · payer ${c.payer?.id || "unavailable"}`;
    option.selected = c.account_id === chosenAccount?.account_id && c.model === chosenAccount?.model; select.append(option);
  }
  $('tool-policy').textContent = (state.tools?.length ? `Tools: ${state.tools.join(', ')} on ${state.tool_host || 'owner'}` : 'Tools: off');
}
function renderTools(records = []) {
  if (!records?.length) return;
  const list = document.createElement('div'); list.className = 'tool-list';
  for (const r of records) {
    const item = document.createElement('details'); const title = document.createElement('summary');
    title.textContent = `${r.call?.name || 'Tool'} · ${r.status} · ${r.execution_site || 'owner'}`;
    const body = document.createElement('pre'); body.textContent = r.output?.content || 'No completed result yet.';
    item.append(title, body); list.append(item);
  }
  $('transcript').append(list);
}
function renderUsage(invocations = []) {
  if (!invocations?.length) return;
  const el = document.createElement('div'); el.className = 'usage-note';
  const first = invocations[0].identity;
  const known = invocations.every(i => Number.isSafeInteger(i.meta?.usage?.input_tokens) && Number.isSafeInteger(i.meta?.usage?.output_tokens));
  el.textContent = `${invocations.length} model call(s) · ${first.actor} · account ${first.account_id} · model ${first.model || "unavailable"} · payer ${first.payer?.id || "unavailable"} · executor ${first.inference_executor} · usage ${known ? 'provider-reported (not a verified invoice)' : 'partly/unreported'}`;
  $('transcript').append(el);
}
$('account-choice').addEventListener('change', () => {
  try { const [account_id, model] = JSON.parse($('account-choice').value); chosenAccount = {account_id, model}; selectionMissing = false; }
  catch { chosenAccount = null; selectionMissing = true; }
  controls();
});
$('acknowledge').addEventListener('click', async () => {
  const epoch=viewEpoch;
  if (!state || state.pending || !confirm('Confirm that you inspected possible file/process effects. This does not undo or replay anything.')) return;
  try { const result=await api('/api/acknowledge', {expected_revision:state.revision,confirmed_inspected:true}); if(epoch===viewEpoch)renderSnapshot(result); }
  catch(error){if(epoch===viewEpoch)message(error.message);}
});

function renderSaved() {
  const saved = state?.saved;
  $('storage-label').textContent = saved ? 'Local / durable branches' : 'Local / explicitly memory only';
  $('history-note').textContent = saved ? (state.context_checkpoint ? 'Explicit context checkpoint active · original tree retained' : 'History retained across restarts · current permissions only') : 'History lasts until the owner exits.';
  $('saved-controls').hidden = !saved;
  if (!saved) { branchesFor = null; return; }
  $('saved-title').textContent = `${saved.title} · ${saved.branch_name}${saved.imported ? ' · imported/unverified source' : ''}`;
  $('saved-title').title = `${saved.session_id} / ${saved.branch_id}`;
  const key = `${state.instance}/${saved.session_id}/${saved.branch_id}/${saved.branch_revision}`;
  if (branchesFor === key) return;
  branchesFor = key;
  const epoch = viewEpoch;
  api('/api/branches').then(result => {
    if (epoch !== viewEpoch || key !== branchesFor) return;
    $('branch-choice').replaceChildren();
    for (const branch of result.branches || []) {
      const option=document.createElement('option'); option.value=branch.id; option.textContent=branch.name;
      option.selected=branch.id===state.saved.branch_id; $('branch-choice').append(option);
    }
  }).catch(error => { if (epoch === viewEpoch && key === branchesFor) message(error.message); });
}
async function changeView(path, extra={}) {
  if (!state?.saved || posting || state.pending || pendingSubmission) return;
  const epoch=viewEpoch; posting=true; controls();
  try {
    const next=await api(path,{expected_revision:state.revision,...extra});
    if(epoch!==viewEpoch)return;
    renderSnapshot(next); $('session-dialog').close(); $('tree-dialog').close();
  } catch(error){if(epoch===viewEpoch){message(error.message);await refreshSnapshot();}}
  finally{if(epoch===viewEpoch){posting=false;controls();}}
}
$('saved-sessions').addEventListener('click',async()=>{
  const epoch=viewEpoch;
  try {const data=await api('/api/sessions');if(epoch!==viewEpoch)return;
    $('session-choice').replaceChildren();
    for(const item of data.sessions||[]){const option=document.createElement('option');option.value=item.id;option.textContent=`${item.title} · ${item.id.slice(-8)} · ${item.directory}`;option.selected=item.id===state.saved?.session_id;$('session-choice').append(option);}
    $('session-dialog').showModal();
  }catch(error){if(epoch===viewEpoch)message(error.message);}
});
$('open-session').addEventListener('click',()=>changeView('/api/session/open',{session_id:$('session-choice').value,branch:'main'}));
$('close-sessions').addEventListener('click',()=>$('session-dialog').close());
$('switch-branch').addEventListener('click',()=>changeView('/api/session/open',{session_id:state.saved.session_id,branch:$('branch-choice').value}));
$('fork-branch').addEventListener('click',()=>{const name=window.prompt('New conversation branch name (does not fork files):');if(name?.trim())changeView('/api/fork',{name:name.trim()});});
$('checkpoint').addEventListener('click',()=>{
  if(!confirm('An explicit checkpoint shortens FUTURE model context. Original history stays in the tree. Include the facts/constraints the next model needs; no automatic summary or model call is performed.'))return;
  const summary=window.prompt('Your context summary:');if(summary?.trim())changeView('/api/checkpoint',{summary});
});
$('session-tree').addEventListener('click',async()=>{
  const epoch=viewEpoch;
  try{const data=await api('/api/tree');if(epoch!==viewEpoch)return;$('node-choice').replaceChildren();
    const depth=new Map();
    for(const node of data.nodes||[]){const level=node.parent_id?(depth.get(node.parent_id)||0)+1:0;depth.set(node.id,level);
      const option=document.createElement('option');option.value=node.id;option.disabled=node.kind==='prompt';
      const description=node.kind==='turn'?`#${node.payload?.id} ${node.payload?.outcome}`:node.kind==='checkpoint'?'context checkpoint':node.kind;
      option.textContent=`${'  '.repeat(Math.min(level,8))}${description} · ${node.actor} · ${node.id.slice(-8)}`;$('node-choice').append(option);}
    $('tree-dialog').showModal();
  }catch(error){if(epoch===viewEpoch)message(error.message);}
});
$('fork-node').addEventListener('click',()=>{const name=window.prompt('New branch name at this completed node:');if(name?.trim()&&$('node-choice').value)changeView('/api/fork',{name:name.trim(),at_node:$('node-choice').value});});
$('close-tree').addEventListener('click',()=>$('tree-dialog').close());
$('export-session').addEventListener('click',async()=>{
  if(!confirm('Export this full session tree, including prompts, results and tool output? It can contain sensitive data. Provider key configuration is not included.'))return;
  const epoch=viewEpoch;
  try{const response=await fetch('/api/export',{credentials:'same-origin',cache:'no-store'});if(!response.ok)throw new Error('Export unavailable while active or unauthorized.');const blob=await response.blob();if(epoch!==viewEpoch)return;
    const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='aw-session.jsonl';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  }catch(error){if(epoch===viewEpoch)message(error.message);}
});
$('use-draft').addEventListener('click',()=>{if(confirm('Use this retained unsent draft in the currently displayed branch?')){draftTargetStale=false;controls();}});
$('discard-draft').addEventListener('click',()=>{$('prompt').value='';draftTargetStale=false;controls();});

// Browser workspace: local single-user mode mounts this automatically; paired
// browser mode keeps the explicit Workspace button. Chat is one ordinary tab.
async function ensureWorkspace(toggleExisting = false) {
  if (desktop) {
    if (toggleExisting) desktop.toggle();
    return desktop;
  }
  const epoch = viewEpoch;
  const config = await api('/api/workspace');
  if (epoch !== viewEpoch || !state) return null;
  if (!config.enabled) {
    message(config.hint || 'Workspace not granted by owner.');
    return null;
  }
  const module = await import('/workspace.js');
  if (epoch !== viewEpoch || !state) return null;
  desktop = module.mountWorkspace({
    chat: $('workspace'),
    config,
    localSingleUser,
    request: async (path, body) => {
      try {
        const next = await api(path, body);
        if (epoch !== viewEpoch || !state) throw new Error('Workspace no longer authorized');
        return next;
      } catch (error) {
        if (epoch === viewEpoch && error.status === 401 && !localSingleUser) {
          lockUI('Pairing expired; restart owner to pair again.');
        }
        throw error;
      }
    },
    getSnapshot: () => state,
    feedback: text => {
      const el = $('prompt');
      el.value += (el.value ? '\n\n' : '') + text;
      controls();
    },
    openSession: session_id => changeView('/api/session/open', {session_id, branch:'main'}),
    settings: () => ensureSettings(true),
    lock: () => {
      if (!localSingleUser) $('lock').click();
    },
  });
  return desktop;
}

$('open-workspace').addEventListener('click', () => {
  ensureWorkspace(true).catch(error => message(error.message));
});

$('context-flow').onclick=async()=>{try {
  if(!contextFlow){const module=await import('/context-flow.js');contextFlow=module.createContextFlow({api,getSnapshot:()=>state,getChoice:()=>chosenAccount||accountChoices(state)[0],refresh:refreshSnapshot,
    guardTarget:()=>!$('prompt').value.trim()||confirm('The composer contains unsent text. Applying may change branch context; keep it for explicit review rather than sending automatically?')});}
  await contextFlow.open();
}catch(e){message(e.message);}};
