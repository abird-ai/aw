# AI-native unified Jobs + Agent runtime — implementation plan

Base: 4a23e92a8f516971bb232b98e4fc000554bcc4ba

Design authority: [DESIGN.md](DESIGN.md)
Overview: [README.md](README.md)

## Committed

None. This plan-writing pass changed documentation only.

## Uncommitted

### Phase 0 — source reconciliation and cutover inventory

- [ ] Re-read live AW/dotlink source, current tests and dirty work before edits; preserve unrelated changes and reserved plan ownership.
- [ ] Inventory every currently registered model tool, semantic operation, ToolEffect, Graph node registry entry and SDK projection affected by the cutover.
- [ ] Map current JobManager/JobHost/JobRuntime/Process ownership, output owner/ACK/replay/MCP Task paths and Agent Session/Actor/Turn cancellation paths.
- [ ] Identify all team/plan/review_release/subagent registrations and non-tool consumers before deleting or de-registering them.
- [ ] Record exact implementation order in this plan before source edits if live architecture has materially changed from the draft baseline.

### Phase 1 — canonical operation language and flat provider surface

- [ ] Introduce/normalize canonical OperationId names: fs.*, bash/pwsh/powershell/batch/agent, jobs.*.
- [ ] Keep provider model tools flat: filesystem names unchanged; agent; jobs_list/get/read/ack/send/cancel/subscribe.
- [ ] Preserve original provider ToolCall wire name/args/call ID separately from canonical resolved operation evidence.
- [ ] Remove grouped agents/jobs op-discriminator schema path from the new implementation; no dynamic per-op ToolEffect repair is needed.
- [ ] Reuse one PreparedOperation/ToolPolicy/Envelope/journal executor path for provider, SDK and Graph callers.
- [ ] Add one AW-host-owned ExecutionStrategy/OperationRouter seam after semantic/revision/ToolPolicy admission and before scheduling/effects. Initial strategies are Local and DelegateAgent; it is not a model field, OperationId, ToolEffect, Job kind or aw-tools Agent dependency.
- [ ] Allow deterministic policy or a bounded guardian/routing model to recommend strategy/target, but require trusted host policy to authorize the route independently; routing evidence can never mint provider/account/payer/tool/resource authority.
- [ ] When guardian/router uses inference, run it as a separately authorized bounded auxiliary invocation with exact InvocationIdentity/purpose/payer/usage evidence and immutable receipt. Parent ToolCall replay reuses committed decision evidence; failed/unknown decision inference cannot authorize execution or trigger blind paid rerouting.
- [ ] Capture a host-only execution footprint/class separately from semantic ToolEffect. Strategy selection must occur before direct tool-wave scheduling; a semantic Read delegated through an opaque Agent never enters a Read parallel wave merely because its semantic effect is Read.
- [ ] Add SDK namespaces tools.fs and tools.jobs; keep tools.agent/bash/pwsh/powershell/batch atomic.
- [ ] Add SDK-only tools.call([operation,args]) over the same resolver.
- [ ] Keep model-facing Graph leaf names identical to flat provider tool names (read/edit/.../agent/bash); map them immediately to canonical fs.* semantic IDs through the same resolver before preparation, retaining the exact compact tuple/when language.
- [ ] Add first-class Control effect atomically across aw-agent-kernel Effect, aw-agent-core ToolEffect, shared aw-tools effect where used, ToolSpec/Envelope/grant matching, adapter conversions/ranks, journal/store/portable projections and scheduler/read-wave classification. Control is neither Read nor Write, never joins Read waves, and does not inherit filesystem Write authority.
- [ ] Add exhaustive Control regression tests for default Off/ReadOnly, source/site/epoch/actor/resource grants, serialization/projection, parallel scheduling and Graph outer-Control-vs-leaf authorization.
- [ ] Preserve no_std/minimal/stateless boundaries for shared operation/effect types.

### Phase 2 — remove superseded public/workflow surfaces

- [ ] Remove model registrations and new canonical support for old job_start/job_restart/job_* compatibility names.
- [ ] Replace old job_start semantics only with the original typed constructor + background=true; do not create another start engine.
- [ ] Remove jobs.output and any jobs.restart proposal; jobs.read is the only new data-plane operation, and a re-run is a fresh explicit replay-protected constructor under current authority.
- [ ] Remove separate agents.* model/API surface and grouped agents({op}) / jobs({op}) paths.
- [ ] Remove subagent/team/plan/review_release model tools; delete implementations/tests if they have no independent non-tool product consumer, otherwise keep only internal non-canonical helpers.
- [ ] Hard pre-1.0 cutover: delete obsolete provider registrations, old job_*/grouped decoders, restart execution code, legacy-only adapters/tests. No migration, drain subsystem, dual schema or legacy replay implementation; preserve current-schema correctness.
- [ ] Before **any** old-state reset, enforce explicit operator-verified disposable isolated dev/test runtime scope, blocked new ingress, terminated original runtime/supervised children, and **no live/unknown external effects, connected clients, replay claims, MCP Tasks or output/ACK owner obligations**. If unprovable, abort reset in that environment or choose a separately isolated deployment; never erase unrelated durable Agent/Session records or reinterpret/re-execute an old serialized request.
- [ ] Keep the **new implementation's** crash/startup/replay/private-artifact/MCP Task/ACK correctness and unrelated durable Agent/Session history; removing legacy code must not weaken current invariants.
- [ ] Ensure removal does not create a second planner/Agent path or retain dead compatibility helpers with no independent product consumer.

### Phase 3 — typed immutable handles, contextual overlays and heterogeneous Job routing

