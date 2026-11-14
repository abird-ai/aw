import test from 'node:test';
import assert from 'node:assert/strict';
import {
  createAgentDocking, createDelegatedSurfaceDocking, createDocumentDocking,
  takeAgentDockTicket, takeDelegatedSurfaceDockTicket, takeDocumentDockTicket,
} from '../web/src/document-docking.js';
import { createAgentViews } from '../web/src/agent-views.js';
import { documentFingerprint } from '../web/src/document-presentation.js';
import { createWorkspaceViews } from '../web/src/workspace-views.js';

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

const agentTarget = Object.freeze({
  store_id: 'store-agent', source_epoch: 'epoch-agent',
  session_id: 'session-agent', branch_id: 'branch-agent',
});
const agentPayload = target => ({
  ...target, title: 'Private delegated Agent', branch_name: 'main', head: 'head-agent',
  revision: '1', turns: [{ prompt: 'private prompt', answer: 'private answer', tool_records: [] }],
  context_blocks: [], needs_inspection: false, retained_bytes: 512, expires_in_ms: 15000,
});

test('distinct-Actor Agent handoff sends only exact target identity and closes source after owner release', async () => {
  const calls = [];
  const agentViews = createAgentViews({ read: async target => agentPayload(target) });
  const viewId = agentViews.open(agentTarget);
  assert.equal(await agentViews.refresh(viewId), true);
  let statuses = 0;
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/register')) return {
      window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
      store_id: agentTarget.store_id, source_epoch: agentTarget.source_epoch,
    };
    if (path.endsWith('/prepare')) return { ticket, grant_generation: '9' };
    if (path.endsWith('/status')) return {
      state: ++statuses === 1 ? 'confirming' : 'accepted', document_id: viewId,
    };
    if (path.endsWith('/confirm')) return { state: 'confirming' };
    if (path.endsWith('/release')) return { state: 'released', document_id: viewId };
    if (path.endsWith('/cancel')) return { state: 'cancelled' };
    throw new Error('Unexpected delegated Agent operation ' + path);
  };
  const docking = createAgentDocking({
    api, agentViews,
    popup: () => ({ closed: false, opener: null, location: { replace() {} } }),
    navigate: (_popup, value) => assert.equal(value, ticket),
    sleep: async () => {},
  });
  assert.equal(await docking.send(viewId, 'bob'), true);
  assert.equal(agentViews.get(viewId), null);
  const prepared = calls.find(call => call.path.endsWith('/prepare'));
  assert.deepEqual(prepared.body, {
    window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
    store_id: agentTarget.store_id, source_epoch: agentTarget.source_epoch,
    session_id: agentTarget.session_id, branch_id: agentTarget.branch_id,
    document_id: viewId, recipient_id: 'bob',
  });
  const serialized = JSON.stringify(calls);
  assert.equal(serialized.includes('private prompt'), false);
  assert.equal(serialized.includes('private answer'), false);
  assert.ok(calls.every(call => !call.path.includes('/api/workspace')));
});

test('lost delegated Agent release response preserves the source and cannot cancel a committed handoff', async () => {
  const calls = [];
  const agentViews = createAgentViews({ read: async target => agentPayload(target) });
  const viewId = agentViews.open(agentTarget);
  assert.equal(await agentViews.refresh(viewId), true);
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/register')) return {
      window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
      store_id: agentTarget.store_id, source_epoch: agentTarget.source_epoch,
    };
    if (path.endsWith('/prepare')) return { ticket, grant_generation: '9' };
    if (path.endsWith('/status')) return { state: 'accepted', document_id: viewId };
    if (path.endsWith('/release')) throw Object.assign(new Error('release response lost'), { status: 503 });
    if (path.endsWith('/cancel')) throw Object.assign(new Error('already released'), { status: 409 });
    throw new Error('Unexpected delegated Agent operation ' + path);
  };
  const docking = createAgentDocking({
    api, agentViews,
    popup: () => ({ closed: false, opener: null, location: { replace() {} } }),
    navigate: () => {}, sleep: async () => {},
  });
  await assert.rejects(docking.send(viewId, 'bob'), /release response lost/);
  assert.equal(agentViews.get(viewId).snapshot.title, 'Private delegated Agent');
  assert.deepEqual(calls.slice(-2).map(call => call.path), [
    '/api/dock/agent/release', '/api/dock/agent/cancel',
  ]);
});

