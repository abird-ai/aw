import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { toolPresentation } from '../web/src/tool-presentation.js';

const css = fs.readFileSync(new URL('../web/src/styles.css', import.meta.url), 'utf8');
const main = fs.readFileSync(new URL('../web/src/main.jsx', import.meta.url), 'utf8');
const terminals = fs.readFileSync(new URL('../web/src/terminal-layout.jsx', import.meta.url), 'utf8');

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

test('Build is an explicit local-terminal saved layout, independent of the active Files/Git target', () => {
  assert.match(main,/workspaceLocalSource\(\)\?\.terminals/);
  assert.match(main,/id:\s*"build-layout",\s*kind:\s*"build-layout",\s*title:\s*"Build"/);
  assert.match(main,/<BuildLayout[\s\S]*?enabled=\{\(\) => Boolean\(workspaceLocalSource\(\)\?\.terminals\)\}/);
  assert.match(terminals,/class="[^"]*build-start-terminal[^"]*"[\s\S]*?onClick=\{\(\) => void start\(\)\}/);
  assert.match(terminals,/class="[^"]*build-add-terminal[^"]*"[\s\S]*?onClick=\{\(\) => void start\(\)\}/);
  assert.doesNotMatch(terminals,/createEffect\([\s\S]{0,220}?terminal_start/,
    'reactive presentation must not start a terminal');
});

test('ActiveTarget retargets only follower Workspace reads while breakout becomes an exact-source pin', () => {
  assert.match(main,/api\("\/api\/workspace\/target\/capabilities",\s*\{\s*target_id:\s*target\.id\s*\}\)/);
  assert.match(main,/api\("\/api\/workspace\/target\/action",\s*\{\s*target_id:\s*targetId,\s*command:\s*body\s*\}/);
  assert.match(main,/function installWorkspaceSource\([\s\S]*?followerSnapshot\(\)/);
  assert.match(main,/workspaceViews\.pinDocument\([\s\S]*?workspace-pin:/);
  assert.match(main,/if \(!document\.delegated && !document\.pinned && !pinWorkspaceDocument\(id\)\)/);
  assert.match(main,/workspaceViews\.unpinDocument\(id\)/);
  assert.match(main,/workspace-target-switch/);
  assert.doesNotMatch(main,/selectWorkspaceTarget[\s\S]{0,1600}?openSession\(/,
    'target selection must not retarget the Agent Session');
  assert.doesNotMatch(main,/selectWorkspaceTarget[\s\S]{0,1600}?terminal_(?:start|claim|input)/,
    'target selection must not take PTY effects');
});

test('terminal pane visibility is separate from retained input ownership', () => {
  assert.match(terminals,/if \(!id \|\| \(!active\(\) && !controlled\(\)\)\) return/);
  assert.match(terminals,/if \(\(active\(\) \|\| controlled\(\)\) && terminalId\(\)\) void refreshScreen\(\)/);
  assert.match(terminals,/disabled=\{controlled\(\)\}[\s\S]*?Release input before switching terminals/);
  assert.match(terminals,/toggleMax=\{\(\) => setMaximizedId\(maximized\(\) \? null : id\)\}/);
  assert.match(css,/\.build-grid\.max-terminal\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0, 1fr\)/);
});