- [ ] Define reusable host-neutral DomainId/PrincipalId/ProjectId/AgentId/TurnId/JobId/NodeId/Revision plus ExecutionRef/ResolvedExecutionRef and Job read/send/control result types.
- [ ] Implement typed segments d:/i:/p:/a:/t:/j:/n: with owner-chosen immutable canonical handles; allocator format is open, but canonical spelling is exactly 1..64 delimiter-safe ASCII bytes (alphanumeric first; then alphanumeric/._~-), case-sensitive with no Unicode/percent/escape aliases.
- [ ] Reserve d:0/i:0/p:0/a:0 as concrete provisioned canonical default objects, not caller-relative aliases, implicit privilege or creation. /d:<h>/i:0 always names the same default principal object for every caller; contextual omission/inheritance represents the current principal. t/j are owner-chosen; Graph n:<coordinate> is 1-based canonical decimal and n:0/leading zeros are invalid.
- [ ] Require slash-prefixed canonical refs to be fully resolved /d:<h>/i:<h>/p:<h>/a:<h> plus optional t/j/n suffix; reject partial /a:0, /i:<h>, etc.
- [ ] Implement contextual refs from exact current Agent context: omitted/"." self, "^" parent, typed structural overlays, t:/j:/n: execution suffixes; reject "..", wildcard/range, duplicates and structural backtracking.
- [ ] Validate execution suffixes through owner-typed transitions: Agent->t, Graph Job->n, owner->j; allow n/j or deeper composition only where the authoritative owner exposes that child relation, and reject invalid transitions before dispatch.
- [ ] Contextual overlay: inherit ancestors above first explicit d/i/p/a, set supplied levels, reset missing descendants to their fixed canonical :0 objects. Without current Agent, all-zero is parser syntax only: the fixed defaults must exist and be authorized or fail; never remap i:0 to the authenticated caller, create a principal/project/Agent from lookup, or grant authority. Standalone dotlink requires explicit live-session Job ref.
- [ ] Canonicalize contextual refs exactly once before authority/replay/effect and journal both requested_ref + fully resolved canonical resolved_ref.
- [ ] Define a thin owner/router contract that resolves fully resolved canonical refs to authoritative owners without copying mutable lifecycle state.
- [ ] Keep aw-tools Graph/finite Job state authoritative for Graph execution and retained node output.
- [ ] Keep Process custody/streams with the original Process owner.
- [ ] Keep Agent lifecycle/Turns/history with AW Session/Actor/TurnJournal.
- [ ] Make jobs.list/get/read/ack/send/cancel/subscribe dispatch through exact resolved ref + current authority.
- [ ] Keep bounded identity/routing/tombstones only: a live descendant survives finite ancestor expiry via its own authoritative owner; release routing once all live/protocol/replay obligations retire. No shadow transcript or finite Process resurrection.

### Phase 4 — Process and Graph Jobs on the new API

- [ ] Keep direct bash/pwsh/powershell as one Job-backed Process execution with optimistic synchronous completion and same-Job handoff.
- [ ] Reserve worst-case finite-Job recoverability capacity before first effect: replay identity plus exposure/list slot when lost-receipt promotion may be required; never silently omit a live exposed Job because capacity filled later.
- [ ] Preserve DL-150 lost-receipt behavior: durable replay owner keeps ambiguous hidden synchronous work recoverable; if no replay owner remains before positive delivery, promote/expose the same finite Job for jobs.list recovery; never discard or rerun effectful work.
- [ ] Background/automatic handoff receipts return compact shortest-safe ref + kind/state + current rev (+ progress when useful), with no argument echo, so jobs.get(ref,rev,wait_ms) can follow without an extra status round.
- [ ] Preserve stream writer quiescence, output retention, timeout/cancellation and unknown stop semantics.
- [ ] Keep batch as one immutable Graph Job; background=true only controls direct response disposition.
- [ ] Remove model-visible job_start; batch({ops,background:true}) and shell(...,background:true) cover the immediate-exposure use cases with the same constructor/replay path.
- [ ] Remove model-visible restart/re-execution from JobManager interfaces exposed through the new API; retained definitions remain evidence only, never executable authority.
- [ ] Rename/project job output reads through jobs.read without changing raw byte offsets, stream sealing, replay owners or ACK watermarks.
- [ ] Preserve Graph node ownership via directly derivable n:<1-based Graph coordinate> under its Job; no extra node allocation/mapping or discovery round. Do not create redundant child j:<handle> resources where Graph node custody suffices.
- [ ] Preserve typed Text/ExactRead/Bytes/Media retained result support and make bread(format=mcp) Graph-eligible when exact typed retention is implemented.

### Phase 5 — stable Agent identity, namespace and coordination parent

- [ ] Implement Agent as a long-lived Job kind backed by exactly one existing owned Session+branch+Actor binding.
- [ ] Reserve a:0 as root/default Agent in every d/i/p namespace; host creates/resolves a:0, while agent(...) allocates another replay-stable immutable a:<handle> in the current d/i/p namespace.
- [ ] Keep one public Agent identity only: stable a:<handle> ref; no second Agent ID/mapping DB.
- [ ] Durably bind new a:<handle> + Session/branch + parent/origin metadata + first Turn admission before paid provider effect; matching replay never creates another Agent/Turn.
- [ ] Record immutable canonical origin_agent/origin_turn refs plus origin_step/origin_call separately from mutable authorized parent/coordinator relation; never encode Turn identity as an assumed numeric ordinal.
- [ ] Implement `^` as an atomically admitted current coordination parent. Reparenting is authenticated host-only and journaled, rejects self/cycles/stale parent generation and never renames the Agent or implies cancellation/tool grant.
- [ ] Seal public lifecycle vocabulary separately from DL-150 outcomes: running|waiting|idle|blocked|cancelling|cancelled|closing|closed|settled; Agent uses running|waiting|idle|blocked|closing|closed and finite executions use running|waiting|cancelling|cancelled|settled.
- [ ] Preserve one writer and durably ordered Turn generations per Agent; each Turn has stable t:<handle> identity while any monotonic sequence/ordinal remains separate metadata, so handle encoding is not mandated.
- [ ] Do not terminally settle the Agent when one Turn completes; return to idle unless closing/blocked.
- [ ] Ensure runtime restart rediscovery uses authoritative Session ownership without claiming an interrupted in-flight Turn survived when it did not.
- [ ] Reuse authoritative SessionStore's existing immutable Session/Agent creation/commit ordering as the Agent jobs.list exposure key; do not add a second Agent exposure DB or migration-only exposure field. Commit a:<handle> + Session/branch + parent/origin + first Turn before paid inference.
- [ ] Lost initial Agent response recovers the same Agent via constructor replay while retained, and via jobs.list from authoritative SessionStore afterward; first synchronous success includes the compact Agent ref so the normal path needs no discovery round.

