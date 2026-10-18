import test from 'node:test';
import assert from 'node:assert/strict';
import { createDocumentDocking, takeDocumentDockTicket } from '../web/src/document-docking.js';

const owner = Object.freeze({
  source_id: 'scope-source', scope_id: 'root-scope', owner_epoch: 'unchanged-owner',
});
const ticket = 'd'.repeat(64);
function fixture() {
  const payload = { kind: 'file', path: 'fixture.rs', text: 'PRIVATE_CONTENT_not_in_link' };
  const target = Object.freeze({ source_id: owner.source_id, scope_id: owner.scope_id, path: 'fixture.rs' });
  let original = { id: 'workspace-document-7', kind: 'file',
    containerId: 'source-container', target, data: payload, pending: false,
    error: '', unavailable: false };
  let received = null, state = 'prepared', accepted = false, confirmed = false;
  let receivingTask, copiedUrl = null, cleared = false;
  const calls = [];
  const sourceIdentity = { ...owner, window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64) };
  const receivingIdentity = { ...owner, window_id: 'a'.repeat(64), window_proof: 'e'.repeat(64) };
  const createClient = (label) => async (path, body) => {
    calls.push({ label, path, body });
    if (path.endsWith('register')) return label === 'source' ? sourceIdentity : receivingIdentity;
    if (path.endsWith('prepare')) return { ticket, state: 'prepared' };
    if (path.endsWith('inspect')) return { kind: 'file', ...owner, path: 'fixture.rs' };
    if (path.endsWith('accept')) { state = 'accepted'; accepted = true; return { state }; }
    if (path.endsWith('confirm')) return { state: 'confirming' };
    if (path.endsWith('heartbeat')) {
      if (state === 'released') return { state: 'released' };
      confirmed = true; return { state: 'accepted' };
    }
    if (path.endsWith('status')) return { state: accepted ? (confirmed ? 'accepted' : 'confirming') : 'prepared',
      document_id: 'workspace-document-7' };
    if (path.endsWith('release')) {
      assert.equal(confirmed, true, 'accepted alone does not close origin');
      state = 'released'; return { state, document_id: 'workspace-document-7' };
    }
    if (path.endsWith('decline')) return { state: 'declined' };
    if (path.endsWith('cancel')) return { state: 'cancelled' };
    throw Error('unknown route: '+path);
  };
  const receiving = createDocumentDocking({
    api: createClient('receiver'),
    views: {
      getDocument: id => received?.id === id ? received : null,
      openContainer: () => 'receiving-files',
      openFile: (_, path) => {
        assert.equal(path, 'fixture.rs');
        received = { id:'received-document', kind:'file', containerId:'receiving-files',
          target, pending:true, data:null, unavailable:false, error:'' };
        return received.id;
      },
      closeDocument: () => { received = null; }, closeContainer: () => {},
    },
    currentOwner: () => owner, select: () => {}, close: () => {},
    popup: () => { throw Error('receiver never opens a popup'); },
    sleep: async () => {
      if (received?.pending) received = { ...received, pending:false, data:payload };
      await new Promise(resolve => setImmediate(resolve));
    },
    isRendered: () => true,
  });
  let source;
  source = createDocumentDocking({
    api: createClient('source'),
    views: { getDocument: id => original?.id === id ? original : null },
    currentOwner: () => owner, select: () => {},
    close: id => { assert.equal(id, original.id); original = null; },
    popup: () => { throw Error('external move must not open a same-profile popup'); },
    makeLink: code => 'http://127.0.0.1:8787/#aw-dock=' + code,
    onLinkReady: (id, url) => {
      assert.equal(id, 'workspace-document-7');
      if (url) {
        copiedUrl = url;
        receivingTask = receiving.receive(ticket);
        receivingTask.catch(() => {});
      } else cleared = true;
    },
    sleep: () => new Promise(resolve => setImmediate(resolve)),
    isRendered: () => true,
  });
  return { source, receiving, calls,
    get original() { return original; },
    get received() { return received; },
    get receivingTask() { return receivingTask; },
    get copiedUrl() { return copiedUrl; },
    get cleared() { return cleared; },
    get state() { return state; },
  };
}

test('manual short-lived URL docks to a separately authenticated client without opening a popup', async () => {
  const f = fixture();
  assert.equal(await f.source.send('workspace-document-7', { external: true }), true);
  assert.deepEqual(await f.receivingTask, {
    containerId: 'receiving-files', documentId: 'received-document',
  });
  assert.equal(f.state, 'released');
  assert.equal(f.original, null);
  assert.equal(f.received?.data.text, 'PRIVATE_CONTENT_not_in_link');
  assert.equal(f.copiedUrl, 'http://127.0.0.1:8787/#aw-dock=' + ticket);
  assert.equal(f.cleared, true);
  assert.equal(takeDocumentDockTicket({
    hash: '#aw-dock='+ticket, pathname:'/', search:'',
  }, {replaceState() {}}), ticket);
  assert.ok(f.calls.every(record => !JSON.stringify(record).includes('PRIVATE_CONTENT')));
  assert.ok(f.calls.some(record => record.label === 'source' && record.path.endsWith('release')));
  assert.ok(f.calls.some(record => record.label === 'receiver' && record.path.endsWith('heartbeat')));
});

test('unconsumed manual link can fail without closing original file or leaving stored presentation', async () => {
  const f = fixture();
  // Suppress the receiver by creating a separate controller with a link sink
  // that deliberately simulates a recipient who never opens the URL.
  let original = { id:'workspace-document-7', kind:'file', containerId:'one',
    target:{ source_id:owner.source_id,scope_id:owner.scope_id,path:'fixture.rs' },
    data:{text:'synthetic'},pending:false,unavailable:false,error:'' };
  let shown, cleared;
  const source = createDocumentDocking({
    api: async (path) => {
      if (path.endsWith('register')) return {
        ...owner, window_id:'b'.repeat(64), window_proof:'c'.repeat(64),
      };
      if (path.endsWith('prepare')) return {ticket};
      if (path.endsWith('status')) return { state:'prepared', document_id:original.id };
      if (path.endsWith('cancel')) return {state:'cancelled'};
      throw Error('unexpected route');
    },
    views:{ getDocument: id => original?.id === id ? original : null },
    currentOwner:()=>owner,
    select:()=>{}, close:()=>{ original=null; },
    popup:()=>{ throw Error('external flow must never open popup'); },
    makeLink:()=> 'http://localhost/#aw-dock='+ticket,
    onLinkReady: (id,url) => { if (url) {shown=url; queueMicrotask(()=>source.dispose());} else cleared=true; },
    sleep:()=>new Promise(resolve=>setImmediate(resolve)),
    isRendered:()=>true,
  });
  await assert.rejects(source.send('workspace-document-7',{external:true}), /not accepted/);
  assert.equal(shown,'http://localhost/#aw-dock='+ticket);
  assert.equal(cleared,true);
  assert.equal(original?.data.text,'synthetic');
  assert.equal(f.calls.length,0,'separate mock client was never used');
});
