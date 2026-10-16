import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as Y from '../web/node_modules/yjs/dist/yjs.mjs';
import { __test as helpers } from '../web/src/multiplayer.js';
import { needsDraftPublicationConsent } from '../web/model.js';

const fixture = JSON.parse(fs.readFileSync(
  new URL('./fixtures/shared_draft_yjs_v1.json', import.meta.url),
  'utf8',
));

function decodeText(encoded) {
  const doc = new Y.Doc();
  Y.applyUpdate(doc, Uint8Array.from(Buffer.from(encoded, 'base64')));
  return doc.getText('prompt').toString();
}

test('Yjs decodes both frozen Yjs and Yrs V1 fixtures identically', () => {
  assert.equal(decodeText(fixture.yjs_13_6_33_update_v1_b64), fixture.text);
  assert.equal(decodeText(fixture.yrs_0_28_0_small_client_update_v1_b64), fixture.text);
});

test('shared-draft text replacement preserves Unicode and combining sequences', () => {
  const doc = new Y.Doc();
  const text = doc.getText('prompt');
  text.insert(0, 'hello 🙂');
  helpers.replaceText(text, 'e\u0301 ≠ é 👩🏽‍💻 שלום');
  assert.equal(text.toString(), 'e\u0301 ≠ é 👩🏽‍💻 שלום');
});

test('nonempty private composer requires explicit publication consent', () => {
  assert.equal(needsDraftPublicationConsent('private text', false), true);
  assert.equal(needsDraftPublicationConsent('   ', false), false);
  assert.equal(needsDraftPublicationConsent('private text', true), false);
});

test('multiplayer code remains a dynamic chunk, not a static main-app import', () => {
  const source = fs.readFileSync(new URL('../web/src/main.jsx', import.meta.url), 'utf8');
  assert.match(source, /import\("\.\/multiplayer\.js"\)/);
  assert.doesNotMatch(source, /^import .*multiplayer\.js/m);
  assert.match(source, /Share this draft/);
  assert.match(source, /Keep private/);
});