### Phase 6 — Turn refs, Step metadata, revisions and Agent control

- [ ] Make each addressable execution resource return externally meaningful rev; only control/read-relevant changes advance it.
- [ ] jobs.send/cancel/subscribe use supplied rev as exact compare-and-act; jobs.get uses rev as conditional observation/change token; jobs.read/ack deliberately omit rev and rely on exact ref + cursor/offset/watermark for replayable paging/ACK.
- [ ] Omitted rev remains valid; SDK auto-propagates rev only for deliberate compare-and-act controls, never silently onto status/read/ACK observations.
- [ ] Bind every active Agent ToolCall to exact canonical Agent ref/Turn/Step/call identity in journal evidence.
- [ ] Preserve Step semantics as model invocation plus complete proposed tool receipt phase, but expose no public s:<handle> ref.
- [ ] Expose exact Turns as stable t:<handle> refs; never reuse/retarget a Turn handle within the Agent.
- [ ] Advance Turn rev on externally meaningful control-state changes including Step transitions so rev can fence the observed Step without a Step resource.
- [ ] Implement jobs.send modes auto|queue|steer|interrupt with Agent root auto->queue and active t:<handle> auto->steer.
- [ ] Implement Agent queue as durable FIFO future Input that never mutates the active Turn; successful jobs.send proves Input custody only, never future inference/model/provider/payer/tool authority.
- [ ] Before every queued Input claim, atomically revalidate current branch writer/source lineage/model/provider/account/payer/budget/ToolPolicy/resource ceilings and blocked/closing/reconciliation state; only then journal replay-stable Turn admission before paid provider effect.
- [ ] Apply one explicit finite **causal-work budget** to *all* model-origin continuation: self-send, peer-send, explicit Agent/Graph child creation, transparent ExecutionStrategy DelegateAgent work and subscription wake. Require delegated continuation authority, reserve bounded Input/work slots before accepting, debit root Turns/payer allowance at admission, propagate immutable causal roots across descendants, and never let a model/router reset its own quota. Same-owner/root meter in existing AW journal; unsupported atomic cross-host metering fails closed.
- [ ] Implement steer after the **complete model invocation and tool-receipt Step** (not BeforeToolBatch); journal immutable source/issuer/request-identified steering Input into that exact Turn for the next Step. Revalidate current model/payer/writer/tool authority; unknown prior effects block auto-continuation. No provider-signed mutation/replacement Turn.
- [ ] Implement interrupt at earliest safe boundary with truthful cancellation/settlement of started effects, then continue the same exact Turn with the journaled steering Input only when safe.
- [ ] queue on an active t:<handle> is the only send mode that creates future-Turn input; steer/interrupt remain within the addressed Turn and never retarget a settled Turn.
- [ ] Make stale t:<handle> or stale supplied rev fail without retargeting to a later Turn/Step.
- [ ] Replay-claim all effectful send before Input/Process effects: matching already-admitted control returns original receipt **before any newly stale rev rejection**; never duplicates Agent queued/steered Input, Process stdin/EOF or Graph delegated input. Changed duplicate fails closed.
- [ ] Define stable **host-envelope invocation identity** for provider ToolCalls (scoped Session/Turn/source/epoch), SDK prepared/retry context and authenticated bridge requests before any Control effect. Retries reuse the identical envelope ID; a new ID means new intent. Missing stable retry identity forbids blind retries after ambiguous delivery; ambiguous stdin writes are fenced unknown, never repeated.
- [ ] jobs.cancel remains idempotent exact-ref intent; jobs.subscribe replacement/clear is replay-idempotent; jobs.ack relies on monotonic exact ref/view/offset rather than a second replay ledger.
- [ ] Implement jobs.cancel({ref,rev?}) with required explicit ref; "." intentionally means self. Agent root closes Agent; t:<handle> cancels only that exact Turn; no public Step cancellation target.
- [ ] Stop admitting new Turn work after cancel intent; settle already-started effects truthfully. Whole-Agent close factually settles queued-but-unconsumed Inputs as unconsumed/closed rather than silently dropping or executing them.
- [ ] Return safely cancelled Turn to idle only when no unresolved external-effect ambiguity remains.
- [ ] Put Agent into blocked/needs-inspection state rather than auto-running later Turns after unresolved unknown effects.

### Phase 7 — child execution, transparent delegation, coordination and ownership

