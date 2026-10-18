import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { toolPresentation } from '../web/src/tool-presentation.js';

const css = fs.readFileSync(new URL('../web/src/styles.css', import.meta.url), 'utf8');

function tokens(selector) {
  const beginning=css.indexOf(selector + ' {');
  assert.ok(beginning>=0, 'missing '+selector);
  const body=css.slice(beginning+selector.length+2).split('}')[0];
  return Object.fromEntries([...body.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6});/g)]
    .map(match=>[match[1],match[2]]));
}
function luma(hex) {
  const channel=index=>parseInt(hex.slice(index,index+2),16)/255;
  const linear=value=>value<=.04045?value/12.92:((value+.055)/1.055)**2.4;
  return .2126*linear(channel(1))+.7152*linear(channel(3))+.0722*linear(channel(5));
}
function contrast(a,b) {
  const [high,low]=[luma(a),luma(b)].sort((x,y)=>y-x);
  return (high+.05)/(low+.05);
}

test('light/dark semantic metadata is readable on both page and Explorer', () => {
  for(const [name,vars] of [
    ['light',tokens(':root')],
    ['dark',tokens(':root[data-effective-theme="dark"]')],
  ]) {
    for(const [role,surface] of [
      ['faint','page'], ['faint','sidebar'], ['muted','page'],
      ['fg','page'], ['danger','page'], ['green-strong','page'],
    ]) {
      assert.ok(vars[role]&&vars[surface],name+' missing semantic '+role+'/'+surface);
      const value=contrast(vars[role],vars[surface]);
      assert.ok(value>=4.5,name+' '+role+' on '+surface+' contrast '+value.toFixed(3));
    }
    assert.notEqual(vars.danger,vars['green-strong']);
    assert.notEqual(vars.faint,vars.fg,'metadata still has a visual hierarchy');
  }
});

test('tool outcomes have explicit factual labels independent of color or symbols', () => {
  for(const status of ['failed','denied','interrupted']) {
    const view=toolPresentation({status});
    assert.deepEqual(view,{tone:'failure',symbol:'×',status});
  }
  assert.deepEqual(toolPresentation({status:'succeeded'}),
    {tone:'success',symbol:'✓',status:'succeeded'});
  for(const status of ['running','pending','unknown','cancelled']) {
    const view=toolPresentation({status});
    assert.equal(view.tone,'neutral');
    assert.equal(view.status,status);
    assert.ok(view.symbol);
  }
  assert.deepEqual(toolPresentation({status:'succeeded',output:{is_error:true}}),
    {tone:'failure',symbol:'×',status:'failed'},
    'negative output cannot be labelled a success');
  assert.equal(toolPresentation(null).status,'unknown');
  assert.deepEqual(toolPresentation({status:'SUCCESS',detail:'untrusted'}),
    {tone:'neutral',symbol:'?',status:'unknown'});
  assert.equal(toolPresentation({status:'X'.repeat(5_000)}).status,'unknown');
});

test('compact tool visuals preserve words, no color-only result or unbounded unknown', () => {
  assert.match(css,/\.tool-row\.failure\s*\{\s*--tool-tone:\s*var\(--tool-failure\)/);
  assert.match(css,/\.tool-row\.success\s*\{\s*--tool-tone:\s*var\(--tool-success\)/);
  assert.match(css,/\.doc-tab\.active\s*\{[\s\S]*?background:\s*var\(--tab-active\)/);
  assert.match(css,/\.tab-strip\s*\{[\s\S]*?overflow-x:\s*auto/);
  assert.match(css,/\.code-diff\s+\.cm-mergeView/);
});
