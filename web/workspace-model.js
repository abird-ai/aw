// Pure browser-local layout state. No credentials, DOM, sockets or model calls.
// The native SDK remains independent; this bounded schema is browser-local v1.
export const KINDS = ['conversation','file','diff','terminal','logs','metrics','tree','site','help'];
export function initialLayout() {
  return {schema:1,next:2,root:{group:'g1'},focused:'g1',navigator:'files',side:'left',
    groups:{g1:{tabs:['v1'],active:'v1'}},views:{v1:{kind:'conversation',title:'Conversation'}}};
}
export function validateLayout(s) {
  if(s.schema!==1||!Number.isSafeInteger(s.next)||s.next<2)throw Error('Invalid layout version/counter');
  const ids=[];const visit=(n,d=0)=>{if(d>8)throw Error('Too many splits');if(n.group){ids.push(n.group);if(!s.groups[n.group])throw Error('Unknown group');}
    else {if(!['columns','rows'].includes(n.axis)||!Number.isFinite(n.ratio)||n.ratio<.15||n.ratio>.85)throw Error('Invalid split');visit(n.first,d+1);visit(n.second,d+1);}};
  visit(s.root);if(!ids.length||ids.length>4||new Set(ids).size!==ids.length||ids.length!==Object.keys(s.groups).length||!ids.includes(s.focused))throw Error('Invalid group membership');
  const views=[];for(const g of Object.values(s.groups)){if(g.tabs.length&&!g.tabs.includes(g.active))throw Error('Active tab missing');for(const id of g.tabs){if(!s.views[id])throw Error('Unknown view');views.push(id);}}
  if(views.length>24||new Set(views).size!==views.length||views.length!==Object.keys(s.views).length)throw Error('Invalid view membership');
  for(const view of Object.values(s.views))if(!KINDS.includes(view.kind)||typeof view.title!=='string'||view.title.length>250)throw Error('Invalid view');
  if(!['files','git','sessions'].includes(s.navigator)||!['left','right','hidden'].includes(s.side))throw Error('Invalid navigator');return s;
}
function replaceNode(node,group,fn){if(node.group)return node.group===group?fn(node):node;return {...node,first:replaceNode(node.first,group,fn),second:replaceNode(node.second,group,fn)};}
function prune(node,id){if(node.group)return node.group===id?null:node;const first=prune(node.first,id),second=prune(node.second,id);return !first?second:!second?first:{...node,first,second};}
export function reduceLayout(before,action) {
  validateLayout(before);const s=structuredClone(before),group=action.group||s.focused;
  const g=s.groups[group];if(!g)throw Error('Unknown group');
  const next=p=>p+(s.next++);
  switch(action.type){
    case 'focus':s.focused=group;break;
    case 'select':if(!g.tabs.includes(action.view))throw Error('Unknown tab');g.active=action.view;s.focused=group;break;
    case 'open':{
      const view=action.target;if(!view||!KINDS.includes(view.kind))throw Error('Unsupported view');
      const existing=Object.entries(s.views).find(([,v])=>v.kind===view.kind&&(view.kind==='conversation'||(view.resource&&v.resource===view.resource)));
      if(existing&&!action.duplicate){const id=existing[0];const owner=Object.entries(s.groups).find(([,x])=>x.tabs.includes(id));owner[1].active=id;s.focused=owner[0];break;}
      const id=next('v');s.views[id]=view;g.tabs.push(id);g.active=id;s.focused=group;break;
    }
    case 'close':{const id=action.view||g.active;if(!g.tabs.includes(id))throw Error('Unknown tab');g.tabs=g.tabs.filter(v=>v!==id);delete s.views[id];if(g.active===id)g.active=g.tabs.at(-1)||null;break;}
    case 'split':{if(Object.keys(s.groups).length>=4)throw Error('At most four content groups in this slice');const id=next('g');s.groups[id]={tabs:[],active:null};s.root=replaceNode(s.root,group,n=>({axis:action.axis||'columns',ratio:.5,first:n,second:{group:id}}));s.focused=id;break;}
    case 'move':{const source=Object.entries(s.groups).find(([,x])=>x.tabs.includes(action.view));if(!source)throw Error('Unknown tab');const original=source[1];original.tabs=original.tabs.filter(v=>v!==action.view);if(original.active===action.view)original.active=original.tabs.at(-1)||null;g.tabs.push(action.view);g.active=action.view;s.focused=group;break;}
    case 'reorder':{if(!g.tabs.includes(action.view))throw Error('Unknown tab');g.tabs=g.tabs.filter(v=>v!==action.view);g.tabs.splice(Math.max(0,Math.min(action.index,g.tabs.length)),0,action.view);break;}
    case 'close_group':{if(Object.keys(s.groups).length===1)throw Error('Keep one content group');const dest=Object.keys(s.groups).find(x=>x!==group);s.groups[dest].tabs.push(...g.tabs);s.groups[dest].active||=g.active;delete s.groups[group];s.root=prune(s.root,group);s.focused=dest;break;}
    case 'navigator':s.navigator=action.kind;break;
    case 'side':s.side=action.side;break;
    case 'resize':{const walk=(n,path)=>{if(!path.length){if(n.group)throw Error('No splitter');return {...n,ratio:Math.min(.85,Math.max(.15,action.ratio))};}const [p,...rest]=path;if(!['first','second'].includes(p)||n.group)throw Error('Invalid splitter');return {...n,[p]:walk(n[p],rest)};};s.root=walk(s.root,action.path||[]);break;}
    default:throw Error('Unknown layout action');
  }
  return validateLayout(s);
}
export function groupRects(layout,width,height,maximize=false){
  validateLayout(layout);if(!Number.isFinite(width)||!Number.isFinite(height)||width<0||height<0)throw Error('Invalid viewport');
  const out={},dividers=[];let fits=true;
  function walk(n,x,y,w,h,path=[]){if(n.group){out[n.group]={x,y,width:w,height:h};if(w<260||h<210)fits=false;return;}
    const col=n.axis==='columns',size=(col?w:h)-6,a=Math.max(0,Math.floor(size*n.ratio)),b=Math.max(0,size-a);
    dividers.push({path,axis:n.axis,x:col?x+a:x,y:col?y:y+a,width:col?6:w,height:col?h:6,origin:col?x:y,extent:col?w:h});
    walk(n.first,x,y,col?a:w,col?h:a,[...path,'first']);walk(n.second,col?x+a+6:x,col?y:y+a+6,col?b:w,col?h:b,[...path,'second']);}
  walk(layout.root,0,0,width,height);
  if(maximize||!fits)return {groups:{[layout.focused]:{x:0,y:0,width,height}},dividers:[],collapsed:!fits};
  return {groups:out,dividers,collapsed:false};
}
export function changeRows(items,tree=false,filter='',folded=new Set()){
  const needle=filter.toLowerCase(),out=[];
  for(const group of ['Conflicts','Staged','Unstaged','Untracked']){
    const matches=items.filter(i=>i.group===group&&String(i.path||i.label).toLowerCase().includes(needle));if(!matches.length)continue;
    out.push({kind:'heading',key:group,label:`${group} (${matches.length})`});const dirs=new Set();
    for(const item of matches){const parts=String(item.path||item.label).split('/'),leaf=parts.pop();let parent='',hidden=false;
      if(tree)for(const name of parts){parent=parent?parent+'/'+name:name;const key=group+'/'+parent;if(!dirs.has(key)&&!hidden){out.push({kind:'folder',key,label:name,depth:parent.split('/').length-1,folded:folded.has(key)});dirs.add(key);}if(folded.has(key))hidden=true;}
      if(!hidden)out.push({...item,kind:'item',label:tree?leaf:item.label,depth:tree?parts.length:0});
    }
  }return out;
}
export function sessionTarget(s){return JSON.stringify([s?.instance,s?.saved?.session_id,s?.saved?.branch_id]);}
export function sessionMetrics(s){const turns=Array.isArray(s?.turns)?s.turns:[];return {completed:turns.filter(t=>t.outcome==='completed').length,failed:turns.filter(t=>t.outcome==='failed').length,cancelled:turns.filter(t=>t.outcome==='cancelled').length,active:s?.pending?1:0,observed_invocations:turns.reduce((n,t)=>n+(t.invocations?.length||0),0),scope:'current displayed ancestry, not unique global billing'};}
