import test from 'node:test';
import assert from 'node:assert/strict';
import { createWorkspaceViews } from '../web/src/workspace-views.js';
import { createDocumentDocking } from '../web/src/document-docking.js';

const owner = { source_id: 'fixture-source', scope_id: 'launch-scope', owner_epoch: 'epoch' };
const scope = 'verified-linked-scope';
const reviewId = 'immutable-review-id';
const ticket = 'a'.repeat(64);

test('retained Git Review constructs an exact read without adding a navigator scope', async () => {
  const pending = [];
  const controller = createWorkspaceViews({
    request(body) {
      let resolve;
      const wait = new Promise(done => { resolve = done; });
      pending.push({ body, resolve });
      return wait;
    },
  });
  controller.setSource({ source_id: owner.source_id, view_root: { id: owner.scope_id, label: '/fixture/root' } });
  const git = controller.openContainer('git');
  const descriptor = { source_id: owner.source_id, scope_id: scope, review_id: reviewId };
  const document = controller.openRetainedReview(git, descriptor);
  assert.deepEqual(pending[0].body, {
    op: 'view', source_id: owner.source_id, scope_id: scope,
    request: { op: 'review', review_id: reviewId },
  });
  assert.throws(() => controller.openContainer('git', scope), /authorized/);
  assert.throws(() => controller.openContainer('files', scope), /authorized/);
  const result = { kind: 'review', id: reviewId, root: '/fixture/linked', path: 'sample.rs',
    basis: 'immutable', read_only: true, binary: false, truncated: false,
    same_agent_root: false, lines: [{ kind: 'added', text: '+synthetic' }] };
  pending[0].resolve({ source_id: owner.source_id, scope_id: scope, result });
  await new Promise(done => setImmediate(done));
  assert.equal(controller.getDocument(document).data.id, reviewId);
  assert.equal(controller.openRetainedReview(git, descriptor), document);
  assert.equal(pending.length, 1);
  const updated = controller.refreshDocument(document);
  assert.equal(pending[1].body.scope_id, scope);
  assert.deepEqual(pending[1].body.request, { op: 'review', review_id: reviewId });
  pending[1].resolve({ source_id: owner.source_id, scope_id: scope, result });
  assert.equal(await updated, true);
  assert.equal(controller.getContainer(git).scope_id, owner.scope_id);
  assert.equal(controller.closeContainer(git), true);
  assert.equal(controller.getDocument(document), null);
});

test('retained Review cannot forge source, scope grammar or navigator kind', () => {
  const controller = createWorkspaceViews({ request: () => Promise.resolve(null) });
  controller.setSource({ source_id: owner.source_id, view_root: { id: owner.scope_id, label: '/fixture' } });
  const git = controller.openContainer('git'), files = controller.openContainer('files');
  for (const descriptor of [
    { source_id: 'foreign', scope_id: scope, review_id: reviewId },
    { source_id: owner.source_id, scope_id: '../escape', review_id: reviewId },
    { source_id: owner.source_id, scope_id: scope, review_id: '/forged' },
    { source_id: owner.source_id, scope_id: scope, review_id: reviewId, root: '/foreign' },
  ]) assert.throws(() => controller.openRetainedReview(git, descriptor), /verified exact/);
  assert.throws(() => controller.openRetainedReview(files,
    { source_id: owner.source_id, scope_id: scope, review_id: reviewId }), /verified exact/);
  assert.equal(controller.listDocuments().length, 0);
});
