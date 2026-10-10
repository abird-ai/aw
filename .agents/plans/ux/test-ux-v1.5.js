/* Browser-only V1.5 regression: no filesystem/network/Agent effects. */
(function(){
'use strict';
const U=window.AwUX,s=U.state,logs=[];let ok=0,bad=0;
const results=document.getElementById('test-results');
function check(label,result){logs.push((result?'PASS ':'FAIL ')+label);if(result)ok++;else bad++;}
function button(name,val){
 return Array.from(document.querySelectorAll('[data-action]')).find(el=>
    el.dataset.action===name&&(val==null||el.dataset.value===val));
}
function click(name,val){
 const el=button(name,val);check('Control exists: '+name+(val?' '+val:''),!!el);
 if(el)el.click();
}
function zoom(){return s.v15ZoomByTab[s.v15Tab]||'';}
function run(){
 check('V1.5 is current',s.version==='v1.5');
 check('exactly two V1.5 modes in toolbar',
   Array.from(document.querySelectorAll('[data-action="set-mode"]')).map(el=>el.dataset.value).join(',')==='minimal,workspace');
 check('Workspace Review is selected',s.mode==='workspace'&&s.v15Tab==='review');
 check('regular header exists in Workspace',document.querySelectorAll('.topbar').length===1);
 check('V1.5 has no redundant global tab-title context bar',
   !document.querySelector('.v15-context-bar'));
 check('review has Agent/Diff/Terminal panes',document.querySelectorAll('.v15-review-tiles [data-pane]').length===3);
 const originalZoom=zoom();
 document.querySelector('[data-pane="agent"] .pane-inner')?.dispatchEvent(
   new MouseEvent('dblclick',{bubbles:true})
 );
 check('double-clicking Agent transcript does NOT maximize',zoom()===originalZoom);
 document.querySelector('[data-pane="agent"] .pane-head')?.dispatchEvent(
   new MouseEvent('dblclick',{bubbles:true})
 );
 check('double-clicking pane HEADER may maximize',zoom()==='agent');
 document.querySelector('[data-pane="agent"] .pane-head')?.dispatchEvent(
   new MouseEvent('dblclick',{bubbles:true})
 );
 check('double-click pane HEADER restores layout',!zoom());
 const initialCount=s.v15Tabs.length;
 click('v15-zoom','agent');
 check('Agent maximize retains same tab',s.v15Tab==='review'&&s.v15Tabs.length===initialCount);
 check('Agent maximize hides secondary Review panes',zoom()==='agent'&&document.querySelectorAll('.v15-content [data-pane]').length===1);
 click('v15-zoom','agent');
 check('Restoring Agent returns original grid',!zoom()&&document.querySelectorAll('.v15-content [data-pane]').length===3);
 click('v15-open-diff-tab','tool.rs');
 check('Review Diff break-out creates independent tab',s.v15Tabs.length===initialCount+1&&s.v15Tab.startsWith('diff:'));
 check('Standalone diff is readable without Git tab',document.querySelectorAll('[data-pane="standaloneDiff"]').length===1);
 check('Standalone Diff title appears in TAB, not repeated in second pane header',
   document.querySelector('.v15-tab-entry.selected .v15-tab-label')?.textContent.includes('tool.rs') &&
   !document.querySelector('.v15-document-pane .pane-head') &&
   document.querySelector('.v15-document-context')?.textContent.indexOf('tool.rs')===-1);
 check('Standalone Diff retains one unobtrusive context line',
   document.querySelectorAll('.v15-document-context').length===1 &&
   !document.querySelector('.v15-document-pane .v15-diff-context'));
 click('v15-zoom','standaloneDiff');
 check('Standalone diff maximizes to distraction-free pane',zoom()==='standaloneDiff'&&!!document.querySelector('.v15-app-immersive'));
 check('Immersive Diff keeps a floating RESTORE control and only one name',
   !!document.querySelector('.v15-document-actions [data-action="v15-zoom"]') &&
   document.querySelectorAll('.v15-document-immersive-name').length===1);
 click('v15-zoom','standaloneDiff');
 check('Standalone restore leaves independent tab intact',!zoom()&&!!document.querySelector('.sidebar'));
 const diffId=s.v15Tab;
 click('v15-close-tab',diffId);
 check('Closing diff tab did not alter Review layout or Agent',!s.v15Tabs.some(t=>t.key===diffId)&&s.v15Tab==='review'&&!!document.querySelector('[data-pane="agent"]'));

 click('v15-select-tab','files');
 const containedTabs=s.v15Tabs.length;
 click('v15-preview-file','hooks.rs');
 check('Contained file selection stays within Files tab',s.v15Tab==='files'&&s.v15Tabs.length===containedTabs&&s.v15FileWithinFiles==='hooks.rs');
 check('Contained viewer reflects selected file',document.querySelector('[data-pane="editor"] .codeview')?.textContent?.includes('authorize'));
 click('v15-zoom','editor');
 check('Files editor maximize hides tree without tab creation',zoom()==='editor'&&
   !document.querySelector('[data-pane="tree"]')&&s.v15Tabs.length===containedTabs);
 check('Maximized contained editor has ONE file title, no duplicated context bar',
   !document.querySelector('.v15-context-bar')&&
   document.querySelectorAll('[data-pane="editor"] .pane-head').length===1 &&
   !document.querySelector('[data-pane="editor"] .v15-file-context'));
 click('v15-zoom','editor');
 check('Files editor restore shows original tree',!zoom()&&!!document.querySelector('[data-pane="tree"]'));
 click('v15-open-file-tab','hooks.rs');
 check('Break-out creates standalone File tab',s.v15Tab.startsWith('file:')&&s.v15Tabs.length===containedTabs+1);
 check('Standalone File has one title and one compact metadata line',
   document.querySelector('.v15-tab-entry.selected .v15-tab-label')?.textContent.includes('hooks.rs') &&
   !document.querySelector('.v15-document-pane .pane-head') &&
   document.querySelectorAll('.v15-document-context').length===1);
 const fileTab=s.v15Tab;
 click('v15-select-tab','files');
 click('v15-open-file-tab','hooks.rs');
 check('Reopening same scoped file deduplicates tabs',s.v15Tab===fileTab&&s.v15Tabs.length===containedTabs+1);
 click('v15-close-tab',fileTab);
 check('Closing standalone file leaves Files container available',!s.v15Tabs.some(t=>t.key===fileTab)&&!!s.v15Tabs.find(t=>t.key==='files'));

 click('v15-select-tab','git');
 const beforeGit=s.v15Tabs.length;
 click('v15-preview-diff','hooks.rs');
 check('Git file selection updates diff within Git only',s.v15Tab==='git'&&s.v15DiffWithinGit==='hooks.rs'&&s.v15Tabs.length===beforeGit);
 click('v15-zoom','gitdiff');
 check('Git contained Diff can maximize independently',zoom()==='gitdiff'&&!document.querySelector('[data-pane="gitlist"]'));
 click('v15-zoom','gitdiff');
 click('v15-open-diff-tab','hooks.rs');
 check('Git diff can break out into independent tab',s.v15Tab.startsWith('diff:')&&s.v15Tabs.length===beforeGit+1);

 click('set-dock','files');
 click('v15-open-file-tab','tool.rs');
 check('Mini Files opens direct standalone File tab',s.v15Tab.startsWith('file:')&&s.v15Tab.endsWith('tool.rs'));
 click('set-dock','git');
 click('v15-open-diff-tab','message.rs');
 check('Mini Git opens direct standalone Diff tab',s.v15Tab.startsWith('diff:')&&s.v15Tab.endsWith('message.rs'));

 // Current aw has ONE Ctrl+Shift+P/F1 command palette, not the old
 // prototype's Ctrl+P Quick Switch. Preserve the six established prefixes
 // precisely (> ! @ # $ /) and add only & Agents.
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'p',ctrlKey:true,bubbles:true}));
 check('Ctrl+P does not steal composer/PTY prompt history',!document.querySelector('.v15-palette-dialog'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'P',ctrlKey:true,shiftKey:true,bubbles:true}));
 check('Ctrl+Shift+P opens one centered aw Command Palette',
   !!document.querySelector('.v15-palette-dialog')&&
   !document.querySelector('.popover.quick'));
 check('Current six aw categories and new Agents are exactly present',
   U.v15Palette.CATEGORIES.map(c=>c.prefix+' '+c.label).join('|')===
   '> Actions|! Windows|@ Files|# Sessions|$ Terminals|/ Commands|& Agents');
 check('Palette preserves prefix-query parsing and category rewrite',
   U.v15Palette.parse('@tool').category==='files' &&
   U.v15Palette.rewrite('@tool','&')==='&tool' &&
   U.v15Palette.parse('!Review').category==='windows');
 click('v15-palette-category','&');
 check('Agents category uses dedicated & prefix',s.paletteQuery==='&'&&
   U.v15Palette.results(s.paletteQuery).every(item=>item.category==='agents'));
 const agentOptions=U.v15Palette.results(s.paletteQuery);
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowUp',bubbles:true}));
 check('ArrowUp wraps existing aw palette selection to last Agent result',
   s.paletteSelected===agentOptions.length-1);
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));
 check('ArrowDown wraps back to first Agent result',s.paletteSelected===0);
 const input=document.querySelector('[data-palette-input]');
 input.value='&hooks';
 input.dispatchEvent(new Event('input',{bubbles:true}));
 check('Typing &hooks filters to matching Agent',s.paletteQuery==='&hooks'&&
   U.v15Palette.results(s.paletteQuery).length===1&&
   U.v15Palette.results(s.paletteQuery)[0].id==='agent-hooks');
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));
 check('Enter from & Agents opens existing Agent within Workspace',
   s.v15SelectedAgent==='hooks'&&s.mode==='workspace'&&s.v15Tab==='agent'&&zoom()==='agent');
 check('Palette closed after selecting an Agent',!document.querySelector('.v15-palette-dialog'));
 // The same palette is available through existing F1 and inside Minimal.
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F1',bubbles:true}));
 check('F1 opens same command palette',!!document.querySelector('.v15-palette-dialog'));
 click('v15-palette-category','@');
 const fileInput=document.querySelector('[data-palette-input]');
 fileInput.value='@tool';
 fileInput.dispatchEvent(new Event('input',{bubbles:true}));
 check('@ Files supports both standalone File and Diff results',
   U.v15Palette.results(s.paletteQuery).some(item=>item.id==='file-tool')&&
   U.v15Palette.results(s.paletteQuery).some(item=>item.id==='diff-tool'));
 click('v15-palette-run','file-tool');
 check('Palette @ Files opens standalone File tab, no contained Files dependency',
   s.v15Tab.startsWith('file:')&&!document.querySelector('.v15-palette-dialog'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F1',bubbles:true}));
 click('v15-palette-category','!');
 check('! Windows category is preserved; % is not aliased',
   U.v15Palette.parse('!Review').category==='windows'&&
   U.v15Palette.parse('%Review').category===null);
 const windowsInput=document.querySelector('[data-palette-input]');
 windowsInput.value='!Review';
 windowsInput.dispatchEvent(new Event('input',{bubbles:true}));
 check('Windows query locates saved Review tab',
   U.v15Palette.results(s.paletteQuery).some(item=>item.value==='review'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 check('Esc dismisses palette without running an action',
   !document.querySelector('.v15-palette-dialog'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F1',bubbles:true}));
 click('v15-palette-category','&');
 const agentInput=document.querySelector('[data-palette-input]');
 agentInput.value='&coordinator';
 agentInput.dispatchEvent(new Event('input',{bubbles:true}));
 click('v15-palette-run','agent-root');
 check('Agent via the shared palette is maximized Workspace, not third mode',
   s.mode==='workspace'&&s.v15Tab==='agent'&&zoom()==='agent');
 check('Sidebar stays present for Agent Workspace',!!document.querySelector('.sidebar'));
 click('set-dock','activity');
 const childTabCount=s.v15Tabs.length;
 click('v15-show-agent','hooks');
 check('Selecting nested child stays in unified Workspace with no new runtime tab',
   s.v15SelectedAgent==='hooks'&&s.mode==='workspace'&&s.v15Tab==='agent'&&
   s.v15Tabs.length===childTabCount);
 check('Child Agent pane has its own factual status and machine context',
   document.querySelector('[data-pane="agent"]')?.textContent.includes('Awaiting approval'));
 click('v15-show-agent','coordinator');
 check('Returning to parent selects the same Agent tab',s.v15SelectedAgent==='coordinator'&&
   s.v15Tab==='agent'&&s.v15Tabs.length===childTabCount);
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
 check('F2 cycles to Minimal and removes all header chrome',s.mode==='minimal'&&!document.querySelector('.topbar')&&
   !document.querySelector('.sidebar')&&!!document.querySelector('.v15-footer'));
 check('Minimal has no upper meta line / tabs',!document.querySelector('.main-tabs')&&
   !document.querySelector('.v15-context-bar'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F1',bubbles:true}));
 check('Headerless Minimal can use the same aw palette',
   !!document.querySelector('.v15-palette-dialog')&&!document.querySelector('.topbar'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
 check('Minimal palette dismisses back to headerless conversation',
   !document.querySelector('.v15-palette-dialog')&&!document.querySelector('.topbar'));
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
 check('F2 cycles back to Workspace, same selected Agent',s.mode==='workspace'&&s.v15Tab==='agent');
 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
 check('F2 never enters obsolete Agents mode',s.mode==='minimal');

 document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
 click('v15-new-tab');
 check('New layout tab is a real empty visual tab',!!document.querySelector('[data-pane="scratch"]')&&s.v15Tabs.some(t=>t.kind==='scratch'));
 click('v15-open-file-tab','tool.rs');
 check('New tab picker can open file without file browser',s.v15Tab.startsWith('file:'));

 click('set-nav','recents');
 check('Recents still available in sidebar',s.nav==='recents'&&!!document.querySelector('.sidebar'));
 click('set-nav','projects');
 check('Projects still available after Recents',s.nav==='projects');
}
try{run()}catch(e){bad++;logs.push('UNCAUGHT '+(e.stack||String(e)));}
results.dataset.status=bad?'failed':'passed';
results.dataset.passed=String(ok);results.dataset.failed=String(bad);
results.textContent=logs.join('\n')+'\n'+ok+' passed, '+bad+' failed';
document.title=bad?'V1.5 UX TEST FAILED':'V1.5 UX TEST PASS ('+ok+')';
})();