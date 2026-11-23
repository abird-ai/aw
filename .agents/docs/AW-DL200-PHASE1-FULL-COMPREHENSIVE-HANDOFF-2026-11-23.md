# AW + dotlink + DL-200 R6 / Phase-1 full comprehensive handoff — 2026-11-23

Prepared: 2026-11-23
Owner: @pvl
Source-authoring authority: **L5 only**

AW:

    /home/pvl/spaces/abird/src/aw

dotlink:

    /home/pvl/spaces/abird/src/dotlink

Read-only dotlink 0.17 release reference:

    /home/pvl/spaces/abird/src/dotlink-017-mounts

---

## 0. Purpose, scope and authority

This is the current full onboarding handoff for the DL-200 R6 implementation line after the
Phase-1 Agent/Sandbox/input/provenance/exposure work through:

    03d1224ef7b2054b3856fa559dcc23ac666f341f
    Rebuild Agent exposure from owner state

It exists so a new agent can recover, from one document:

- the original architectural problem and design lineage;
- DL-150 and unified Jobs/Agent foundations inherited by DL-200;
- R1→R6 decisions and rejected alternatives;
- the shared AW/dotlink Process correction and release convergence;
- every Phase-1 implementation checkpoint through the current local HEAD;
- current schema/owner boundaries and public/runtime semantics;
- what is implemented vs designed vs deferred;
- exact green evidence and explicitly unavailable evidence;
- local-vs-remote publication state;
- source ownership, frozen areas and Git rules;
- the exact next implementation boundary;
- a copy-ready starting prompt for the next agent.

This file is context/navigation, not a replacement normative specification.

Authority order:

1. live source + tests + actually executed results;
2. AW/AGENTS.md;
3. sealed DL-200 R6 SPEC.md;
4. IMPLEMENTATION.md;
5. plan TODO.md;
6. AW/.agents/docs/CURRENT_HANDOFF.md;
7. this file;
8. older dated handoffs/reviews.

If this file conflicts with live source or sealed normative design, the source/SPEC wins.

Do not infer push/tag/release/deploy authority from this handoff.

---

## 1. Mandatory operating constraints

### 1.1 Source-authoring machine

Use **dotlink---l5 only** for source-authoring work.

Do not:

- use A1;
- create another worktree;
- author source on x2;
- mutate the read-only release checkout.

x2 may be used only for explicitly useful execution/inference work, never as source authority.

### 1.2 Canonical working copies

    AW        /home/pvl/spaces/abird/src/aw
    dotlink   /home/pvl/spaces/abird/src/dotlink
    release   /home/pvl/spaces/abird/src/dotlink-017-mounts   # read-only

### 1.3 Reserved parallel work

