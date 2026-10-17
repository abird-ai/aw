import test from 'node:test';
import assert from 'node:assert/strict';
import { acceptSnapshot, submission } from '../web/model.js';
import { AGENT_VIEW_LIMITS, checkedViewSequence, createAgentViews } from '../web/src/agent-views.js';
import { api } from '../web/src/api.js';

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

const target = (branch = 'branch-alpha', epoch = 'owner-epoch') => ({
  store_id: 'store-exact', source_epoch: epoch, session_id: 'session-exact', branch_id: branch,
});
const payload = (resource, patch = {}) => ({
  ...resource, title: 'Saved Agent', branch_name: 'same display name', head: 'head-unchanged',
  revision: '9007199254740993', turns: [{ prompt: 'Question 雪', answer: 'Answer α', tool_records: [] }],
  context_blocks: [], needs_inspection: false, retained_bytes: 512, expires_in_ms: 15000, ...patch,
});
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture(reader) {
  let time = 0, changes = 0;
  const calls = [];
  const state = createAgentViews({ now: () => time, onChange: () => changes++, read: reader || ((resource, options) => {
    const reply = deferred();
    calls.push({ resource, ...options, ...reply });
    return reply.promise;
  }) });
  return { state, calls, at(value) { time = value; }, changes: () => changes };
}
async function load(f, id, patch) {
  const pending = f.state.refresh(id);
  f.calls.at(-1).resolve(payload(f.state.get(id).target, patch));
  assert.equal(await pending, true);
  return f.state.get(id).snapshot;
}

test('pinned view IDs are distinct from exact resource IDs and duplicate labels', async () => {
  const f = fixture();
  const a = f.state.open(target()), b = f.state.open(target('branch-beta')), duplicate = f.state.open(target());
  assert.notEqual(a, b);
  assert.notEqual(a, duplicate);
  assert.notEqual(a, target().branch_id);
  await load(f, a);
  await load(f, b, { turns: [{ prompt: 'Other', answer: 'Distinct λ' }] });
  assert.equal(f.state.get(a).snapshot.turns[0].answer, 'Answer α');
  assert.equal(f.state.get(b).snapshot.turns[0].answer, 'Distinct λ');
  assert.deepEqual(f.calls[0].resource, target());
  assert.deepEqual(Object.keys(f.calls[0].resource).sort(), ['branch_id', 'session_id', 'source_epoch', 'store_id']);
  for (const invalid of [{ ...target(), root: '/tmp/forged' }, { ...target(), branch_id: '' },
    { ...target(), store_id: 'a'.repeat(129) }, { ...target(), source_epoch: 'bad\nowner' }]) {
    assert.throws(() => f.state.open(invalid), /exact/);
  }
});

test('each pinned view keeps independent Find, viewport and selection', async () => {
  const f = fixture(), a = f.state.open(target()), b = f.state.open(target('branch-beta'));
  await load(f, a); await load(f, b);
  f.state.updateView(a, { find: { query: '雪', selected: 2 }, viewport: { top: 180, left: 8 }, selection: { anchor: 2, head: 9 } });
  f.state.updateView(b, { find: { query: 'α', selected: 1 }, viewport: { top: 30, follow: true } });
  assert.deepEqual(f.state.get(a).find, { query: '雪', selected: 2 });
  assert.deepEqual(f.state.get(b).find, { query: 'α', selected: 1 });
  assert.equal(f.state.get(a).viewport.top, 180);
  assert.equal(f.state.get(b).viewport.top, 30);
  assert.deepEqual(f.state.get(a).selection, { anchor: 2, head: 9 });
  assert.equal(f.state.get(b).selection, null);
  assert.throws(() => { f.state.get(a).snapshot.turns[0].answer = 'mutated'; }, TypeError);
  assert.throws(() => { f.state.get(a).find.query = 'mutated'; }, TypeError);
  const before = f.state.get(a);
  assert.throws(() => f.state.updateView(a, { selection: { anchor: -1, head: 1 } }));
  assert.throws(() => f.state.updateView(a, { viewport: { top: Infinity } }));
  assert.equal(f.state.get(a), before);
});

