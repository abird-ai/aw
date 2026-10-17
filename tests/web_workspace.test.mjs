import test from 'node:test';import assert from 'node:assert/strict';
import {initialLayout,reduceLayout,validateLayout,groupRects,changeRows,sessionTarget,sessionMetrics,KINDS} from '../web/workspace-model.js';
import { createWorkspaceViews, checkedWorkspaceSequence, WORKSPACE_VIEW_LIMITS } from '../web/src/workspace-views.js';
import { createFileDocking, takeFileDockTicket } from '../web/src/file-docking.js';
const open=(s,kind,resource)=>reduceLayout(s,{type:'open',target:{kind,title:kind,resource}});
test('one conversation without network dependencies',()=>{const s=initialLayout();assert.equal(Object.keys(s.views).length,1);assert.equal(s.views.v1.kind,'conversation');});
for(const kind of KINDS)test(`group accepts ${kind}`,()=>{const s=open(initialLayout(),kind,kind);validateLayout(s);assert(Object.values(s.views).some(v=>v.kind===kind));});
test('moving keeps view target and identity',()=>{let s=open(initialLayout(),'terminal','pty-a');const v=s.groups.g1.active;s=reduceLayout(s,{type:'split'});s=reduceLayout(s,{type:'move',view:v});assert.equal(s.groups[s.focused].active,v);assert.equal(s.views[v].resource,'pty-a');});
test('invalid move preserves input',()=>{const s=initialLayout(),copy=JSON.stringify(s);assert.throws(()=>reduceLayout(s,{type:'move',view:'fake'}));assert.equal(JSON.stringify(s),copy);});
test('view reopening does not duplicate underlying resource',()=>{let s=open(initialLayout(),'terminal','pty');s=reduceLayout(s,{type:'split'});s=open(s,'terminal','pty');assert.equal(Object.values(s.views).filter(v=>v.kind==='terminal').length,1);});
test('explicit duplicate has separate view same resource',()=>{let s=open(initialLayout(),'terminal','pty');s=reduceLayout(s,{type:'open',target:{kind:'terminal',title:'terminal',resource:'pty'},duplicate:true});assert.equal(Object.values(s.views).filter(v=>v.kind==='terminal').length,2);});
test('close group moves tabs not destroys resources',()=>{let s=reduceLayout(initialLayout(),{type:'split'});s=open(s,'site','site-a');const id=s.groups[s.focused].active;s=reduceLayout(s,{type:'close_group'});assert.equal(s.views[id].resource,'site-a');assert.equal(Object.keys(s.groups).length,1);});
test('last group cannot close',()=>assert.throws(()=>reduceLayout(initialLayout(),{type:'close_group'})));
test('small viewport retains desired split',()=>{const s=reduceLayout(initialLayout(),{type:'split'}),before=JSON.stringify(s);assert.equal(groupRects(s,400,300).collapsed,true);assert.equal(JSON.stringify(s),before);assert.equal(Object.keys(groupRects(s,1200,600).groups).length,2);});
test('navigation not a fourth worktree family',()=>assert.throws(()=>reduceLayout(initialLayout(),{type:'navigator',kind:'worktrees'})));
test('all left or right is view geometry only',()=>{let s=reduceLayout(initialLayout(),{type:'side',side:'right'});assert.equal(s.side,'right');assert.equal(s.views.v1.kind,'conversation');});
test('same file in staged and unstaged stays separate',()=>{const rows=changeRows([{id:'1',group:'Staged',path:'src/a',label:'src/a'},{id:'2',group:'Unstaged',path:'src/a',label:'src/a'}],true);assert.equal(rows.filter(v=>v.kind==='item').length,2);});
test('fold affects only its change group',()=>{const rows=changeRows([{id:'1',group:'Staged',path:'src/a',label:'src/a'},{id:'2',group:'Unstaged',path:'src/a',label:'src/a'}],true,'',new Set(['Staged/src']));assert.deepEqual(rows.filter(v=>v.kind==='item').map(v=>v.id),['2']);});
test('filter leaves no empty invented groups',()=>assert.equal(changeRows([{group:'Staged',path:'a'}],false,'z').length,0));
test('no hidden context on model or tab switch',()=>{const s={instance:'i',saved:{session_id:'s',branch_id:'b'}};assert.equal(sessionTarget(s),sessionTarget({...s,model:'other'}));assert.notEqual(sessionTarget(s),sessionTarget({...s,instance:'new'}));});
test('metrics are observed counts not cost estimates',()=>{const m=sessionMetrics({turns:[{outcome:'completed',invocations:[{}]},{outcome:'failed'}],pending:{}});assert.equal(m.completed,1);assert.equal(m.active,1);assert.equal(m.observed_invocations,1);assert.equal(m.dollars,undefined);});
test('limit groups',()=>{let s=initialLayout();for(let i=0;i<3;i++)s=reduceLayout(s,{type:'split'});assert.throws(()=>reduceLayout(s,{type:'split'}));});
test('forged descriptor is not supported',()=>assert.throws(()=>open(initialLayout(),'raw-script','x')));
test('invalid ratios rejected',()=>{const s=initialLayout();s.root={axis:'columns',ratio:NaN,first:{group:'g1'},second:{group:'g1'}};assert.throws(()=>validateLayout(s));});
test('reorder and selection are independent',()=>{let s=open(initialLayout(),'logs','logs');const active=s.groups.g1.active;s=reduceLayout(s,{type:'reorder',view:active,index:0});assert.equal(s.groups.g1.tabs[0],active);assert.equal(s.groups.g1.active,active);});
test('finite view cap',()=>{let s=initialLayout();for(let i=0;i<23;i++)s=open(s,'file',String(i));assert.throws(()=>open(s,'file','overflow'));});

