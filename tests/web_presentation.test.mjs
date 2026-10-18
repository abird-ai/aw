import test from 'node:test';
import assert from 'node:assert/strict';
import { boundedPresentation, documentFingerprint } from '../web/src/document-presentation.js';
import { createDocumentDocking } from '../web/src/document-docking.js';

const digest = 'a'.repeat(64);
const pane = Object.freeze({ anchor: 17, head: 30, from: 8, top: 310, left: 12 });
const presentation = (kind = 'file') => ({
  version: 1, fingerprint: digest, query: 'a needle', selected: 2,
  focus: kind === 'file' ? 'find' : 'pane1',
  panes: kind === 'file' ? [{ ...pane }] : [{ ...pane }, { ...pane, anchor: 40, head: 50 }],
});
const ticket = 'e'.repeat(64);
const owner = { source_id: 'root-source', scope_id: 'root-scope', owner_epoch: 'unchanged-epoch' };

test('bounded editor metadata contains positions and Find, no text or unbounded structures', () => {
  for (const kind of ['file', 'review']) {
    const clean = boundedPresentation(kind, presentation(kind));
    assert.equal(Object.isFrozen(clean), true);
    assert.equal(Object.isFrozen(clean.panes[0]), true);
    assert.deepEqual(clean, presentation(kind));
    assert.ok(JSON.stringify(clean).length < 2048);
  }
  for (const [field, value] of [
    ['version', 2], ['fingerprint', 'B'.repeat(64)],
    ['fingerprint', 'a'.repeat(63)], ['query', 'a'.repeat(513)],
    ['query', '𐍈'.repeat(510)], ['query', 'unsafe\nline'],
    ['selected', 4096], ['selected', -2], ['focus', 'pane1'],
    ['panes', []], ['panes', [{ ...pane, anchor: 4_194_305 }]],
    ['panes', [{ ...pane, top: -1 }]], ['panes', [{ ...pane, left: NaN }]],
    ['panes', [{ ...pane, arbitrary: 'unexpected' }]],
  ]) {
    assert.throws(() => boundedPresentation('file', { ...presentation(), [field]: value }),
      /Invalid bounded/, field);
  }
  assert.throws(() => boundedPresentation('file', { ...presentation(), content: 'leak' }), /Invalid/);
  assert.throws(() => boundedPresentation('review', presentation('file')), /Invalid/);
  assert.throws(() => boundedPresentation('review', { ...presentation('review'), focus: 'nonexistent' }), /Invalid/);
});

test('SHA-256 snapshot fingerprint distinguishes File content, Review identity and displayed lines', async () => {
  const file = { text: 'private bytes are never sent directly\n' };
  const rev = { kind: 'review', id: 'immutable', root: '/fixture', lines: [{ text: '+safe', kind: 'added' }] };
  const a = await documentFingerprint('file', file);
  assert.match(a, /^[0-9a-f]{64}$/);
  assert.equal(a, await documentFingerprint('file', file));
  assert.notEqual(a, await documentFingerprint('file', { text: file.text + 'X' }));
  assert.notEqual(a, await documentFingerprint('review', rev));
  assert.notEqual(await documentFingerprint('review', rev),
    await documentFingerprint('review', { ...rev, lines: [{ text: '+changed', kind: 'added' }] }));
  await assert.rejects(documentFingerprint('file', null), /unavailable/);
});