test('fresh unchanged authorization reuses the entire snapshot and ordinary view state', async () => {
  const f = fixture(), id = f.state.open(target());
  const first = await load(f, id);
  f.state.updateView(id, { find: { query: 'α', selected: 3 }, selection: { anchor: 1, head: 7 }, viewport: { top: 90 } });
  f.at(5000);
  const refreshed = f.state.refresh(id);
  assert.equal(f.state.get(id).snapshot, first);
  f.calls.at(-1).resolve(Object.fromEntries(Object.entries(payload(target())).reverse()));
  assert.equal(await refreshed, true);
  assert.equal(f.calls.length, 2, 'same head still performs a fresh authorized read');
  assert.equal(f.state.get(id).snapshot, first);
  assert.deepEqual(f.state.get(id).find, { query: 'α', selected: 3 });
  assert.deepEqual(f.state.get(id).selection, { anchor: 1, head: 7 });
  assert.equal(f.state.get(id).viewport.top, 90);
  assert.equal(f.state.get(id).expiresAt, 20000);
  assert.equal(f.state.get(id).refreshAt, 10000);
  const changed = await load(f, id, { turns: [{ prompt: 'Question 雪', answer: 'Changed despite same HEAD' }] });
  assert.notEqual(changed, first);
  assert.equal(f.state.get(id).find.query, 'α');
  assert.equal(f.state.get(id).find.selected, 0);
  assert.equal(f.state.get(id).selection, null);
});

test('unchanged-head denial immediately clears every duplicate exact resource', async () => {
  const f = fixture(), a = f.state.open(target()), duplicate = f.state.open(target()), other = f.state.open(target('branch-beta'));
  await load(f, a); await load(f, duplicate); const safe = await load(f, other);
  f.state.updateView(duplicate, { find: { query: 'private' }, selection: { anchor: 1, head: 2 } });
  const held = f.state.refresh(duplicate), denied = f.state.refresh(a);
  const oldReply = f.calls.at(-2), denial = f.calls.at(-1);
  let protectedAtAbort;
  oldReply.signal.addEventListener('abort', () => {
    protectedAtAbort = [a, duplicate].some(id => Boolean(f.state.get(id)?.snapshot));
  });
  denial.reject(Object.assign(new Error('Permission revoked with same HEAD'), { status: 403 }));
  assert.equal(await denied, false);
  assert.equal(oldReply.signal.aborted, true);
  assert.equal(protectedAtAbort, false, 'all duplicate caches are cleared before synchronous abort callbacks');
  for (const id of [a, duplicate]) {
    assert.equal(f.state.get(id).snapshot, null);
    assert.deepEqual(f.state.get(id).find, { query: '', selected: 0 });
    assert.equal(f.state.get(id).selection, null);
  }
  assert.equal(f.state.get(other).snapshot, safe);
  oldReply.resolve(payload(target()));
  assert.equal(await held, false);
  assert.equal(f.state.get(duplicate).snapshot, null);
});

test('closed and reopened exact IDs cannot accept a held real Promise reply', async () => {
  const f = fixture(), old = f.state.open(target());
  const held = f.state.refresh(old), stale = f.calls[0];
  assert.equal(f.state.close(old), true);
  assert.equal(stale.signal.aborted, true);
  const replacement = f.state.open(target());
  assert.notEqual(replacement, old);
  const fresh = f.state.refresh(replacement), current = f.calls[1];
  current.resolve(payload(target(), { title: 'Current incarnation' }));
  assert.equal(await fresh, true);
  stale.resolve(payload(target(), { title: 'Closed incarnation' }));
  assert.equal(await held, false);
  assert.equal(f.state.get(old), null);
  assert.equal(f.state.get(replacement).snapshot.title, 'Current incarnation');
});

test('close and clear scrub protected pending entries before synchronous abort callbacks', async () => {
  const f = fixture(), a = f.state.open(target());
  await load(f, a);
  f.state.updateView(a, { find: { query: 'private' }, selection: { anchor: 1, head: 4 } });
  const closed = f.state.refresh(a), closeReply = f.calls.at(-1);
  const observations = [];
  closeReply.signal.addEventListener('abort', () => observations.push(f.state.get(a)));
  f.state.close(a);
  assert.deepEqual(observations, [null]);
  const b = f.state.open(target()), c = f.state.open(target('branch-beta'));
  await load(f, b); await load(f, c);
  const first = f.state.refresh(b), firstReply = f.calls.at(-1);
  const second = f.state.refresh(c), secondReply = f.calls.at(-1);
  for (const call of [firstReply, secondReply]) {
    call.signal.addEventListener('abort', () => observations.push(f.state.list()));
  }
  f.state.clear('Logout');
  assert.deepEqual(observations, [null, [], []]);
  for (const call of [closeReply, firstReply, secondReply]) call.resolve(payload(call.resource));
  assert.deepEqual(await Promise.all([closed, first, second]), [false, false, false]);
  assert.deepEqual(f.state.list(), []);
});

test('logout, source replacement and disposal reject all delayed completions', async () => {
  for (const action of ['clear', 'dispose']) {
    const f = fixture(), id = f.state.open(target());
    const pending = f.state.refresh(id), old = f.calls[0];
    f.state[action]('Authority ended');
    assert.equal(old.signal.aborted, true);
    old.resolve(payload(target()));
    assert.equal(await pending, false);
    assert.deepEqual(f.state.list(), []);
    if (action === 'dispose') assert.throws(() => f.state.open(target()), /disposed/);
    else {
      const next = f.state.open(target('branch-alpha', 'new-owner'));
      const fresh = f.state.refresh(next);
      f.calls.at(-1).resolve(payload(target()));
      assert.equal(await fresh, false, 'old source epoch cannot satisfy new resource');
      assert.equal(f.state.get(next).snapshot, null);
    }
  }
});

