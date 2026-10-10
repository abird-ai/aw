/* V1.5 preview adapter to the EXISTING aw command palette contract.
 * Source of category/prefix truth: src/workspace/palette.rs (read-only).
 * DESIGN MOCK ONLY: does not execute an aw command, file edit, PTY or provider.
 */
(function(){
'use strict';
const U=window.AwUX, s=U.state, A=(action,label,klass,value,title)=>
  U.action(action,label,klass,value,title), e=U.safe;
const CATEGORIES=[
 {id:"actions",prefix:">",label:"Actions",css:"info"},
 {id:"windows",prefix:"!",label:"Windows",css:"warn"},
 {id:"files",prefix:"@",label:"Files",css:"ok"},
 {id:"sessions",prefix:"#",label:"Sessions",css:"accent"},
 {id:"terminals",prefix:"$",label:"Terminals",css:"info"},
 {id:"commands",prefix:"/",label:"Commands",css:"error"},
 {id:"agents",prefix:"&",label:"Agents",css:"accent"},
];
const PREFIX=Object.fromEntries(CATEGORIES.map(x=>[x.prefix,x.id]));
const CAT=Object.fromEntries(CATEGORIES.map(x=>[x.id,x]));
function parse(raw){
 const v=String(raw||"").trimStart();
 const prefix=v[0];
 return {category:PREFIX[prefix]||null,body:PREFIX[prefix]?v.slice(1).trimStart():v};
}
function rewrite(raw,prefix){
 const q=parse(raw);
 return q.category===PREFIX[prefix]?q.body:prefix+q.body;
}
function entries(){
 const staticEntries=[
  {id:"action-new",category:"actions",label:"New workspace tab",
     description:"Create a visual layout tab", action:"v15-new-tab"},
  {id:"action-spaces",category:"actions",label:"Open Spaces",
     description:"Space membership and local/fleet contexts",action:"open-spaces"},
  {id:"action-fleet",category:"actions",label:"Open Machines / Fleet",
     description:"Connected hosts and execution site",action:"open-fleet"},
  {id:"action-maximize",category:"actions",label:"Maximize focused pane",
     description:"Visual zoom only · keeps layout and source",action:"v15-palette-max"},
  {id:"action-diff",category:"actions",label:"Open hooks.rs diff as tab",
     description:"Unstaged · aw/main · pvl-x2",action:"v15-open-diff-tab",value:"hooks.rs"},
  {id:"file-tool",category:"files",label:"tool.rs",
     description:"File · crates/aw-agent-core/src/ · pvl-x2",action:"v15-open-file-tab",value:"tool.rs"},
  {id:"file-hooks",category:"files",label:"hooks.rs",
     description:"File · crates/aw-agent-core/src/ · pvl-x2",action:"v15-open-file-tab",value:"hooks.rs"},
  {id:"file-agent",category:"files",label:"agent.rs",
     description:"File · aw-agent-core/src/ · pvl-x2",action:"v15-open-file-tab",value:"agent.rs"},
  {id:"diff-tool",category:"files",label:"tool.rs · Diff",
     description:"Unstaged Diff · aw/main · pvl-x2",action:"v15-open-diff-tab",value:"tool.rs"},
  {id:"diff-hooks",category:"files",label:"hooks.rs · Diff",
     description:"Unstaged Diff · aw/main · pvl-x2",action:"v15-open-diff-tab",value:"hooks.rs"},
  {id:"diff-message",category:"files",label:"message.rs · Diff",
     description:"Unstaged Diff · aw/main · pvl-x2",action:"v15-open-diff-tab",value:"message.rs"},
  {id:"session-main",category:"sessions",label:"aw/main · current",
     description:"Session · native Agent coordinator",action:"v15-show-agent",value:"coordinator"},
  {id:"session-hooks",category:"sessions",label:"aw/hooks · delegated",
     description:"Session · child Agent hooks · pvl-a1",action:"v15-show-agent",value:"hooks"},
  {id:"pty-build",category:"terminals",label:"build",
     description:"PTY · pvl-x2 · owner",action:"v15-palette-pty",value:"build"},
  {id:"pty-tests",category:"terminals",label:"tests",
     description:"PTY · pvl-a1 · viewer",action:"v15-palette-pty",value:"tests"},
  {id:"pty-debug",category:"terminals",label:"debug",
     description:"PTY · pvl-x2 · paused",action:"v15-palette-pty",value:"debug"},
  {id:"pty-shell",category:"terminals",label:"shell",
     description:"PTY · pvl-a1 · idle",action:"v15-palette-pty",value:"shell"},
  {id:"slash-think",category:"commands",label:"/think",
     description:"Agent command · reasoning effort (demo only)",action:"v15-palette-command",value:"/think"},
  {id:"slash-theme",category:"commands",label:"/theme",
     description:"Agent command · theme (demo only)",action:"v15-palette-command",value:"/theme"},
  {id:"slash-settings",category:"commands",label:"/settings",
     description:"Agent command · settings (demo only)",action:"v15-palette-command",value:"/settings"},
  {id:"agent-root",category:"agents",label:"coordinator",
     description:"● working · aw/main · pvl-x2",action:"v15-show-agent",value:"coordinator"},
  {id:"agent-runtime",category:"agents",label:"runtime lead",
     description:"● working · child of coordinator · pvl-x2",action:"v15-show-agent",value:"runtime"},
  {id:"agent-hooks",category:"agents",label:"hooks lead",
     description:"◐ awaiting review · child · pvl-a1",action:"v15-show-agent",value:"hooks"},
 ];
 const windows=s.v15Tabs.map((t,i)=>({
   id:"window-"+t.key,category:"windows",label:t.label,
   description:(t.kind==="layout"?"Window/layout":t.kind==="file"?"Standalone File tab":"Standalone Diff tab")+
      " · aw/main"+(i===0?" · current group":""),
   action:"v15-select-tab",value:t.key,
 }));
 return [...staticEntries.slice(0,5),...windows,...staticEntries.slice(5)];
}
function results(raw){
 const q=parse(raw);
 const phrase=q.body.toLocaleLowerCase();
 const all=entries().filter(item=>!q.category||item.category===q.category);
 const matches=all.filter(item=>!phrase ||
    [item.label,item.description,item.value||""].some(f=>f.toLocaleLowerCase().includes(phrase)));
 // Use predictable top-ranked search, no implicit learning from runtime/tool state.
 return matches.slice(0,50);
}
function markup(){
 if(s.popover!=="palette")return "";
 const q=s.paletteQuery||"";
 const parsed=parse(q),matches=results(q);
 const selected=matches.length?
   Math.min(Math.max(0,s.paletteSelected||0),matches.length-1):0;
 s.paletteSelected=selected;
 let categories=CATEGORIES.map(x=>A("v15-palette-category",
   '<span class="'+x.css+'">'+e(x.prefix)+'</span> '+e(x.label),
   "v15-palette-category "+(parsed.category===x.id?"selected":""),
   x.prefix,"Filter "+x.label)).join('<span class="v15-palette-cat-separator">·</span>');
 let rows=matches.slice(Math.max(0,selected-7),Math.max(0,selected-7)+10)
   .map((item,index)=>{
     const absolute=Math.max(0,selected-7)+index,cat=CAT[item.category];
     const id=e(item.id);
     return A("v15-palette-run",
       '<span class="v15-palette-row-kind '+cat.css+'">'+e(cat.label)+'</span>'+
       '<span class="v15-palette-row-body"><span class="v15-palette-row-label">'+e(item.label)+'</span>'+
       '<span class="v15-palette-row-description">'+e(item.description)+'</span></span>',
       'v15-palette-result '+(absolute===selected?"selected":""),item.id,
       item.label+" · "+item.description);
   }).join("");
 if(!matches.length)rows='<div class="v15-palette-empty">No matching results. Switch a category or adjust the query.</div>';
 return '<div class="v15-palette-layer">'+
   A("v15-palette-close","","v15-palette-backdrop",null,"Dismiss command palette")+
   '<section class="v15-palette-dialog" role="dialog" aria-modal="true" aria-label="Command Palette">'+
   '<header class="v15-palette-header"><span class="strong">Command Palette</span>'+
     '<span class="faint">Ctrl+Shift+P / F1 · Esc</span></header>'+
   '<div class="v15-palette-search"><span class="accent">›</span>'+
    '<input type="text" data-palette-input aria-label="Search command palette" '+
    'placeholder="Search actions, files, sessions, windows, terminals, commands, agents" '+
    'value="'+e(q)+'" autocomplete="off" spellcheck="false"></div>'+
   '<div class="v15-palette-categories" role="group" aria-label="Filter command palette">'+categories+'</div>'+
   '<div class="v15-palette-results" role="listbox" aria-label="Palette results">'+rows+'</div>'+
   '<footer class="v15-palette-footer"><span>↑ ↓ select · Enter open · Esc dismiss</span>'+
     '<span>'+matches.length+' results'+(parsed.category?' · '+e(CAT[parsed.category].label):'')+'</span></footer>'+
   '</section></div>';
}
U.v15Palette={CATEGORIES,parse,rewrite,entries,results,markup};
})();
