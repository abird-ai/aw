import test from 'node:test';
import assert from 'node:assert/strict';
import { acceptSnapshot, acceptDelta, canSend, isSubmitKey, submission, utf8Bytes, MAX_PROMPT_BYTES, accountChoices, draftTargetChanged, submissionSettled, principalLabel } from '../web/model.js';
const snapshot = (extra={}) => ({ instance:'i', sequence:0, revision:0, turns:[], model:'fixture', profile:'test', pending:null, ...extra });
test('send disabled before pairing', () => assert.equal(canSend(null,false,'hello'),false));
test('send disabled while busy', () => assert.equal(canSend(snapshot({pending:{run_id:1}}),false,'hello'),false));
test('send disabled for empty text', () => assert.equal(canSend(snapshot(),false,'  '),false));
test('UTF-8, not JS length, bounds prompts', () => { assert.equal(utf8Bytes('🐦'),4); assert.equal(canSend(snapshot(),false,'🐦'.repeat(MAX_PROMPT_BYTES/2)),false); });
test('normal Enter submits', () => assert.equal(isSubmitKey({key:'Enter',isComposing:false,shiftKey:false}),true));
test('Shift Enter inserts newline', () => assert.equal(isSubmitKey({key:'Enter',shiftKey:true}),false));
test('IME Enter does not submit', () => {assert.equal(isSubmitKey({key:'Enter',isComposing:true}),false);assert.equal(isSubmitKey({key:'Enter',keyCode:229}),false);});
test('stale snapshots do not roll back same owner', () => { const now=snapshot({sequence:8}); assert.equal(acceptSnapshot(now,snapshot({sequence:2})),now); });
test('new owner snapshot accepted', () => assert.equal(acceptSnapshot(snapshot({sequence:8}),snapshot({instance:'other'})).instance,'other'));
test('malformed snapshot rejected', () => assert.throws(() => acceptSnapshot(null,{})));
test('contiguous correct run delta appends', () => {const r=acceptDelta(snapshot({pending:{run_id:1,answer:'a'}}),1,{run_id:1,text:'b'});assert.equal(r.snapshot.pending.answer,'ab');assert.equal(r.applied,true);});
test('wrong run delta requires snapshot', () => assert.equal(acceptDelta(snapshot({pending:{run_id:1,answer:''}}),1,{run_id:2,text:'x'}).resync,true));
test('event gap requests snapshot', () => assert.equal(acceptDelta(snapshot({pending:{run_id:1,answer:''}}),2,{run_id:1,text:'x'}).resync,true));
test('duplicate delta ignored', () => assert.equal(acceptDelta(snapshot({sequence:4}),4,{run_id:1,text:'x'}).applied,false));
test('request binds exact revision and id', () => assert.deepEqual(submission(snapshot({revision:7}),'hello','same-id'),{request_id:'same-id',expected_revision:7,prompt:'hello'}));
test('submission cannot mutate the input snapshot', () => {const s=snapshot(); submission(s,'hi','id'); assert.equal(s.revision,0);assert.equal(s.turns.length,0);});

const accounts = [{id:'mine',allowed_models:['m1'],account_kind:'personal',inference_executor:'device'},{id:'team',allowed_models:['m2'],account_kind:'shared-server',inference_executor:'server'}];
test('account selection only includes allowed account and model',()=>{const s=snapshot({accounts});assert.deepEqual(submission(s,'hello','id',{account_id:'mine',model:'m1'}),{request_id:'id',expected_revision:0,prompt:'hello',account_id:'mine',model:'m1'});});
test('unlisted account cannot be silently used',()=>assert.throws(()=>submission(snapshot({accounts}),'hi','id',{account_id:'someone-else',model:'m1'})));
test('unlisted model cannot fall back',()=>assert.throws(()=>submission(snapshot({accounts}),'hi','id',{account_id:'mine',model:'other'})));
test('shared account must be explicitly selected',()=>{const body=submission(snapshot({accounts}),'hi','id',{account_id:'team',model:'m2'});assert.equal(body.account_id,'team');});
test('credential actor and billing fields are not copied from client choice',()=>{const body=submission(snapshot({accounts}),'hi','id',{account_id:'mine',model:'m1',api_key:'SENTINEL',actor:'other',payer:'victim',endpoint:'https://wrong.invalid'});assert.deepEqual(Object.keys(body).sort(),['account_id','expected_revision','model','prompt','request_id']);assert(!JSON.stringify(body).includes('SENTINEL'));});
test('uncertain mutation blocks send until explicit inspection acknowledgement',()=>assert.equal(canSend(snapshot({needs_reconciliation:true}),false,'continue'),false));
test('choices are inert metadata projections',()=>{const before=JSON.stringify(accounts);assert.equal(accountChoices(snapshot({accounts})).length,2);assert.equal(JSON.stringify(accounts),before);});
test('each tab keeps its own requested account choice',()=>{const shared=snapshot({accounts});const alice=submission(shared,'a','id-a',{account_id:'mine',model:'m1'});const bob=submission(shared,'b','id-b',{account_id:'team',model:'m2'});assert.equal(alice.account_id,'mine');assert.equal(bob.account_id,'team');assert.equal(shared.revision,0);});