test('authentication expiry removes all protected views, not only the failed branch', async () => {
  const f = fixture(), a = f.state.open(target()), b = f.state.open(target('branch-beta'));
  await load(f, a); await load(f, b);
  const denied = f.state.refresh(a);
  f.calls.at(-1).reject(Object.assign(new Error('Not authenticated'), { status: 401 }));
  assert.equal(await denied, false);
  assert.deepEqual(f.state.list(), []);
  assert.equal(f.state.get(b), null);
});

test('request-start expiry clears content and derived state even before timer pulse', async () => {
  const f = fixture(), id = f.state.open(target());
  const pending = f.state.refresh(id);
  f.at(14999); f.calls[0].resolve(payload(target()));
  assert.equal(await pending, true);
  f.state.updateView(id, { find: { query: 'private', selected: 2 }, selection: { anchor: 1, head: 4 } });
  assert.equal(f.state.get(id).expiresAt, 15000);
  f.at(15000);
  assert.equal(f.state.get(id).snapshot, null, 'getter expires before caller can copy stale content');
  assert.equal(f.state.get(id).find.query, '');
  assert.equal(f.state.get(id).selection, null);
  assert.equal(f.state.get(id).status, 'expired');
  const copying = fixture(), another = copying.state.open(target());
  const finishing = copying.state.refresh(another);
  const response = payload(target());
  Object.defineProperty(response.turns[0], 'answer', { enumerable: true, get() {
    copying.at(15000); // Time passes during bounded validation/serialization.
    return 'Copied after the request grant expired';
  } });
  copying.calls[0].resolve(response);
  assert.equal(await finishing, false);
  assert.equal(copying.state.get(another).snapshot, null);
});

test('fresh reply after prior cache expiry cannot preserve expired selection', async () => {
  const f = fixture(), id = f.state.open(target());
  const initial = await load(f, id);
  f.state.updateView(id, { find: { query: 'private' }, selection: { anchor: 1, head: 4 } });
  f.at(14000);
  const refresh = f.state.refresh(id);
  f.at(16000); f.calls.at(-1).resolve(payload(target()));
  assert.equal(await refresh, true);
  assert.notEqual(f.state.get(id).snapshot, initial);
  assert.equal(f.state.get(id).find.query, '');
  assert.equal(f.state.get(id).selection, null);
  assert.equal(f.state.get(id).expiresAt, 29000);
});

test('expired pending work cannot supersede a fresh same-view request', async () => {
  const f = fixture(), id = f.state.open(target());
  const old = f.state.refresh(id), stale = f.calls[0];
  assert.equal(await f.state.refresh(id), false, 'one request per view');
  f.at(15000); f.state.expire();
  assert.equal(stale.signal.aborted, true);
  const fresh = f.state.refresh(id);
  f.calls.at(-1).resolve(payload(target(), { title: 'Fresh' }));
  assert.equal(await fresh, true);
  stale.resolve(payload(target(), { title: 'Expired' }));
  assert.equal(await old, false);
  assert.equal(f.state.get(id).snapshot.title, 'Fresh');
});

test('view and physical in-flight bounds survive ignored abort signals', async () => {
  const f = fixture(), ids = Array.from({ length: 8 }, (_, index) => f.state.open(target(`branch-${index}`)));
  assert.throws(() => f.state.open(target('overflow')), /At most 8/);
  const held = ids.slice(0, 4).map(id => f.state.refresh(id));
  assert.equal(f.calls.length, 4);
  assert.equal(await f.state.refresh(ids[4]), false);
  f.state.close(ids[0]);
  assert.equal(await f.state.refresh(ids[4]), false, 'aborted transport still consumes its physical slot until settled');
  f.calls[0].resolve(payload(target('branch-0')));
  assert.equal(await held[0], false);
  const admitted = f.state.refresh(ids[4]);
  assert.equal(f.calls.length, 5);
  for (const call of f.calls.slice(1)) call.resolve(payload(call.resource));
  assert.equal(await admitted, true);
  await Promise.all(held);
});