function harness(kind = 'file', changed = false) {
  const data = kind === 'file'
    ? { text: 'first\nneedle\nlast\n' }
    : { kind: 'review', id: 'retained-review', root: '/fixture', path: 'test.rs',
        lines: [{ kind: 'added', text: '+needle' }] };
  const scope = kind === 'review' ? 'non-root-linked-scope' : owner.scope_id;
  const target = kind === 'file' ? { source_id: owner.source_id, scope_id: scope, path: 'test.rs' }
    : { source_id: owner.source_id, scope_id: scope, review_id: data.id };
  const initial = {
    id: 'workspace-document-37', kind, containerId: 'original',
    target, data, unavailable: false, pending: false, error: '',
  };
  let current = initial, recorded = null, restored = null, released = false, accepted = false;
  let registerCount = 0, polls = 0, reads = 0;
  const calls = [];
  const expected = { query: 'needle', selected: 0, focus: 'find',
    panes: Array.from({ length: kind === 'file' ? 1 : 2 },
      (_, index) => ({ anchor: index + 1, head: index + 3, from: 0, top: 75 + index, left: 0 })) };
  const api = async (endpoint, body) => {
    calls.push({ endpoint, body });
    if (endpoint.endsWith('register')) return { ...owner, window_id: (++registerCount === 1 ? 'a' : 'b').repeat(64),
      window_proof: 'c'.repeat(64) };
    if (endpoint.endsWith('prepare') || endpoint.endsWith('prepare-review')) {
      recorded = body.presentation;
      return { ticket };
    }
    if (endpoint.endsWith('status')) return { state: ++polls >= 2 ? 'accepted' : 'prepared',
      document_id: initial.id };
    if (endpoint.endsWith('release')) { released = true; return { state: 'released', document_id: initial.id }; }
    if (endpoint.endsWith('inspect')) return {
      kind, source_id: owner.source_id, scope_id: scope, owner_epoch: owner.owner_epoch,
      ...(kind === 'file' ? { path: target.path } : { review_id: target.review_id }),
      presentation: recorded,
    };
    if (endpoint.endsWith('accept')) { accepted = true; return { state: 'accepted' }; }
    if (endpoint.endsWith('cancel')) return { state: 'cancelled' };
    if (endpoint.endsWith('decline')) return { state: 'declined' };
    throw Error('unknown test operation ' + endpoint);
  };
  const views = {
    getDocument(id) { return id === current?.id ? current : null; },
    openContainer(k) { assert.equal(k, kind === 'file' ? 'files' : 'git'); return 'receiver'; },
    openFile(_container, path) {
      assert.equal(path, target.path); reads++;
      current = { ...initial, id: 'received', containerId: 'receiver', data: null, pending: true };
      return current.id;
    },
    openRetainedReview(_container, descriptor) {
      assert.deepEqual(descriptor, target); reads++;
      current = { ...initial, id: 'received', containerId: 'receiver', data: null, pending: true };
      return current.id;
    },
    closeDocument(id) { if (current?.id === id) current = null; },
    closeContainer() {},
  };
  const docking = createDocumentDocking({
    api, views, currentOwner: () => owner,
    capture: () => expected, restore: async (_id, meta) => { restored = meta; return true; },
    select: () => {}, close: () => { current = null; },
    popup: () => ({ closed: false }), navigate: () => {},
    sleep: async () => { if (current?.pending) current = { ...current, pending: false,
      data: changed ? { ...data, ...(kind === 'file' ? { text: 'changed bytes' }
        : { lines: [{ kind: 'added', text: '+new content' }] }) } : data }; },
    isRendered: () => true,
  });
  return {
    docking, calls, expected, data,
    get presentation() { return recorded; },
    get restored() { return restored; },
    get released() { return released; },
    get accepted() { return accepted; },
    get reads() { return reads; },
    get current() { return current; },
  };
}

