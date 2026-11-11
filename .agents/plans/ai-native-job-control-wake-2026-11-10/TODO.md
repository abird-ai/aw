# AI-native Jobs, Control, wake and Agent execution — implementation queue

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Base: AW 4a23e92a8f516971bb232b98e4fc000554bcc4ba; dotlink b31683c79a8ffd7c5540ac9388e607d8af6c7ffa
Status: **design finalized; implementation not yet authorized**.

Read [DESIGN.md](DESIGN.md) before source work. Preserve other owners' changes and both coordinator-only global TODO indexes.

## Committed

None. No implementation work is authorized or committed for this plan.

## Uncommitted

### Phase 0 — finalized design + execution-start reconciliation

- [x] Finalize canonical call/IR: `[operation,args]`; Graph node `[operation,args,when?]`; grouped object-wire `op` is the sole secondary form.
- [x] Finalize public semantic namespaces: `agents.run` and `jobs.list|get|output|ack|send|cancel|subscribe`.
- [x] Define one Agent implementation: root/child/recursive/team/planned worker differ by lineage/delegation bindings, not runtime type.
- [x] Define identities: durable Agent ID resolves Session/lineage; one Agent Turn creates one Job; settled follow-up creates a fresh Job.
- [x] Keep `background` as direct-call response disposition only; Graph nodes never carry it; keep `batch` as the top-level Graph constructor.
- [x] Define argument partition: ExecutionArgs vs title metadata vs background disposition vs wake subscription.
- [x] Define semantic operation IDs `jobs.*` / `agents.run` and kernel-safe `jobs_*` / `agents_run`; old `job_*` is compatibility only.
- [x] Define `jobs.send` inbound vs `jobs.output` outbound, including shell stdout/stderr and bounded Agent public result without transcript/reasoning leakage.
- [x] Define fresh Agent ID recovery and exposed Agent Job -> Agent ID projection under continuation authority.
- [x] Define immutable Graph + interactive Agent/Process nodes; `batch` and `jobs.*` are not Graph nodes initially.
- [x] Define typed retained Graph results so `bread` is not permanently direct-only.
- [x] Define PreparedOperation + non-authoritative EffectSet; no Composite authority.
- [x] Define parent-owned wake and now/step/turn semantics; settled/attention only initially.
- [ ] At implementation start, re-read live AW/dotlink HEAD and dirty work and reconcile source/ABI changes without overwriting other work.
- [ ] Confirm actual retained interactive stdin support and platform differences in current AW/dotlink Process owners; unsupported hosts hide/reject it.
- [ ] Confirm concrete authenticated parent binding for every wake-capable host; unavailable hosts hide/reject wake.
- [ ] Agree explicit vNext rollout/compatibility feature boundary before changing advertised production schemas.

### Phase 1 — canonical operation/effect/authority layer

- [ ] Add Effect::Control across kernel/host mappings while preserving no_std+alloc and exact source/site/epoch grants.
- [ ] Add canonical OperationId + PreparedOperation resolution. Atomic tools keep fixed effects; grouped jobs/agents and batch resolve exact operations/nodes before authorization.
- [ ] Normalize object-wire `jobs({op,...})` / `agents({op,...})` immediately into the compact canonical IR.
- [ ] Partition constructor args into ExecutionArgs / Metadata / Disposition / Subscription before semantic hash/effect planning.
- [ ] Preserve host transport replay invariants: conflicting redelivery metadata/disposition/subscription under one request identity fails closed or retains the first admitted envelope; it never respawns work.
- [ ] Add internal non-authoritative EffectSet for Graph/audit/scheduling; never add Composite as an authority effect.
- [ ] Separate filesystem/data ReadOnly from independently granted Control, Process and Agent delegation; no old configuration widens silently.
- [ ] Reuse current ToolPolicy, Envelope, TurnJournal and ToolBindingAuthority; no parallel admission/policy system.

### Phase 2 — Agent unification and constructors