- [ ] Implement DelegateAgent as ordinary AW Agent/Turn execution: target an existing currently admissible authorized Agent or allocate a new ordinary Agent through the canonical Agent path. Do not create a delegate/subagent resource type or second runner. Newly allocated completion-only delegates normally close after the delegated Turn unless host policy explicitly retains/reuses them, preserving identity/history without polluting live discovery.
- [ ] Freeze ExecutionStrategy, exact delegate target/Agent factory decision, route policy/version, delegated authority ceiling and execution footprint before delegated paid/external effects. Matching replay reattaches the same Agent/Turn; after admission never silently reroute/fallback because policy changed or delivery became ambiguous.
- [ ] DelegateAgent requires independent current Agent/provider/account/model/payer/causal-work/tool/resource authority in addition to the original semantic operation grant. Execution mechanisms may add separately authorized inference/private-scratch/process costs, but the delegate may not introduce user-visible/persistent semantic side effects outside the operation's trusted implementation contract; a semantic Read never becomes filesystem Write merely because it is delegated.
- [ ] Build a bounded host-authored delegate task/context projection from the admitted semantic operation plus explicitly delegated context. Do not inherit parent transcript, provider-private context, credentials or unrelated branch state by default.
- [ ] Keep the original ToolCall ID/name/canonical OperationId/semantic ToolEffect/admitted args/result ceiling/result contract immutable. Record strategy/delegate Agent/Turn/route provenance separately in journal evidence.
- [ ] Parent ToolCall/Step waits without parent model inference while the delegate runs, remains cancellable, and resumes only after the exact child Turn factually settles. Child may use a different authorized model/provider/tool set without changing parent Agent/Turn/model identity.
- [ ] Adapt child output into the original ToolCall receipt only through the semantic operation's trusted result adapter. Free-form child text can never fabricate success for exact native read/write/bash/effect contracts; required native/effect evidence must exist. Result hooks/guardian inference may separately transform only the model-visible projection under their own bounded InvocationIdentity; projection failure never rewrites factual receipt/status.
- [ ] Transparent delegation is completion-only by default. It may not block awaiting the synchronously blocked parent model. If a delegate needs request/response attention and lacks an independent controller, transparent routing is ineligible: select another completion-capable strategy or use the explicit Agent/Job handoff/control path.
- [ ] Parent cancellation propagates to the delegated Turn only while explicit ToolCall ownership remains. Preserve committed/unknown child effects truthfully; unresolved delegated ambiguity cannot become parent success.
- [ ] Allocate independently durable Process/Graph/Wait children as local j:<handle> beneath owning Turn; Graph nodes use **n:<1-based coordinate>** beneath the Graph Job, without a second node-ID map.
- [ ] Agent creation allocates stable a:<handle> at d/i/p Agent-namespace level instead of a child j:<handle>; record creator Turn/Step/ToolCall as origin metadata.
- [ ] Record exact parent Agent/Turn/Step/ToolCall -> child resolved ref ownership/evidence for durable nested work.
- [ ] Keep atomic bounded filesystem calls as ordinary ToolCall receipts rather than public Job proliferation.
- [ ] Allow synchronous finite nested Jobs to remain hidden when no independent control/handoff is needed.
- [ ] Expose the same hierarchical child ref when work hands off or independent control is needed, without creating another execution.
- [ ] Keep path ancestry/origin distinct from current ownership; cancellation propagates only to children/Agents still explicitly owned by that exact Turn.
- [ ] Preserve committed/unknown child effects even if parent cancellation succeeds.
- [ ] Support wait-capable child execution without holding global Actor/runtime locks or consuming model inference while blocked.
- [ ] Define future durable watch/wait Job extension point but do not add a watch/until core model tool in this implementation.
- [ ] Keep Notify as factual event only, not a Job or implicit model invocation.

### Phase 8 — jobs.list/get/read observation + typed data plane

- [ ] Implement jobs.list({cursor?,limit?}) as flat per-Job-authorized discovery **strictly in current authenticated canonical AW d/i/p namespace** (or dotlink live session); no mutable-parent or cross-principal expansion. Merge authoritative Agent creation sequence with finite Job exposure sequence, immutable newest-first/canonical tie-break, never subtree enumeration.
- [ ] Make list cursor stateless/owner-verifiable and bind exact d/i/p scope or standalone runtime generation, authenticated listing authority epoch, exposure position/tie-breaker and projection version; scope/runtime/authority mismatch or tampering rejects, and each page revalidates current per-Job authority.
- [ ] Implement jobs.get({ref?,rev?,wait_ms?}) as status/progress only with owner-specific lost-wakeup-safe conditional observation: changed rev returns current snapshot, equal rev may wait, timeout returns latest factual snapshot; progress preserves DL-150 absolute cumulative/fixed-or-unknown-total grammar and final result authority.
- [ ] Define jobs.read natural defaults: **Process/Process-backed node stdout**, native/Graph result, Agent latest committed Turn result. Explicit invalid views reject; omitted ref means current Agent only in authenticated AW context.
- [ ] Advertise compact typed ReadView result|stdout|stderr|messages|tool_calls|tool_outputs|trace, SendInput={text}|{eof:true}, and optional **AW Agent-only `filter`** ("default"|"none"|custom category booleans/tools/exclude). `filter` and `view` are mutually exclusive; standalone dotlink omits Agent history filtering. Reject unsupported combinations before publication.
- [ ] Canonicalize contextual refs once for jobs.read; do not add revision tokens to history/stream paging. Every page revalidates current disclosure/owner authority while exact ref + cursor/offset preserve continuation identity.
- [ ] Process views: stdout/stderr/result with raw retained-byte offset, exact base64/readable text projection, wait_ms, EOF/truncation and no implicit ACK; presentation encoding never changes next offset and ACK never erases immutable truncation facts.
- [ ] Graph views: whole result plus directly derivable n:<1-based coordinate> result/stdout/stderr depending on node kind.
- [ ] Agent views: result/messages/tool_calls/tool_outputs/trace.
- [ ] Support exact Agent Turn selection by t:<handle>; Step remains metadata in Turn/tool records; root history uses opaque cursor/limit.
- [ ] Keep raw-byte offset distinct from opaque record/history cursor.
- [ ] Keep independent byte/record limits and atomic history records; oversized first record fragments only as typed payload with stable record identity. Opaque cursor binds exact canonical Agent/Turn ref, normalized **view or filter** (including category/tool selection/inversion), record/byte position, projection version and current disclosure epoch; stale/mismatched rejects. Always advance with copyable `more cursor=...` or actionable error, never false completeness.
- [ ] Use one small Agent-history filter instead of include lists/top-level tools: omission/"default" latest result, "none" all currently authorized public categories, custom true category booleans with optional exact flat tool names, `exclude:true` subtracts matches. Require a true category; reject tools without selected tool category. Positive forbidden categories deny; inverse never widens disclosure. Single `view` remains the shortest ordinary call; no regex/glob/query engine.
- [ ] Project only allowlisted public SessionStore/Turn/ToolRecord fields with **current per-category and per-resource disclosure**. `view:"trace"` requires permission for every constituent it reveals; filtered projections include only current authorized content, never widen via `"none"`/inverse, and deduplicate final answer when result+messages are selected. Historical secrets/revoked content/provider-private material remain inaccessible.
- [ ] Never expose hidden reasoning/provider_context/system/private host context/credentials or inaccessible branch content.
- [ ] Keep active/unreconciled Turn reads fail-closed unless the owner can prove a safe committed public prefix.
- [ ] Define one transport-neutral JobReadPage typed representation and derive both model and SDK projections from it.
- [ ] Model renderer should prefer shortest safe contextual refs; SDK may additionally expose fully resolved canonical_ref without parsing model text.
- [ ] Ensure typed media uses native SDK bytes where possible and preserves MIME/hash/exact byte semantics.
- [ ] Make jobs.ack explicit ref/view/offset only, no rev; watermark cannot exceed bytes retained **and delivered/available to this owner's exact output lease**. Lost-response retries are idempotent; other MCP Task/replay owners and immutable terminal facts survive cleanup. Agent history never ACK-consumed.