test('delegated Agent receiver uses only its ticket reader and scrubs on known revoke', async () => {
  const calls = [];
  const agentViews = createAgentViews({ read: async () => { throw new Error('owner reader must not be used'); } });
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/receive')) return {
      window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
      resource: { kind: 'agent_view', ...agentTarget }, grant_generation: '4',
    };
    if (path.endsWith('/read')) return agentPayload(agentTarget);
    if (path.endsWith('/accept')) {
      const [view] = agentViews.list();
      agentViews.updateView(view.id, {
        find: { query: 'private', selected: 1 }, selection: { anchor: 1, head: 4 },
      });
      return { state: 'accepted' };
    }
    if (path.endsWith('/heartbeat')) {
      const error = new Error('grant revoked'); error.status = 403; throw error;
    }
    throw new Error('Unexpected delegated Agent operation ' + path);
  };
  const docking = createAgentDocking({ api, agentViews, popup: () => null, sleep: async () => {} });
  await assert.rejects(docking.receive(ticket), /revoked/);
  assert.deepEqual(agentViews.list(), []);
  assert.deepEqual(calls.map(call => call.path), [
    '/api/dock/agent/receive', '/api/dock/agent/read',
    '/api/dock/agent/accept', '/api/dock/agent/heartbeat',
  ]);
});

test('unknown delegated Agent acceptance outcome remains inspectable but cannot silently re-register', async () => {
  const agentViews = createAgentViews({ read: async () => { throw new Error('owner reader must not be used'); } });
  let receives = 0;
  const api = async path => {
    if (path.endsWith('/receive')) {
      receives++;
      if (receives > 1) throw Object.assign(new Error('ticket already received'), { status: 409 });
      return {
        window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
        resource: { kind: 'agent_view', ...agentTarget }, grant_generation: '7',
      };
    }
    if (path.endsWith('/read')) return agentPayload(agentTarget);
    if (path.endsWith('/accept')) return { state: 'accepted' };
    if (path.endsWith('/heartbeat')) throw Object.assign(new Error('response lost'), { status: 503 });
    throw new Error('Unexpected delegated Agent operation ' + path);
  };
  const docking = createAgentDocking({ api, agentViews, popup: () => null, sleep: async () => {} });
  await assert.rejects(docking.receive(ticket), /response lost/);
  const [retained] = agentViews.list();
  assert.equal(retained.snapshot.title, 'Private delegated Agent');
  await assert.rejects(docking.receive(ticket), /already received/);
  assert.equal(agentViews.list().length, 1, 'reload/retry cannot silently mint a second delegated view');
});

const delegatedFileTarget = Object.freeze({
  source_id: 'surface-delegated', scope_id: 'scope-delegated', path: 'src/private.rs',
});

