# AI-native unified Jobs + Agent runtime — implementation plan

Design authority: [DESIGN.md](DESIGN.md)
Overview: [README.md](README.md)
Handoff: [NEXT-AGENT.md](NEXT-AGENT.md)

This TODO is the **provisional execution order**, not a second copy of the design. `DESIGN.md`
is normative for proposed semantics, invariants and acceptance details. Live source/tests
outrank dated prose for what exists. Parallel implementation is a head start, **not approval
or freezing of still-reviewed public/API contracts**. Reconcile changes in DESIGN before
accepting their checkpoint.

## Implementation discipline

- Implement **vertical slices**, not one checkbox/test per invariant.
- During a phase, run only the compile/test command needed to develop the code being
  changed. Do not repeatedly requalify unchanged filesystem, Graph, replay, native
  backend or platform primitives.
- At each **checkpoint**, run the grouped user-visible flows and failure boundaries
  below once. A checkpoint passes only when the whole slice is coherent.
- Carry a passed unchanged checkpoint forward. Re-run a prior lane only when later
  work changes its owning crate/API/schema, a dependency it relies on, or its
  authority/replay/concurrency semantics.
- Prefer one coherent implementation commit per passed checkpoint or tightly related
  vertical slice. Do not create evidence-only commits.
- Record concise executed commands/results in this TODO when a checkpoint is completed.
  No giant evidence bundles, duplicated logs or per-test prose.
- Native OS/hardware gates run when their implementation changed or at the final gate
  on a capable host; otherwise preserve the last accepted result and mark the gate
  unchanged/pending honestly.
- Keep the normal Local/single-user path cheap: no mandatory Session/team lookup,
  guardian/model call, collaboration allocation, extra queue or generic registry.

## Phase 0 — reconcile live state and freeze the implementation map

Goal: start from the actual current architecture without redesigning it during implementation.

- [ ] Re-read live AW/dotlink source, current plan index, affected tests and dirty work.
      Map the current `aw-tools` JobManager/Graph/PreparedOperation owners,
      `aw-tools-adapter`, Agent/Actor/TurnJournal, SessionStore, Process/output
      retention/replay/MCP Task paths, and the thin `aw-policy`/`aw-session`/`aw-host`
      skeleton.
- [ ] Produce one short implementation map in the commit/working notes: exact crates/files
      owning OperationId/ToolEffect, refs, Job control, Agent Turn control, policy,
      Session coordination and dotlink bridge. If live ownership materially differs
      from DESIGN, fix the plan once before implementation rather than carrying two paths.
- [ ] Identify obsolete pre-vNext routes/helpers to delete during cutover. No migration,
      compatibility decoder, second Job/Agent engine or parallel policy/session store.
- [ ] Reconcile the approved initial Graph eligibility: native DL-150 Graph leaves + AW `agent`,
      **no nested `batch` on any surface**; prove the paid-guardian replay ordering, mixed-owner
      Job-list ordering, explicit SDK Agent owner, and remote Start linearization in the design
      before freezing dependent public interfaces. Implementation code does not decide these.

### Checkpoint 0 — baseline / ownership
- [ ] Current targeted crates build in the configurations needed to start implementation;
      existing unrelated failures are identified before edits. No broad qualification run
      is required here.

## Phase 1 — shared language, authority primitives and Local host seam

Goal: establish the reusable low-level contract while keeping Local execution behavior
and DL-150 semantics unchanged.

Implement together:
- canonical semantic OperationIds and flat provider names; one mechanical model-name →
  semantic-ID resolver used by direct tools and model-facing Graph;
- first-class `Control` effect through kernel/core/policy/Envelope/adapter/journal
  projections and scheduler classification;
- typed immutable handles/ExecutionRef/Revision and directly derivable Graph
  `n:<1-based coordinate>`;
- portable `WriterFence = NotApplicable | Epoch(nonzero)` in `aw-agent-kernel`/
  authoritative Agent/Session owners; `aw-tools` receives only opaque host-verified owner
  generation/admission tokens or callbacks, **not an Agent-kernel dependency**;
- host-neutral borrowed `GraphActivationContext`, wait/control-ancestry query input and
  bounded PendingApproval value shapes in shared layers only where multiple owners use them;