// Shipping Solid controller. The historical reducer cases above remain intact.
const wsSource = (source_id = 'surface-owner') => ({ source_id, view_root: { id: 'launch-scope', label: '/fixture/launch' } });
function wsFixture() {
  const calls = [], events = [];
  let external = 1;
  let subscriber = () => {};
  const state = createWorkspaceViews({ externalDocumentCount: () => external, onChange: event => { events.push(event); subscriber(event); },
    request(body, options) {
      let resolve, reject;
      const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
      calls.push({ body, ...options, resolve, reject });
      return promise;
    } });
  state.setSource(wsSource());
  return { state, calls, events, external(value) { external = value; }, observe(value) { subscriber = value; } };
}
const wsReply = (call, result) => ({ source_id: call.body.source_id, scope_id: call.body.scope_id, result });
const wsFile = (call, value = 'file text 雪') => ({ context: call.body.scope_id, path: call.body.request.path, text: value, read_only: true });
const wsList = call => ({ context: call.body.scope_id, path: call.body.request.path, entries: [
  { name: 'lib.rs', path: 'src/lib.rs', kind: 'file' },
], incomplete: false });
const wsReview = (review_id = 'review-original', patch = {}) => ({ kind: 'review', id: review_id,
  root: '/fixture/launch', path: 'src/lib.rs', basis: 'immutable original',
  lines: [{ kind: 'context', text: 'original review 雪' }], binary: false, truncated: false,
  same_agent_root: true, read_only: true, ...patch });
async function wsSettleFile(fixture, documentId, value) {
  const call = fixture.calls.at(-1);
  call.resolve(wsReply(call, wsFile(call, value)));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(fixture.state.getDocument(documentId).pending, false);
  return fixture.state.getDocument(documentId).data;
}

test('shipping workspace capabilities admit only launch Files and opaque known Git scopes', async () => {
  const f = wsFixture(), files = f.state.openContainer('files'), git = f.state.openContainer('git');
  assert.notEqual(files, git);
  assert.equal(f.state.getContainer(files).scope_id, 'launch-scope');
  assert.throws(() => f.state.openContainer('files', '/tmp/forged'), /authorized/);
  assert.throws(() => f.state.openContainer('git', 'unknown'), /authorized/);
  const selecting = f.state.selectWorktree(git, { inventory_id: 'inventory-a', item_id: 'worktree-a' });
  const call = f.calls.at(-1);
  assert.deepEqual(call.body, { op: 'view', source_id: 'surface-owner', scope_id: 'launch-scope',
    request: { op: 'git_open', inventory_id: 'inventory-a', item_id: 'worktree-a' } });
  call.resolve(wsReply(call, { kind: 'context', context: { id: 'checkout-b', label: '/fixture/other' }, agent_unchanged: true }));
  assert.equal(await selecting, true);
  assert.equal(f.state.getContainer(git).scope_id, 'checkout-b');
  assert.equal(f.state.getContainer(files).scope_id, 'launch-scope');
  assert.throws(() => f.state.openContainer('files', 'checkout-b'), /authorized/);
  assert.equal(f.state.getContainer(f.state.openContainer('git', 'checkout-b')).scope_id, 'checkout-b');
});

test('pending File allocation is atomic, reused in its container and independent elsewhere', async () => {
  const f = wsFixture(), first = f.state.openContainer('files'), other = f.state.openContainer('files');
  const a = f.state.openFile(first, 'src/lib.rs'), b = f.state.openFile(other, 'src/lib.rs');
  assert.equal(f.state.openFile(first, 'src/lib.rs'), a);
  assert.equal(f.calls.length, 2);
  assert.notEqual(a, b);
  assert.equal(f.state.getDocument(a).containerId, first);
  assert.equal(f.state.getDocument(a).pending, true);
  assert.deepEqual(f.state.getDocument(a).target, { source_id: 'surface-owner', scope_id: 'launch-scope', path: 'src/lib.rs' });
  f.calls[1].resolve(wsReply(f.calls[1], wsFile(f.calls[1], 'other view')));
  f.calls[0].resolve(wsReply(f.calls[0], wsFile(f.calls[0], 'first view')));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getDocument(a).data.text, 'first view');
  assert.equal(f.state.getDocument(b).data.text, 'other view');
  assert.equal(f.events.some(event => event.type === 'activate'), false);
  assert.throws(() => { f.state.getDocument(a).target.path = 'changed'; }, TypeError);
});

test('same-window File reparenting retains exact identity, pending read and cache after origin closes', async () => {
  const f = wsFixture(), first = f.state.openContainer('files'), second = f.state.openContainer('files');
  const id = f.state.openFile(first, 'src/雪.rs'), read = f.calls.at(-1);
  const original = f.state.getDocument(id).target;
  assert.equal(f.state.moveDocument(id, first), false, 'same destination is a no-op');
  assert.equal(f.state.moveDocument(id, second), true);
  assert.equal(f.state.getDocument(id).containerId, second);
  assert.equal(f.state.getDocument(id).target, original, 'no new target authority');
  assert.equal(f.calls.length, 1, 'move never starts a second request');
  assert.equal(f.state.closeContainer(first), true);
  assert.equal(read.signal.aborted, false, 'origin close must not cancel a reparented view');
  read.resolve(wsReply(read, wsFile(read, 'retained pending read')));
  await new Promise(resolve => setImmediate(resolve));
  const result = f.state.getDocument(id).data;
  assert.equal(result.text, 'retained pending read');
  assert.equal(f.state.listDocuments().length, 1);
  assert.equal(f.state.openFile(second, 'src/雪.rs'), id, 'reparent remains canonical for that view');
  assert.equal(f.state.getDocument(id).data, result);
  assert.equal(f.state.closeContainer(second), true);
  assert.equal(f.state.getDocument(id), null);
});