Another owner reserves:

    AW/.agents/plans/ux/**

Never inspect, list, read, stage, reset, stash, modify, commit or otherwise touch that subtree or
its staged entries.

Treat it as opaque concurrent state.

Preserve all unrelated/concurrently owned work.

### 1.4 Frozen areas

Unless the user explicitly promotes them:

- Sandbox/mount/config-v13 implementation remains frozen;
- dotlink 0.17 Sandbox/mount implementation remains frozen;
- /home/pvl/spaces/abird/src/dotlink-017-mounts remains read-only;
- do not reopen published 0.17 D0–D7/shared Process work without a concrete regression;
- future strong Linux PID-namespace/PID-1 supervision remains future design.

### 1.5 User implementation style

For each implementation turn:

- aim for roughly 30 minutes;
- stop at the next coherent green checkpoint;
- prefer one real integration/regression over many granular fixtures;
- do not over-engineer evidence, pinning or handoff-only test infrastructure;
- keep primitives simple, elegant, reusable and composable;
- maximize reuse across crates when semantic ownership is the same;
- favor zero-cost/static abstractions where natural;
- take easy performance wins when maintenance remains simple;
- prefer clean/simple design over convoluted micro-optimization;
- optimize proven hot paths, not imagined ones.

### 1.6 Git rules

- Signed commits only.
- Never disable signing.
- Commit exact owned paths only.
- Use a private GIT_INDEX_FILE.
- Build the commit from the expected parent.
- CAS-update the branch ref.
- Verify the signature after commit creation.
- Preserve unrelated staging.
- Never force-push.
- Push/tag/release/deploy only when separately authorized.

Expected signature:

    Good "git" signature for pvl@prasannavl.com with ED25519 key
    SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI

---

## 2. Exact current Git/publication state

### 2.1 AW local implementation HEAD

    03d1224ef7b2054b3856fa559dcc23ac666f341f
    Rebuild Agent exposure from owner state

Branch:

    main

### 2.2 Actual verified remote refs

Read-only remote verification on 2026-11-23:

    AW remote main
    78339410710472ec51d5ff46a5c9e586b67ff30d
    Own logical Sandboxes

    dotlink remote main
    85ceccc1088e201c98c994a46a68834440eeab32
    Publish DL-200 R6 consumer and repin AW

    dotlink remote release/0.17
    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2
    v0.17.9

AW local HEAD is **9 commits ahead of the verified remote AW main**.

The local origin/main tracking ref is stale at 0f3f3eb...; do not use that stale tracking ref as
publication truth.

### 2.3 Dotlink AW pin

dotlink intentionally remains pinned to the published R6/Phase-0 AW revision:

    20a63a4d4547cc20b2a154787823125b82f5e385
    Complete DL-200 R6 Process and replay correction

Do not repin dotlink merely because later Phase-1 AW code exists. Repin only for a real shared
dependency consumer and only after separately authorized AW publication.

### 2.4 Local-vs-published distinction

Current local AW implementation includes everything through 03d1224.

Remote AW main includes through 7833941.

Therefore these later local commits are committed/signed but not published:

    9633dfb Expose Sandbox reads and close
    1d4988b Route non-current Agent close
    426d1f8 Authorize cross-Project Agent create
    c6718fc Own staged Agent input plans
    e546a75 Send staged Agent inputs atomically
    503706d Expose staged Agent input host API
    838ea82 Project immutable Turn provenance
    90a1b86 Project finite Job provenance
    03d1224 Rebuild Agent exposure from owner state

No push is implied by this handoff.

---

## 3. Mandatory read order for the next agent

Read before changing source:

1. AW/AGENTS.md
2. this file
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
11. dotlink/AGENTS.md
12. dotlink/.agents/docs/CURRENT_HANDOFF.md
13. live source/tests for every owner touched.

Historical/forensic detail only when needed:

- earlier AW/dotlink comprehensive handoffs under .agents/docs/;
- R1–R5 resolved checklist;
- immutable DL-150 reference under .agents/plans/dotlink-tool-port/reference/;
- old dotlink 0.17 handoffs.

Never use older prose to override current source or sealed R6.

---

## 4. Architecture north star

### 4.1 One truth, one owner

R6's central rule:

> One authoritative source for each semantic truth. Mutable truth has one Owner and one
> linearization point; immutable truth has one stable Fact identity; uncertain external work has
> one Operation custody.

Global names identify resources.

They do not imply:

- a global transaction;
- a global order;
- cross-store cursor comparability;
- cross-store visibility;
- authority.

Derived structures are projections/adapters/cursors, never second owners.

### 4.2 Public concepts

| Concept | Meaning / owner boundary |
|---|---|
| Agent | globally named collaboration/security/history/execution envelope |
| Job | Agent, Process or Graph work; Turns are Agent timeline |
| Sandbox | durable logical mutable execution environment |
| Event | immutable source-scoped fact in one local store |
| Checkpoint | immutable history anchor + captured named SourceSet |
| Extension | installed typed definitions, not runtime authority |

Private Process Scope owns one process/dispatch lifetime.

Sandbox and Scope are deliberately separate.

Vault grants are separate capabilities and never Sandbox contents.

### 4.3 Canonical namespace

    /d:<domain>/i:<principal>/p:<project>
      ├── /a:<agent>[/t:<turn>[/j:<job>[/n:<node>]]]
      └── /s:<sandbox>

i: is the single principal-handle namespace for users/groups.

A canonical ref is identity, not permission.

### 4.4 Agent vs Session

Public identity/security is Agent-owned.

Internal SessionStore remains backing history/journal/recovery structure.

Do not resurrect Session as a public resource/API.

### 4.5 Project

Project is a Workspace-backed public view, not another database or mutable owner.

### 4.6 Model Jobs surface

Exactly:

    jobs_list
    jobs_get
    jobs_read
    jobs_send
    jobs_ack
    jobs_cancel

No model queue CRUD.

No Jobs-only subscription API.

No pause-after-turn model verb.

### 4.7 Agent model constructor

    agent(task, title?, background=false, from?)

History source may be Agent or settled Turn.

Model cannot choose trusted provider/project/Sandbox/Vault/source authority.

### 4.8 Host Agent construction north star

    agent.create(
      from?   = Agent ref | settled Turn ref,
      project = trusted Project selection/default current/source,
      title?,
      task?,
      access  = private | inherit | custom principal grants,
      sandbox = auto | Sandbox ref
    )

History ancestry, delegation lineage and Sandbox ancestry remain distinct.

### 4.9 Event model

One Event concept.

Signals were removed.

NATS-like subject projection:

- single-token wildcard;
- terminal subtree wildcard;
- system + custom Agent Events in one model;
- custom names use #name.

Cross-store Event→Agent delivery is unsupported in v1.

### 4.10 Agent access vocabulary

Seven permissions:

    view
    contribute
    submit
    control
    derive
    share
    manage

Effective rights are UNION of:

- implicit owner rights;
- direct grant;
- active matching group grants.

access_generation is a security/observation fence, not Agent rev.

### 4.11 Sandbox / Source / Vault boundaries

- Sandbox = logical mutable execution identity/state.
- SourceDriver owns source capture/materialization semantics.
- Checkpoint captures HistoryBase + exact SourceSet.
- Vault grants are separate provider-backed capabilities.
- History fork never inherits Sandbox/source/Vault authority.
- Agent↔Sandbox rebind is intentionally absent.
- sandbox.create(from=...) remains Phase 3 because truthful materialization requires Prepared +
  SourceDriver/checkpoint semantics.

### 4.12 Extension taxonomy

Keep these distinct:

- Hook = in-flight Observe/Transform/Gate;
- Handler = post-commit reaction;
- Action = explicit invocation;
- Tool = executable capability;
- View = read-only derived data;
- State = extension-local persistence;
- Resource = inert content;
- UI = semantic UI contribution;
- Driver = pluggable implementation;
- Policy = authority/admission.

ExtensionRegistry must not become another runtime/database/policy engine.

---

## 5. Six private roles — vocabulary, not framework

| Role | Meaning |
|---|---|
| Owner | sole mutable authority / linearization point |
| Fact | immutable stable record |
| Operation | durable external-effect/replay/recovery custody |
| Projection | rebuildable derived index |
| Cursor | bounded observation/navigation progress |
| Scope | one Process lifetime |

Do not create base classes/managers around these role names.

---

## 6. R6 private mechanics

### 6.1 StoreScope / StorePos

- StoreScope = store_id + restore_generation.
- StorePos wraps local commit_log.seq.
- Positions compare only inside one StoreScope.
- Restore generation fences old cursors/projections.
- Never turn StorePos into a global clock.

### 6.2 Replay / Prepared / Unknown

Reuse existing authenticated request/tool replay identity.

Rules:

- same authenticated identity never repeats an external effect;
- claim Prepared before uncertain/materializing external work;
- Unknown is inspected, never blindly replayed;
- final AW publication transaction rechecks current AW authority;
- publication commit is the only visibility point;
- external mutable evidence must remain immutable or provider-fenced through commit;
- no global replay ledger.

### 6.3 Projection publication

Rebuildable projections:

1. capture head H;
2. build privately through H;
3. under final fence verify current head is still H or catch up;
4. atomically activate a contiguous safe prefix;
5. never issue a cursor high-water over a hole;
6. never swap an H-stale replacement over newer rows.

### 6.4 Cursor envelopes

Consumer-specific.

May bind:

- caller;
- normalized selector;
- StoreScope/affinity;
- placement/security generation;
- high-water;
- consumer-specific frontier.

Cursor is never authority.

### 6.5 EventScan

Future Phase 2 source-first ordered primitive.

Not:

- Handler executor;
- Input scheduler;
- final authorization.

### 6.6 DriverKey

Stable driver namespace/local ID.

Artifact-specific schema versioning.

No global driver database/schema clock.

### 6.7 Typed registry staging

One private staged-publication primitive may be reused by actual leaf registries.

Do not prebuild unused registries.

### 6.8 Shared Process runtime

aw-process is the Agent-neutral owner for:

- Scope;
- Child;
- stdin/pipe custody;
- wait;
- semantic stop;
- reap;
- bounded drain;
- ProcessTerminal settlement.

Hosts retain:

- authorization;
- Sandbox/executable preparation;
- output storage;
- product projection.

Terminal/PTY custody remains separate.

---

## 7. Historical design lineage

### 7.1 AW foundations before DL-200

AW already had:

- one Rust Agent;
- one user-facing aw;
- immutable TurnJournal;
- ToolRegistry/ToolPolicy;
- exact Actor/source/site/effect grants;
- intent-before-effect;
- Unknown/reconciliation;
- provider/account/payer/custody separation;
- paid-result preservation;
- owner locks/writer epochs/failover;
- SecretStore;
- durable Session/journal/recovery;
- native Workspace/TUI/browser/headless/SDK projections;
- portable/minimal/no_std dependency boundaries.

Historical numbered phases stop at 68.

Do not invent Phase69.

### 7.2 DL-150 foundations

DL-150 established lower semantics inherited by DL-200:

- shared host-neutral Tool/Graph/finite-Job semantics;
- deterministic Graph/node semantics;
- replay separated from current authority;
- raw retained output vs serialized result;
- explicit ACK release;
- bounded paging/results;
- physical Process exit vs semantic stop vs output coverage separation;
- no blind retry of Unknown;
- exact authority/revocation at effect boundary.

### 7.3 Unified Jobs + Agent — 2026-11-09 → 2026-11-14

Converged on:

- Jobs as consolidated work/control plane;
- Agent/Process/Graph as Job kinds;
- Turns as Agent timeline;
- history/collaboration distinct from Process/Graph execution;
- bounded subagent delegation;
- current authority checks at new-effect boundaries;
- Graph ownership constraints;
- pre-1.0 hard-cutover philosophy;
- dotlink unified Jobs bridge.

Historical milestone:

    AW      0a6c6a0... Complete unified Jobs and Agent runtime
    dotlink c90a660... unified Jobs bridge

### 7.4 Cross-repo convergence — 2026-11-15 → 2026-11-20

Converged:

- shared Tool/Graph/output/ACK semantics;
- dotlink 0.17 hardening;
- Remote Start replay;
- Process control/cancellation;
- AW/dotlink shared Process extraction;
- bounded output/drain/interactive stdin.

### 7.5 DL-200 R1

Public-model simplification:

- Agent = public collaboration/security/history unit;
- Jobs = unified work plane;
- Event = one fact/subscription substrate;
- Session removed from public API;
- Project = Workspace-backed view;
- minimal Agent/Sandbox/Checkpoint/Vault/Extension APIs;
- model agent(task, from?);
- exact history fork semantics.

Published R1 provenance:

    132bfd7

### 7.6 R2–R5

Correctness hardening resolved:

- authoritative Agent migration;
- ACL/groups/principals;
- HistorySpan/Base/Grant;
- scalable DisclosureManifest;
- exact settled-Turn forks;
- Event subjects/wildcards/security/store affinity/migration;
- Jobs pagination/high-water/tree authorization;
- SourceSet/checkpoint/SourceDriver recovery;
- Sandbox/Source/Vault boundaries;
- typed Extension registries;
- host input plans;
- replay/tombstones;
- publication-fenced construction;
- Process settlement / strong supervisor design.

Reference:
DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md.

### 7.7 R6 seal

R6 retained the public model and seven implementation phases while simplifying private mechanics:

- six descriptive private roles, no framework;
- one owner per mutable truth;
- StoreScope/StorePos;
- reuse replay/Prepared vocabulary;
- ExternalEvidence;
- one projection-publication protocol;
- consumer-specific cursor envelopes;
- one EventScan;
- DriverKey + per-artifact schema;
- one typed registry staging core;
- dedicated aw-process;
- Tree→Occupancy terminology;
- explicit rejection of generic managers/coordinators.

Seal commit:

    7c3800c

R6 sealed on 2026-11-21.

---

## 8. Shared Process / dotlink correction — Phase 0

### 8.1 Original Linux bug

dotlink v0.17.7 exposed a Bubblewrap --die-with-parent issue:

Linux PR_SET_PDEATHSIG is tied to the forking thread, not merely the process.

A child launched on a retireable Tokio worker could be SIGKILLed when that worker retired while dotlink
itself remained alive.

The correction belonged in shared Process launch ownership, not a dotlink-local second runtime.

### 8.2 Implemented shape

Linux shared launch uses one private logical Spawn Facility:

- fixed/add-only Spawn Lanes;
- N=1 initially;
- lane threads do not retire;
- only spawn + exact DirectScope attachment occurs on lane;
- full Spawned Child/DirectScope/pid custody returns;
- Process futures remain movable;
- panicked lane poisons launch admission;
- no public spawn-pool framework.

### 8.3 Absolute execution deadline

One absolute Running deadline covers:

- spawn-lane wait;
- pre-fork admission;
- child wait;
- stop/drain.

Pre-launch cancellation/expiry produces factual no-Process outcome.

Once Launching wins, child follows normal Process settlement.

### 8.4 Terminal semantics

Keep separate:

- physical child exit;
- StopRecord;
- Occupancy;
- output completeness;
- product success projection.

### 8.5 Occupancy

Occupancy replaces ambiguous Tree naming.

Compatibility projection in dotlink:

    Empty    -> omit tree
    Live     -> tree="leftovers"
    Unknown  -> tree="unknown"

### 8.6 Windows

Shared platform owns:

- no-window launch semantics;
- exact active-member query;
- bounded occupancy settle.

No process-image/conhost guessing.

### 8.7 Dotlink product policy remains dotlink-owned

One bounded-turn policy:

- 35m turn horizon;
- 10m report reserve;
- 25m new-command ceiling;
- checkpoint_minutes=None;
- jobs_get.next derived from factual Running deadline.

Do not move this product policy into AW.

### 8.8 Published convergence

Important AW commits:

    307c7f5 Extract shared Process runtime
    320c27d Relax shared Process dependency constraints
    22b5562 Expose shared Process semantic stop
    46e1125 Bound Process output drain globally
    a3ba87c Preempt interactive Process input at deadline
    7c16708 Flush captured output at drain bound
    59d67ba Qualify bounded Process drain flush
    0d2dc10 Record cross-repo Process convergence
    20a63a4 Complete DL-200 R6 Process and replay correction

dotlink published consumer:

    85ceccc Publish DL-200 R6 consumer and repin AW

release reference:

    576d0a1 = v0.17.9

### 8.9 Phase 0B

Completed replay/admission/disclosure/lifetime corrections include:

- compact replay tombstones;
- O(1) replay lookup;
- Unknown remains non-retryable;
- payload disclosure separated from factual replay;
- participant provider/payer identity correctness;
- authorization-id lifetime correctness;
- Process consumer correction;
- no second Job/Process owner.

---

## 9. Deferred Process/platform evidence

Explicitly deferred by the user; do not claim green and do not block later phases:

- real Bubblewrap worker-retirement qualification on a host that permits required mounts;
- Windows-native Process qualification.

L5 limitation:

- Bubblewrap outer sandbox rejects required /proc mount;
- no Windows-native target/host is available.

These are deferred evidence, not unresolved architecture.

---

## 10. Current schema and owner evolution

Current AW Store schema:

    v43

Relevant migrations:

    038_agent_owner.sql
    039_domain_principals.sql
    040_history_owner.sql
    041_sandbox_owner.sql
    042_agent_input_plan.sql
    043_agent_input_replay.sql

### v38 — Agent owner

Authoritative:

    agent_state_current
    agent_acl_current

Owns:

- canonical handles;
- backing Session/branch;
- owner;
- title;
- Agent rev;
- acl_rev;
- lifecycle/admission;
- immutable parent/delegation lineage;
- history pointer;
- Sandbox pointer.

Legacy Session title/participation remain compatibility/backing data only.

### v39 — Domain principals/groups

Owns:

- one Domain-scoped principal namespace;
- user/group principal kinds;
- group identity/membership/revision;
- no nested groups in v1.

Authentication remains owned by users.

Principal registration is not a second login/security owner.

### v40 — immutable history/disclosure

Owns:

- HistorySpan;
- HistoryBase;
- HistoryGrant;
- persistent content-addressed DisclosureManifest DAG.

Sealing is lazy, idempotent and revision-neutral.

### v41 — logical Sandbox owner

/d:/i:/p:/s: is the Sandbox ID.

Owns:

- Project identity;
- logical mutable execution layout;
- parent;
- rev;
- open/closed lifecycle.

Does not own:

- SourceBindings;
- Vault;
- mount/network/device realization;
- Process lifetime.

### v42 — staged Agent input plan

One staged plan per Agent:

- inputs_rev;
- opaque q: IDs;
- encrypted SecretStore payload;
- sparse order;
- creator/source;
- last editor;
- followup/steer/interrupt intent.

### v43 — staged input mutation replay

One shared replay owner for:

    add
    edit
    delete
    send

Keyed by authenticated stable user + host request ID.

No per-method replay ledgers.

---

## 11. Phase-1 implementation ledger — complete local sequence

Everything below is landed locally on main.

### Foundations

    9784619 Start DL-200 Phase 1 store-scope foundation

- StoreScope over store_id + restore_generation;
- StorePos over local commit log position;
- no global clock.

    d9f53db Wire Agent owner seam
    77fced2 Make Agent recovery exact
    b7059fb Mark accepted Agent input
    0e82f73 Activate Agent owner state

- v38 Agent owner/ACL becomes authoritative;
- exact rev/lifecycle/recovery cutover;
- accepted Input becomes Agent-owned mutation evidence.

    10aa467 Add Domain principal groups
    bcca6dd Add group Agent access

- v39 principal namespace/groups;
- group-aware effective Agent authorization;
- no Agent-rev fanout on group membership changes.

    706f6f1 Add immutable history owner
    a5aa8fd Add full Agent access
    0db1144 Fork Agent history
    0f3f3eb Wire Agent from refs

- v40 HistorySpan/Base/Grant + DisclosureManifest;
- seven-permission ACL;
- exact structurally shared forks;
- Agent/settled-Turn source selectors;
- model agent(..., from?).

    05d8a74 Refresh DL-200 handoff

Docs checkpoint.

### External host state/lifecycle

    15968db Add host Agent state shell

Store already implements:

- agent.list
- agent.get
- agent.edit
- agent.admission
- agent.access.list
- agent.access.set
- lower close seam.

No second state owner.

    6bc8f6f Preempt active Agent close

- reusable live close bit + active Turn cancellation token;
- close can preempt paid active Turn.

    e0a9562 Retire Agent wake recipients

- close retires transitional source subscriptions/wakes;
- no Phase-2 Event engine pulled forward.

    91e79c4 Close Agent finite work
    c9501e5 Compose stable Agent close
    6bc8c97 Retain root Agent Job runtime
    0af6a63 Add root Agent close seams
    086ed74 Compose root Agent close

- JobRuntime::close_owner;
- finite Job admission fence/cancel/quiescence;
- stable-child close composition;
- trusted active-Turn cancel seam;
- terminal closing -> closed Store seam;
- root close composition over existing owners;
- no second lifecycle owner.

### Host-created live Agents and initial task publication

    ed0332f Activate host-created Agents

- reconstruct published Agent into live Actor/Router/JobRuntime worker;
- activation is post-commit runtime/cache state.

    53f1201 Publish initial Agent task

Task-bearing create becomes publication-correct:

1. pre-mint child IDs;
2. durable Prepared create receipt;
3. encrypted task custody;
4. one final transaction:
   - create/fork Agent;
   - publish Pending Actor input;
   - emit agent.created;
   - transition receipt Prepared→Applied.

Unknown never silently retries.

    0c695a6 Expose host Agent create

- native/public agent.create;
- provider selection rebuilt for canonical stable child principal;
- Router::for_actor;
- replay-safe live registration;
- strict host fields;
- RPC ID = durable create identity.

### Logical Sandbox

    7833941 Own logical Sandboxes

- v41 owner;
- migration binds existing Agents;
- exact Project+layout sharing;
- immutable Agent→Sandbox binding;
- trusted sandbox=auto;
- explicit same-Project Sandbox selection.

This is the last AW commit currently published remotely.

    9633dfb Expose Sandbox reads and close

Native/stdio:

    sandbox_list
    sandbox_get
    sandbox_close

sandbox_create intentionally unavailable until Phase 3 materialization.

### Non-current close / cross-Project create

    1d4988b Route non-current Agent close

HostAgentClose:

- root . keeps RootAgentClose;
- other canonical Agents:
  - control/rev preflight;
  - live controller check;
  - authoritative closing/held transition;
  - existing runtime owns factual settlement.

    426d1f8 Authorize cross-Project Agent create

Current native/public redirect policy:

- destination i: must equal authenticated stable user;
- active destination Domain membership;
- current Workspace writer;
- redirected history requires share + derive;
- explicit destination Sandbox required;
- sandbox=auto redirect fails until Phase 3;
- raw project= never impersonates another principal.

### Staged input plan and Send

    c6718fc Own staged Agent input plans

- v42 plan owner;
- inputs_rev;
- encrypted drafts;
- add/edit/delete/reorder;
- creator/editor provenance;
- exact Turn validation;
- close freeze;
- no Agent-rev churn for drafting.

    e546a75 Send staged Agent inputs atomically

- v43 mutation replay;
- HostAgentInputs;
- all selected items validated as a set;
- followups publish through ActorQueue;
- at most one steer/interrupt due existing Runtime pending-control invariant;
- control uses Prepared AuthoritySteer + live stage + transactional accept;
- failure withdraws exact live segment or marks Unknown;
- Send advances inputs_rev + Agent rev exactly once.

    503706d Expose staged Agent input host API

Native/stdio all five:

    agent_inputs_list
    agent_inputs_add
    agent_inputs_edit
    agent_inputs_delete
    agent_inputs_send

One AgentInputsPort.

RPC frame ID is durable mutation identity.

### Provenance

    838ea82 Project immutable Turn provenance

TurnFactProvenance:

    original_agent
    original_turn
    actor
    created_at

Derived in batch from existing v40 structural history.

Fork/refork preserve original attribution.

Explicit historical jobs_read records expose it.

Natural/latest result path remains unchanged.

llm.attempt_settled commit evidence now carries canonical Agent + Turn.

    90a1b86 Project finite Job provenance

Finite Job:

    canonical_job_ref
    original_agent_ref
    original_turn
    initiating_actor

Persisted in immutable exposure reservation evidence and projected from immutable TaskIdentity.

Finite list/get/ack/send/cancel receipts append bounded provenance.

Raw jobs_read bytes remain raw.

ProcessTerminal remains Agent-agnostic.

ModelProcessFact freezes canonical Agent/Turn/actor at Prepared; reduced close/terminate receipts expose it
without PID/program/env/output/private Session metadata.

### Exposure normalization

    03d1224 Rebuild Agent exposure from owner state

Current local HEAD.

Closes the Agent exposure/index normalization requirement:

- agent_state_current remains authoritative;
- job_exposure_current remains rebuildable mixed Agent/finite-Job projection;
- owner-locked normal Store open checks exact projection consistency;
- exact projection => zero writes;
- stale/missing Agent rows => replace only Agent projection rows;
- finite Job rows untouched;
- publication position restored from immutable original session.created / branch.created evidence;
- no synthetic event;
- no commit-log append;
- no Agent rev;
- no state/title/lifecycle/admission/history/Sandbox mutation;
- trusted-local direct mode still does not repair/migrate;
- old repair_missing_agent_exposures remains pre-v38 compatibility only.

---

## 12. Current authoritative Agent contract

### 12.1 Identity

Public canonical Agent identity is frozen under:

    /d:<domain>/i:<principal>/p:<project>/a:<agent>

Backing Session/branch is internal.

### 12.2 Revision domains

Do not conflate:

- Agent rev;
- Agent acl_rev;
- group rev;
- access_generation;
- inputs_rev;
- Event rev;
- StorePos;
- writer epoch;
- provider/account credential versions.

### 12.3 Agent rev advances on factual Agent mutations

Examples:

- title;
- lifecycle/admission;
- close settlement;
- accepted input/control where owned by Agent contract;
- ACL mutation;
- Send publication boundary where staged input becomes authoritative runtime input.

Draft staged-input edits do not advance Agent rev.

Group membership churn does not fan out Agent rev.

Reads never manufacture Agent rev.

### 12.4 Lifecycle

    open
    closing
    closed

Admission can be open/held while lifecycle is open.

Close moves to closing/held atomically.

Closing/closed cannot be reopened.

Terminal close requires:

- no active Turn;
- finite work quiescent;
- factual runtime settlement;
- existing owner proofs.

### 12.5 Parent lineage

Delegation parent is immutable lineage metadata.

It is not:

- history ancestry;
- ACL inheritance;
- visibility grant;
- Sandbox ancestry.

### 12.6 Access

Allow-only seven-permission model.

Owner authority is implicit, never stored as ACL row.

Direct/group grants are durable.

access_generation fences observations/security cursors/wakes.

### 12.7 Group semantics

- one namespace for user/group handles;
- no nested groups v1;
- group manager != automatic group member;
- group-owned Agent uses live active membership;
- group rev changes fence access generation without Agent-rev fanout.

---

## 13. Current history/disclosure contract

### 13.1 HistoryBase vs cache identity

HistoryBase = immutable structural history identity.

Do not conflate with model cache/prefix key.

### 13.2 Sealing

- lazy;
- content-addressed;
- idempotent;
- revision-neutral;
- no Agent changed fact.

### 13.3 Exact settled Turn selection

Turn selector must be settled/attached immutable fact.

Active/unsettled Turn fails closed.

No fallback to mutable Agent head.

### 13.4 DisclosureManifest

Persistent content-addressed DAG.

Tracks protected provenance/authority roots.

History bytes with different protected provenance cannot alias.

### 13.5 HistoryGrant

Immutable disclosure evidence.

Not the same thing as DisclosureManifest.

New audience broadening requires current share authority and manifest closure validation.

### 13.6 Exact fork

Fresh Session/Agent.

Inherited HistoryBase is structurally shared.

No copied Turn nodes.

Destination local tail diverges independently.

### 13.7 Fork does not inherit

- active run;
- prompt;
- staged input;
- Process;
- Graph;
- watch;
- Sandbox;
- SourceBinding;
- Vault;
- source Agent rev.

### 13.8 Provenance through forks

Committed Turn attribution stays original:

    original_agent
    original_turn
    actor
    created_at

Fork/refork does not re-author inherited history.

---

## 14. Current host Agent shell

### 14.1 Already implemented Store operations

Authoritative Store methods already exist for:

    agent.list
    agent.get
    agent.edit
    agent.admission
    agent.access.list
    agent.access.set
    agent.create
    agent close seams

### 14.2 Native headless ports currently present

Dedicated ports currently exist for:

    agent_create
    agent_close
    agent_inputs_list
    agent_inputs_add
    agent_inputs_edit
    agent_inputs_delete
    agent_inputs_send
    sandbox_list
    sandbox_get
    sandbox_close

### 14.3 Concrete current host gap

The following authoritative Store operations are not yet exposed through one dedicated native headless
port:

    agent.list
    agent.get
    agent.edit
    agent.admission
    agent.access.list
    agent.access.set

This is the next coherent implementation slice.

Do not create new state/replay owners for them.

---

## 15. Current Agent create contract

### 15.1 Fresh and history-backed create share one lower owner

No second constructor/publication path.

Reuse:

- resources create-in-workspace owner / candidate-ID variants;
- exact-history lower constructor;
- disclosure/HistoryGrant owner;
- Agent ACL owner;
- Agent Sandbox owner;
- existing replay identity.

### 15.2 Initial task

Prepared before external SecretStore effect.

One final publication transaction.

Applied replay returns same Agent.

Unknown never silently retries.

### 15.3 Provider/runtime actor

Destination stable user's canonical principal.

Never rewrite a personal account's InvocationIdentity to pretend it belongs to another actor.

### 15.4 Cross-Project policy

Current native/public policy supports same-user cross-Project placement only.

Requires:

- stable destination user = authenticated user;
- active destination Domain membership;
- current destination Workspace writer;
- history redirect source share + derive;
- explicit destination Sandbox.

Trusted delegation to another principal's Project remains future host policy.

### 15.5 Sandbox

Same-Project auto uses current authoritative Sandbox.

Explicit ref must be:

- open;
- exact destination Project;
- exact trusted execution layout.

No Agent rebind.

---

## 16. Current staged input contract

### 16.1 Draft plan

One owner-local inputs_rev.

Opaque q: IDs.

Text in SecretStore, never audit plaintext.

### 16.2 Draft permissions

- list: view;
- followup draft: contribute;
- steer/interrupt draft: control;
- modify another user's draft: manage.

### 16.3 Send permissions

- followup Send: submit;
- steer/interrupt Send: control;
- another user's draft: manage.

### 16.4 Turn targeting

- steer/interrupt = exact active Turn;
- followup after = already-known Turn;
- no silent retarget.

### 16.5 Send atomicity

Supports:

- any number of followups;
- at most one steer/interrupt in one Send because Runtime allows one pending same-Turn authority update.

Final publication:

- accepts exact prepared control if any;
- publishes followup ActorQueue facts;
- removes selected drafts;
- advances inputs_rev once;
- advances Agent rev once;
- emits input changed fact;
- records Applied replay.

### 16.6 Replay

RPC frame ID is mutation identity.

Exact replay = no new effect/revision/custody.

Changed semantics under same request ID = conflict.

---

## 17. Current Sandbox contract

### 17.1 Logical owner

Sandbox ID is canonical ref itself.

Owns logical mutable execution layout.

### 17.2 Migration

Existing Agents are bound transactionally.

Same Project+layout may share one logical Sandbox.

Distinct layouts never collapse.

### 17.3 Public current surface

    sandbox_list
    sandbox_get
    sandbox_close

Current-Project-bound.

Cross-Project access fails closed.

### 17.4 Intentionally unavailable

    sandbox_create
    sandbox.create(from=...)

Reason:

True fresh/derived Sandbox creation requires Prepared external materialization semantics and SourceDriver /
checkpoint capture/restore.

Do not fake isolation by minting another logical ID over the same mutable environment.

---

## 18. Current Jobs/Process contract

### 18.1 Jobs model

Agent/Process/Graph work share Jobs control surface.

Turns remain Agent timeline.

### 18.2 Finite Job runtime

JobManager remains factual finite Job owner.

JobRuntime/JobHost compose execution and retained output.

### 18.3 Job exposure

job_exposure_current is rebuildable mixed Agent/finite-Job projection.

It is not authority.

### 18.4 Finite provenance

Immutable reservation evidence stores:

    canonical_job_ref
    original_agent_ref
    original_turn
    initiating_actor

Publisher/releaser cannot rewrite initiating actor.

### 18.5 Public finite receipts

Bounded canonical provenance appended to:

- list;
- get;
- ack;
- successful send;
- successful cancel.

Raw read bytes remain raw.

### 18.6 Process

aw-process remains Agent-neutral.

ProcessTerminal remains Agent-neutral.

Model Process control keeps canonical Agent/Turn/actor provenance in ModelProcessFact, not in shared
Process runtime.

---

## 19. Agent exposure/index normalization

### 19.1 Authoritative truth

agent_state_current.

### 19.2 Rebuildable mixed projection

job_exposure_current.

### 19.3 Normal Store open

Under owner lock:

- compare authoritative Agent state against Agent projection rows;
- exact => no writes;
- mismatch => replace only Agent rows;
- preserve finite Job rows.

### 19.4 Original publication position

Recovered from immutable original creation commit evidence:

- session.created;
- branch.created.

No synthetic event.

### 19.5 Not allowed

Normalization does not:

- mutate Agent rev;
- mutate title/lifecycle/admission/history/Sandbox;
- append commit-log/audit facts;
- rebuild from legacy Session naming;
- run in trusted-local direct mode.

---

## 20. Explicitly rejected designs — do not reintroduce

Do not create:

- generic TransactionManager;
- generic OperationManager;
- global replay ledger;
- universal revision clock;
- generic Jobs+Events traversal engine;
- distributed Event router;
- second Event bus;
- public spawn-pool framework;
- second Process owner;
- second Project database;
- public Session API;
- EventLink service;
- generic Source service alongside Sandbox sources;
- Agent↔Sandbox rebind API;
- generic workflow engine;
- every registry implementation upfront;
- cross-store Event→Agent delivery v1;
- cross-store ordered recursive Jobs traversal v1;
- automatic Unknown-effect retry;
- evidence-pin/test infrastructure whose only reason is satisfying a handoff.

Do not merge:

- Agent and Sandbox;
- Sandbox and Process Scope;
- Jobs and Events;
- HistoryGrant and DisclosureManifest;
- Agent rev / Event rev / StorePos / security generations;
- exact provenance facts and authorization roots.

---

## 21. Deliberate distinctions a new agent must preserve

- SessionStore still owns Turn/journal/recovery.
- Agent tables own Agent envelope/security/lifecycle.
- Projection repair/bootstrap is not public authority.
- Principal existence != Domain membership.
- Group manager != group member.
- Owner authority is implicit, not ACL row.
- access_generation != Agent rev.
- inputs_rev != Agent rev.
- HistoryGrant != DisclosureManifest.
- HistoryBase != model cache identity.
- History ancestry != delegation lineage.
- History fork != same-Session branch fork.
- History fork never inherits Sandbox/source/Vault.
- Sandbox != Process Scope.
- Process Occupancy != main-command exit.
- Terminal/PTY custody != ordinary Process Job.
- dotlink timeout/horizon policy belongs to dotlink.
- Event provenance belongs to Phase 2; do not pull it into Phase 1.
- Raw jobs_read process output stays raw bytes.
- canonical ref is identity, not permission.

---

## 22. Current validation evidence

### 22.1 Published Phase 0 / dotlink

Known published green:

- all-target/all-feature source graph;
- shell 86/86;
- Graph 13/13;
- queued Spawn-Lane cancellation;
- pre-spawn timeout/no-Process projection;
- canonical spawn refusal;
- strict Clippy;
- fmt/diff.

### 22.2 Explicitly deferred

Not claimed green:

- real Bubblewrap worker-retirement;
- Windows-native Process gate.

### 22.3 Phase-1 owner foundations

Green evidence includes:

- StoreScope;
- v37→v38 Agent migration;
- v38→v39 principal backfill;
- group lifecycle/access generation fencing;
- seven-permission ACL;
- v40 history owner;
- bounded DisclosureManifest;
- exact history fork;
- model agent(from=...);
- host state shell;
- active close preemption;
- wake retirement;
- finite-work close;
- stable/root close;
- host-created activation;
- task-bearing create/replay;
- provider principal preservation;
- v40→v41 Sandbox migration;
- Sandbox auto/explicit binding;
- Sandbox list/get/close;
- cross-Project create with explicit destination Sandbox;
- v41→v42→v43 staged-plan migration;
- staged input add/edit/delete/reorder;
- atomic Send;
- Send replay;
- steer/interrupt exact control;
- five-method AgentInputsPort;
- Turn provenance through source/fork/refork;
- model settlement canonical Agent/Turn;
- finite Job provenance;
- model Process provenance;
- Agent exposure rebuild/normalization.

### 22.4 Latest exposure rebuild evidence

Focused regression proves:

- real Agent title/admission mutation survives projection deletion/rebuild;
- two Agents recover exact original publication positions;
- published finite Job row survives;
- canonical Agent lookup works after rebuild;
- commit-log cardinality unchanged;
- second normalization pass is zero-write/no-op.

### 22.5 Broad historical test debt

Do not spend a turn fixing unrelated broad fixture debt unless it blocks relevant work.

Previously documented examples:

- stale-schema remote-start/remote-view downgrade fixtures;
- unrelated compact status-bound debt;
- deferred Bubblewrap/Windows gates.

Never call a partial run a full pass.

---

## 23. Current phase map

### Phase 0 / 0B — COMPLETE / PUBLISHED

- shared Process convergence;
- Occupancy;
- aw-process;
- stable Linux Spawn Facility;
- strict child terminal;
- replay/disclosure/provider/lifetime corrections;
- dotlink consumer/product policy;
- dotlink pinned to AW Phase-0 publication.

### Phase 1 — LATE / UNDERWAY

Implemented:

- StoreScope/StorePos;
- v38 Agent owner/ACL;
- v39 principals/groups;
- seven-permission access;
- access_generation;
- v40 history/disclosure;
- exact forks/selectors;
- model agent(from?);
- external host state lower shell;
- close lifecycle composition;
- host activation;
- task-bearing create;
- public agent.create;
- v41 Sandbox owner/binding;
- Sandbox read/close host API;
- non-current close;
- same-user cross-Project create;
- v42 staged input owner;
- v43 mutation replay;
- atomic Send;
- five-method input host API;
- Turn/Tool/model provenance;
- finite Job/Process provenance;
- authoritative Agent exposure normalization/rebuild.

Concrete remaining gap found by current audit:

    native AgentStatePort:
      agent.list
      agent.get
      agent.edit
      agent.admission
      agent.access.list
      agent.access.set

After that, re-audit the sealed host surface before declaring Phase 1 complete.

### Phase 2 — FUTURE

One EventScan for:

- reads;
- subscriptions;
- Handler;
- source-first proof/deny/unknown;
- final Input/Handler reauthorization/dedupe/ack.

Do not pull forward now.

### Phase 3 — FUTURE

- DriverKey;
- SourceDriver;
- SandboxDriver;
- SourceBindings;
- Checkpoint;
- Vault;
- Prepared capture/restore;
- sandbox.create;
- checkpoint restore via shared lower constructor.

### Phase 4 — FUTURE FORMAL JOBS CUTOVER

- StorePos high-water;
- stable Jobs pagination;
- ancestor-fenced tree traversal;
- exact conditional wait;
- full migration cleanup.

Some narrow Jobs/Agent adapters already exist; do not claim formal Phase-4 completion.

### Phases 5–6 — FUTURE

- final typed leaf registries;
- ExtensionRegistry composition;
- dynamic Process extensions;
- TypeScript SDK.

### Phase 7 — FUTURE

- retire compatibility paths;
- remove obsolete Session/Event/Jobs duplicates;
- final docs/status reconciliation;
- grouped cross-repo qualification.

No eighth phase.

---

## 24. Exact next implementation boundary

### 24.1 Build one thin AgentStatePort

Do not create new storage or replay owners.

The authoritative Store implementations already exist.

Expose, through one concurrent headless port:

    agent_list
    agent_get
    agent_edit
    agent_admission
    agent_access_list
    agent_access_set

Use strict sealed parsing.

Recommended shape:

- one AgentStatePort trait in src/headless.rs;
- one native implementation in src/main_full.rs;
- Store methods remain sole state/ACL owners;
- use RPC ID only where an existing mutation replay contract genuinely needs it; do not invent per-method
  operation ledgers;
- read operations are pure Store projections;
- title/admission/access mutations use existing Agent rev/ACL owners;
- no provider/model/Sandbox/Vault/source authority fields;
- current Project/ref defaults only where SPEC already defines them;
- non-current Agent refs must remain canonical;
- parameter whitelists reject authority-bearing extras before dispatch.

### 24.2 Candidate Store methods to reuse

Current source already has:

    SessionStore::agent_get
    SessionStore::agent_list
    SessionStore::agent_edit
    SessionStore::agent_admission
    SessionStore::agent_access_list_ref
    SessionStore::agent_access_set_ref

Do not fork these implementations.

### 24.3 Likely focused regression

Extend the existing strict headless frame/capability regression and the existing host state-shell regression.

Avoid a new fixture family.

Prove:

- all six capabilities advertise together;
- valid list/get reach port;
- edit/admission/access-set use existing revision semantics;
- unknown/authority-bearing fields rejected before port;
- no new state table/replay ledger;
- existing create/close/inputs capabilities remain unchanged.

### 24.4 After AgentStatePort

Freshly audit SPEC §28 host surface.

If all Phase-1 host operations are covered:

- update TODO/current handoff;
- run focused owner gates;
- declare Phase 1 complete only if no real remaining owner/exposure gap exists.

Then move to Phase 2.

Do not prebuild EventScan, SourceDriver or Phase-4 Jobs traversal during AgentStatePort work.

---

## 25. Source map for the next agent

### Current Agent/host hot paths

    crates/aw-store/src/agent_owner.rs
    crates/aw-store/src/agent_host.rs
    crates/aw-store/src/agent_input_plan.rs
    crates/aw-store/src/principal_groups.rs
    crates/aw-store/src/history_owner.rs
    crates/aw-store/src/sandbox_owner.rs
    crates/aw-store/src/job_exposure.rs
    crates/aw-store/src/model_process.rs
    crates/aw-store/src/resources.rs
    crates/aw-store/src/context/
    crates/aw-store/src/lib.rs

    src/agent_lifecycle.rs
    src/agent_jobs.rs
    src/actor_owner.rs
    src/model_process_control.rs
    src/headless.rs
    src/main_full.rs
    src/subagents.rs

### Relevant migrations

    038_agent_owner.sql
    039_domain_principals.sql
    040_history_owner.sql
    041_sandbox_owner.sql
    042_agent_input_plan.sql
    043_agent_input_replay.sql

### Shared Process

    crates/aw-process/
    crates/aw-platform-unix/
    crates/aw-platform-win/
    crates/aw-tools/
    crates/aw-tools-adapter/

### dotlink shared consumer

    src/jobs.rs
    src/mcp/tools/shell.rs
    src/mcp/tools/graph.rs
    src/mcp/tools/jobs.rs
    src/remote_start.rs
    src/limits.rs
    src/transports/

Do not touch dotlink merely because AW Phase 1 advances.

---

## 26. Documentation map

### Normative

    .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
    .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
    .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
    .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md

### Current navigation

    .agents/docs/CURRENT_HANDOFF.md
    .agents/docs/AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md
    .agents/docs/AW-DL200-PHASE1-NEXT-AGENT-2026-11-23.md

### Historical design provenance

    .agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
    .agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
    .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md

Do not create a competing normative spec.

---

## 27. Full implementation chronology

### A. Pre-DL-200

- AW Agent/TurnJournal/ToolPolicy foundations;
- dotlink DL-150 Process/Job/Graph/FS/runtime hardening;
- unified Jobs/Agent convergence;
- cross-repo shared Tool/Process/output/ACK semantics.

### B. DL-200 design

    132bfd7  R1 published baseline
    7c3800c  R6 architecture seal

R2–R5 detailed correctness decisions are preserved in the resolved design checklist.

### C. Shared Process / Phase 0

    307c7f5 Extract shared Process runtime
    320c27d Relax shared Process dependency constraints
    22b5562 Expose shared Process semantic stop
    46e1125 Bound Process output drain globally
    a3ba87c Preempt interactive Process input at deadline
    7c16708 Flush captured output at drain bound
    59d67ba Qualify bounded Process drain flush
    0d2dc10 Record cross-repo Process convergence
    20a63a4 Complete DL-200 R6 Process and replay correction
    85ceccc dotlink: Publish DL-200 R6 consumer and repin AW

### D. Phase 1

    9784619 Start DL-200 Phase 1 store-scope foundation
    d9f53db Wire Agent owner seam
    77fced2 Make Agent recovery exact
    b7059fb Mark accepted Agent input
    0e82f73 Activate Agent owner state
    10aa467 Add Domain principal groups
    bcca6dd Add group Agent access
    706f6f1 Add immutable history owner
    a5aa8fd Add full Agent access
    0db1144 Fork Agent history
    0f3f3eb Wire Agent from refs
    05d8a74 Refresh DL-200 handoff
    15968db Add host Agent state shell
    6bc8f6f Preempt active Agent close
    e0a9562 Retire Agent wake recipients
    91e79c4 Close Agent finite work
    c9501e5 Compose stable Agent close
    6bc8c97 Retain root Agent Job runtime
    0af6a63 Add root Agent close seams
    086ed74 Compose root Agent close
    ed0332f Activate host-created Agents
    53f1201 Publish initial Agent task
    0c695a6 Expose host Agent create
    7833941 Own logical Sandboxes
    9633dfb Expose Sandbox reads and close
    1d4988b Route non-current Agent close
    426d1f8 Authorize cross-Project Agent create
    c6718fc Own staged Agent input plans
    e546a75 Send staged Agent inputs atomically
    503706d Expose staged Agent input host API
    838ea82 Project immutable Turn provenance
    90a1b86 Project finite Job provenance
    03d1224 Rebuild Agent exposure from owner state

---

## 28. Working procedure for the next implementation turn

At turn start:

1. git rev-parse HEAD
2. git status --short --branch
3. inspect only exact owned paths
4. never inspect .agents/plans/ux/**
5. reread exact SPEC/IMPLEMENTATION section for the slice
6. identify existing owner to reuse
7. implement one coherent cut
8. reuse an existing regression where possible
9. run focused real integration
10. run strict relevant check/Clippy/fmt/diff
11. update CURRENT_HANDOFF.md + plan TODO.md factually
12. private-index expected-parent signed commit
13. verify signature
14. pause
15. push only if separately requested

Do not spend the turn expanding evidence after the actual integration is green.

---

## 29. Atomic Git checkpoint pattern

Conceptual pattern:

    parent="$(git rev-parse HEAD)"
    idx="$(mktemp /tmp/aw-index.XXXXXX)"
    trap 'rm -f "$idx"' EXIT

    GIT_INDEX_FILE="$idx" git read-tree "$parent"
    GIT_INDEX_FILE="$idx" git add -- <exact-owned-paths>
    GIT_INDEX_FILE="$idx" git diff --cached --check "$parent"

    tree="$(GIT_INDEX_FILE="$idx" git write-tree)"
    commit="$(git commit-tree -S "$tree" -p "$parent" -m '<terse message>')"

    git update-ref refs/heads/main "$commit" "$parent"
    git verify-commit "$commit"

Then synchronize only exact owned paths into the ordinary index if required.

Never inspect or stage reserved UX paths.

---

## 30. Things to re-audit before claiming Phase 1 complete

After AgentStatePort:

- compare SPEC §28 host surface to actual headless capabilities;
- verify no missing non-current Agent operation is required;
- verify Agent exposure normalization is still exact/no-op on healthy Store;
- verify no legacy Session state is being treated as public authority;
- verify no Agent/Sandbox/Process duplicate owner has appeared;
- verify staged input replay remains one shared v43 owner;
- verify Event provenance work was not accidentally pulled into Phase 1;
- verify no Phase-4 pagination/tree machinery was prebuilt;
- verify no public sandbox_create is advertised before Phase 3 materialization;
- verify no raw provider/model/Vault/source authority field entered host RPCs.

If no concrete gap remains, Phase 1 may be marked complete and Phase 2 can begin.

---

## 31. Starting prompt

The copy-ready companion is:

    AW/.agents/docs/AW-DL200-PHASE1-NEXT-AGENT-2026-11-23.md

Use that prompt verbatim unless the user gives newer instructions.

---

## 32. Definition of a good future handoff

A future handoff must keep these distinctions explicit:

- sealed design vs implemented source;
- local commit vs published remote commit;
- implemented source vs executed evidence;
- current AW local HEAD vs dotlink-pinned AW revision;
- authoritative owner vs rebuildable projection;
- durable facts vs live runtime acceleration;
- Agent vs Session;
- Agent history vs Sandbox ancestry;
- Agent rev vs ACL/security/input/store ordering revisions;
- current host surface vs future Phase-2/3/4 APIs;
- green tests vs deferred/unavailable platform evidence.

Do not create handoff prose that hides uncertainty or turns future design into implemented fact.