- [ ] Refactor/expose delegated execution around the same Agent/TurnMachine/runtime; delegation service only allocates lineage, narrows authority, binds Session and invokes the existing controller.
- [ ] Introduce semantic `agents.run`: no id => fresh authorized child Agent; id => new Turn/new Job on an existing authorized Agent.
- [ ] Ensure a fresh Agent ID is returned/recoverable even for synchronous completion; expose Job ID only when the Job is exposed/handed off.
- [ ] Exposed Agent Jobs project Agent ID through jobs.list/get only under continuation authority.
- [ ] Keep bash/pwsh, batch and agents.run as direct constructors.
- [ ] Add optional title/background/wake to direct constructors; background affects response disposition only.
- [ ] Graph node schemas reject/omit background; interactive remains semantic; wake is extracted to node-scoped subscription metadata.

### Phase 3 — common Jobs API + observation

- [ ] Implement grouped jobs list/get/output/ack/cancel using the shared JobManager and exact per-op authority.
- [ ] Add strict jobs.send and jobs.subscribe.
- [ ] Add node-scoped get/output/ack/send/subscribe where meaningful; one-node Jobs may omit node.
- [ ] Keep jobs.get status-only and jobs.output data-only.
- [ ] Preserve shell stdout/stderr/result and bounded Agent public result; never expose private transcript/reasoning/provider deltas as implicit Job output.
- [ ] Implement bounded wait_ms on get/output with race-free revision notification, truthful timeout/EOF and no execution-lock blocking.
- [ ] Maintain native MCP Task/replay/output-owner obligations and legacy job_* compatibility until explicit schema cutover.

### Phase 4 — one factual event + wake path

- [ ] Project factual Job/Graph/Agent/Process settlement into one bounded event vocabulary; no new event database.
- [ ] Support only settled and authenticated Agent attention wake events initially; no progress/output-driven paid wake loops.
- [ ] Register initial wake before roots can start/settle.
- [ ] jobs.subscribe atomically replaces/clears one caller-scoped future rule; post-terminal subscribe is non-retroactive.
- [ ] Convert an event into Agent Input only through the authenticated parent's current wake grant/owner/writer/policy; child Notify alone never steers parent.
- [ ] Preserve now/step/turn relative to the receiving Agent; reject stale exact boundaries rather than silently retargeting.
- [ ] Seal synchronous-result vs handoff races so synchronously delivered result is not duplicated as a wake.
- [ ] Shell settlement event occurs only after Process outcome classification and writer/output quiescence.
- [ ] Standalone dotlink without an authentic AW parent remains passive.

### Phase 5 — interactive input and cross-Agent communication

- [ ] jobs.send Agent target uses current ActorMessage::Input and exact active Turn, source sequence, safe boundary and delegation authority.
- [ ] jobs.send shell target requires retained interactive stdin and exact process.input authority; text/eof only initially.
- [ ] jobs.send is inbound control; jobs.output remains outbound stdout/stderr/result.
- [ ] Default child->parent communication remains factual attention/settlement + parent-owned subscription. Direct child/peer->parent jobs.send requires explicit communication topology/grant.
- [ ] Settled Agent receives no jobs.send; follow-up uses fresh `agents.run({id,task})`.
- [ ] Keep Runtime out-of-band cancellation/owner controls serviceable while model/tool/approval/Graph child waits; no lock-held await deadlock.

### Phase 6 — immutable Graph host Agent nodes

- [ ] Extend Graph NodeTool/NodePlan with a transport-neutral host-node contract; aw-tools stays unaware of provider/Agent internals.
- [ ] Represent Agent semantic node as `["agents.run",args,when?]`; Graph owns async execution, so background is invalid/absent.
- [ ] Keep `batch` top-level only and `jobs.*` direct-only initially; no nested Graph/Job control nodes.
- [ ] Admit fixed task/Agent id/interactivity/edges/budgets/delegated ceiling before roots start; live steering never rewrites topology/initial execution args.
- [ ] Reconcile one Agent Turn's factual Session outcome into one Graph node outcome, including unknown/cancel/skip.
- [ ] Address live Graph child by Job ID + node; durable Agent ID remains distinct and disclosed only with continuation authority.
- [ ] Only AW-capable hosts advertise Agent nodes; standalone dotlink remains native/shell unless a real authenticated AW bridge exists.