test('document reparenting refuses collisions, foreign kinds and closed lifetimes atomically', async () => {
  const f = wsFixture(), a = f.state.openContainer('files'), b = f.state.openContainer('files');
  const git = f.state.openContainer('git');
  const first = f.state.openFile(a, 'src/a.rs'), second = f.state.openFile(b, 'src/a.rs');
  await wsSettleFile(f, second, 'destination content');
  const original = f.state.getDocument(first), before = f.state.listDocuments().length;
  assert.throws(() => f.state.moveDocument(first, git), /matching view/);
  assert.throws(() => f.state.moveDocument(first, b), /already open/);
  assert.throws(() => f.state.moveDocument('foreign-document', b), /no longer open/);
  assert.throws(() => f.state.moveDocument(first, 'forged-container'), /closed/);
  assert.equal(f.state.getDocument(first), original);
  assert.equal(f.state.listDocuments().length, before);
  assert.equal(f.calls.length, 2, 'rejected moves cannot dispatch new reads');
  assert.equal(f.state.closeDocument(second), true);
  assert.equal(f.state.moveDocument(first, b), true);
  const held = f.calls[0];
  f.state.closeContainer(b);
  assert.equal(held.signal.aborted, true);
  held.resolve(wsReply(held, wsFile(held, 'late redirected disclosure')));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getDocument(first), null);
  assert.equal(f.state.getContainer(a).kind, 'files');
});

test('moved Git review retains original checkout even when destination navigates elsewhere', async () => {
  const f = wsFixture(), a = f.state.openContainer('git'), b = f.state.openContainer('git');
  const alternate = f.state.selectWorktree(b, { inventory_id: 'worktrees', item_id: 'other' }), chosen = f.calls.at(-1);
  chosen.resolve(wsReply(chosen, { kind: 'context', context: { id: 'different-checkout', label: '/other' }, agent_unchanged: true }));
  assert.equal(await alternate, true);
  const id = f.state.openReview(a, { inventory_id: 'original-inventory', item_id: 'changed-file' });
  const held = f.calls.at(-1), original = f.state.getDocument(id).target;
  assert.equal(f.state.moveDocument(id, b), true);
  assert.equal(f.state.getDocument(id).target, original);
  assert.equal(f.state.getContainer(b).scope_id, 'different-checkout');
  f.state.closeContainer(a);
  held.resolve(wsReply(held, wsReview('immutable-from-launch')));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getDocument(id).target.scope_id, 'launch-scope');
  assert.equal(f.state.getDocument(id).target.review_id, 'immutable-from-launch');
  const refreshed = f.state.refreshDocument(id), reload = f.calls.at(-1);
  assert.equal(reload.body.scope_id, 'launch-scope');
  assert.deepEqual(reload.body.request, { op: 'review', review_id: 'immutable-from-launch' });
  reload.resolve(wsReply(reload, wsReview('immutable-from-launch')));
  assert.equal(await refreshed, true);
  assert.equal(f.state.getDocument(id).containerId, b);
  assert.equal(f.state.getDocument(id).data.id, 'immutable-from-launch');
  assert.equal(f.events.some(event => event.type === 'activate'), false);
});

test('combined 32 document admission counts Agent/pins and preserves a pending duplicate at capacity', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  f.external(31);
  const id = f.state.openFile(owner, 'a.rs');
  assert.equal(f.state.openFile(owner, 'a.rs'), id);
  assert.throws(() => f.state.openFile(owner, 'b.rs'), /32/);
  assert.throws(() => f.state.assertCanOpenDocument(), /32/);
  assert.equal(f.state.listDocuments().length, 1);
  assert.equal(f.calls.length, 1);
  await wsSettleFile(f, id);
  f.external(30);
  assert.equal(f.state.assertCanOpenDocument(), true);
  f.external(Number.NaN);
  assert.throws(() => f.state.openFile(owner, 'c.rs'), /count/);
  assert.equal(f.state.listDocuments().length, 1);
});

test('container count and navigator metadata have independent bounded immutable state', () => {
  const f = wsFixture(), a = f.state.openContainer('files'), b = f.state.openContainer('git');
  f.state.patchNavigator(a, { query: '雪', view: 'list', expanded: ['', 'src'], selected: 'src/lib.rs' });
  f.state.patchNavigator(b, { mode: 'branches', collapsed: ['branch/group'] });
  assert.equal(f.state.getContainer(a).navigator.query, '雪');
  assert.equal(f.state.getContainer(b).navigator.query, '');
  assert.equal(f.state.getContainer(a).navigator.mode, 'changes');
  assert.equal(f.state.getContainer(b).navigator.mode, 'branches');
  const before = f.state.getContainer(a);
  for (const patch of [{ root: '/forged' }, { query: 'x'.repeat(257) }, { expanded: Array(257).fill('x') },
    { collapsed: ['same', 'same'] }, { selected: 'bad\nname' }, { mode: 'checkout' },
    { expanded: Array.from({ length: 16 }, (_, i) => `${i}${'x'.repeat(8190)}`) }]) {
    assert.throws(() => f.state.patchNavigator(a, patch));
    assert.equal(f.state.getContainer(a), before);
  }
  assert.throws(() => before.navigator.expanded.push('changed'), TypeError);
  for (let index = 2; index < 8; index++) f.state.openContainer('files');
  assert.throws(() => f.state.openContainer('files'), /8/);
  assert.equal(f.state.listContainers().length, WORKSPACE_VIEW_LIMITS.containers);
});

test('close/reopen real deferred replies cannot populate a reused resource lifetime', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const first = f.state.openFile(owner, 'same.rs'), held = f.calls[0];
  f.state.closeDocument(first);
  const next = f.state.openFile(owner, 'same.rs');
  assert.notEqual(first, next);
  assert.equal(held.signal.aborted, true);
  f.calls[1].resolve(wsReply(f.calls[1], wsFile(f.calls[1], 'fresh')));
  held.resolve(wsReply(held, wsFile(held, 'obsolete')));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getDocument(first), null);
  assert.equal(f.state.getDocument(next).data.text, 'fresh');
});

