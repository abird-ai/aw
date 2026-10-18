import test from 'node:test';
import assert from 'node:assert/strict';
import { createDocumentDocking, takeDocumentDockTicket } from '../web/src/document-docking.js';

const ticket = 'a'.repeat(64);
const owner = { source_id: 'source-dock', scope_id: 'root-scope', owner_epoch: 'epoch' };
const linkedScope = 'non-root-scope';
const reviewId = 'retained-review-39';
function fixture() {
  const calls = [], controls = [];
  let doc = { id: 'workspace-document-37', containerId: 'origin', kind: 'review',
    target: { source_id: owner.source_id, scope_id: linkedScope, review_id: reviewId },
    data: { kind: 'review', id: reviewId, root: '/fixture/linked', lines: [{ kind: 'added', text: '+PRIVATE_DIFF_CONTENT' }] },
    pending: false, unavailable: false, error: '' };
  let registration = 0, statuses = 0;
  const server = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('register')) return { window_id: (++registration === 1 ? 'b' : 'd').repeat(64),
      window_proof: (registration === 1 ? 'c' : 'e').repeat(64), ...owner };
    if (path.endsWith('prepare-review')) return { ticket };
    if (path.endsWith('status')) return { state: (++statuses < 2 ? 'prepared' : 'accepted'), document_id: 'workspace-document-37' };
    if (path.endsWith('release')) return { state: 'released', document_id: 'workspace-document-37' };
    if (path.endsWith('inspect')) return { kind: 'review', source_id: owner.source_id,
      scope_id: linkedScope, owner_epoch: owner.owner_epoch, review_id: reviewId };
    if (path.endsWith('accept')) return { state: 'accepted' };
    if (path.endsWith('heartbeat')) return { state: 'released' };
    if (path.endsWith('cancel')) return { state: 'cancelled' };
    if (path.endsWith('decline')) return { state: 'declined' };
    throw Error('Unexpected API operation');
  };
  const views = {
    getDocument: id => id === doc?.id ? doc : null,
    openContainer(kind, scope) {
      assert.equal(kind, 'git');
      assert.equal(scope, undefined, 'Review reception must not register the linked worktree as a general capability');
      return 'receiving-git';
    },
    openRetainedReview(containerId, descriptor) {
      assert.equal(containerId, 'receiving-git');
      assert.deepEqual(descriptor, {
        source_id: owner.source_id, scope_id: linkedScope, review_id: reviewId,
      });
      doc = { id: 'received-review', containerId, kind: 'review',
        target: descriptor, data: null, pending: true, error: '', unavailable: false };
      return doc.id;
    },
    closeDocument(id) { if (doc?.id === id) doc = null; controls.push('closed'); },
    closeContainer() { controls.push('closed-container'); },
  };
  const make = extra => createDocumentDocking({
    api: server, views, currentOwner: () => owner,
    select: (...value) => controls.push({ selected: value }),
    close: id => { assert.equal(id, 'workspace-document-37'); doc = null; },
    popup: () => ({ closed: false, opener: null, location: { replace() {} } }),
    navigate: (_popup, value) => assert.equal(value, ticket),
    sleep: async () => { if (doc?.pending) doc = { ...doc, pending: false, data: {
      kind: 'review', id: reviewId, lines: [{ kind: 'added', text: '+freshly fetched' }],
    } }; },
    isRendered: () => Boolean(doc?.data),
    ...extra,
  });
  return { make, calls, controls, get document() { return doc; } };
}

test('cross-window Git Review origin prepares only immutable scope and ID, no plaintext', async () => {
  const f = fixture();
  assert.equal(await f.make().send('workspace-document-37'), true);
  assert.equal(f.document, null);
  const prepared = f.calls.find(value => value.path.endsWith('prepare-review'));
  assert.deepEqual(prepared.body, {
    window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
    owner_epoch: owner.owner_epoch, source_id: owner.source_id,
    scope_id: linkedScope, document_id: 'workspace-document-37', review_id: reviewId,
  });
  assert.ok(f.calls.every(value => !JSON.stringify(value).includes('PRIVATE_DIFF_CONTENT')));
  assert.ok(f.calls.every(value => value.path !== '/api/workspace/action'));
});

test('receiving Git Review creates one exact independently loaded read-only document', async () => {
  const f = fixture();
  assert.deepEqual(await f.make().receive(ticket), {
    containerId: 'receiving-git', documentId: 'received-review',
  });
  assert.equal(f.document.target.scope_id, linkedScope);
  assert.equal(f.document.target.review_id, reviewId);
  assert.deepEqual(f.controls[0].selected, ['receiving-git', 'received-review', 'review']);
  assert.deepEqual(f.calls.map(value => value.path), [
    '/api/dock/register', '/api/dock/inspect', '/api/dock/accept',
    '/api/dock/heartbeat',
  ]);
});

test('fragment is stripped and never used as browser authorization', () => {
  const history = { calls: [], replaceState(...args) { this.calls.push(args); } };
  const location = { hash: '#aw-dock=' + ticket, pathname: '/', search: '?mode=review' };
  assert.equal(takeDocumentDockTicket(location, history), ticket);
  assert.deepEqual(history.calls, [[null, '', '/?mode=review']]);
  assert.equal(takeDocumentDockTicket({ hash: '#aw-dock=bad', pathname: '/' }, history), null);
});

test('forged Review cannot bypass receiver source validation', async () => {
  const f = fixture();
  const docking = f.make({
    api: async (path, body) => path.endsWith('inspect')
      ? { kind: 'review', source_id: 'forged', scope_id: linkedScope,
        owner_epoch: owner.owner_epoch, review_id: reviewId }
      : f.calls.push({ path, body }) && { window_id: 'b'.repeat(64),
        window_proof: 'c'.repeat(64), ...owner },
  });
  await assert.rejects(docking.receive(ticket), /unavailable/);
  assert.ok(f.calls.every(value => value.path !== '/api/workspace/action'));
});