- pure `aw-policy` constraints/preferences compiler, `aw-session` coordinator evidence,
  and thin `aw-host` composition with default Local/no-guardian execution;
- preserve no_std/minimal/stateless boundaries and avoid new allocation/lookup cost on the
  direct filesystem hot path.

Do **not** implement multi-user policy, Agent delegation or dotlink bridge behavior yet.
Unsupported advanced paths must fail closed.

### Checkpoint A — shared foundation + unchanged Local behavior
- [ ] Validate as one group:
  1. provider catalog remains flat and bounded; model Graph uses the same flat leaf spelling;
  2. representative direct `read/edit` and compact native `batch` still produce DL-150
     outcomes/`more`/`incomplete` and deterministic node coordinates; **nested `batch`
     is rejected on every Graph entrypoint**. The AW `agent` leaf is only projected when
     its exact Agent owner adapter exists (Phase 3/4), not exposed by an unfinished skeleton;
     standalone dotlink never advertises Agent leaves;
  3. Control is denied under Off/ReadOnly without an explicit grant and does not join Read waves;
  4. minimal/no-default/no_std-capable crate boundaries still compile where supported,
     and standalone `aw-tools` has no `aw-agent-kernel`/Session/provider dependency;
  5. Local host path performs no guardian/provider/session/collaboration work beyond the
     pre-existing Agent invocation.
- Do not rerun full native backend suites here unless Phase 1 changed their implementation.

## Phase 2 — finite Process/Graph Jobs and the new jobs.* data/control plane

Goal: land the complete finite-Job API before layering Agent Jobs on top.

Implement together:
- Process/Graph Job refs, exposure/listing, lifecycle/revision snapshots and deterministic
  list cursor. Establish one **project-scoped host listing/routing index** and its comparable
  immutable exposure sequence for finite Jobs; reserve capacity before effects. Keep a thin
  Agent-root publication hook for Phase 3, when authoritative Agent creation exists. The index
  holds only ref/owner/position, not copied lifecycle/history;
- `jobs_list/get/read/ack/send/cancel` for standalone finite Jobs;
- Process natural stdout, Graph natural result, node result/stdout semantics;
- raw-byte retention/offset/ACK ownership, exact EOF/truncation, lost-receipt promotion,
  same-Job shell handoff and idempotent control replay;
- interactive Process stdin with **both** current Control and independent current
  `process.input` authority;
- shell/`batch` `background:true` replacing old `job_start`; no restart capability;
- AW contextless SDK/host Process/Graph constructors require an explicitly selected **existing
  authorized Agent owner**; never auto-create/bind `a:0` from parser defaults; standalone
  dotlink finite Jobs keep their own authenticated live-session namespace;
- until the actual Agent/Turn Graph owner is wired in Phase 3/4, provider-facing Graph
  stays on reviewed DL-150 native/shell leaves. Thereafter AW may advertise its explicit
  `agent` leaf; typed SDK/host-only `jobs.*` leaves need separate owner qualifications,
  and **nested `batch` remains rejected everywhere**.

### Checkpoint B — finite Job user flows
- [ ] Validate these flows together:
  1. shell completes synchronously **or** hands off to the same Job; lost response never reruns it;
  2. background shell/Graph returns compact ref+rev and `jobs_get(wait_ms)` observes settlement;
  3. Process stdout/stderr/result and Graph whole/node reads page exactly; read never ACKs;
  4. ACK is monotonic/idempotent and another replay/Task owner prevents premature cleanup;
  5. interactive stdin succeeds when currently authorized, then fails before bytes after revoke;
  6. cancel/timeout/unknown preserves writer quiescence and factual uncertain effects;
  7. duplicate effectful control returns its original receipt; changed duplicate fails closed;
  8. finite-Job exposure/list ordering and lost-receipt promotion use one stable host
     sequence, with no missing/duplicated refs; Agent/finite mixed pages are tested after
     Phase 3 wires the authoritative SessionStore Agent owner.
- Re-run DL-150 native filesystem/Graph conformance only if shared Graph/filesystem semantics
  changed during Phase 2.

## Phase 3 — Agent as a Job, Turns, authority segments and Agent history

Goal: make Agent lifecycle/control use the same Job plane without a second Agents API.

