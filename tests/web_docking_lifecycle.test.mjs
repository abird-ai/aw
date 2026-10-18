import test from 'node:test';
import assert from 'node:assert/strict';
import { createDocumentDocking } from '../web/src/document-docking.js';

const ticket = 'f'.repeat(64);
const owner = Object.freeze({
  source_id: 'owner-source', scope_id: 'original-root', owner_epoch: 'owner-instance',
});
const snapshot = Object.freeze({ kind: 'file', text: 'synthetic content never transferred' });
const target = Object.freeze({ source_id: owner.source_id, scope_id: owner.scope_id, path: 'fixture.rs' });
const later = () => new Promise(resolve => setImmediate(resolve));

function caseOwner({ crashAfterAccept = false, loseReleaseResponse = false } = {}) {
  let state = 'prepared', confirmationRequested = false, confirmed = false;
  let retired = false, registered = 0, released = false, sourceClosed = false;
  let original = { id: 'workspace-document-1', containerId: 'original-file-view',
    kind: 'file', target, data: snapshot, pending: false, error: '', unavailable: false };
  let receiving = null, receiverTask;
  const calls = [];
  const credentials = registered => ({
    ...owner, window_id: (registered === 1 ? 'b' : 'd').repeat(64),
    window_proof: (registered === 1 ? 'c' : 'e').repeat(64),
  });
  async function api(route, body) {
    calls.push({ route, body });
    if (route.endsWith('register')) return credentials(++registered);
    if (route.endsWith('prepare')) return { state: 'prepared', ticket };
    if (route.endsWith('inspect')) return { kind: 'file', ...owner, path: target.path };
    if (route.endsWith('accept')) {
      if (state !== 'prepared' || retired) throw Object.assign(Error('not available'), { status: 409 });
      state = 'accepted';
      return { state };
    }
    if (route.endsWith('confirm')) {
      assert.equal(state, 'accepted', 'confirmation requires prior receiver acceptance');
      confirmationRequested = true;
      return { state: 'confirming' };
    }
    if (route.endsWith('heartbeat')) {
      if (crashAfterAccept && !retired) {
        retired = true;
        throw Error('receiver page was closed before confirmation');
      }
      if (retired) throw Object.assign(Error('retired'), { status: 403 });
      if (state === 'released') return { state: 'released' };
      if (confirmationRequested) confirmed = true;
      return { state: confirmed ? 'accepted' : 'confirming' };
    }
    if (route.endsWith('status')) {
      if (retired) return { state: 'receiver_unavailable', document_id: original?.id || 'workspace-document-1' };
      if (state === 'prepared') return { state, document_id: original.id };
      return { state: state === 'released' ? 'released' : (confirmed ? 'accepted' : 'confirming'),
        document_id: original?.id || 'workspace-document-1' };
    }
    if (route.endsWith('release')) {
      if (!confirmed || retired || state !== 'accepted') throw Object.assign(Error('missing recent ACK'), { status: 409 });
      state = 'released'; released = true;
      if (loseReleaseResponse) throw Error('HTTP release response lost');
      return { state: 'released', document_id: original.id };
    }
    if (route.endsWith('retire')) { retired = true; return { state: 'retired' }; }
    if (route.endsWith('decline')) { state = 'declined'; return { state: 'declined' }; }
    if (route.endsWith('cancel')) {
      if (state !== 'prepared') throw Object.assign(Error('accepted receipt cannot roll back'), { status: 409 });
      state = 'cancelled';
      return { state: 'cancelled' };
    }
    throw Error('Unexpected docking route ' + route);
  }
  const originalViews = { getDocument: id => id === original?.id ? original : null };
  const receivingViews = {
    getDocument: id => id === receiving?.id ? receiving : null,
    openContainer: kind => { assert.equal(kind, 'files'); return 'receiver-files'; },
    openFile: (_container, path) => {
      assert.equal(path, target.path);
      receiving = { id: 'receiver-document-1', containerId: 'receiver-files', kind: 'file',
        target, data: null, pending: true, unavailable: false, error: '' };
      return receiving.id;
    },
    closeDocument: () => { receiving = null; }, closeContainer: () => {},
  };
  const receiver = createDocumentDocking({
    api, views: receivingViews, currentOwner: () => owner, select: () => {},
    close: () => {}, popup: () => ({}),
    sleep: async () => {
      if (receiving?.pending) receiving = { ...receiving, pending: false, data: snapshot };
      await later();
    },
    isRendered: () => true,
    retireWindow: body => { void api('/api/dock/retire', body); },
  });
  const source = createDocumentDocking({
    api, views: originalViews, currentOwner: () => owner, select: () => {},
    close: id => {
      assert.equal(id, original.id);
      assert.equal(state, 'released');
      sourceClosed = true; original = null;
    },
    popup: () => ({ closed: false }),
    navigate: () => {
      receiverTask = receiver.receive(ticket);
      receiverTask.catch(() => {}); // separately awaited by the test
    },
    sleep: later,
    isRendered: () => true,
  });
  return {
    source, receiver, api, calls,
    get receiverTask() { return receiverTask; },
    get original() { return original; },
    get received() { return receiving; },
    get state() { return state; },
    get confirmed() { return confirmed; },
    get released() { return released; },
    get sourceClosed() { return sourceClosed; },
  };
}