test('actual UTF-8 payload and aggregate snapshot bytes are bounded independently of claimed bytes', async () => {
  const f = fixture(), a = f.state.open(target()), b = f.state.open(target('branch-beta')), c = f.state.open(target('branch-gamma'));
  const large = { turns: [{ prompt: '', answer: 'x'.repeat(3 * 1024 * 1024) }], retained_bytes: 0 };
  const first = await load(f, a, large), second = await load(f, b, large);
  const refused = f.state.refresh(c); f.calls.at(-1).resolve(payload(target('branch-gamma'), large));
  assert.equal(await refused, false);
  assert.match(f.state.get(c).error, /8 MiB/);
  assert.equal(f.state.get(a).snapshot, first);
  assert.equal(f.state.get(b).snapshot, second);
  f.state.close(a);
  await load(f, c, large);
  const oversized = f.state.refresh(b);
  f.calls.at(-1).resolve(payload(target('branch-beta'), { turns: [{ prompt: '', answer: '雪'.repeat(1500000) }], retained_bytes: 1 }));
  assert.equal(await oversized, false);
  assert.match(f.state.get(b).error, /4 MiB/);
  assert.equal(f.state.get(b).snapshot, null);
});

test('malformed authority, precision and expiry responses fail closed', async () => {
  const f = fixture(), id = f.state.open(target());
  for (const patch of [{ source_epoch: 'wrong' }, { branch_id: 'wrong' }, { revision: 9007199254740992 },
    { revision: '18446744073709551616' }, { revision: '01' }, { expires_in_ms: 15001 },
    { turns: [{ prompt: 'missing answer' }] }, { context_blocks: [{ before_turn: 2, text: 'bad offset' }] }, { root: '/not-authority' }]) {
    const pending = f.state.refresh(id); f.calls.at(-1).resolve(payload(target(), patch));
    assert.equal(await pending, false);
    assert.equal(f.state.get(id).snapshot, null);
  }
  await load(f, id, { revision: '18446744073709551615' });
  assert.equal(f.state.get(id).snapshot.revision, '18446744073709551615');
  assert.equal(checkedViewSequence(Number.MAX_SAFE_INTEGER - 1), Number.MAX_SAFE_INTEGER);
  for (const bad of [Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1, -1, 0.5, NaN, Infinity]) {
    assert.throws(() => checkedViewSequence(bad), /exhausted/);
  }
  assert.equal(AGENT_VIEW_LIMITS.pending, 4);
});

test('transport merges caller cancellation and retains exact authenticated JSON request', async t => {
  const controller = new AbortController(), headers = deferred();
  let options;
  t.mock.method(globalThis, 'fetch', async (_path, request) => {
    options = request;
    return new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => reject(new DOMException('Caller cancelled', 'AbortError')), { once: true });
      headers.resolve();
    });
  });
  const pending = api('/api/agent-view', target(), 1000, { signal: controller.signal });
  await headers.promise;
  assert.equal(options.credentials, 'same-origin');
  assert.equal(options.cache, 'no-store');
  assert.equal(options.method, 'POST');
  assert.deepEqual(JSON.parse(options.body), target());
  controller.abort();
  await assert.rejects(pending, error => error.name === 'AbortError');
});

test('transport preserves cancellation during delayed response body parsing', async t => {
  const controller = new AbortController(), reading = deferred();
  t.mock.method(globalThis, 'fetch', async (_path, request) => ({
    ok: true,
    json: () => new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => reject(new DOMException('Body cancelled', 'AbortError')), { once: true });
      reading.resolve();
    }),
  }));
  const pending = api('/api/agent-view', target(), 1000, { signal: controller.signal });
  await reading.promise;
  controller.abort();
  await assert.rejects(pending, error => error.name === 'AbortError');
});

test('transport timeout cancels a delayed body and preserves HTTP denial status', async t => {
  t.mock.method(globalThis, 'fetch', async (_path, request) => ({
    ok: true,
    json: () => new Promise((_resolve, reject) => {
      request.signal.addEventListener('abort', () => reject(new DOMException('Timed out', 'AbortError')), { once: true });
    }),
  }));
  await assert.rejects(api('/api/agent-view', target(), 1), error => error.name === 'AbortError');
  t.mock.restoreAll();
  for (const status of [401, 403, 409, 429, 503]) {
    t.mock.method(globalThis, 'fetch', async () => ({ ok: false, status, json: async () => ({ error: `Denied ${status}` }) }));
    await assert.rejects(api('/api/agent-view', target()), error => error.status === status && error.message === `Denied ${status}`);
    t.mock.restoreAll();
  }
});

test('settled transport removes caller-abort listener and timeout', async t => {
  const controller = new AbortController();
  let signal;
  t.mock.method(globalThis, 'fetch', async (_path, request) => {
    signal = request.signal;
    return { ok: true, json: async () => ({ ok: true }) };
  });
  assert.deepEqual(await api('/api/agent-view', target(), 5, { signal: controller.signal }), { ok: true });
  controller.abort();
  await new Promise(resolve => setTimeout(resolve, 15));
  assert.equal(signal.aborted, false);
});