test('explicit document refresh fences the held earlier reply and preserves equal snapshot identity', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files'), id = f.state.openFile(owner, 'a.rs');
  const first = await wsSettleFile(f, id, 'stable');
  const heldRead = f.state.refreshDocument(id), held = f.calls.at(-1);
  const refresh = f.state.refreshDocument(id), fresh = f.calls.at(-1);
  assert.equal(held.signal.aborted, true);
  fresh.resolve(wsReply(fresh, wsFile(fresh, 'stable')));
  assert.equal(await refresh, true);
  assert.equal(f.state.getDocument(id).data, first);
  held.resolve(wsReply(held, wsFile(held, 'old')));
  assert.equal(await heldRead, false);
  assert.equal(f.state.getDocument(id).data, first);
  const changed = f.state.refreshDocument(id), changedCall = f.calls.at(-1);
  changedCall.resolve(wsReply(changedCall, wsFile(changedCall, 'changed')));
  assert.equal(await changed, true);
  assert.notEqual(f.state.getDocument(id).data, first);
});

test('physical request slots remain bounded until ignored abort Promises actually settle', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const ids = Array.from({ length: 4 }, (_, index) => f.state.openFile(owner, `${index}.rs`));
  const waiting = f.state.openFile(owner, 'waiting.rs');
  assert.equal(f.calls.length, 4);
  assert.match(f.state.getDocument(waiting).error, /busy/);
  ids.forEach(id => f.state.closeDocument(id));
  assert.equal(f.calls.every(call => call.signal.aborted), true);
  assert.equal(await f.state.refreshDocument(waiting), false);
  assert.equal(f.calls.length, 4);
  f.calls.forEach(call => call.resolve(wsReply(call, wsFile(call, 'obsolete'))));
  await new Promise(resolve => setImmediate(resolve));
  const retry = f.state.refreshDocument(waiting), call = f.calls.at(-1);
  assert.equal(f.calls.length, 5);
  call.resolve(wsReply(call, wsFile(call, 'admitted later')));
  assert.equal(await retry, true);
  assert.equal(f.state.getDocument(waiting).data.text, 'admitted later');
});

test('close container scrubs all its cache/metadata before any synchronous abort callback', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const a = f.state.openFile(owner, 'a.rs'); await wsSettleFile(f, a, 'protected A');
  const b = f.state.openFile(owner, 'b.rs'); await wsSettleFile(f, b, 'protected B');
  f.state.patchNavigator(owner, { query: 'private query', expanded: ['', 'private/path'] });
  const read = f.state.refreshDocument(a), held = f.calls.at(-1);
  let rendered = f.state.listDocuments().map(document => document.data);
  f.observe(() => { rendered = f.state.listDocuments().map(document => document.data); });
  let atAbort;
  held.signal.addEventListener('abort', () => { atAbort = {
    container: f.state.getContainer(owner), documents: f.state.listDocuments(), rendered,
  }; });
  assert.equal(f.state.closeContainer(owner), true);
  assert.equal(atAbort.container, null);
  assert.deepEqual(atAbort.documents, []);
  assert.deepEqual(atAbort.rendered, [], 'UI subscriber must clear before synchronous transport abort callbacks');
  held.resolve(wsReply(held, wsFile(held, 'late')));
  assert.equal(await read, false);
});

test('source replacement clears same-named scopes, pending data and metadata before abort callbacks', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files'), id = f.state.openFile(owner, 'a.rs');
  await wsSettleFile(f, id, 'protected');
  const reading = f.state.refreshDocument(id), held = f.calls.at(-1);
  let atAbort;
  held.signal.addEventListener('abort', () => { atAbort = [f.state.listContainers().length, f.state.listDocuments().length]; });
  f.state.setSource(wsSource('replacement-owner'));
  assert.deepEqual(atAbort, [0, 0]);
  const freshOwner = f.state.openContainer('files'), fresh = f.state.openFile(freshOwner, 'a.rs');
  assert.notEqual(freshOwner, owner);
  assert.notEqual(fresh, id);
  assert.equal(f.calls.at(-1).body.source_id, 'replacement-owner');
  held.resolve(wsReply(held, wsFile(held, 'old protected')));
  assert.equal(await reading, false);
  assert.equal(f.state.getDocument(fresh).data, null);
  await wsSettleFile(f, fresh, 'new source');
});

