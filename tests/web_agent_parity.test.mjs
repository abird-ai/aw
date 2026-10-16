import test from 'node:test';
import assert from 'node:assert/strict';
import { acceptSnapshot, submission } from '../web/model.js';

test('resource projection survives snapshot acceptance but never enters prompt submission', () => {
  const snapshot = acceptSnapshot(null, {
    instance: 'fixture',
    sequence: 1,
    revision: 2,
    turns: [],
    pending: null,
    needs_reconciliation: false,
    accounts: [{ id: 'a', allowed_models: ['m'] }],
    resources: {
      context: ['system prompt · config · 10 B'],
      skills: ['review · /local/review.md · 20 B'],
    },
  });
  assert.deepEqual(snapshot.resources.skills, ['review · /local/review.md · 20 B']);
  const body = submission(snapshot, 'hello', 'request-1', { account_id: 'a', model: 'm' });
  assert.deepEqual(Object.keys(body).sort(), ['account_id', 'expected_revision', 'model', 'prompt', 'request_id']);
  assert.equal(JSON.stringify(body).includes('/local/review.md'), false);
});

test('delta acceptance preserves browser-only resource/status projection', () => {
  const snapshot = acceptSnapshot(null, {
    instance: 'fixture',
    sequence: 1,
    revision: 2,
    turns: [],
    pending: { run_id: 7, prompt: 'p', answer: '' },
    resources: { context: ['c'], skills: ['s'] },
  });
  assert.equal(snapshot.resources.context[0], 'c');
});