Implement together:
- stable Agent `a:<handle>`, exact Turn `t:<handle>`, heterogeneous routing to authoritative
  Session/Actor/TurnJournal owner, and one-time Agent↔Session/branch binding;
- publish Agent roots into the **existing project Job-list index**, reusing the same
  comparable exposure sequence as Phase 2 finite Jobs; commit creation/list publication
  atomically and keep SessionStore as the sole Agent lifecycle owner;
- require an explicitly selected existing authorized Agent owner for contextless AW SDK
  Job-producing calls; parsing defaults never fabricate a new root Agent;
- `jobs_list/get/read/send/cancel` for Agent/Turn refs; queue/steer/interrupt semantics;
- immutable TurnJournal `AuthoritySegment` per authenticated Input/control boundary;
  all causally following Steps inherit the current segment until another authenticated boundary;
- lightweight `ModelInvocationAdmission` binding each paid Step to exact authority-segment
  fingerprint + existing provider/account InvocationIdentity + host invocation ID;
- causal-work/payer budget across model-origin self/peer sends, children and continuations;
- Agent `jobs_read` natural latest result plus bounded views/filter/history cursors;
- cancellation/close truthfulness and exact replay/unknown-effect behavior;
- unresolved active Turn uses public `waiting`/`cancelling` while its **Agent** is `blocked`;
  an irreconcilable Turn can settle with factual `unknown`/`mixed`, never a nonexistent
  public `Turn blocked` lifecycle state.

### Checkpoint C — Agent lifecycle + replay
- [ ] Validate these flows together:
  1. create Agent → first Turn → idle → continue via root send with stable Agent identity;
  2. queue while busy creates future input; steer/interrupt stays in the exact active Turn;
  3. a steering AuthoritySegment persists across tool-result-driven Steps and never reverts;
  4. replay/lost initial Agent response recovers the same Agent/Turn with no duplicate paid effect;
  5. provider retry under a changed authority fingerprint is a new admission and cannot reuse
     previous paid/private continuation state;
  6. Turn cancel vs whole-Agent close has exact scope and preserves unknown effects;
  7. default Agent read is compact; filtered/history reads remain bounded and disclosure-safe;
  8. unresolved paid/external attempt blocks the Agent without emitting `blocked` for a
     Turn, losing the accepted Input, or dispatching a new Step;
  9. interleaved Agent creation + finite Job exposure across several `jobs.list` pages
     remains one immutable comparable order, replay-stable without duplicate lifecycle state;
  10. contextless AW SDK without an existing authorized Agent owner denies Job constructors
      before effects; explicitly bound owner succeeds.

## Phase 4 — scoped Policy, Ask and ExecutionStrategy delegation

Goal: add advanced host policy/delegation without changing semantic operation identity,
model surface or Local hot-path behavior.

Implement as **three coherent owner-local lanes, then one combined Checkpoint D** (no test-per-field bureaucracy):
1. Proven grants and replay/approval admission: authenticate positive `AuthenticatedGrantPath`
   evidence from existing authority stores and intersect with pure `aw-policy`
   constraints/denies/preferences/limits;
   - coherent PolicyContext/effective-policy revisions, current-authority revalidation,
     deny precedence, exact payer/provider/site authorization;
   - **claim the original replay identity and bounded paid/approval/child budget before
     any guardian/router invocation**. Persist per-attempt evidence; unknown paid attempt
     cannot be automatically rerun after crash; admitted argument `Narrow`/route/policy
     are immutable evidence *inside* the original replay claim;
   - admission guardian `Allow|Deny|Ask|Narrow`; after Graph seal, Node decisions can only
     Allow/Deny/Ask or narrow route/resource/provider/payer/limits;
   - replay-stable `PendingApproval`, with whole-Graph multi-Ask as one bounded set in
     the **existing** Graph Job;
2. Single-operation execution route: AW-host `Local | DelegateAgent` for original
   semantic operations; an ordinary Agent/Turn must invoke the original trusted executor
   for factual receipts. Intersect original and delegate tool/provider/payer/disclosure
   authority and reject self/ancestor/wait-cycle controls before paid admission;