### Phase 9 — Job-kind events and unified wake modes

- [ ] Advertise result|settled|attention only. Process supports result/settled; Agent supports attention; AW Graph supports attention iff admitted topology contains an **attention-capable** Agent node. **Completion-only is default** unless the captured host delegation permits a blocking parent-attention/request-response path; no new model flag. The same predicate governs proactive handoff and runtime prohibition of blocked-parent waits. Standalone dotlink invents no Agent/Graph attention.
- [ ] Agent emits result per committed Turn; settled only when the long-lived Agent Job closes.
- [ ] Process/Graph finite completion may produce result+settled at one checkpoint; coalesce duplicate wake admission for one recipient/rule.
- [ ] attention remains authenticated Agent-originated only; stdout/tool text cannot forge it.
- [ ] Register wake rule before the relevant execution can emit an event.
- [ ] jobs.subscribe requires explicit source; empty events clears. Treat events as a bounded unique semantic set: reject duplicate event names and canonicalize admitted/replay-hashed order as result,settled,attention. Reject self-source and bound **cross-Agent causal cycles**, hop depth, pending Inputs and automatic paid-Turn budgets via existing Actor owner. Downstream model-issued jobs.send/child actions inherit the automatic wake's causal chain; only authenticated external Input starts a fresh chain. No default rule or second event DB.
- [ ] Serialize subscription register/replace/clear and factual event commit at the **same source-owner linearization point**. Capture matching recipient-bound subscription generation and mode **at event commit**, use that in bounded delayed-delivery intent and dedupe source-ref/event/event_rev/recipient/rule-generation. A later clear affects only later committed events; current recipient authority may suppress old delivery, but must not reinterpret it. Finite result+settled coalesces; conflicting subscribed modes reject.
- [ ] For steer/interrupt wake delivery, atomically resolve/bind the receiving Agent's current exact t:<handle>/rev at delivery; subscriptions never carry source-side Step fields.
- [ ] Revalidate recipient owner/writer/lineage/grant/budget at delivery; unknown delivery never blindly retries paid inference.
- [ ] Preserve passive progress/Notify as non-inference observations.

### Phase 10 — Agent Graph nodes and deterministic footprint

- [ ] Register agent as an AW-hosted Graph leaf through a host-neutral node adapter; standalone dotlink cannot fake it.
- [ ] Graph Agent node represents one admitted Turn; node terminal fact follows that Turn, not long-lived Agent Job lifetime.
- [ ] Return/expose the stable a:<handle> Agent ref from node result only when authorized/needed for continuation; never encode creator Graph/Turn ancestry into Agent identity.
- [ ] Derive one conservative Agent Graph footprint from its **full delegated authority ceiling** (native FS, Process/sandbox reach, external/network/MCP effects, child delegation), never task text or advisory shell writes.
- [ ] Treat unbounded Agent Process/network/provider/external effects as **opaque conflicts with potentially interacting Graph nodes**, not safe to overlap by default; explicit Graph dependencies add workflow ordering, without claiming outside-service transactions.
- [ ] If bounded footprint is unavailable, conservatively conflict with relevant filesystem-observing/mutating nodes.
- [ ] Revalidate delegated ceiling/topology before first effect/Start; drift/revocation settles unlaunched.
- [ ] Make Graph preflight two-phase and root-free: first complete semantic/schema/ToolPolicy/topology/when/ceiling preflight for **all** nodes; then select/authorize/freeze ExecutionStrategy and captured footprint for **all** nodes; only then build conflicts/start roots. DelegateAgent keeps original coordinate/operation/outcome/more/incomplete/result ceilings. Any guardian/router inference in phase 2 is separately journaled preparation usage, never a leaf result/effect; route failure/unknown starts no roots.
- [ ] At Graph preflight, capture actual parent-blocking capability from host delegation; **ordinary completion-only children remain optimistic foreground**. If an attention-capable child could require the synchronously blocked sole parent, reserve/expose and hand off the **same Graph Job before roots run**. Apply the same rule to direct `agent(...)` with a blocked launching parent. Authenticated Graph attention surfaces actionable node/child refs; runtime rejects unauthorized parent waits and controller loss cannot deadlock.
- [ ] Prevent concurrent writer ownership of the same Agent Job/Session branch.

### Phase 11 — dotlink thin integration

- [ ] Consume one reviewed exact AW Git revision; no copied/sibling shared engine.
- [ ] Expose standalone jobs_list/get/read/ack/send/cancel for Process/Graph Jobs. Advertise jobs_subscribe and constructor wake only through a real authenticated AW Agent host; standalone dotlink never invents a wake recipient.
- [ ] Retain flat filesystem and shell/batch provider surfaces with canonical dotted semantic mapping below the wire.
- [ ] Remove old job_* compatibility routes and job_restart/job_start from the new public API.
- [ ] Preserve OpenAI replay, MCP Task, OAuth/profile denies, protected paths, sandbox and output ownership.
- [ ] Standalone dotlink must not create Agent Jobs, Agent Turn history or Agent wake recipient identities; omit wake from advertised shell/batch schemas when no current receiving Agent exists.
- [ ] If an authenticated AW bridge is present, canonicalize/route Agent refs to the AW owner rather than copying Agent state into dotlink.
- [ ] Keep transport stdout protocol-clean and all platform security invariants unchanged.

### Phase 12 — documentation and public cutover

- [ ] Update live tool/API docs to only the new surface after implementation is accepted.
- [ ] Remove stale examples that teach old job_*, subagent/team/plan/review_release/grouped op/restart APIs.
- [ ] Document one model spelling across direct tools and Graph (flat read/edit/...); SDK generic calls, policy and internal evidence use dotted fs.* semantic IDs through one mechanical mapping table.
- [ ] Document jobs.read views and typed SDK return shapes.
- [ ] Document typed immutable handles, fully resolved slash-canonical refs, contextual overlay resolution, d:0/i:0/p:0/a:0 defaults, ".", "^", t:/j:/n: execution segments, stable Agent identity/origin/parent separation, send modes, wait/wake behavior and the AW-only Local/DelegateAgent ExecutionStrategy seam.
- [ ] Do not rewrite historical sealed plans; link them only as history if needed.