test('distinct-Actor File handoff sends exact identity only and closes source after release', async () => {
  const calls = [], links = [];
  let document = {
    id: 'workspace-document-51', containerId: 'workspace-container-1', kind: 'file',
    target: delegatedFileTarget,
    data: { context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
      text: 'PRIVATE FILE BYTES', read_only: true },
    delegated: false, pending: false, unavailable: false, error: '',
  };
  let statuses = 0;
  const views = {
    getDocument: id => id === document?.id ? document : null,
    closeDocument(id) { assert.equal(id, document.id); document = null; return true; },
    openDelegatedDocument() { throw new Error('source must not open receiver document'); },
    refreshDocument() { throw new Error('source must not refresh receiver document'); },
  };
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/register')) return {
      window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
      store_id: 'store-team', source_epoch: 'owner-epoch',
    };
    if (path.endsWith('/prepare')) return { ticket, grant_generation: '12' };
    if (path.endsWith('/status')) return {
      state: ++statuses === 1 ? 'confirming' : 'accepted', document_id: 'workspace-document-51',
    };
    if (path.endsWith('/confirm')) return { state: 'confirming' };
    if (path.endsWith('/release')) return { state: 'released', document_id: 'workspace-document-51' };
    if (path.endsWith('/cancel')) return { state: 'cancelled' };
    throw new Error('Unexpected delegated Surface operation ' + path);
  };
  const sourcePresentation = {
    query: 'PRIVATE', selected: 0, focus: 'pane0',
    panes: [{ anchor: 2, head: 7, from: 0, top: 144, left: 9 }],
  };
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {}, capture: () => sourcePresentation,
    onLinkReady: (_id, value) => { if (value) links.push(value); },
    makeLink: value => 'https://owner.invalid/#aw-surface-dock=' + value,
  });
  docking.stage('workspace-document-51');
  assert.equal(await docking.send('workspace-document-51', 'bob'), true);
  assert.equal(document, null);
  const preparedBody = calls.find(call => call.path.endsWith('/prepare')).body;
  const { presentation, ...identity } = preparedBody;
  assert.deepEqual(identity, {
    window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
    store_id: 'store-team', source_epoch: 'owner-epoch',
    source_id: delegatedFileTarget.source_id, scope_id: delegatedFileTarget.scope_id,
    owner_epoch: 'owner-epoch', document_id: 'workspace-document-51',
    recipient_id: 'bob', kind: 'file', path: delegatedFileTarget.path,
  });
  assert.deepEqual(presentation, {
    ...sourcePresentation, version: 1,
    fingerprint: await documentFingerprint('file', {
      context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
      text: 'PRIVATE FILE BYTES', read_only: true,
    }),
  });
  assert.equal(JSON.stringify(calls).includes('PRIVATE FILE BYTES'), false);
  assert.deepEqual(links, ['https://owner.invalid/#aw-surface-dock=' + ticket]);
});

test('lost delegated File release response preserves source and cannot pretend transfer completed', async () => {
  const calls = [];
  let document = {
    id: 'workspace-document-release-loss', containerId: 'workspace-container-1', kind: 'file',
    target: delegatedFileTarget,
    data: { context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
      text: 'release loss bytes', read_only: true },
    delegated: false, pending: false, unavailable: false, error: '',
  };
  const views = {
    getDocument: id => id === document?.id ? document : null,
    closeDocument() { throw new Error('source must remain open without a factual release receipt'); },
    openDelegatedDocument() { throw new Error('source must not open receiver document'); },
    refreshDocument() { throw new Error('source must not refresh receiver document'); },
  };
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/register')) return {
      window_id: 'b'.repeat(64), window_proof: 'c'.repeat(64),
      store_id: 'store-team', source_epoch: 'owner-epoch',
    };
    if (path.endsWith('/prepare')) return { ticket, grant_generation: '31' };
    if (path.endsWith('/status')) return { state: 'accepted', document_id: document.id };
    if (path.endsWith('/release')) {
      throw Object.assign(new Error('release response lost'), { status: 503 });
    }
    if (path.endsWith('/cancel')) {
      throw Object.assign(new Error('already committed'), { status: 409 });
    }
    throw new Error('Unexpected delegated Surface operation ' + path);
  };
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {},
    makeLink: value => 'https://owner.invalid/#aw-surface-dock=' + value,
  });
  await assert.rejects(docking.send(document.id, 'bob'), /release response lost/);
  assert.equal(document.data.text, 'release loss bytes');
  assert.deepEqual(calls.slice(-2).map(call => call.path), [
    '/api/dock/surface/release', '/api/dock/surface/cancel',
  ]);
});

