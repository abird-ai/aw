/* Browser-only self-check for the offline design simulator; no real services. */
(function(){
  'use strict';
  const U=window.AwUX, s=U.state, logs=[];
  let failed=0,passed=0;
  const results=document.getElementById('test-results');
  function check(name, condition){
    if(condition){passed++;logs.push('PASS '+name);}
    else {failed++;logs.push('FAIL '+name);}
  }
  function findAction(name,value){
    return Array.from(document.querySelectorAll('[data-action]')).find(
      b=>b.dataset.action===name&&(value==null||b.dataset.value===value)
    );
  }
  function click(name,value){
    const b=findAction(name,value);
    if(!b){check('button exists: '+name+' '+(value||''),false);return;}
    b.click();
  }
  function inspect(){
    check('V1.4 initially visible',s.version==='v1.4');
    check('top header shows Space',document.querySelector('.topbar')?.textContent.includes('abird'));
    check('upper project navigator present',!!findAction('set-nav','recents'));
    check('mini dock available',!!findAction('set-dock','git'));
    check('native Agent and real PTY-like panes',document.querySelectorAll('.review-tiles .pane').length===3);
    click('set-dock','git');
    check('switch to mini Git keeps main Review',s.dock==='git'&&s.main==='review');
    check('mini Git lists exact worktree',document.querySelector('.side-lower')?.textContent.includes('aw/main'));
    click('open-git-file','hooks.rs');
    check('mini Git selection updates Diff without switching layout',s.selectedFile==='hooks.rs'&&s.main==='review');
    check('Diff shows newly selected file',document.querySelector('.diffpane')?.textContent.includes('hooks.rs'));
    click('set-dock','files');
    check('switch to mini Files independently',s.dock==='files'&&s.main==='review');
    click('open-file','tool.rs');
    check('file entry opens rich File pane',s.main==='files'&&s.selectedFile==='tool.rs');
    click('set-dock','activity');
    click('set-main','review');
    click('set-nav','recents');
    check('full Recents replaces project tree',s.nav==='recents'&&document.querySelector('.side-upper')?.textContent.includes('seed reviewer'));
    check('compact duplicate Recent list is suppressed',!document.querySelector('.activity-children'));
    click('set-nav','projects');
    check('Activity Recents expansion restored',!!document.querySelector('.activity-children'));
    click('set-group','now');
    check('Now shows real nested Agents',document.querySelector('.activity-children')?.textContent.includes('hooks'));
    click('toggle-dock');
    check('lower dock collapses to one header',s.collapsed&&document.querySelector('.side-lower.collapsed'));
    click('toggle-dock');
    check('dock expands without changing current Agent or tab',!s.collapsed&&s.main==='review');
    click('toggle-space-menu');
    check('Space quick picker appears',!!document.querySelector('.popover.spaces'));
    click('set-scope','x2');
    check('Local x2 switches visual context',s.scope==='x2'&&document.querySelector('.topbar')?.textContent.includes('Local · x2'));
    click('open-fleet');
    check('machine names open management view',s.main==='fleet');
    click('toggle-machine-menu');
    check('machine chevron shows Local Space list',!!document.querySelector('.popover.machines'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
    check('Escape closes popup',!document.querySelector('.popover'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'p',ctrlKey:true,bubbles:true}));
    check('Ctrl+P opens global quick switch',!!document.querySelector('.popover.quick'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
    check('F2 cycles from Workspace to Minimal',s.mode==='minimal'&&!document.querySelector('.sidebar'));
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'F2',bubbles:true}));
    check('F2 cycles to Agents',s.mode==='agents'&&!!document.querySelector('.sidebar'));
    click('set-mode','workspace');
    click('set-layout','grid');
    click('set-main','review');
    click('focus-pane','diff');
    document.querySelector('[data-pane="diff"]')?.dispatchEvent(
      new MouseEvent('dblclick',{bubbles:true})
    );
    check('double-click zooms focused Diff',s.zoom==='diff');
    click('toggle-zoom');
    check('zoom restores same layout',s.zoom===''&&s.main==='review');
    click('open-spaces');
    click('design-info','space');
    check('add Space is explicitly only a preview',s.toast.includes('preview does not execute'));
  }
  try{inspect();}
  catch(err){failed++;logs.push('UNCAUGHT '+String(err&&err.stack||err));}
  results.dataset.status=failed?'failed':'passed';
  results.dataset.passed=String(passed);
  results.dataset.failed=String(failed);
  results.textContent=logs.join('\n')+'\n'+passed+' passed, '+failed+' failed';
  document.title=failed?'aw UX CHECK FAILED':'aw UX CHECK PASS ('+passed+')';
})();