## Tests and verification

### Schema/model surface
- [ ] Exactly 25 maximum canonical core model tool names before effective-grant filtering.
- [ ] No grouped agents/jobs op schemas, no model-visible generic call, no legacy aliases.
- [ ] Generic jobs.* schemas stay compact and strongly typed: ReadView, Agent-only `filter` presets/custom booleans/exclude, SendInput, Event and send-mode enums/oneOf. `filter` and `view` mutually exclude; standalone dotlink omits Agent history filter; no untyped strings or huge specialization union.
- [ ] Non-destructive self-oriented calls may omit ref -> current Agent; jobs.cancel always requires explicit ref and "." is explicit self.
- [ ] Slash refs are fully resolved canonical identities; non-slash refs are contextual overlays; local work does not repeat d/i/p/a prefixes.
- [ ] jobs.send mode enum is exactly auto|queue|steer|interrupt; no live/ready/next/after/finish/complete/now/step/turn aliases.
- [ ] Provider wire names resolve to one canonical operation and immutable original ToolCall remains evidenced.
- [ ] SDK tools.fs.*, tools.jobs.*, tools.agent and tools.call use the same resolver.
- [ ] SDK propagates known rev automatically only for intentional compare-and-act controls; jobs.get rev is conditional observation, jobs.read/ack have no rev. All optional-rev calls work without rev.
- [ ] Graph uses the same flat model leaf names as direct calls and exact DL-150 tuple/when grammar; internal resolved evidence is canonical fs.* with no second model-facing spelling.
- [ ] Model jobs.get rendering emits shortest safe contextual refs and compact current/actionable descendants; jobs.list is flat exact-scope exposed-root discovery with deterministic newest-first ordering and no recursive subtree semantics.
- [ ] jobs.list cursor binds canonical d/i/p or standalone runtime generation, listing authority epoch, exposure position/tie-breaker and projection version; cross-scope/runtime/authority replay and tampering reject.
- [ ] Public lifecycle spellings are exactly running|waiting|idle|blocked|cancelling|cancelled|closing|closed|settled; no run/terminal/rN aliases in model/SDK output.

### Authority/effect regression
- [ ] Control exists as a first-class exhaustive variant in kernel, agent-core, shared tools where applicable, adapters, policy/grants, Envelope/journal/store/portable projections and scheduling classification.
- [ ] Control never inherits Read or Write authority, never joins parallel Read waves, and default Off/ReadOnly remains fail-closed except for explicitly granted Control operations.
- [ ] Graph outer Control admission never substitutes for leaf Read/Write/Process authorization; source/site/epoch/actor/resource matching remains exact.

### ExecutionStrategy / transparent delegation
- [ ] ExecutionStrategy is AW-host-only and absent from model/provider schemas, OperationId, ToolEffect and pure aw-tools. Initial strategies are Local and DelegateAgent.
- [ ] Original ToolCall/canonical operation/admitted args/semantic ToolEffect/result ceiling/result contract remain identical across Local versus DelegateAgent; strategy/provenance is separate journal evidence.
- [ ] Strategy is selected before direct wave scheduling. Graph uses whole-graph two-phase preflight: all semantic leaves validate first, then all strategies/route authorities/footprints freeze, then conflict planning/root start. DelegateAgent uses its captured execution footprint, so semantic Read through opaque delegation never joins a Read wave or bypasses Graph conflict ordering.
- [ ] DelegateAgent route requires independent current Agent/provider/account/model/payer/causal-work/tool/resource authority; original tool grant cannot implicitly authorize inference/delegation. The delegate's semantic side effects remain bounded by the original trusted operation implementation contract even when its execution footprint is broader.
- [ ] Delegated context is bounded and host-authored; parent transcript/provider-private context/credentials/unrelated branch state are absent unless explicitly delegated.
- [ ] Parent model performs no invocation while a transparent delegate runs; child may use another authorized model/provider/tool set while parent Agent/Turn/model identity remains unchanged.
- [ ] Delegate result satisfies the original semantic result/effect contract through a trusted adapter. Free-form child text cannot fabricate native read/write/bash success; visible result transformation never changes factual receipt.
- [ ] Strategy admission is replay-stable: duplicate parent ToolCall reuses any committed routing-decision receipt and reattaches the same delegate Agent/Turn, changed router policy does not retarget, and ambiguous started delegation never auto-falls back to Local/another Agent.
- [ ] Parent cancellation reaches the delegated Turn only while exact ownership remains; committed/unknown child effects stay factual and unresolved ambiguity never becomes parent success.
- [ ] Transparent delegation is completion-only by default and cannot block on the synchronously blocked parent model. Interactive request/response requires an independent controller or the explicit Agent/Job handoff path.
- [ ] Existing-Agent routing admits only an exact safe new Turn under current writer policy; a busy/current Turn is never silently injected or replaced.
- [ ] DelegateAgent work debits the same authenticated causal-work/payer allowance as explicit model-created Agent/Graph work and cannot mint a new root.

### DL-150 filesystem/Graph regression
- [ ] 13 filesystem schema/conformance suite remains green.
- [ ] shallow ls / recursive find / grep / exact mutation / paging / incomplete semantics unchanged.
- [ ] deterministic native conflict ordering and pre-Start topology revalidation remain green.
- [ ] Graph captured result ceilings cannot be changed by outer response budgets.
- [ ] Graph leaf ExecutionStrategy is frozen during preflight; delegated footprint may conservatively add conflicts but never changes node coordinate, semantic operation, outcome, when, more/incomplete or captured work/result ceilings.
- [ ] typed bread/media Graph result preserves exact semantics.

### Process Job
- [ ] direct shell synchronous result remains one Job execution.
- [ ] long/background shell exposes same Job, not another command; lost/abandoned foreground receipt promotes the same Job only when replay ownership cannot recover hidden delivery.
- [ ] stdout/stderr/result read paging and ACK exact: raw offsets independent of encoding, ACK monotonic/idempotent/no-rev, cannot exceed retained bytes, and immutable truncation/status survives release.
- [ ] cancel/timeout/unknown stop and writer quiescence correct; ambiguous process stop never becomes confirmed cancelled.