test('delegated File receiver restores bounded Find, caret and scroll before acceptance', async () => {
  const calls = [];
  const views = createWorkspaceViews({
    request: async () => { throw new Error('ordinary Workspace reader must not be used'); },
  });
  const fileData = {
    context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
    text: 'receiver private bytes', read_only: true,
  };
  const presentation = {
    version: 1, fingerprint: await documentFingerprint('file', fileData),
    query: 'private', selected: 0, focus: 'pane0',
    panes: [{ anchor: 3, head: 11, from: 0, top: 222, left: 17 }],
  };
  let restored = null;
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/receive')) return {
      window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
      resource: { kind: 'file', ...delegatedFileTarget, owner_epoch: 'owner-epoch' },
      grant_generation: '14', presentation,
    };
    if (path.endsWith('/read')) return {
      source_id: delegatedFileTarget.source_id, scope_id: delegatedFileTarget.scope_id,
      result: fileData,
    };
    if (path.endsWith('/accept')) {
      assert.deepEqual(restored, presentation, 'presentation restores before Accept');
      return { state: 'accepted' };
    }
    if (path.endsWith('/heartbeat')) return { state: 'released' };
    throw new Error('Unexpected delegated Surface operation ' + path);
  };
  let received;
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {}, isRendered: () => true,
    restore: async (_id, state) => { restored = state; return true; },
    onReceiveDocument: (id, kind, target) => { received = { id, kind, target }; },
  });
  const result = await docking.receive(ticket);
  assert.equal(result.kind, 'file');
  assert.equal(result.documentId, received.id);
  assert.equal(received.kind, 'file');
  assert.deepEqual(received.target, delegatedFileTarget);
  const document = views.getDocument(received.id);
  assert.equal(document.delegated, true);
  assert.equal(document.containerId, null);
  assert.equal(document.data.text, 'receiver private bytes');
  assert.deepEqual(views.listContainers(), []);
  assert.deepEqual(calls.map(call => call.path), [
    '/api/dock/surface/receive', '/api/dock/surface/read',
    '/api/dock/surface/accept', '/api/dock/surface/heartbeat',
  ]);
  assert.deepEqual(calls[1].body, {
    window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
  });
});

test('delegated Review receiver restores bounded two-pane Find, selection, scroll and focus', async () => {
  const calls = [];
  const target = {
    source_id: 'surface-review', scope_id: 'scope-review', review_id: 'review-delegated-9',
  };
  const reviewData = {
    kind: 'review', id: target.review_id, root: '/fixture/review', path: 'src/lib.rs',
    basis: 'immutable review', lines: [
      { kind: 'context', text: 'alpha' },
      { kind: 'added', text: '+private delta' },
    ],
    binary: false, truncated: false, same_agent_root: true, read_only: true,
  };
  const presentation = {
    version: 1, fingerprint: await documentFingerprint('review', reviewData),
    query: 'private', selected: 0, focus: 'pane1',
    panes: [
      { anchor: 1, head: 4, from: 0, top: 120, left: 3 },
      { anchor: 2, head: 8, from: 0, top: 360, left: 11 },
    ],
  };
  const views = createWorkspaceViews({
    request: async () => { throw new Error('ordinary Workspace reader must not be used'); },
  });
  let restored = null, received = null;
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/receive')) return {
      window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
      resource: { kind: 'review', ...target, owner_epoch: 'owner-epoch' },
      grant_generation: '22', presentation,
    };
    if (path.endsWith('/read')) return {
      source_id: target.source_id, scope_id: target.scope_id, result: reviewData,
    };
    if (path.endsWith('/accept')) {
      assert.deepEqual(restored, presentation, 'two-pane presentation restores before Accept');
      return { state: 'accepted' };
    }
    if (path.endsWith('/heartbeat')) return { state: 'released' };
    throw new Error('Unexpected delegated Review operation ' + path);
  };
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {}, isRendered: () => true,
    restore: async (_id, state) => { restored = state; return true; },
    onReceiveDocument: (id, kind, exact) => { received = { id, kind, target: exact }; },
  });
  const result = await docking.receive(ticket);
  assert.deepEqual(result, { documentId: received.id, kind: 'review' });
  assert.deepEqual(received.target, target);
  assert.equal(views.getDocument(received.id).data.id, target.review_id);
  assert.equal(views.getDocument(received.id).containerId, null);
  assert.deepEqual(views.listContainers(), []);
  assert.deepEqual(calls.map(call => call.path), [
    '/api/dock/surface/receive', '/api/dock/surface/read',
    '/api/dock/surface/accept', '/api/dock/surface/heartbeat',
  ]);
});