3. Graph routing: one Graph coordinator or activation-time Node routing, deterministic
   dependency-closed `GraphActivationContext`, captured union footprints and conflict
   positions claimed **before** per-node router/Ask. Immutable prepared args/`when`/topology,
   existing JobManager and exact owned-child settlement remain authoritative.

Use targeted tests while building each lane; run **one** integrated Checkpoint D after all three.

### Checkpoint D — policy/delegation failure boundaries
- [ ] Validate as one integrated matrix:
  1. Local default performs zero auxiliary model calls and remains semantically identical;
  2. DelegateAgent returns the original factual operation receipt; free-form child prose cannot
     fabricate native/Process/Job-control success;
  3. missing original grant, delegate grant, payer, provider or disclosure permission denies;
  4. delegated self/ancestor/wait-cycle controls reject before child admission;
  5. whole-Graph delegation keeps one JobManager; Node routing is identical under different
     incidental parallel completion orders;
  6. router/Ask wait cannot let a later conflicting node overtake;
  7. admission-time Narrow can revise before seal; Node-time argument mutation rejects;
  8. Graph multi-Ask starts zero roots, accepts approvals independently/idempotently,
     resumes only when all required approvals pass, and replay never reruns guardian;
  9. crash between paid guardian acceptance and its durable decision does **not**
     duplicate inference on replay (same original request/claim, unknown/pending attempt);
     a policy-generation change also cannot turn original redelivery into new paid work;
  10. policy revocation blocks new effects without retargeting/rerunning already-started work;
  11. nested `batch` rejects before roots on model/SDK/host surfaces; AW Agent Graph nodes
      (advertised only with the real Agent adapter) can spawn further Agents within one
      inherited child-depth/causal/payer budget, settle owned effects before dependency
      success, and cannot wait on a synchronously blocked sole parent controller.
- Measure auxiliary calls/tokens/latency once here; do not remeasure unchanged Local primitives.

## Phase 5 — shared Sessions and multi-user coordination

Goal: add collaboration by extending the existing Session/Agent owner, not creating another
coordination runtime.

Implement together:
- Session participation predicates View/Contribute/Submit/Control/Manage over existing stores;
- immutable Agent↔Session/branch binding; `a:0` binds once and is never sequentially rebound;
- portable WriterFence enforcement for branch + Workspace owners;
- full-Session sharing vs approved-context sanitized Session;
- one existing Agent input queue and one Turn writer for all participants;
- participant-origin/payer/audience evidence and current-authority checks at input claim/effects;
- cross-participant control starts a new AuthoritySegment; provider input is rebuilt from the
  authorized public projection when actor/disclosure changes; previous actor private continuation
  is never reused;
- membership/audience revocation, explicit service-principal unattended work and curated
  ownership transfer semantics.

### Checkpoint E — multi-user user-visible flows
- [ ] Validate together:
  1. Alice shares an existing personal Session with Bob without changing owner/Agent/branch;
  2. Bob Submit queues under Bob's grants/payer; Alice-only provider/tool/resource rights remain absent;
  3. Bob Control steer creates a Bob authority segment that persists across multiple tool/model
     Steps; Carol steering later creates a Carol segment;
  4. unsafe disclosure context rejects cross-user steer before provider dispatch;
  5. revoking Bob after queue acceptance blocks future claims/effects/pages but keeps factual receipts;
  6. `a:0` cannot bind a second Session concurrently or sequentially;
  7. stale WriterFence/failover cannot create a second paid Turn or writer;
  8. approved-context Session excludes private history/provider state by default.
- Single-user Local path must remain unchanged in allocation/model-call/membership-lookup behavior.

## Phase 6 — dotlink thin integration

Goal: consume one reviewed AW/shared revision and project only concrete remote execution,
never AW policy/Session/Agent ownership.

- [ ] **D1 may begin after reviewed AW A/B shared refs and finite Job contracts**, independent
      of Agent/Session-sharing checkpoints; consume one exact reviewed shared revision (no
      sibling path/copy). Land standalone dotlink flat jobs/ref/read/ack/control with original
      sandbox/launcher/protected-path checks and no Agent leaf or nested `batch`.