### Agent identity / namespace
- [ ] Agent admission atomically persists stable a:<handle> + Session/branch + parent/origin + first Turn before paid inference; jobs.list uses existing immutable SessionStore creation/commit ordering rather than a new exposure table/field.
- [ ] lost fresh-agent response recovers the same Agent through constructor replay while retained, then jobs.list from authoritative SessionStore after replay retirement; no second Agent exposure database and no duplicate paid effect.
- [ ] canonical handle lexical grammar accepts valid numeric/slug/UUID/hash-like spellings, rejects delimiters/control/Unicode/escape aliases, and compares byte-exact case-sensitive identity.
- [ ] a:0 is the reserved stable root/default Agent; additional Agents allocate replay-stable immutable handles in the same d/i/p namespace.
- [ ] Agent remains idle/addressable after Turn completion.
- [ ] canonical Agent identity does not change when coordinator/parent changes.
- [ ] `^` resolves current authorized parent atomically; no parent, denied cross-principal parent, stale reparent, self-parent or coordination cycle fail closed without retargeting already admitted messages.
- [ ] origin Agent/Turn/Step/ToolCall remains immutable and distinct from parent relation.
- [ ] runtime restart does not silently resurrect unknown in-flight Turn.
- [ ] no second Agent ID/transcript database.

### Typed refs / contextual overlays / revisions / Agent control
- [ ] omitted ref and "." resolve current Agent; "^" resolves parent; typed non-slash refs apply contextual overlay semantics; slash refs require fully resolved d/i/p/a.
- [ ] d:0/i:0/p:0/a:0 are fixed existing canonical default objects, never caller-relative aliases or implied creation. /d:<h>/i:0 has one identity for all callers; current principal comes only from contextual inheritance. t/j may use owner-chosen 0; n:0/leading-zero Graph coordinates reject.
- [ ] overlay resolution inherits ancestors above first explicit structural level and resets omitted descendants to fixed :0 canonical objects; contextless all-zero is parsing only, and missing/unauthorized defaults fail closed without remapping identity, creation or cross-tenant privilege.
- [ ] structural order d->i->p->a is enforced; no duplicates, backtracking, "..", wildcard/range or alias layer.
- [ ] canonical handles/refs never retarget or reuse within their containing namespace, even after retention expiry; expired stale refs remain permanently gone. Graph n coordinates may repeat only under a different Graph Job parent because that is a distinct namespace.
- [ ] jobs.send/cancel/subscribe supplied current rev succeeds and stale rev has no effect; jobs.get rev/wait returns newer factual snapshot rather than stale and cannot lose a transition; jobs.read/ack expose no rev.
- [ ] Turn rev advances on externally meaningful Step transitions so rev can fence the observed Step.
- [ ] Agent root auto/queue accepts exactly one durable FIFO Input; claim starts exactly one next Turn only after current writer/provider/account/payer/budget/ToolPolicy/resource authority succeeds.
- [ ] revoked/changed authority after queue acceptance cannot run paid inference; Agent blocks with durable Input preserved until authoritative resolution.
- [ ] duplicate queued continuation/steering or Process stdin delivery returns original factual receipt **before rechecking a newly stale rev**; no double Input, Turn or written bytes; a new request with stale rev still fails.
- [ ] SDK/bridge controls have a stable host-envelope invocation identity established before effect, reused across retries (provider IDs scoped by Session/Turn/source); a new ID means new intent. No identity/proof means no blind retry of ambiguous effects, especially Process stdin.
- [ ] A model-origin self-send, peer-send or child Agent/Graph launch shares the same authenticated causal root and finite queued Input/automatic Turn/child-depth/payer allowance as subscription wakes; models cannot reset or split budgets. Quota exhaustion rejects further autonomous Input without erasing factual work; host must atomically meter cross-owner roots or refuse.
- [ ] active t:<handle> auto resolves to steer; explicit steer awaits complete current Step **including all tool receipts**, journals source/issuer/control identity within same Turn, revalidates before next Step; unknown prior effect blocks automatic continuation.
- [ ] explicit interrupt uses earliest safe boundary and preserves truthful started effects.
- [ ] queue on active t:<handle> never injects into the current Turn.
- [ ] stale Turn/rev cannot affect a later Turn/Step.
- [ ] exact t:<handle> cancel cancels only that Turn; stale rev leaves newer state untouched.
- [ ] jobs.cancel with omitted ref is schema-invalid; explicit ref "." closes current Agent, prevents future Turns and journals all queued-but-unconsumed Inputs as unconsumed/closed.
- [ ] unknown external effect blocks automatic future Turn.

### Child Jobs / waits / coordination
- [ ] Process/Graph durable child refs follow exact owner transitions (for example t:5/j:1 and t:5/j:2/n:4); n:4/j:1 is accepted only when the node owns independent durable child work; invalid t/j/n transitions fail before dispatch.
- [ ] Agent ToolCall creation returns stable a:<handle> rather than nesting Agent under creator Turn.
- [ ] origin/ToolCall correlation remains exact for both child j:<handle> and stable a:<handle> Agents.
- [ ] synchronous finite child can remain hidden without losing factual receipt.
- [ ] handoff exposes the same canonical child ref.
- [ ] Turn cancel propagates only to currently owned descendants/Agents, never merely by path ancestry/origin.
- [ ] cancelled/unknown child outcome remains factual in parent Turn; confirmed admitted Turn cancellation maps to fail/skip cancelled as appropriate, while ambiguous effects settle unknown/mixed and block the Agent.
- [ ] waiting child uses no paid model inference and holds no global runtime lock.
- [ ] wait completion cannot resume a stale/replaced Turn.