test('delegated presentation fingerprint mismatch cannot accept or retain receiver plaintext', async () => {
  const calls = [];
  const views = createWorkspaceViews({
    request: async () => { throw new Error('ordinary Workspace reader must not be used'); },
  });
  let receivedId = null;
  const api = async (path, body) => {
    calls.push({ path, body });
    if (path.endsWith('/receive')) return {
      window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
      resource: { kind: 'file', ...delegatedFileTarget, owner_epoch: 'owner-epoch' },
      grant_generation: '23',
      presentation: {
        version: 1, fingerprint: 'f'.repeat(64), query: 'private', selected: 0, focus: 'pane0',
        panes: [{ anchor: 0, head: 1, from: 0, top: 0, left: 0 }],
      },
    };
    if (path.endsWith('/read')) return {
      source_id: delegatedFileTarget.source_id, scope_id: delegatedFileTarget.scope_id,
      result: {
        context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
        text: 'different receiver bytes', read_only: true,
      },
    };
    throw new Error('Accept must not run after fingerprint mismatch: ' + path);
  };
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {}, isRendered: () => true,
    onReceiveDocument: id => { receivedId = id; },
  });
  await assert.rejects(docking.receive(ticket), /content changed/);
  assert.equal(views.getDocument(receivedId), null);
  assert.equal(calls.some(call => call.path.endsWith('/accept')), false);
});

test('known delegated File revoke after acceptance closes and scrubs the exact receiver document', async () => {
  const views = createWorkspaceViews({ request: async () => { throw new Error('ordinary reader must not run'); } });
  const api = async path => {
    if (path.endsWith('/receive')) return {
      window_id: 'd'.repeat(64), window_proof: 'e'.repeat(64), ticket,
      resource: { kind: 'file', ...delegatedFileTarget, owner_epoch: 'owner-epoch' },
      grant_generation: '15',
    };
    if (path.endsWith('/read')) return {
      source_id: delegatedFileTarget.source_id, scope_id: delegatedFileTarget.scope_id,
      result: { context: delegatedFileTarget.scope_id, path: delegatedFileTarget.path,
        text: 'must be scrubbed', read_only: true },
    };
    if (path.endsWith('/accept')) return { state: 'accepted' };
    if (path.endsWith('/heartbeat')) throw Object.assign(new Error('grant revoked'), { status: 403 });
    throw new Error('Unexpected delegated Surface operation ' + path);
  };
  let id;
  const docking = createDelegatedSurfaceDocking({
    api, views, sleep: async () => {}, onReceiveDocument: value => { id = value; },
  });
  await assert.rejects(docking.receive(ticket), /revoked/);
  assert.equal(views.getDocument(id), null);
  assert.deepEqual(views.listDocuments(), []);
});

test('delegated Surface fragment is correlation-only and stripped before receiver reads', () => {
  const history = { calls: [], replaceState(...args) { this.calls.push(args); } };
  const location = { hash: '#aw-surface-dock=' + ticket, pathname: '/app', search: '?mode=file' };
  assert.equal(takeDelegatedSurfaceDockTicket(location, history), ticket);
  assert.deepEqual(history.calls, [[null, '', '/app?mode=file']]);
  assert.equal(takeDelegatedSurfaceDockTicket({ hash: '#aw-surface-dock=bad', pathname: '/' }, history), null);
});

test('delegated Agent fragment is correlation-only and stripped before receiver reads', () => {
  const history = { calls: [], replaceState(...args) { this.calls.push(args); } };
  const location = { hash: '#aw-agent-dock=' + ticket, pathname: '/app', search: '?mode=agent' };
  assert.equal(takeAgentDockTicket(location, history), ticket);
  assert.deepEqual(history.calls, [[null, '', '/app?mode=agent']]);
  assert.equal(takeAgentDockTicket({ hash: '#aw-agent-dock=bad', pathname: '/' }, history), null);
});