test('typed scope denial scrubs all matching scope data and derived labels before abort, preserving another scope', async () => {
  const f = wsFixture(), files = f.state.openContainer('files'), git = f.state.openContainer('git');
  const selecting = f.state.selectWorktree(git, { inventory_id: 'i', item_id: 'w' }), select = f.calls.at(-1);
  select.resolve(wsReply(select, { kind: 'context', context: { id: 'other-scope', label: '/other' }, agent_unchanged: true }));
  await selecting;
  const other = f.state.openReview(git, { inventory_id: 'j', item_id: 'd' }), review = f.calls.at(-1);
  review.resolve(wsReply(review, wsReview())); await new Promise(resolve => setImmediate(resolve));
  const preserved = f.state.getDocument(other).data;
  const a = f.state.openFile(files, 'a.rs'); await wsSettleFile(f, a, 'private');
  const b = f.state.openFile(files, 'b.rs'); await wsSettleFile(f, b, 'private two');
  f.state.patchNavigator(files, { query: 'private query', expanded: ['private'] });
  const heldRead = f.state.refreshDocument(a), held = f.calls.at(-1);
  let rendered = f.state.listDocuments().filter(document => document.target.scope_id === 'launch-scope').map(document => document.data);
  f.observe(() => { rendered = f.state.listDocuments().filter(document => document.target.scope_id === 'launch-scope').map(document => document.data); });
  let scrubbedAtAbort = false;
  held.signal.addEventListener('abort', () => { scrubbedAtAbort = !f.state.getDocument(a).data && !f.state.getDocument(b).data
    && rendered.every(value => value === null) && f.state.getContainer(files).navigator.query === '' && f.state.getContainer(files).label === 'Unavailable scope'; });
  const denied = f.state.refreshDocument(b), call = f.calls.at(-1);
  call.reject(Object.assign(new Error('scope revoked'), { status: 403, code: 'scope_denied', source_id: 'surface-owner', scope_id: 'launch-scope' }));
  assert.equal(await denied, false);
  assert.equal(scrubbedAtAbort, true);
  assert.equal(f.state.getDocument(a).unavailable, true);
  assert.equal(f.state.getDocument(other).data, preserved);
  assert.throws(() => f.state.openContainer('files'), /unavailable/);
  held.resolve(wsReply(held, wsFile(held, 'obsolete')));
  assert.equal(await heldRead, false);
  const retry = f.state.refreshDocument(a), fresh = f.calls.at(-1);
  fresh.resolve(wsReply(fresh, wsFile(fresh, 'explicitly authorized')));
  assert.equal(await retry, true);
  assert.equal(f.state.getDocument(a).unavailable, false);
});

for (const error of [{ status: 401 }, { status: 403, code: 'source_changed', source_id: 'surface-owner', scope_id: 'launch-scope' }]) {
  test(`auth/source denial ${error.status}/${error.code || 'auth'} closes all views without adopting error echoes`, async () => {
    const f = wsFixture(), owner = f.state.openContainer('files'), id = f.state.openFile(owner, 'a.rs');
    f.calls[0].reject(Object.assign(new Error('denied'), error));
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(f.state.getDocument(id), null);
    assert.deepEqual(f.state.listContainers(), []);
    assert.throws(() => f.state.openContainer('files'), /source unavailable/);
  });
}

test('navigator search/query and explicit refresh fence held old listings, searches and palette results', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const listing = f.state.loadNavigator(owner, { op: 'files', path: '' }), listCall = f.calls.at(-1);
  const searching = f.state.loadNavigator(owner, { op: 'file_search', query: 'old' }), searchCall = f.calls.at(-1);
  f.state.patchNavigator(owner, { query: 'new' });
  assert.equal(searchCall.signal.aborted, true);
  const control = new AbortController();
  const palette = f.state.searchFiles(owner, 'palette', 80, { signal: control.signal }), paletteCall = f.calls.at(-1);
  const refreshed = f.state.refreshNavigator(owner), fresh = f.calls.at(-1);
  assert.equal(listCall.signal.aborted, true);
  assert.equal(paletteCall.signal.aborted, true);
  fresh.resolve(wsReply(fresh, wsList(fresh)));
  await new Promise(resolve => setImmediate(resolve));
  const freshSearch = f.calls.at(-1);
  assert.equal(freshSearch.body.request.query, 'new');
  freshSearch.resolve(wsReply(freshSearch, { context: 'launch-scope', query: 'new', entries: [] }));
  assert.equal(await refreshed, true);
  listCall.resolve(wsReply(listCall, wsList(listCall)));
  searchCall.resolve(wsReply(searchCall, { context: 'launch-scope', query: 'old', entries: [] }));
  paletteCall.resolve(wsReply(paletteCall, { context: 'launch-scope', query: 'palette', entries: [] }));
  assert.equal(await listing, false); assert.equal(await searching, false); assert.equal(await palette, null);
  assert.equal(f.state.getContainer(owner).search.query, 'new');
  assert.equal(f.state.getContainer(owner).listings[''].entries[0].path, 'src/lib.rs');
});

test('foreign palette search is cancellable, uses shared physical slots and never changes navigator search', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  f.state.patchNavigator(owner, { query: 'navigator' });
  const control = new AbortController(), pending = f.state.searchFiles(owner, 'palette', 80, { signal: control.signal });
  const held = f.calls.at(-1);
  control.abort();
  assert.equal(held.signal.aborted, true);
  assert.equal(f.state.getContainer(owner).navigator.query, 'navigator');
  held.resolve(wsReply(held, { context: 'launch-scope', query: 'palette', entries: [] }));
  assert.equal(await pending, null);
  assert.equal(f.state.getContainer(owner).search, null);
  const aborted = new AbortController(); aborted.abort();
  assert.equal(await f.state.searchFiles(owner, 'already aborted', 80, { signal: aborted.signal }), null);
  assert.equal(f.calls.length, 1);
});

test('worktree selection changes only its navigator while in-flight immutable reviews keep original scope', async () => {
  const f = wsFixture(), a = f.state.openContainer('git'), b = f.state.openContainer('git');
  const doc = f.state.openReview(a, { inventory_id: 'original-inventory', item_id: 'change' }), read = f.calls.at(-1);
  const selecting = f.state.selectWorktree(a, { inventory_id: 'worktrees', item_id: 'checkout' }), selection = f.calls.at(-1);
  selection.resolve(wsReply(selection, { kind: 'context', context: { id: 'other-checkout', label: '/other' }, agent_unchanged: true }));
  assert.equal(await selecting, true);
  read.resolve(wsReply(read, wsReview('original-review')));
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getContainer(a).scope_id, 'other-checkout');
  assert.equal(f.state.getContainer(b).scope_id, 'launch-scope');
  assert.deepEqual(f.state.getDocument(doc).target, { source_id: 'surface-owner', scope_id: 'launch-scope', review_id: 'original-review' });
  const refreshed = f.state.refreshDocument(doc), fresh = f.calls.at(-1);
  assert.equal(fresh.body.scope_id, 'launch-scope');
  assert.deepEqual(fresh.body.request, { op: 'review', review_id: 'original-review' });
  fresh.resolve(wsReply(fresh, wsReview('original-review')));
  assert.equal(await refreshed, true);
});