for (const kind of ['file', 'review']) {
  test('handoff ' + kind + ' binds and independently restores exact presentation before acceptance', async () => {
    const f = harness(kind);
    f.docking.stage('workspace-document-37');
    assert.equal(await f.docking.send('workspace-document-37'), true);
    assert.match(f.presentation.fingerprint, /^[a-f0-9]{64}$/);
    assert.equal(f.presentation.query, f.expected.query);
    assert.equal(f.presentation.panes.length, kind === 'file' ? 1 : 2);
    assert.deepEqual(f.calls.slice(-2).map(entry => entry.endpoint),
      ['/api/dock/status', '/api/dock/release']);
    assert.ok(!JSON.stringify(f.calls).includes('first\\nneedle\\nlast'),
      'original source text is never in the presentation ticket');

    const receiving = harness(kind);
    const state = boundedPresentation(kind, {
      ...receiving.expected, version: 1,
      fingerprint: await documentFingerprint(kind, receiving.data),
    });
    const scope = kind === 'review' ? 'non-root-linked-scope' : owner.scope_id;
    let mounted = null, restored = null, accepted = false;
    const calls = [];
    const incoming = createDocumentDocking({
      api: async (path) => {
        calls.push(path);
        if (path.endsWith('register')) return { ...owner,
          window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64) };
        if (path.endsWith('inspect')) return { kind, source_id: owner.source_id,
          scope_id: scope, owner_epoch: owner.owner_epoch,
          ...(kind === 'file' ? { path: 'test.rs' } : { review_id: receiving.data.id }),
          presentation: state };
        if (path.endsWith('accept')) {
          assert.deepEqual(restored, state, 'position restoration must finish before admission');
          accepted = true;
          return { state: 'accepted' };
        }
        if (path.endsWith('heartbeat')) return { state: 'released' };
        throw Error('Unexpected read-only handoff route ' + path);
      },
      views: {
        getDocument: id => mounted?.id === id ? mounted : null,
        openContainer: type => {
          assert.equal(type, kind === 'file' ? 'files' : 'git');
          return 'receiver-container';
        },
        openFile: () => {
          assert.equal(kind, 'file');
          mounted = { id: 'received-file', kind, pending: true, data: null,
            target: { source_id: owner.source_id, scope_id: scope, path: 'test.rs' } };
          return mounted.id;
        },
        openRetainedReview: (_id, target) => {
          assert.equal(kind, 'review');
          assert.deepEqual(target, { source_id: owner.source_id,
            scope_id: scope, review_id: receiving.data.id });
          mounted = { id: 'received-review', kind, pending: true, data: null, target };
          return mounted.id;
        },
        closeDocument: () => { mounted = null; }, closeContainer: () => {},
      },
      currentOwner: () => owner,
      select: () => {}, close: () => {}, popup: () => ({}),
      sleep: async () => { mounted = { ...mounted, pending: false,
        data: receiving.data, unavailable: false, error: '' }; },
      isRendered: () => true,
      restore: async (_, meta) => { restored = meta; return true; },
    });
    const result = await incoming.receive(ticket);
    assert.equal(result.documentId, mounted.id);
    assert.equal(accepted, true);
    assert.deepEqual(restored, state);
    assert.deepEqual(calls, ['/api/dock/register', '/api/dock/inspect', '/api/dock/accept', '/api/dock/heartbeat']);
    assert.ok(!JSON.stringify(calls).includes('first\\nneedle\\nlast'));
  });
}

test('receiver refuses changed original bytes before accepting, without reusing stale offsets', async () => {
  const f = harness('file', true);
  const current = f.current;
  assert.equal(current.id, 'workspace-document-37');
  const state = boundedPresentation('file', {
    ...f.expected, version: 1, fingerprint: await documentFingerprint('file', f.data),
  });
  const calls = [];
  const receiver = createDocumentDocking({
    api: async (route) => {
      calls.push(route);
      if (route.endsWith('register')) return { ...owner, window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64) };
      if (route.endsWith('inspect')) return { kind: 'file', ...owner, path: 'test.rs', presentation: state };
      if (route.endsWith('decline')) return { state: 'declined' };
      throw Error('Must not accept changed bytes');
    },
    views: {
      getDocument: id => id === 'received' ? currentReceiver : null,
      openContainer: () => 'receiver', openFile: () => 'received',
      closeDocument: () => { currentReceiver = null; },
      closeContainer: () => {},
    },
    currentOwner: () => owner, select: () => {}, close: () => {}, popup: () => ({}),
    isRendered: () => true, restore: async () => { throw Error('Should not restore mismatched snapshot'); },
    sleep: async () => { currentReceiver = { ...currentReceiver, pending: false,
      data: { text: 'not the source text anymore' } }; },
  });
  let currentReceiver = { id: 'received', kind: 'file', target: current.target,
    data: null, pending: true, unavailable: false, error: '' };
  await assert.rejects(receiver.receive(ticket), /content changed/);
  assert.equal(currentReceiver, null);
  assert.deepEqual(calls, ['/api/dock/register', '/api/dock/inspect', '/api/dock/decline']);
});