### jobs.read
- [ ] get remains status-only.
- [ ] read remains data-only and never implicitly ACKs.
- [ ] Process/Graph raw offsets stable and wait_ms lost-wakeup safe.
- [ ] Agent default returns latest committed result only.
- [ ] messages returns sanitized public conversation with no duplicate result.
- [ ] tool_calls and tool_outputs correlate IDs and exact canonical operations.
- [ ] trace is chronological and deduplicated.
- [ ] Agent-only `filter` offers omitted/"default" result, "none" all currently authorized public categories, and custom true category booleans/optional flat `tools` plus `exclude:true` inversion; require at least one true category, reject tools without selected tool category, never bypass disclosure or redaction. Model tool names resolve through verified semantic identity, not untrusted stored arguments.
- [ ] Agent root history cursor pagination and exact t:<handle> oversized-read continuation advance without false completeness; cursors bind canonical ref/normalized view **or filter**, projection version/current disclosure epoch and can point inside typed record payloads; stale/mismatched cursor rejects. Model shows copyable `more cursor=...` only.
- [ ] active/unreconciled/compacted history truthfully fails or marks incomplete.
- [ ] Per-category/per-resource current disclosure for result/messages/tool_calls/tool_outputs/trace, including revoked historical sensitive data; `trace` never grants its constituent categories. No reasoning/provider_context/system/credential/private branch leakage.
- [ ] ACK rejected for Agent durable history views.
- [ ] SDK typed objects and model renderer agree on semantics and pagination.
- [ ] typed media/bytes are lossless in SDK path.

### Events/wake
- [ ] Agent Turn completion emits result without settling Agent Job; Agent whole close emits settled.
- [ ] constructor wake is registered atomically before first effect so fast completion cannot be missed.
- [ ] finite result+settled coalesces duplicate wake; conflicting modes on the same terminal checkpoint reject at subscription registration.
- [ ] attention authentic and cannot be forged from stdout/tool output; AW Graph child attention projects n:<coordinate> plus an authorized Agent/Turn ref for parent action, standalone dotlink never claims it.
- [ ] subscription persists across Agent Turns until replace/clear/terminality/revocation. Event commit and subscription replace/clear share **one authoritative source-owner linearization point**; event captures matching subscription generation/mode; delayed delivery never applies a later rule, and current recipient authority can still suppress it. Dedupe source/event/rev/recipient/rule generation, preserve already accepted Inputs. Bound downstream self/peer sends, child launches and wakes by the same causal-work budget.
- [ ] subscription replacement/clear/non-retroactive semantics and auto|queue|steer|interrupt delivery modes are correct.
- [ ] revoked/missing recipient or later inference/payer/tool authority fails/blocks without forged exactly-once paid inference.

### Agent Graph node
- [ ] Agent node terminal tracks admitted Turn, not Agent lifetime.
- [ ] stable Agent a:<handle> continuation ref survives Graph node settlement and remains independent of creator Graph/Turn retention.
- [ ] captured full delegated authority footprint (native FS, shell/Process, network/MCP, delegated children) prevents timing-dependent Graph overlap; task text/advisory writes never prove safety.
- [ ] opaque Process/external effects conservatively conflict with interacting native/Process/Agent Graph nodes; explicit dependencies give additional ordering but no outside transaction.
- [ ] external service effects remain nontransactional even with serial Graph dependencies; the scheduler never silently overlaps opaque potentially interacting work.
- [ ] same-branch concurrent writer denied.
- [ ] Ordinary completion-only Agent children remain eligible for optimistic foreground execution; **captured host delegation** permits parent-blocking attention only explicitly and is immutable for admitted nodes. Such attention-capable child with sole blocked parent forces proactive same-Job handoff at Graph preflight; direct foreground agent creation follows the same rule. Parent-blocking waits without authority reject rather than deadlock; event exposes actionable child ref with no second model engine.

### Removal
- [ ] no old job_start/job_restart/job_* public tool routes in new API; background=true on original shell/batch constructors covers immediate exposure. **Before resetting any incompatible transient pre-vNext state**, prove isolated disposable dev/test scope, closed ingress, terminated old owned processes, zero external/unknown paid/effect/Task/replay/ACK obligations; otherwise abort reset or isolate a new deployment. No migration or compatibility runner.
- [ ] no jobs.output/restart; fresh explicit constructor is the only re-execution path and retained Job definition never grants execution authority.
- [ ] no agents.* public API/grouped op tool.
- [ ] no subagent/team/plan/review_release model tools.
- [ ] no compatibility aliases, decoders, migration or drain path; obsolete pre-vNext implementation code/state adapters and compatibility tests are deleted in the hard cutover.
- [ ] new-schema startup/replay/MCP Task/ACK/filesystem recovery behavior remains intact; unrelated durable Agent/Session data is not deleted merely because the Job API breaks.

### Platform/build
- [ ] aw-tools unit/conformance suite.
- [ ] secured Unix backend suite.
- [ ] Windows backend/cross-build on capable environment.
- [ ] aw-tools-adapter suite.
- [ ] Agent core/runtime/store suites.
- [ ] AW CLI/library all-target check and strict Clippy.
- [ ] stateless/minimal/no-default/no_std+alloc gates.
- [ ] dotlink stdio/MCP/OpenAI replay/MCP Task suites.
- [ ] native Bubblewrap, Windows and macOS execution gates run on capable hosts or remain explicitly pending.

### Model-first efficiency
- [ ] Measure exact serialized effective provider catalog bytes and representative tokenizer estimate.
- [ ] Measure first-valid-call/tool-selection rate.
- [ ] Measure full model/tool round count and result tokens for representative fs, Graph, shell handoff, jobs.list recovery, known `n:<coordinate>` output without discovery, long-poll get, Agent queue/steer, transparent DelegateAgent execution with parent model idle, **model-issued self/peer/delegated causal-budget enforcement**, history fragments and long-Graph deliberate rerun without job_restart.
- [ ] Compare normal one-category `jobs_read(view)` versus Agent-history `filter:"default"`, `"none"`, custom positive combined categories and `exclude:true`; measure schema tokens and whether combined read replaces multiple calls/oversized trace.
- [ ] Compare foreground completion-only child Agent and authorized attention-capable child proactive handoff for first-result latency and total tool rounds, without relaxing deadlock prevention.
- [ ] Multi-Job recovery identifies correct Process/Graph/Agent with no unnecessary round trips.
- [ ] Agent default jobs.read does not dump transcript/tool evidence.
- [ ] No material regression is accepted merely to preserve an old API.

## Execution guard

This plan is the next implementation design, but writing it does not authorize source edits, commit, push, version, tag, release or deployment by itself.

Before implementation, revalidate live state and ownership. Preserve prior plans unchanged. Coordinator alone updates the global plan queue.