test('fresh two-party confirmation follows accept and precedes single source release', async () => {
  const t = caseOwner();
  assert.equal(await t.source.send('workspace-document-1'), true);
  assert.deepEqual(await t.receiverTask, {
    containerId: 'receiver-files', documentId: 'receiver-document-1',
  });
  assert.equal(t.sourceClosed, true);
  assert.equal(t.original, null);
  assert.equal(t.received.data, snapshot);
  const events = t.calls.map(x => x.route);
  const position = route => events.indexOf('/api/dock/' + route);
  assert.ok(position('accept') < position('confirm'));
  assert.ok(position('heartbeat') < position('confirm'),
    'pre-challenge heartbeat is harmless and carries no release permission');
  assert.ok(events.lastIndexOf('/api/dock/heartbeat') > position('confirm'),
    'a separate post-challenge heartbeat must precede release');
  assert.ok(position('heartbeat') < position('release'));
  assert.equal(events.filter(e => e === '/api/dock/release').length, 1);
  assert.ok(t.calls.every(x => !JSON.stringify(x).includes(snapshot.text)));
});

test('receiver crash after accept but before live confirmation keeps original document', async () => {
  const t = caseOwner({ crashAfterAccept: true });
  await assert.rejects(t.source.send('workspace-document-1'), /Handoff not accepted/);
  await assert.rejects(t.receiverTask, /receiver page was closed/);
  assert.equal(t.state, 'accepted', 'the owner can remember the unknown acceptance');
  assert.equal(t.original?.data, snapshot);
  assert.equal(t.sourceClosed, false);
  assert.equal(t.released, false);
  assert.ok(t.calls.some(x => x.route.endsWith('heartbeat')), 'receiver stopped before fresh proof');
  // A crash can occur before the origin even observes Accepted. That is still
  // a safe denial, not proof that the origin had time to send Confirm.
  assert.ok(!t.calls.some(x => x.route.endsWith('release')));
});

test('ambiguous origin release response never destroys the original tab or duplicates effects', async () => {
  const t = caseOwner({ loseReleaseResponse: true });
  await assert.rejects(t.source.send('workspace-document-1'), /response lost/);
  assert.deepEqual(await t.receiverTask, {
    containerId: 'receiver-files', documentId: 'receiver-document-1',
  });
  assert.equal(t.state, 'released', 'owner committed release despite lost reply');
  assert.equal(t.sourceClosed, false, 'client cannot assume success without exact response');
  assert.equal(t.original.data, snapshot);
  assert.equal(t.released, true);
  assert.equal(t.calls.filter(x => x.route.endsWith('release')).length, 1);
});