test('wrong source/scope/review replies and expired inventory errors never silently retarget a view', async () => {
  const f = wsFixture(), owner = f.state.openContainer('git');
  const doc = f.state.openReview(owner, { inventory_id: 'i', item_id: 'd' }), call = f.calls.at(-1);
  call.resolve({ source_id: 'foreign', scope_id: 'launch-scope', result: wsReview() });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.state.getDocument(doc).data, null);
  assert.match(f.state.getDocument(doc).error, /exact source/);
  const retry = f.state.refreshDocument(doc), retried = f.calls.at(-1);
  retried.resolve(wsReply(retried, wsReview('exact-review'))); await retry;
  const original = f.state.getDocument(doc).data;
  const wrong = f.state.refreshDocument(doc), other = f.calls.at(-1);
  other.resolve(wsReply(other, wsReview('wrong-review')));
  assert.equal(await wrong, false);
  assert.equal(f.state.getDocument(doc).data, original);
  assert.equal(f.state.getDocument(doc).target.review_id, 'exact-review');
  const expired = f.state.refreshDocument(doc), gone = f.calls.at(-1);
  gone.reject(Object.assign(new Error('Refresh the inventory explicitly'), { status: 409, code: 'refresh_required' }));
  assert.equal(await expired, false);
  assert.equal(f.state.getDocument(doc).data, original);
  assert.match(f.state.getDocument(doc).error, /explicitly/);
  assert.throws(() => f.state.openReview(owner, { op: 'review', review_id: 'forged' }), /inventory target/);
});

test('cache limits reject new data without evicting authorized visible snapshots', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const large = 'x'.repeat(3 * 1024 * 1024);
  const first = f.state.openFile(owner, 'a.rs'); const a = await wsSettleFile(f, first, large);
  const second = f.state.openFile(owner, 'b.rs'); const b = await wsSettleFile(f, second, large);
  const overflow = f.state.openFile(owner, 'c.rs'); await wsSettleFile(f, overflow, large);
  assert.equal(f.state.getDocument(overflow).data, null);
  assert.match(f.state.getDocument(overflow).error, /8 MiB/);
  assert.equal(f.state.getDocument(first).data, a); assert.equal(f.state.getDocument(second).data, b);
  const oversized = f.state.openFile(owner, 'oversized.rs'); await wsSettleFile(f, oversized, 'x'.repeat(WORKSPACE_VIEW_LIMITS.replyBytes));
  assert.equal(f.state.getDocument(oversized).data, null);
  assert.match(f.state.getDocument(oversized).error, /4 MiB/);
  f.state.closeDocument(first);
  const retry = f.state.refreshDocument(overflow), call = f.calls.at(-1);
  call.resolve(wsReply(call, wsFile(call, large)));
  assert.equal(await retry, true);
  assert.equal(f.state.getDocument(second).data, b);
});

test('directory caching is bounded without evicting earlier authorized directories', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  for (let index = 0; index < WORKSPACE_VIEW_LIMITS.directories + 1; index++) {
    const pending = f.state.loadNavigator(owner, { op: 'files', path: `dir${index}` }), call = f.calls.at(-1);
    call.resolve(wsReply(call, wsList(call)));
    assert.equal(await pending, index < WORKSPACE_VIEW_LIMITS.directories);
  }
  assert.equal(Object.keys(f.state.getContainer(owner).listings).length, WORKSPACE_VIEW_LIMITS.directories);
  assert.equal(f.state.getContainer(owner).listings.dir0.path, 'dir0');
  assert.match(f.state.getContainer(owner).error, /Directory cache/);
});

test('workspace identifiers use checked monotonic safe integers and disposal admits no more work', () => {
  assert.equal(checkedWorkspaceSequence(Number.MAX_SAFE_INTEGER - 1), Number.MAX_SAFE_INTEGER);
  for (const value of [-1, 1.5, Number.NaN, Number.MAX_SAFE_INTEGER]) assert.throws(() => checkedWorkspaceSequence(value), /exhausted/);
  const f = wsFixture(), owner = f.state.openContainer('files');
  for (const path of ['/root', '../escape', 'a/../b', 'a//b', 'bad\\name', 'bad\nname']) assert.throws(() => f.state.openFile(owner, path), /relative path/);
  assert.equal(f.calls.length, 0);
  f.state.dispose();
  assert.throws(() => f.state.setSource(wsSource()), /Invalid/);
  assert.throws(() => f.state.openContainer('files'), /unavailable/);
});

test('ordinary explicit refresh preserves Git mode/view and Files query/tree metadata coherently', async () => {
  const f = wsFixture(), git = f.state.openContainer('git'), files = f.state.openContainer('files');
  f.state.patchNavigator(git, { mode: 'worktrees', view: 'list', query: 'checkout', collapsed: ['group'], selected: 'selected' });
  const gitBefore = f.state.getContainer(git).navigator;
  const gitRefresh = f.state.refreshNavigator(git), call = f.calls.at(-1);
  assert.equal(call.body.request.mode, 'worktrees');
  assert.equal(f.state.getContainer(git).navigator, gitBefore);
  call.resolve(wsReply(call, { inventory_id: 'worktree-inventory', context: 'launch-scope', root: '/fixture/launch', mode: 'worktrees', branch: null, head: null, items: [] }));
  assert.equal(await gitRefresh, true);
  assert.equal(f.state.getContainer(git).inventory.mode, f.state.getContainer(git).navigator.mode);
  f.state.patchNavigator(files, { query: 'lib', expanded: ['', 'src'], view: 'list', selected: 'src/lib.rs' });
  const before = f.state.getContainer(files).navigator;
  const fileRefresh = f.state.refreshNavigator(files), listing = f.calls.at(-1);
  assert.equal(f.state.getContainer(files).navigator, before);
  listing.resolve(wsReply(listing, wsList(listing)));
  await new Promise(resolve => setImmediate(resolve));
  const search = f.calls.at(-1);
  assert.deepEqual(search.body.request, { op: 'file_search', query: 'lib', limit: 120 });
  search.resolve(wsReply(search, { context: 'launch-scope', query: 'lib', entries: [] }));
  assert.equal(await fileRefresh, true);
  assert.equal(f.state.getContainer(files).navigator, before);
  assert.equal(f.state.getContainer(files).listings[''].path, '');
  assert.equal(f.state.getContainer(files).search.query, 'lib');
});