- [ ] **D2 waits for AW E** and the necessary Policy/Start contracts. Bridge only resolved
      AW canonical intent plus authenticated actor/source/site/epoch/writer/grant evidence.
      Each remote effect requires an AW-owner **source-fenced Start authorization** on the exact
      original replay attempt, linearized with revocation before dotlink's local Start check.
      Revocation before AW Start denies; an already authorized/possibly launched attempt remains
      factual/unknown, never transparently retried. Dotlink independently rechecks current
      launcher/sandbox/Process authority; AW unavailability blocks new remote effects.
- [ ] Keep standalone Agent/filter/subscribe/wake absent. Initial bridge rejects remote
      dotlink-source wake/subscription until a reviewed source-owner-fenced protocol exists.
- [ ] Keep nested `batch` disabled **even in SDK/host Graphs**. AW model Graph adds
      `agent` to DL-150 eligible leaves; standalone dotlink Graph stays native/shell-only.
      Advanced `jobs.*` Graph controls stay explicitly qualified typed SDK/host-only.

### Checkpoint F — standalone + bridge
- [ ] Validate together:
  1. standalone Process/Graph jobs_list/get/read/ack/send/cancel;
  2. standalone schema has no Agent/filter/subscribe/wake or advanced model Graph growth;
  3. AW bridge controls the exact remote Job with stable replay identity and writer fences;
  4. upstream AW grant **and** dotlink local sandbox/launcher grant are both required;
  5. stale actor/team/source/site/writer evidence and revoked Process stdin deny before effect;
     race AW revocation against remote **Start authorization**: revoke-first denies, Start-first
     has one accepted/unknown factual outcome; owner offline/old proof cannot start a new effect;
  6. no Agent/Session/transcript state appears in dotlink and no duplicate remote Job/Graph owner;
  7. MCP Task/replay/ACK ownership and stdout protocol cleanliness remain intact.

## Phase 7 — hard cutover, cleanup and final qualification

Goal: remove obsolete code only after the replacement flows are working, then qualify the
consolidated product once.

- [ ] Remove old `job_*`, `job_start`, `job_restart`, grouped jobs/agents, separate Agents API,
      subagent/team/plan/review_release model routes and compatibility-only helpers/tests that no
      longer have an independent product consumer.
- [ ] Keep the pre-1.0 cutover hard: no migration/dual-schema/legacy replay service. Reset old
      transient dev/test state only through the DESIGN quiescence gate.
- [ ] Remove dead duplicate policy/session/host wiring exposed by the new canonical path.
- [ ] Update live public docs/examples/SDK references to the new model/SDK names.
- [ ] Capture final provider catalog/schema bytes + representative tokenizer estimate and compare
      model/tool rounds for the core user-visible workflows.

### Checkpoint G — final product gate
- [ ] Run one final grouped qualification:
  - workspace/package build + strict Clippy for crates changed by this implementation;
  - minimal/no-default/no_std/alloc lanes whose dependency boundaries changed;
  - AW user flows from Checkpoints B–E and dotlink bridge flow from F;
  - DL-150 filesystem/Graph conformance **once** if shared primitive code changed since its last
    accepted checkpoint;
  - replay/unknown-effect/retention/ACK and policy/authority failure boundaries touched since
    their last accepted checkpoint;
  - platform-native Bubblewrap/Windows/macOS gates only when affected or on a capable final host;
  - provider schema/token snapshot and first-valid-call/round-count comparison.
- [ ] Do not rerun unchanged primitive suites solely because the final phase removed old routes.
      Re-run by ownership/dependency trigger, not habit.
- [ ] Leave any unavailable external/platform gate explicitly pending; do not call a partial run full.

## Completion

The implementation is complete only when:
- prerequisite **Checkpoint 0** and execution **Checkpoints A–G (seven)** all pass; an
  unavailable external/platform gate remains explicitly **pending** and is not a passed
  product qualification or a substitute for a required internal behavior;
- no old model/API route remains reachable;
- one canonical Agent/Session/Actor/Turn/Graph/Job ownership model remains;
- default Local/single-user path retains its lightweight behavior;
- AW and dotlink docs match the implemented surface;
- no unchecked TODO above represents remaining product behavior rather than an external gate.

## Execution guard

This plan authorizes **implementation sequencing only**. It does not itself authorize push,
version, tag, release or deployment. Preserve unrelated work and reserved plan ownership.