### Phase 7 — typed binary/media Graph results

- [ ] Generalize retained Graph result body to Text / ExactRead / Bytes / Media.
- [ ] Enable bread(format=mcp) in Graph without lossy text flattening.
- [ ] Preserve host MIME/format/size/authority validation, raw-byte paging, SHA/MIME metadata, ACK/replay ownership and transport ceilings.
- [ ] Keep node outcome/dependency semantics independent from presentation.

### Phase 8 — dynamic policy + SDK projection

- [ ] Dynamic evaluators receive PreparedOperation/Graph summary and return bounded allow/deny/narrow/ask data only; static grants remain the ceiling.
- [ ] Support host-selected LocalRules/Program/QuickJS/Python/MCP/Model/Pipeline/Human evaluators without a second Agent/policy runtime.
- [ ] Revalidate narrowed operations fully; policy failure/timeout/revocation is fail-closed and cannot mint authority.
- [ ] Project typed SDK methods `tools.jobs.get`, `tools.agents.run`, etc. to the same canonical tuple; generic `tools.call([op,args])` uses identical validation/policy/journal/executor.
- [ ] No ambient QuickJS/Python filesystem/subprocess/network escape.
- [ ] Optional passive SDK watch is observation sugar only; no second event bus/model-facing wait tool.

### Phase 9 — dotlink adapter migration and release readiness

- [ ] Integrate reviewed AW shared crates through one exact Git revision.
- [ ] Add thinnest grouped jobs object-wire projection; normalize immediately to canonical jobs.* IDs and preserve replay/MCP Task/output semantics.
- [ ] Do not expose agents.run or wake in standalone dotlink unless a real authenticated AW host integration exists.
- [ ] Preserve sandbox/profile/deny/protected-path/transport/session semantics.
- [ ] Run available native platform gates and leave unavailable Windows/macOS/Bubblewrap execution gates explicitly open.
- [ ] Update only this new plan as implementation lands; sealed DL-150 history stays intact.
- [ ] Separate explicit authorization required for commit, push, version, tag, release or deployment.

## Tests and verification

- [ ] Compact IR/object-wire equivalence and exact operation/effect/grant tests.
- [ ] Root/child Agent implementation parity and no second Agent executor.
- [ ] ReadOnly FS + permitted Control/delegation without filesystem Write/shell.
- [ ] Background/foreground same-execution handoff and replay identity.
- [ ] Graph rejects/omits background, batch and jobs.* nodes while interactive Agent node remains steerable.
- [ ] jobs.send/jobs.output directionality; shell stdout/stderr and Agent public result.
- [ ] Live Agent steering vs settled Agent fresh-run boundary and Agent-ID recovery.
- [ ] Fast completion/writer quiescence/wake dedup/post-terminal subscribe.
- [ ] get/output wait lost-notification race, timeout/current status and EOF.
- [ ] Typed bread/media Graph output + raw-byte ACK/replay ownership.
- [ ] Dynamic policy never widens static authority.
- [ ] Existing AW kernel/no-std/minimal/stateless/shared tools/platform/Agent gates.
- [ ] Existing dotlink Job/Graph/MCP/OpenAI replay/stdio and strict lint/check gates.
- [ ] Schema bytes/token estimate and representative model calls compared to old job_* + subagent surface; avoid per-field fixture explosion.
- [ ] No earlier canonical plan/source/unrelated user work modified inadvertently.

## Execution guard

Design is finalized only. Before source edits, re-read live AGENTS.md and current source/dirty work, then obtain explicit implementation authorization. The coordinator alone edits the global plan indexes.