test('ordinary resource denial clears that resource while other scope documents and navigator state survive', async () => {
  const f = wsFixture(), owner = f.state.openContainer('files');
  const a = f.state.openFile(owner, 'private.rs'); await wsSettleFile(f, a);
  const b = f.state.openFile(owner, 'other.rs'); const other = await wsSettleFile(f, b, 'still readable');
  f.state.patchNavigator(owner, { query: 'keep', expanded: ['', 'src'], view: 'list' });
  const before = f.state.getContainer(owner).navigator;
  const reading = f.state.refreshDocument(a), denied = f.calls.at(-1);
  denied.reject(Object.assign(new Error('File not readable'), { status: 403, code: 'read_denied', source_id: 'surface-owner', scope_id: 'launch-scope' }));
  assert.equal(await reading, false);
  assert.equal(f.state.getDocument(a).data, null);
  assert.equal(f.state.getDocument(b).data, other);
  assert.equal(f.state.getContainer(owner).navigator, before);
  assert.equal(f.state.getContainer(owner).unavailable, false);
});

test('identity grammar matches backend while readable unsupported Unix colon labels remain listable', async () => {
  const f = wsFixture();
  for (const source_id of ['white space', 'unicode-雪', 'slash/id', 'dot.id', 'newline\n']) {
    assert.throws(() => f.state.setSource(wsSource(source_id)), /Invalid/);
  }
  const owner = f.state.openContainer('files');
  const listing = f.state.loadNavigator(owner, { op: 'files', path: '' }), call = f.calls.at(-1);
  call.resolve(wsReply(call, { context: 'launch-scope', path: '', incomplete: false, entries: [
    { name: 'C:notes', path: 'C:notes', kind: 'file' },
    { name: 'C:symlink', path: 'C:symlink', kind: 'unsupported' },
  ] }));
  assert.equal(await listing, true);
  assert.equal(f.state.getContainer(owner).listings[''].entries[1].kind, 'unsupported');
});

const dockTicket = 'a'.repeat(64);
function dockFixture() {
  const calls = [], notifications = [], owner = { source_id: 'source-dock', scope_id: 'scope-dock', owner_epoch: 'owner-epoch' };
  const target = { source_id: owner.source_id, scope_id: owner.scope_id, path: 'src/hello.rs' };
  let document = { id: 'workspace-document-37', kind: 'file', containerId: 'container-one', target, data: { path: target.path, text: 'PRIVATE_FILE_CONTENT' }, pending: false, unavailable: false, error: '' };
  let registered = 0, statusCalls = 0, accepted = false, released = false;
  const window = { window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64), ...owner };
  const receiver = { ...window, window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64) };
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('register')) return (++registered === 1 ? window : receiver);
    if (path.endsWith('prepare')) return { ticket: dockTicket, state: 'prepared' };
    if (path.endsWith('status')) return { state: ++statusCalls === 1 ? 'prepared' : 'accepted', document_id: document?.id };
    if (path.endsWith('release')) { released = true; return { state: 'released', document_id: document?.id }; }
    if (path.endsWith('cancel')) return { state: 'cancelled' };
    if (path.endsWith('decline')) return { state: 'declined' };
    if (path.endsWith('inspect')) return { kind: 'file', ...owner, path: target.path };
    if (path.endsWith('accept')) { accepted = true; return { state: 'accepted' }; }
    throw Error('unknown mock API');
  };
  const views = {
    getDocument: id => document?.id === id ? document : null,
    openContainer: () => 'destination',
    openFile: (id, path) => {
      assert.equal(id, 'destination'); assert.equal(path, target.path);
      document = { id: 'receiver-file', containerId: 'destination', kind: 'file', target, data: null, pending: true, error: '', unavailable: false };
      return document.id;
    },
    closeDocument: id => { if (document?.id === id) document = null; notifications.push('closed-file'); },
    closeContainer: () => notifications.push('closed-container'),
  };
  const popup = { closed: false, close() { this.closed = true; }, opener: {}, location: { replace() {} } };
  const make = overrides => createFileDocking({
    api, views, currentOwner: () => owner, select: (...args) => notifications.push(['select', ...args]),
    close: id => { notifications.push(['release-document', id]); if (document?.id === id) document = null; },
    popup: () => popup, navigate: (p, ticket) => { assert.equal(p, popup); assert.equal(ticket, dockTicket); notifications.push('navigated'); },
    sleep: async () => {}, onProgress: (id, active) => notifications.push(['progress', id, active]),
    ...overrides,
  });
  return { api, calls, owner, target, window, receiver, views, popup, make, notifications,
    get document() { return document; }, setDocument: value => { document = value; },
    get accepted() { return accepted; }, get released() { return released; },
  };
}