test('account choice preserves only public payer metadata',()=>{const projected=accountChoices(snapshot({accounts:[{id:'team',allowed_models:['m'],payer:{kind:'shared',id:'project',api_key:'sentinel'},api_key:'sentinel'}]}));assert.deepEqual(projected[0].payer,{kind:'shared',id:'project'});assert(!JSON.stringify(projected).includes('sentinel'));});

const saved = (extra={}) => snapshot({saved:{session_id:'session-a',branch_id:'branch-a',head:'node-a',branch_revision:2},...extra});
test('saved submission binds exact branch and session without copying owner metadata',()=>{const s=saved();s.saved.api_key='NEVER_COPY';const b=submission(s,'hello','same');assert.equal(b.session_id,'session-a');assert.equal(b.branch_id,'branch-a');assert(!JSON.stringify(b).includes('NEVER_COPY'));});
test('malformed saved target is rejected on snapshot',()=>assert.throws(()=>acceptSnapshot(null,snapshot({saved:{session_id:'s'}}))));
test('same branch live revision does not retarget existing draft',()=>assert.equal(draftTargetChanged(saved(),saved({sequence:4,revision:8}),'draft'),false));
test('different saved branch requires draft retarget consent',()=>assert.equal(draftTargetChanged(saved(),saved({saved:{session_id:'session-a',branch_id:'branch-b',head:'node-b'}}),'draft'),true));
test('new owner with old draft requires explicit recovery',()=>assert.equal(draftTargetChanged(saved(),saved({instance:'new-owner'}),'draft'),true));
test('empty draft needs no retarget warning',()=>assert.equal(draftTargetChanged(saved(),saved({instance:'new-owner'}),'  '),false));
test('first authorized snapshot does not invent a previous draft target',()=>assert.equal(draftTargetChanged(null,saved(),'draft'),false));
test('settled persistent receipt is an existing result not a fresh stream',()=>assert.equal(submissionSettled({state:'settled'}),true));
test('recovered interrupted receipt is not replayed as a new model call',()=>assert.equal(submissionSettled({state:'interrupted'}),true));
test('accepted or absent receipt is not a completed run',()=>{assert.equal(submissionSettled({state:'accepted'}),false);assert.equal(submissionSettled(null),false);});

test('authenticated actor label is a projection, never a request field', () => { const s=snapshot({identity:{actor:'internal:engineering:alice'}}); assert.equal(principalLabel(s),'internal:engineering:alice'); assert.equal(submission(s,'hi','req').actor,undefined); });
test('peer label cannot change requested account', () => {const s=snapshot({identity:{actor:'internal:engineering:bob'},accounts}); const p=submission(s,'hello','req',{account_id:'team',model:'m2'});assert.equal(principalLabel(s),'internal:engineering:bob');assert.equal(p.account_id,'team');assert.equal(p.payer,undefined);});
test('malformed labels have a safe display fallback',()=>{for(const actor of ['<script>','a\nb','', 'a'.repeat(129),null])assert.equal(principalLabel(snapshot({identity:{actor}})),'Authenticated participant');});
test('no snapshot does not invent a Unix actor',()=>assert.equal(principalLabel(null),'Authenticated participant'));