test('docking ticket is removed from fragment before disclosure and never treated as a grant', () => {
  const changes = [], history = { replaceState: (...args) => changes.push(args) };
  assert.equal(takeFileDockTicket({ hash: '#aw-dock=' + dockTicket, pathname: '/', search: '' }, history), dockTicket);
  assert.deepEqual(changes, [[null, '', '/']]);
  assert.equal(takeFileDockTicket({ hash: '#aw-dock=broken', pathname: '/app', search: '?v=1' }, history), null);
  assert.deepEqual(changes.at(-1), [null, '', '/app?v=1']);
  assert.equal(takeFileDockTicket({ hash: '#unrelated' }, history), null);
  assert.equal(changes.length, 2);
});

test('owner-approved File handoff releases only after distinct receiver acceptance', async () => {
  const f = dockFixture();
  const result = await f.make().send('workspace-document-37');
  assert.equal(result, true);
  assert.equal(f.released, true);
  assert.equal(f.document, null);
  assert.deepEqual(f.notifications.slice(0, 2), [['progress', 'workspace-document-37', true], 'navigated']);
  assert.deepEqual(f.notifications.slice(-2), [['release-document', 'workspace-document-37'], ['progress', 'workspace-document-37', false]]);
  assert.deepEqual(f.calls.map(call => call.path), [
    '/api/dock/register', '/api/dock/prepare', '/api/dock/status', '/api/dock/status', '/api/dock/release',
  ]);
  const prepared = f.calls.find(call => call.path.endsWith('prepare')).body;
  assert.deepEqual(Object.keys(prepared).sort(), ['document_id', 'owner_epoch', 'path', 'scope_id', 'source_id', 'window_id', 'window_proof']);
  assert.equal(prepared.path, 'src/hello.rs');
  assert.ok(f.calls.every(call => !JSON.stringify(call).includes('PRIVATE_FILE_CONTENT')));
});

test('cancelled or blocked File popout keeps original view and cannot create authority', async () => {
  const f = dockFixture();
  await assert.rejects(f.make({ popup: () => null }).send('workspace-document-37'), /blocked/);
  assert.equal(f.calls.length, 0);
  const cancelled = f.make({ navigate: () => { f.popup.closed = true; } });
  await assert.rejects(cancelled.send('workspace-document-37'), /not accepted/);
  assert.equal(f.document.id, 'workspace-document-37');
  assert.equal(f.calls.at(-1).path, '/api/dock/cancel');
  assert.equal(f.released, false);
});

test('receiver rereads the original capability before acceptance without receiving plaintext in ticket', async () => {
  const f = dockFixture();
  // The receiver registered in its own window, independently of origin.
  const file = f.make({
    isRendered: () => Boolean(f.document?.data),
    sleep: async () => { if (f.document?.pending) f.setDocument({ ...f.document, pending: false, data: { path: f.target.path, text: 'AUTHORIZED_RECEIVER_BYTES' } }); },
  });
  const result = await file.receive(dockTicket);
  assert.deepEqual(result, { containerId: 'destination', documentId: 'receiver-file' });
  assert.equal(f.accepted, true);
  assert.equal(f.document.data.text, 'AUTHORIZED_RECEIVER_BYTES');
  assert.deepEqual(f.calls.map(call => call.path), ['/api/dock/register', '/api/dock/inspect', '/api/dock/accept']);
  assert.equal(f.notifications.filter(event => event === 'closed-file').length, 0);
});

test('denied receiver and uncertain acceptance cannot be converted into a successful transfer', async () => {
  const denied = dockFixture();
  const receiver = denied.make({ sleep: async () => denied.setDocument({ ...denied.document, pending: false, error: 'scope_denied' }) });
  await assert.rejects(receiver.receive(dockTicket), /failed authorization/);
  assert.ok(denied.notifications.includes('closed-file'));
  assert.ok(denied.notifications.includes('closed-container'));
  assert.equal(denied.calls.some(call => call.path.endsWith('accept')), false);
  assert.equal(denied.calls.filter(call => call.path.endsWith('decline')).length, 1,
    'receiver reports a known failed read to original owner');
  const unknown = dockFixture();
  const uncertain = unknown.make({
    api: async (path, body) => path.endsWith('accept') ? Promise.reject(Error('response lost')) : unknown.api(path, body),
    isRendered: () => Boolean(unknown.document?.data),
    sleep: async () => unknown.setDocument({ ...unknown.document, pending: false, data: { text: 'fresh' } }),
  });
  await assert.rejects(uncertain.receive(dockTicket), /response lost/);
  assert.equal(unknown.document?.id, 'receiver-file', 'unknown commit retains exact already-authorized view for inspection');
  assert.equal(unknown.notifications.includes('closed-container'), false);
  assert.equal(unknown.calls.some(call => call.path.endsWith('decline')), false,
    'unknown acceptance cannot be falsely downgraded to a confirmed rejection');
  const revoked = dockFixture();
  const knownDenied = revoked.make({
    api: async (path, body) => path.endsWith('accept')
      ? Promise.reject(Object.assign(Error('grant revoked'), { status: 403 }))
      : revoked.api(path, body),
    isRendered: () => Boolean(revoked.document?.data),
    sleep: async () => revoked.setDocument({ ...revoked.document, pending: false, data: { text: 'old' } }),
  });
  await assert.rejects(knownDenied.receive(dockTicket), /grant revoked/);
  assert.equal(revoked.document, null, 'confirmed denial must scrub the old authorized snapshot');
  assert.equal(revoked.notifications.includes('closed-container'), true);
  assert.equal(revoked.calls.some(call => call.path.endsWith('decline')), true);
  const wrongOwner = dockFixture();
  await assert.rejects(wrongOwner.make({ currentOwner: () => ({ ...wrongOwner.owner, owner_epoch: 'replaced' }) }).receive(dockTicket), /unavailable/);
  assert.equal(wrongOwner.notifications.length, 0);
});
