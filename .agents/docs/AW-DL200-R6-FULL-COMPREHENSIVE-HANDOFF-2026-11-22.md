# AW + dotlink + DL-200 R6 full comprehensive handoff — 2026-11-22

Prepared: 2026-11-22
Owner: @pvl
Source-authoring authority: **L5 only**

AW:
`/home/pvl/spaces/abird/src/aw`

dotlink:
`/home/pvl/spaces/abird/src/dotlink`

Read-only dotlink 0.17 release reference:
`/home/pvl/spaces/abird/src/dotlink-017-mounts`

---

## 0. Purpose and authority

This is the **current comprehensive onboarding handoff** for DL-200 R6 and the shared AW/dotlink
runtime work that precedes it.

It exists so a new agent can recover:

- the original architecture and why it changed;
- the R1→R6 design lineage;
- the shared Process/dotlink release correction;
- the exact implementation checkpoints already landed;
- the current durable schema/owner contracts;
- the current validation evidence;
- the frozen/deferred areas;
- the exact next implementation boundary;
- the operating/Git rules needed to continue without corrupting parallel work.

This is a **context/navigation handoff**, not a second normative specification.

Authority order:

1. live source + tests + actually executed results;
2. `AW/AGENTS.md`;
3. sealed DL-200 R6 `SPEC.md`;
4. DL-200 R6 `IMPLEMENTATION.md`;
5. DL-200 R6 `TODO.md`;
6. `AW/.agents/docs/CURRENT_HANDOFF.md`;
7. this comprehensive handoff;
8. older dated handoffs/design reviews.

If prose conflicts with live source or the sealed SPEC, the source/SPEC wins.

No tag, release or deployment is authorized merely by this handoff.

---

## 1. Current state in one page

### 1.1 Current published Git boundaries

Verified actual GitHub `main` refs on 2026-11-22:

```text
AW main
0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e
Wire Agent from refs

dotlink main
85ceccc1088e201c98c994a46a68834440eeab32
Publish DL-200 R6 consumer and repin AW

dotlink release/0.17
576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2
v0.17.9
```

dotlink still intentionally pins all six AW Git dependencies to the published Phase-0/R6 source:

```text
20a63a4d4547cc20b2a154787823125b82f5e385
Complete DL-200 R6 Process and replay correction
```

That is correct. The later AW Phase-1 Agent/history/access work is not a reason to repin dotlink
unless a real shared dependency consumer requires it.

Local dotlink `origin/main` may appear stale after explicit SSH pushes; actual GitHub `main` was
verified at `85ceccc...`.

### 1.2 Current design/phase status

- **DL-200 R6 is sealed and normative.**
- The shared Process/release-correction amendment is part of sealed R6.
- Phase 0 shared Process convergence is published.
- Phase 0B replay/admission/disclosure/lifetime correction is complete and published.
- Real Bubblewrap worker-retirement and Windows-native qualification were explicitly deferred by
  the user; they remain follow-up evidence and do not block Phase 1.
- **Phase 1 is underway.**
- Phase 1 has advanced substantially through:
  - StoreScope/StorePos;
  - authoritative Agent owner cutover;
  - Domain principal/groups;
  - seven-permission Agent ACL;
  - group-aware access generation;
  - immutable history/disclosure owners;
  - exact history forks;
  - Agent/settled-Turn `from` selectors;
  - model `agent(..., from?)`.
- The complete external host §28.3 `agent.create` shell and remaining public Agent lifecycle shell
  are **not** complete.
- Phase 2+ remain future work. Do not mark Phase 4 complete merely because narrow public/model
  adapters were pulled forward to exercise Phase-1 owners.

### 1.3 Last implementation checkpoint before this docs refresh

```text
0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e
Wire Agent from refs
```

This documentation refresh may sit one docs-only commit above that implementation checkpoint.

---

## 2. Mandatory read order for the next agent

Read before editing source:

1. `AW/AGENTS.md`
2. **this file**
3. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md`
4. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md`
5. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md`
6. `AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`
7. `AW/.agents/docs/CURRENT_HANDOFF.md`
8. `AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md`
9. `AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
10. `dotlink/AGENTS.md`
11. `dotlink/.agents/docs/CURRENT_HANDOFF.md`
12. live source/tests for every owner touched

For forensic history when needed:

- `AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md`
- `AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
- `AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
- `AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md`
- older AW/dotlink 0.17 handoffs under `.agents/docs/`
- immutable DL-150 reference under `.agents/plans/dotlink-tool-port/reference/`

Do not use an old handoff to override current source or the sealed R6 SPEC.

---

## 3. Non-negotiable operating constraints

### 3.1 Machine/source authority

- Use **dotlink---l5 only** for source-authoring work.
- Do **not** use A1.
- x2 may be an inference/execution host only when explicitly useful; it is not source authority.
- Work in the canonical checkouts above.
- Do not create another worktree.

### 3.2 Parallel ownership

- Preserve all unrelated/concurrently owned work.
- **Never inspect, list, stage, reset, stash, modify or commit
  `AW/.agents/plans/ux/**`.**
- Treat that subtree and its staging as opaque.
- `/home/pvl/spaces/abird/src/dotlink-017-mounts` is read-only release reference.

### 3.3 Frozen/unpromoted work

Unless the user explicitly promotes it:

- Sandbox/mount/config-v13 implementation remains frozen.
- The 017 Sandbox/mount implementation remains frozen.
- The future strong Linux PID-namespace/PID-1 supervisor remains design/future work.
- Do not reopen published 0.17 D0–D7/shared-runtime/process-control work without a concrete
  regression.

### 3.4 Implementation style requested by the user

For each implementation turn:

- target roughly **30 minutes**;
- stop at the next coherent green checkpoint;
- do not over-engineer granular tests/evidence;
- prefer grouped owner/failure-boundary regressions over fixture forests;
- keep Rust primitives simple, elegant, composable and reusable;
- maximize reuse across libraries where semantic ownership matches;
- prefer zero-cost/static abstractions where natural;
- take easy hot-path wins when they do not add maintenance complexity;
- if high performance requires a convoluted design, prefer the clean design first and optimize
  proven hot paths later.

### 3.5 Git rules

- Signed commits only. Never disable signing.
- Commit exact owned paths only.
- Use a private `GIT_INDEX_FILE`.
- Build the commit from the expected parent and CAS-update the branch ref.
- Verify the commit signature before moving the branch.
- Preserve unrelated staging.
- Never force-push.
- Do not push/tag/release/deploy unless separately requested.
- The user's standing implementation preference in this workstream has been a terse
  **caveman-style signed checkpoint commit on each coherent turn**, but push remains separately
  gated.

Relevant doc:
`.agents/docs/GIT-WORKFLOW.md`.

---

## 4. Developer mental model — the R6 north star

### 4.1 One truth, one owner

The central R6 rule:

> One authoritative source for each semantic truth. Mutable truth has one Owner and one
> linearization point; immutable truth has one stable Fact identity; uncertain external work has
> one Operation custody.

Global names identify resources. They do **not** create a global transaction or global order.

Consistency/order remains store-local.

Derived data is a projection, capability, cursor or adapter—not a second owner.

### 4.2 Public concepts

| Concept | Owns / represents |
|---|---|
| Agent | globally named collaboration/security envelope, history continuation and execution |
| Job | Agent, Process or Graph work; Turns are Agent timeline; Nodes belong to Graphs |
| Sandbox | durable logical mutable execution environment + named SourceBindings |
| Event | immutable source-scoped fact ordered in one local Event store |
| Checkpoint | immutable history anchor + exact captured named SourceSet |
| Extension | installed typed definitions, never runtime authority |

Process **Scope** is private and owns one dispatch/process lifetime.

Sandbox answers *where/how* execution may occur. Scope answers *which processes belong to this
dispatch lifetime*. Neither owns the other.

Vault grants remain separate capabilities, never Sandbox contents/checkpoint secrets.

### 4.3 Canonical namespace

```text
/d:<domain>/i:<principal>/p:<project>
  ├── /a:<agent>[/t:<turn>[/j:<job>[/n:<node>]]]
  └── /s:<sandbox>
```

`i:` is one user-or-group principal namespace.

A global ref does not grant visibility, authority, disclosure, cross-store cursor order or a
multi-store transaction.

### 4.4 Model Jobs surface

The final model Jobs surface is exactly:

```text
jobs_list
jobs_get
jobs_read
jobs_send
jobs_ack
jobs_cancel
```

No queue CRUD model surface.
No Jobs-only subscription API.
No pause-after-turn model verb.
No separate steer/interrupt send-mode vocabulary.

`jobs_send` is directed Agent input or Process stdin/EOF.

### 4.5 Agent construction

Model:

```text
agent(task, title?, background=false, from?)
```

Host north star:

```text
agent.create(
  from?    = <Agent ref | settled Turn ref>,
  project  = <Project ref; default current/source Project>,
  title?,
  task?,
  access   = "private" | "inherit" | [{ principal, permissions }, ...],
  sandbox  = "auto" | <Sandbox ref>
)
```

History ancestry, delegation lineage and Sandbox ancestry are distinct.

`checkpoint.restore` must eventually reuse the same lower construction/publication owner while
also restoring its SourceSet into a new Agent + new Sandbox.

### 4.6 Event model

There is one **Event** concept. Signals were removed.

Canonical Event subject projection is NATS-like:

- one-token wildcard;
- terminal subtree wildcard;
- system Events and custom Agent Events in the same subject model;
- custom Agent Event names are `#name`;
- no second Signal/topic bus.

Examples:

```text
d:acme.i:alice.p:core.a:reviewer.e:changed
d:acme.i:alice.p:core.a:reviewer.e:#ready
```

V1 cross-store Event→Agent delivery and cross-store ordered Jobs/Event traversal are explicitly
unsupported.

### 4.7 Agent vs Session

Public identity/security is Agent-owned.

Internal SessionStore remains the backing Turn/journal/recovery owner.

Do not resurrect a public AW Session resource/API.

A Session is implementation structure; the Agent is the globally named collaboration/security
envelope.

### 4.8 Project

Project is a Workspace-backed public view, not a second database/owner.

### 4.9 Groups/principals

- one Domain-scoped principal-handle namespace;
- user/group kinds are metadata, not different ref grammars;
- no nested groups in v1;
- group membership changes advance group rev, not Agent rev fanout;
- group-owned Agent means active group members have owner authority;
- group management authority is distinct from group membership.

### 4.10 Sandboxes / Sources / Vault

- Sandbox = durable logical mutable execution state.
- Sandbox ancestry is orthogonal to Agent history ancestry and authority.
- SourceDriver owns named source implementation/capture/restore semantics.
- Checkpoint captures exact immutable HistoryBase + named SourceSet.
- Vault grants are separate provider-backed capabilities.
- History fork never inherits Sandbox/Vault/source authority.
- Checkpoint restore recreates SourceSet but does not restore credentials/live leases.

### 4.11 Extensions

ExtensionRegistry composes typed definitions.

Installed definitions are inert until explicitly bound/activated.

Keep taxonomy:

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

Do not make ExtensionRegistry a second runtime/policy/database.

---

## 5. Six private roles — vocabulary, not a framework

| Role | Meaning | Example |
|---|---|---|
| Owner | sole mutable authority/commit boundary | `agent_state_current`, Agent Input |
| Fact | immutable record | HistorySpan, Event, ProcessTerminal |
| Operation | durable effect/replay/recovery custody | Prepared/Unknown claim |
| Projection | rebuildable derived index | Job exposure, Event subject index |
| Cursor | bounded observation progress | Jobs high-water, Event read cursor |
| Scope | one Process lifetime | private child/containment owner |

Do not create generic base classes/managers around these names.

---

## 6. R6 private mechanics

### 6.1 StoreScope + StorePos

- Reuses existing persistent `store_id`.
- Reuses nonzero `restore_generation`.
- `StoreScope = { StoreId, RestoreGeneration }`.
- `StorePos(u64)` wraps store-local `commit_log.seq`.
- StorePos values compare only within the same StoreScope.
- Same-store restore increments generation; old cursors/projections become stale.
- Public/persisted compatibility may remain `u64` where required.

Do not turn StorePos into a global clock or universal revision.

### 6.2 Replay / Prepared / ExternalEvidence

Reuse existing authenticated request/tool replay identities.

Rules:

- same authenticated identity never repeats an external effect;
- before uncertain external/materializing work, claim Prepared using the existing owner;
- Unknown is inspected/reconciled, never blindly replayed;
- external mutable authority must be immutable proof or provider-held/fenced through AW publication;
- AW-store authority is rechecked in the final publication transaction;
- publication commit is the single visibility point;
- replay without current disclosure may return only minimal non-retryable acknowledgement.

No global replay ledger.

### 6.3 Projection publication

Rebuildable projections:

- build privately through head H;
- under final swap/write fence verify current head == H or catch up the intervening range;
- atomically activate only a contiguous safe prefix;
- never issue a cursor high-water over a projection hole;
- never swap an H-stale replacement over newer rows.

### 6.4 Cursor envelopes

Cursor state is consumer-specific and may bind:

- caller;
- normalized selector;
- StoreScope/affinity;
- placement/security generation;
- high-water/progress;
- traversal frontier where that consumer needs it.

A cursor is navigation/observation state, never authority.

Do not create one generic Jobs+Events traversal engine.

### 6.5 EventScan

Future Phase-2 scanner is one source-first ordered primitive producing candidate/proven-skip/
blocked/stale/end style outcomes.

It is not:

- a Handler executor;
- an Input scheduler;
- final authorization.

### 6.6 DriverKey

Stable driver namespace/local ID only.

Each persisted SourceBinding/anchor/recovery artifact carries its own schema version.

No global driver database/schema clock.

### 6.7 Typed registry staging

One private staged publication mechanic may be reused by real leaf registries.

Do not prebuild every registry family.

### 6.8 Shared Process runtime

`aw-process` is the Agent-neutral async owner for:

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
- output storage/retention;
- product projection.

`aw-terminal` remains separate transferable PTY/Terminal custody.

---

## 7. Shared Process / dotlink release-correction lineage

### 7.1 Original issue

dotlink v0.17.7 identified that Bubblewrap `--die-with-parent` relies on Linux
`PR_SET_PDEATHSIG`, where the relevant parent is the **forking thread**.

A child spawned on a retireable Tokio worker could therefore receive SIGKILL when that worker
retired even though the host process remained alive.

The fix belongs at the shared Process launch boundary, not in another dotlink-local lifecycle owner.

### 7.2 Sealed/implemented shape

Linux `aw-process` uses one private logical Spawn Facility:

- O(1) stable process-lifetime Spawn Lane threads;
- **N=1 initially**;
- async bounded admission;
- only `Command::spawn` + exact `DirectScope` attachment occurs on the stable lane;
- resulting `Process: Send` remains a lightweight O(n) async object;
- private RAII transfer of `{ Child, DirectScope, pid }`;
- lost receiver after spawn cleans the exact child/scope;
- lane does not retire while the host lives;
- panic poisons future launch admission but does not kill the lifetime-anchor thread;
- no `spawn_blocking` / `block_in_place` lifetime anchor.

### 7.3 Absolute deadline / stale launch

One absolute execution deadline is established at canonical Running/Start.

It covers Spawn-Lane queueing + Process execution.

Lane-side cancellation/deadline state is rechecked immediately before fork.

Outcomes:

- cancelled before Launching: no Process, no ProcessTerminal, no StopRecord;
- timed out before Launching: no Process, no ProcessTerminal;
- Launching wins: real child follows ordinary Process settlement;
- definite spawn refusal remains factual no-Process failure;
- setup uncertainty remains Unknown.

### 7.4 Terminal semantics

`ProcessTerminal` owns:

- physical exit;
- optional StopRecord;
- Occupancy;
- per-stream output completeness.

`Occupancy::{Empty, Live, Unknown}` replaced the internal old Tree terminology.

dotlink keeps compatibility projection:

- Empty → no `tree`;
- Live → `tree="leftovers"`;
- Unknown → `tree="unknown"`.

Physical exit, semantic stop and descendant occupancy are distinct facts.

Stop evidence is reconciled against physical exit before product wording.

### 7.5 Strict Linux child terminal

Pidfd/SIGCHLD is only a wake.

Owned main-child terminal proof uses parent-owned non-reaping `waitid(... WNOWAIT)`.

Do not treat ECHILD + pidfd readability as owned terminal proof.

### 7.6 Windows source correction

Source carries:

- `CREATE_SUSPENDED | CREATE_NO_WINDOW`;
- private Job termination marker;
- async/cancel/deadline-preemptible post-exit Job accounting settle.

Windows-native qualification is still open because L5 has no native Windows Rust target/host.

### 7.7 dotlink product policy

dotlink owns conversational product horizon, not AW:

- 35-minute turn horizon;
- 10-minute report/hand-back reserve;
- therefore 25-minute max new command ceiling;
- local-only/unbounded surfaces do not inherit that product advice;
- advice uses current absolute execution deadline;
- retained Job timeout truth is never rewritten.

### 7.8 Published convergence

AW Phase-0 corrective publication:

`20a63a4d4547cc20b2a154787823125b82f5e385`

dotlink consumer publication:

`85ceccc1088e201c98c994a46a68834440eeab32`

dotlink pins exactly AW `20a63a4...`.

Read-only release reference remains exact `v0.17.9 / 576d0a1...`.

---

## 8. Phase 0B — completed replay/admission/disclosure/lifetime work

Do not redo without a concrete regression.

Completed contracts include:

- original authenticated replay identity retained;
- replay never re-runs external effects;
- current disclosure can reduce replay output to opaque/minimal acknowledgement;
- compact consumed AW policy rows become minimal indexed never-retry tombstones only after durable
  recovery boundaries;
- immutable audit retains full historical evidence;
- dotlink Remote Start compacts to payload-free never-retry tombstones rather than deleting
  authorization identity;
- Unknown remains a tombstone;
- participant Provider/payer identity was corrected;
- long-lived admission no longer depends on lifetime Session audit/event scans;
- live capacities are active-resource bounds, not retained-history ceilings.

---

## 9. Historical design lineage

### 9.1 DL-150 foundations

DL-150 established lower contracts inherited by DL-200 unless explicitly superseded:

- one shared host-neutral Tool/Graph/finite-Job semantics layer;
- deterministic Graph/node semantics;
- replay and current-authority separation;
- raw retained output vs serialized result;
- explicit ACK release;
- bounded paging/result budgets;
- physical Process exit vs semantic stop vs output coverage separation;
- no blind retry of Unknown effects;
- exact authority/revocation at effect boundary.

Reference:
`.agents/plans/dotlink-tool-port/reference/`.

### 9.2 2026-11-09 → 2026-11-14 unified Jobs + Agent runtime

This line established:

- Jobs as consolidated work/control plane;
- Agent/Process/Graph as meaningful Job kinds;
- Turns as Agent timeline rather than top-level Jobs;
- Agent history/collaboration distinct from execution Process/Graph;
- subagents under bounded delegation;
- current authority/grant checks at new-effect boundaries;
- Graph ownership constraints;
- hard pre-1.0 cutover philosophy;
- dotlink unified Jobs bridge.

Primary historical handoff:
`UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`.

### 9.3 2026-11-15 → 2026-11-20 cross-repo convergence

This work converged:

- shared Tool/Graph/output/ACK semantics;
- dotlink 0.17 hardening;
- Remote Start replay;
- Process control/cancellation;
- release branch corrections;
- AW/dotlink shared Process extraction;
- bounded output/drain/interactive-stdin semantics.

Historical handoffs preserve detailed D4–D7/0.17 chronology.

### 9.4 DL-200 R1 — public model simplification

R1 simplified the public architecture around:

- Agent as public collaboration/security/history unit;
- Jobs as unified work plane;
- Event as one fact/subscription substrate;
- Session removed from public API;
- Project as Workspace-backed view;
- minimal Agent/Sandbox/Checkpoint/Vault/Extension APIs;
- model `agent(task, from?)`;
- exact history fork semantics.

### 9.5 R2–R5 — correctness hardening

R2–R5 resolved the detailed contracts around:

- authoritative Agent migration;
- Agent ACL/groups/principal identity;
- immutable HistorySpan/Base/Grant;
- scalable DisclosureManifest;
- exact settled-Turn forks;
- Event subject/wildcards/store affinity/security;
- Event migration and delivery;
- Job pagination/high-water/tree authorization;
- SourceSet/checkpoint/SourceDriver recovery;
- Sandbox/Source/Vault boundaries;
- typed Extension registries;
- host input plans;
- replay identity/tombstones;
- publication-fenced construction;
- Process settlement and strong-supervisor design.

Exact historical design checklist:
`DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md`.

### 9.6 R6 — simplification without a new public architecture

R6 retained the public model/phases and simplified private mechanics:

- six descriptive roles, no framework;
- one authoritative owner per truth;
- StoreScope/StorePos;
- shared replay/Prepared vocabulary, no new ledger;
- ExternalEvidence;
- one projection publication protocol;
- consumer-specific cursor envelopes;
- one EventScan;
- DriverKey + per-artifact schema;
- one typed registry staging core;
- dedicated `aw-process`;
- Tree→Occupancy source rename;
- explicit rejection of generic managers/coordinators.

R6 was explicitly sealed 2026-11-21.

Commit provenance:

- `132bfd7` — published R1 baseline;
- `7c3800c` — R6 architecture seal.

---

## 10. Phase-1 implementation ledger — every landed checkpoint

The following sequence is the current implementation line after Phase 0:

### 10.1 `9784619` — Start DL-200 Phase 1 store-scope foundation

Landed:

- private StoreScope over existing `store_id + restore_generation`;
- transparent `StorePos(u64)`;
- typed commit-log position flow into Job exposure;
- exact Agent lookup through existing exposure projection + authority recheck.

No generic cursor engine was prebuilt.

### 10.2 `d9f53db` — Wire Agent owner seam

Introduced the mutation seam needed to move Agent truth away from audit-derived semantics.

### 10.3 `77fced2` — Make Agent recovery exact

Recovery advances exact changed Agents and avoids arbitrary broad rev fanout.

Cold recovery may dedupe arbitrary changed `(session,branch)` keys.

### 10.4 `b7059fb` — Mark accepted Agent input

Prepared→Accepted Input custody now advances the exact Agent owner.

### 10.5 `0e82f73` — Activate Agent owner state

Schema v38 authoritative owner cutover became active.

Key tables:

#### `agent_state_current`

Owns:

- frozen canonical handles;
- backing Session + exact branch;
- owner kind/handle;
- title;
- Agent `rev`;
- `acl_rev`;
- lifecycle `open|closing|closed`;
- admission `open|held`;
- history_base_id/history_grant_id pointers;
- immutable delegation parent lineage;
- Sandbox pointer.

#### `agent_acl_current`

Owns direct user/group allow-only grants.

v38 migration:

- exclusive BEGIN IMMEDIATE + migration lease;
- seeds all existing exact Agent bindings from complete v37 exposure + authoritative Session/User
  facts;
- initial Agent rev uses exact `MAX(audit.seq)+1` bridge;
- initial participation ACL is conservative;
- no invented `derive/share`;
- verifies branch/exposure/owner completeness and FKs;
- activates schema only after complete seed;
- no online dual-write period.

After activation, audit is evidence, not Agent rev owner.

Creation/fork/Turn/Input/Applied-Control/subscription/child/close/recovery paths now update the exact
Agent owner.

### 10.6 `10aa467` — Add Domain principal groups

Schema v39 adds one Domain principal namespace:

#### `domain_principals`

- key: Domain + principal handle;
- kind: user/group;
- optional user backing;
- active/deleted tombstone state;
- stable no-handle-reuse identity.

#### `principal_groups`

- one owner user;
- monotonic group rev;
- name/display;
- deletion tombstone.

#### `principal_group_members`

- explicit user-set membership;
- active/removed state;
- membership revision;
- no nesting.

Migration backfills stable user handles from:

- Domain memberships;
- Agent owners;
- direct Agent ACL principals.

Domain membership insertion registers the user principal automatically.

A principal may exist in a Domain without becoming Domain admin/member.

Group roster reads require group-management authority; ordinary principal metadata does not leak
membership.

Group delete refuses current authority-bearing Agent owner/ACL references.

### 10.7 `bcca6dd` — Add group Agent access

Effective Agent authorization became:

```text
owner rights
UNION direct user grant
UNION all matching active group grants
```

Group-owned Agent owner authority resolves through live membership.

No deny entries.

One bounded ordered SQLite stream evaluates ACL dependencies; no heap-side grant map.

Opaque 32-byte `access_generation` is derived from:

- exact Agent + actor;
- Agent `acl_rev`;
- active user security epoch;
- owner kind/handle;
- direct/group grant revs;
- exact group handles/revs;
- effective permissions.

It is a **security observation fence**, not a mutation rev.

Group membership changes:

- advance group rev;
- do not advance Agent rev;
- immediately change/deny effective access;
- old wake/history observations become stale.

AgentJobs history cursors and wake authority bind to this generation.

Wake authority digest moved to v3 and the mint→persist boundary revalidates the full generation
without adding another durable receipt column.

Mixed Agent/finite-Job exposure candidates became group-aware before authoritative recheck.

### 10.8 `706f6f1` — Add immutable history owner

Schema v40 adds lazy immutable history/disclosure owners.

#### `history_spans`

Store exact:

- source Agent ref;
- backing Session/branch;
- local root;
- exact head;
- content digest;
- provenance digest;
- manifest digest;
- total Turns;
- next Turn.

#### `history_span_nodes`

Stores exact ordered node IDs + recorded node digests for backing integrity.

#### `history_bases`

Structurally composes:

- optional parent base;
- one local span;
- history content digest;
- manifest root;
- Turn coordinates.

#### `history_grants`

Immutable disclosure fact:

- exact base;
- source Agent ref;
- destination Agent ref;
- manifest root;
- creator;
- optional superseded prior grant.

#### `history_grant_principals`

Normalized user/group disclosed audience.

#### DisclosureManifest

Persistent content-addressed SHA-256 DAG:

- `disclosure_authority_roots`;
- `disclosure_manifest_nodes`;
- `disclosure_manifest_entries`;
- `disclosure_manifest_children`.

Current fanout = 64.

Per-seal input bound = 16,384.

Manifest structural sharing avoids giant mutable flat JSON closure.

Current provenance extraction conservatively captures:

- `node_dependencies` as exact source-node facts + stable legacy Session authority roots;
- materialized facts as exact fact hashes + materialization-policy roots.

Unsupported/ambiguous authority expansion fails closed.

Lazy seal:

- content-addressed/idempotent;
- no Agent rev;
- no Agent changed Event;
- physically retains Session journal as local source;
- repeated same-branch seal replaces the same local tail rather than parenting it onto itself;
- inherited foreign prefix remains structurally shared.

Existing checkpoint path became the first production seal consumer.

### 10.9 `a5aa8fd` — Add full Agent access

Introduced full seven-permission vocabulary:

```text
view
contribute
submit
control
derive
share
manage
```

Rules:

- owner has all permissions implicitly;
- non-view permission requires explicit view;
- allow-only;
- direct user/group ACL rows;
- group union semantics;
- owner cannot be represented as redundant ACL row.

APIs/primitives:

- `AgentPermission`;
- `AgentPermissions`;
- `AgentAccessEvidence`;
- `agent_access_list`;
- `agent_access_set`;
- one canonical ACL mutation path.

Old five-permission compatibility setters became thin adapters.

`access_generation` digest version advanced to include all seven bits.

New history audience expansion:

- requires current Agent `share`;
- seals exact history boundary;
- bounded-walks disclosure authority roots;
- revalidates source disclosure;
- preserves all already-disclosed user/group principal identities;
- mints one immutable superseding HistoryGrant;
- performs the single ACL mutation/rev bump in the same transaction.

Narrow/remove does not rewrite historical grants.

A principal already factually disclosed may be re-added without requiring a new `share`.

Materialized external-policy roots still fail closed until source-equivalent share authority exists.

### 10.10 `0db1144` — Fork Agent history

Added structurally shared exact-history destination Agent construction.

Lower constructor:

`create_agent_from_agent_history(..., access=private|inherit)`

Semantics:

- current source `derive` required;
- exact latest committed source boundary sealed;
- disclosure closure validated;
- destination gets a **fresh Session + fresh Agent**;
- destination is physically just its own root/local tail;
- inherited Turns are not copied into destination nodes;
- HistoryBase/HistoryGrant pointers reference shared immutable history;
- destination Session `next_turn` starts at inherited base next_turn;
- fresh Agent rev/runtime/input state;
- `private` carries only canonical owner;
- `inherit` copies direct source user/group ACL rows as fresh grant-rev-1 rows;
- source/destination ACLs evolve independently;
- history ancestry does not copy Sandbox/runtime/source state.

History read/activation now renders:

```text
flatten(inherited HistoryBase spans) + destination-local projected tail
```

using the existing context renderer.

Each inherited backing node is re-hashed against the recorded immutable span digest before projection.

Busy source rule:

- active Turn moves branch head to unresolved prompt;
- active run already stores `produced_from_head`;
- fork uses that committed prior head + turn ordinal;
- forking during Turn N inherits only through N-1;
- destination starts divergent Turn at N;
- no in-flight prompt/run/Process/Graph/watch/source rev is inherited.

Destination HistoryGrant starts a new lineage; the source grant is evidence, not destination owner.

### 10.11 `0f3f3eb` — Wire Agent from refs

Added exact history source selectors/adapters.

Canonical Store adapter:

`create_agent_from_history_ref`

Store accepts only **absolute canonical**:

- Agent ref;
- settled Turn ref.

Turn rules:

- canonical decimal handle;
- local attached settled result may define exact boundary;
- otherwise inherited HistoryBase chain is searched;
- active/unsettled Turn fails closed;
- no fallback to mutable Agent head.

Model `agent(..., from?)`:

- reuses existing `ExecutionRef` grammar;
- contextual `.`, relative and parent resolution occurs at caller surface;
- resolved source must remain in current Project for model control;
- canonical ref is passed into the existing durable child-allocation transaction;
- Store rechecks derive/disclosure before publication;
- history is attached before existing child relation/replay facts commit;
- failure leaves no half-published child.

Replay:

- canonical `from` participates in semantic fingerprint;
- `from="."` and equivalent absolute Agent ref replay the same committed child;
- no-`from` request keeps old fingerprint JSON shape byte-for-byte for upgrade compatibility.

Policy delegates carry no history source.

---

## 11. Current authoritative Agent contracts

### 11.1 Identity

Canonical Agent ref components are frozen by owner state.

Public Agent lookup must return an authoritative exact owner/binding after any projection candidate
lookup.

Job exposure remains a projection/candidate owner, not Agent truth.

### 11.2 Revisions

Keep distinct:

- Agent `rev`;
- Agent `acl_rev`;
- group rev;
- user/security epochs;
- derived `access_generation`;
- StorePos;
- Event `event_rev`;
- future `inputs_rev`;
- output offsets.

Do not collapse into one clock.

### 11.3 What advances Agent rev

Meaningful Agent-owned mutations do.

Examples already wired:

- Turn admission/result/attachment;
- accepted Input entering runtime custody;
- Applied Actor Control;
- changed subscription/child attachment where Agent-owned;
- title CAS;
- direct ACL edit;
- lifecycle close transitions;
- exact recovery mutation.

Reads do not.

Process output, internal Graph progress, cursor reads, host staging do not.

Group membership change does not fan out Agent rev.

### 11.4 Lifecycle

Observable lifecycle:

`open | closing | closed`

Admission:

`open | held`

Close is execution/admission terminal behavior, not delete/purge.

Closed history/access/checkpoint management remains conceptually allowed.

### 11.5 Parent lineage

`parent_agent_ref` means delegation creator lineage.

It does **not** mean:

- history fork ancestry;
- Session branch ancestry;
- HistoryBase parent.

Keep those independent.

### 11.6 Current access behavior

Seven permissions as above.

Effective access is allow-only union.

Group membership/security changes can invalidate `access_generation` without Agent rev.

Agent access metadata/rosters require manage/owner-style authority.

### 11.7 Current host/model construction reality

Implemented:

- lower exact Agent history constructor;
- canonical Agent/settled-Turn Store adapter;
- model `agent(task,title?,background?,from?)`;
- model contextual ref canonicalization;
- same-Project model source restriction;
- private/inherit lower ACL profiles.

Not yet complete as the full host §28.3 surface:

- trusted destination Project redirect/defaulting;
- custom access array in one constructor request;
- Sandbox `auto | ref` selection;
- optional initial task semantics at host layer;
- full host lifecycle shell.

Do not claim those complete.

---

## 12. Current history/disclosure contracts

### 12.1 HistoryBase identity vs model cache identity

HistoryBase is immutable history identity.

It is not the invocation PrefixContentKey.

Do not merge those concepts.

### 12.2 Sealing

Sealing is internal memoization/projection work.

It is idempotent and Agent-revision neutral.

### 12.3 Settled Turn selection

A settled Turn ref is an exact immutable observed-history selector.

It must not be interpreted as mutable source Agent rev.

Local run lookup requires attached non-running final result.

Inherited Turn lookup walks immutable HistoryBase spans.

### 12.4 DisclosureManifest

Manifest contains:

- exact fact references;
- stable authority roots.

Exact provenance and deduplicated authorization roots are different data.

Authority roots identify what must be rechecked; they are not cached authorization except where
a future explicitly immutable export grant says so.

### 12.5 Fork disclosure

For a principal already covered by source HistoryGrant:

- previously granted immutable legacy authority roots need not be spuriously reauthorized;
- newly introduced roots are checked.

Materialized/external roots are treated as revocable/stronger and currently fail closed without an
implemented source-equivalent share contract.

### 12.6 Retention/GC

Design is reachability-based, not crash-sensitive refcounts.

Rows are immutable against mutation but remain reclaimable when future reachability reconciliation
proves they are unreachable.

No GC accelerator should become the authority.

---

## 13. Current principal/group contracts

- one Domain-scoped user/group handle namespace;
- stable user handles preserve already-published `i:<owner_user_id>`;
- user principal existence is not the same as active authentication;
- Domain membership is not the same as principal existence;
- no group nesting;
- group edit is complete member replacement under expected rev;
- group owner is group-management authority, not necessarily a group member;
- group-owned Agent authority belongs to active group members;
- group deletion tombstones identity and refuses current Agent owner/ACL references;
- if future Project/Sandbox/Vault live references become group-bearing, deletion checks must expand
  before those refs can be created.

---

## 14. Current Jobs/access observations

### 14.1 AgentJobs

AgentJobs current authorization uses Agent ACL, not legacy Session participation.

Durable wake and history-cursor authority bind to current access evidence.

### 14.2 Mixed exposure

Mixed Agent/finite Job listing:

- uses projection candidates;
- candidate SQL recognizes user/group owner/grants;
- exact authoritative row access is rechecked before disclosure.

### 14.3 Future Jobs pagination contract

Final Phase-4 listing:

```text
publication_position DESC, canonical_ref ASC
```

Page 1 captures immutable high-water.

Later pages only consider rows <= that high-water.

Tree frontier is navigation only; ancestor gates are reauthorized on resume.

Cross-store recursive Jobs federation remains unsupported in v1.

---

## 15. Future Event contracts that must not regress

Phase 2 is not implemented by this Phase-1 work, but its design is sealed.

Key contracts:

- one immutable `commit_log` Event truth;
- source-local `event_rev`;
- opaque stable Event ID;
- canonical reversible subject encoding;
- broad wildcard projection is rebuildable;
- one contiguous indexed-through position;
- source-first authorization:
  - proven source deny may privately advance;
  - visible source with unresolved external authority blocks without cursor advance;
- Event read cursor binds caller + selector + EventStore/placement affinity;
- hidden sources cannot leak direct count/position/gap;
- subscriptions have generation/baseline/lag semantics;
- active enabled=true is a no-op preserving backlog;
- lagged re-enable creates new generation/baseline;
- Agent EventInput admission rechecks current source/recipient/disclosure + dedupe + causal budget
  atomically;
- v1 delivery requires Event/subscription owner and Agent Input owner in same transaction/store;
- EventHandlers remain separate at-least-once consumers;
- self-delivery suppressed in v1;
- no EventLink service;
- custom Event data <= 4096 bytes / depth <= 8;
- model publish budget <= 32 per Turn;
- active subscriptions <= 128 per Agent;
- events.read limit <= 256, wait <= 30s, internal scan <= 4096 rows/call;
- causal delivery <= 16 hops / <= 256 EventInputs per root.

---

## 16. Future Sandbox/Checkpoint/Source/Vault contracts

### 16.1 Checkpoint

Checkpoint = exact immutable HistoryBase/head + immutable named SourceSet.

Empty SourceSet is valid for history-only capture.

Restore:

- always new Agent;
- always new Sandbox;
- exact captured source materialization;
- no credentials/live leases.

### 16.2 SourceDriver

Externally effectful capture/restore must use Prepared/Unknown semantics.

RecoveryDescriptor must be non-secret:

- opaque IDs/digests/version proofs/provider/Vault refs;
- never bearer token/raw secret/credential URL.

Source count:

- default max 128;
- hard max 1024.

Git is first mandatory SourceDriver.

### 16.3 Sandbox

Logical mutable environment, not persistent OS namespace.

V1 host surface is small:

- sandbox.list/get/create/close;
- sandbox.sources.list/set.

Agent→Sandbox binding immutable in v1.

Change environment by creating a new Agent/Sandbox, not live rebind.

### 16.4 Vault

Vault grants do not inherit from:

- history fork;
- ACL inheritance;
- Sandbox sharing;
- checkpoint restore.

Immediate revocation of an already materialized capability may require factual Process termination.

---

## 17. Explicitly rejected designs — do not reintroduce

Do **not** create:

- generic `TransactionManager`;
- generic `OperationManager`;
- global replay ledger;
- universal revision clock;
- one generic Jobs+Events traversal engine;
- distributed Event router;
- second Event bus;
- public spawn-pool framework;
- second Process lifetime owner;
- second Project database;
- public Session API;
- EventLink service;
- generic Source service alongside Sandbox sources;
- Agent↔Sandbox live rebind API;
- generic workflow engine;
- all registry implementations upfront;
- cross-store Event→Agent delivery in v1;
- cross-store ordered recursive Jobs traversal in v1;
- automatic Unknown-effect retry;
- hidden evidence pinning/test infrastructure whose only purpose is to satisfy a handoff.

Also do not merge:

- Agent and Sandbox;
- Sandbox and Process Scope;
- Jobs and Events;
- HistoryGrant and DisclosureManifest;
- Agent rev / Event rev / StorePos / security generations;
- exact provenance facts and authorization roots.

---

## 18. Validation evidence — current known green gates

### 18.1 Published Phase 0 / dotlink

Pinned ordinary qualification included:

- all-target/all-feature build;
- shell **86/86**;
- Graph **13/13**;
- queued real Spawn-Lane cancellation;
- pre-spawn timeout/no-Process projection;
- canonical spawn refusal;
- strict Clippy;
- fmt/diff check.

Historical v0.17.6 evidence for unchanged semantics remains useful, but old long-running SIGKILL
interruptions are not evidence of an external sender after the v0.17.7 worker-retirement finding.

### 18.2 Deferred platform evidence

Not green/available on L5:

- real Bubblewrap worker-retirement gate: outer sandbox rejects required `/proc` mount;
- Windows-native Process gate: no native Windows target/host.

Do not claim these green.

### 18.3 Phase 1 foundations

Known green evidence includes:

- StoreScope tests 2/2;
- mixed Job exposure regression;
- coordination 24/24;
- populated v37→v38 Agent-owner migration regression;
- branch-local Agent rev/title isolation;
- direct ACL revoke/regrant fencing;
- lifecycle/control/recovery regressions;
- populated v38→v39 principal backfill;
- principal/group lifecycle tests 2/2;
- group membership remove/re-add wake/history fencing with unchanged Agent rev;
- history owner tests **4/4**;
- 130-entry manifest bounded-fanout/content-addressing test;
- exact settled/active history fork test;
- legacy same-Session `fork_shares_prefix_without_changing_original_head`;
- model `agent(from=".")` inheritance + contextual/absolute replay equivalence;
- existing no-`from` stable Agent replay regression;
- strict all-target/all-feature `aw-store + aw-cli` Clippy;
- all-feature checks;
- fmt + diff checks.

### 18.4 Known unrelated/test debt

Do not spend a turn “fixing” these unless they block relevant work:

- two broad `remote_view` tests have historically carried stale-schema assumptions;
- a broad aw-tools compact status-bound issue has been unrelated to this owner work;
- Bubblewrap/Windows gates remain environment-deferred as above.

---

## 19. Exact implementation phase map

### Phase 0 / 0B — COMPLETE/PUBLISHED

- shared Process convergence;
- Occupancy;
- aw-process;
- stable Linux Spawn Facility;
- strict child terminal;
- replay/disclosure/provider/lifetime corrections;
- dotlink consumer/product policy;
- dotlink pin to AW Phase-0 publication.

### Phase 1 — UNDERWAY

Landed:

- StoreScope/StorePos;
- authoritative Agent owner migration;
- Agent rev/lifecycle/access owner;
- Domain principal/groups;
- group-aware seven-permission access;
- access generation/wake/history fences;
- immutable HistorySpan/Base/Grant;
- DisclosureManifest DAG;
- lazy seal;
- exact history forks;
- Agent/settled-Turn selectors;
- model `agent(from?)`.

Still Phase-1/current-boundary work:

- complete external host Agent creation/lifecycle shell over existing owners;
- provenance propagation through new Event/Tool/Process/model-output facts where still needed;
- minimal authoritative snapshots where a real consumer requires them;
- rebuild/normalize Agent lookup/exposure from `agent_state_current` where still pending;
- reachability/flatten acceleration only when proven necessary;
- StoreScope-bound cursor/projection-head helpers only for real consumers.

### Phase 2 — FUTURE

One EventScan + Event projection/read/subscription/Input/Handler completion.

### Phase 3 — FUTURE

DriverKey, SourceDrivers, Sandbox, Checkpoint, Vault, recoverable capture/restore.

### Phase 4 — FUTURE FORMAL CUTOVER

Full public Agent lifecycle/input shell and final Jobs cutover/pagination.

Some narrow Agent adapters already exist to exercise Phase-1 owners; do not equate that with formal
Phase-4 completion.

### Phases 5–6 — FUTURE

Typed leaf registries + dynamic Extension composition, reusing existing Process owner.

### Phase 7 — FUTURE

Retire legacy compatibility paths, final documentation/status reconciliation and grouped cross-repo
qualification.

No eighth phase.

---

## 20. Exact next implementation boundary

### 20.1 Recommended next coherent slice

Implement the **external host Agent lifecycle/creation shell** by composing existing owners.

Do not create another constructor.

The Store/history/access lower owners already exist.

Target public host shape from SPEC §28:

```text
agent.create(...)
agent.list/get
agent.edit(...)
agent.close(...)
agent.admission(...)
agent.access.list/set(...)
```

Likely next cut should focus first on `agent.create` + minimal read/edit/close/admission wiring that
naturally reuses:

- `create_agent_from_history_ref`;
- fresh `resources::create_in_workspace_tx`;
- `agent_access_list/set`;
- `set_agent_title`;
- current lifecycle/close helpers;
- current AgentBinding/canonical ref owner;
- existing replay identity/fingerprint envelope.

### 20.2 Host `agent.create` still-open policy fields

Need reviewed/defaulted host handling for:

- `from? = Agent | settled Turn`;
- `project` default current/source Project;
- title;
- optional initial task;
- `access = private | inherit | custom grants`;
- `sandbox = auto | Sandbox ref`.

Rules:

- history ancestry must not grant Sandbox/source/Vault authority;
- custom audience broadening must pass current disclosure/share closure;
- model cannot choose these trusted destination fields;
- alternate Project redirect requires proper trusted authority;
- Sandbox selection is independent of history ancestry;
- publication must remain one transaction / one existing construction owner.

### 20.3 After host lifecycle shell

Return to the remaining Phase-1 owner work before Phase 2:

- provenance propagation for new committed facts;
- minimal snapshots where required;
- exposure/lookup normalization from Agent owner;
- reachability/flatten acceleration only if a real consumer demands it.

---

## 21. How to work the next implementation turn

At turn start:

1. `git rev-parse HEAD`
2. `git status --short --branch`
3. inspect only relevant owned paths
4. reread the exact SPEC/IMPLEMENTATION section for the slice
5. identify one current owner to reuse
6. implement one coherent cut
7. reuse an existing regression where possible
8. strict targeted Clippy/check/fmt/diff
9. update `CURRENT_HANDOFF.md` + plan `TODO.md` with factual state
10. signed private-index expected-parent commit
11. verify signature
12. pause
13. push only when separately asked

Do not spend the turn adding increasingly granular evidence once the real integration path is green.

---

## 22. Atomic Git checkpoint pattern

Conceptual pattern already used successfully throughout this work:

```sh
EXPECTED=$(git rev-parse HEAD)
INDEX=$(mktemp)
ALLOW=$(mktemp)
trap 'rm -f "$INDEX" "$ALLOW"' EXIT

PATHS=(
  exact/owned/path1
  exact/owned/path2
)

GIT_INDEX_FILE="$INDEX" git read-tree "$EXPECTED"
GIT_INDEX_FILE="$INDEX" git add -- "${PATHS[@]}"
GIT_INDEX_FILE="$INDEX" git diff --cached --check
TREE=$(GIT_INDEX_FILE="$INDEX" git write-tree)

NEW=$(printf '%s\n' 'Terse concrete message' |
  git commit-tree -S "$TREE" -p "$EXPECTED")

printf '%s %s\n' "$(git config user.email)" "$(git config user.signingkey)" > "$ALLOW"
git -c gpg.ssh.allowedSignersFile="$ALLOW" verify-commit "$NEW"

git update-ref refs/heads/main "$NEW" "$EXPECTED"
git reset -q HEAD -- "${PATHS[@]}"
```

Verify the expected signature:

```text
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
```

Never inspect/stage the reserved UX subtree.

---

## 23. Current cross-repo source map

### AW Phase-1 hot files

```text
crates/aw-store/src/agent_owner.rs
crates/aw-store/src/principal_groups.rs
crates/aw-store/src/history_owner.rs
crates/aw-store/src/job_exposure.rs
crates/aw-store/src/agent_job_events.rs
crates/aw-store/src/subagents.rs
crates/aw-store/src/resources.rs
crates/aw-store/src/context/
crates/aw-store/src/lib.rs
src/agent_jobs.rs
src/subagents.rs
```

Migrations:

```text
038_agent_owner.sql
039_domain_principals.sql
040_history_owner.sql
```

### Shared Process hot files

```text
crates/aw-process/
crates/aw-platform-unix/
crates/aw-platform-win/
crates/aw-tools/
crates/aw-tools-adapter/
```

### dotlink shared consumer

```text
src/jobs.rs
src/mcp/tools/shell.rs
src/mcp/tools/graph.rs
src/mcp/tools/jobs.rs
src/remote_start.rs
src/limits.rs
src/transports/
```

Do not touch dotlink just because AW Phase 1 moved forward.

---

## 24. Documentation architecture

### Normative

1. `SPEC.md` — semantic contracts
2. `IMPLEMENTATION.md` — seven phase implementation boundaries/gates
3. `TODO.md` — current implementation queue/status
4. live source/tests — implementation truth

### Current handoff/navigation

- this comprehensive file;
- `CURRENT_HANDOFF.md`;
- `AW-DL200-R6-NEXT-AGENT-2026-11-22.md`.

### Historical design provenance

- R1–R5 archived checklist;
- 2026-11-21 R6 comprehensive handoff;
- 2026-11-20 DL-200 handoff;
- shared AW/dotlink handoffs;
- unified Jobs/Agent 2026-11-14 handoff;
- DL-150 reference.

Do not copy normative detail into a new competing spec.

---

## 25. Full chronology / milestone ledger

### A. AW product/runtime foundations

Earlier AW established:

- one Rust Agent and one user-facing `aw`;
- durable TurnJournal/Session state;
- ToolRegistry/ToolPolicy;
- provider/account/payer/custody separation;
- exact Actor/source/site/effect grants;
- intent before effect;
- Unknown/reconciliation;
- paid-result preservation;
- native Workspace/TUI/browser/headless/SDK projections;
- writer epochs/failover;
- SecretStore/device/fleet/team/context-flow/retry foundations;
- portable/no_std boundaries.

Historical numbered phases stop at 68; do not invent Phase69.

### B. dotlink runtime hardening / DL-150

Built:

- native Process lifecycle;
- descriptor-rooted filesystem;
- Bubblewrap sandbox defaults;
- retained stdout/stderr;
- replay request-key dedupe;
- MCP tasks;
- raw output + ACK;
- canonical Graph/Job runtime;
- shared filesystem/Graph language.

### C. Unified Jobs/Agent

By 2026-11-14:

- AW commit `0a6c6a0...` finalized unified Jobs/Agent runtime;
- dotlink `c90a660...` finalized bridge;
- Jobs/Agent public mental model converged.

### D. DL-200 R1

Public-model simplification sealed.

### E. R2–R5

Security/recovery/migration/Events/Sources/Sandbox/Vault/Extensions/Inputs/Process details hardened.

### F. R6 seal

`7c3800c` seals simplified private architecture.

### G. Shared Process extraction/correction

Important AW commits:

```text
307c7f5 Extract shared Process runtime
320c27d Relax shared Process dependency constraints
22b5562 Expose shared Process semantic stop
46e1125 Bound Process output drain globally
a3ba87c Preempt interactive Process input at deadline
7c16708 Flush captured output at drain bound
59d67ba Qualify bounded Process drain flush
0d2dc10 Record cross-repo Process convergence
20a63a4 Complete DL-200 R6 Process and replay correction
```

### H. dotlink convergence

```text
85ceccc Publish DL-200 R6 consumer and repin AW
```

Exact release reference:

`576d0a1 = v0.17.9`.

### I. Phase 1 implementation sequence

```text
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
```

This is the current implementation line.

---

## 26. Things a new agent should not “simplify away”

These distinctions are deliberate:

- legacy SessionStore still owns Turn/journal/recovery;
- Agent owner table owns Agent envelope/security/lifecycle;
- Job exposure projector may retain legacy namespace derivation during migration/rebuild;
- `legacy_agent_binding_unchecked` is projection/bootstrap machinery, not public authority;
- direct user and group principal existence is not Domain membership;
- group manager is not automatically a group member;
- owner authority is implicit, not an ACL row;
- `access_generation` is not Agent rev;
- HistoryGrant is not DisclosureManifest;
- HistoryBase is not PrefixContentKey;
- History ancestry is not delegation lineage;
- history fork is not same-Session branch fork;
- history fork does not inherit Sandbox/source/Vault;
- wake authority digest can commit to access generation without another durable column;
- external materialized authority currently fails closed on audience expansion;
- dotlink's product timeout/horizon belongs to dotlink, not AW;
- Process Occupancy is not main-command exit;
- Terminal/PTY custody remains separate from ordinary Process Jobs.

---

## 27. Starting prompt for the next agent

The companion copy-ready file is:

`AW/.agents/docs/AW-DL200-R6-NEXT-AGENT-2026-11-22.md`

Equivalent prompt:

```text
Use dotlink---l5 as the only source-authoring connector.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Never inspect/list/stage/reset/stash/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless explicitly promoted.
Do not push/tag/release/deploy unless separately authorized.

Read in this order:
1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. dotlink/AGENTS.md
11. dotlink/.agents/docs/CURRENT_HANDOFF.md
12. live source/tests for every owner touched

Current published refs:
  AW main:        0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e
  AW Phase-0:     20a63a4d4547cc20b2a154787823125b82f5e385
  dotlink main:   85ceccc1088e201c98c994a46a68834440eeab32
  dotlink AW pin: 20a63a4d4547cc20b2a154787823125b82f5e385
  release ref:    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

R6 and the shared Process/release-correction amendment are sealed.
Phase 0 + 0B are complete/published.
Real Bubblewrap worker-retirement and Windows-native qualification are explicitly deferred evidence.
Phase 1 is underway and current source already includes:
- StoreScope/StorePos;
- v38 authoritative Agent owner + ACL cutover;
- v39 Domain principals/groups;
- group-aware seven-permission Agent access + access_generation;
- v40 immutable HistorySpan/Base/Grant + DisclosureManifest DAG;
- exact structurally shared history forks;
- canonical Agent/settled-Turn history selectors;
- model agent(..., from?) with contextual ref canonicalization and replay compatibility.

Last implementation checkpoint:
  0f3f3eb Wire Agent from refs

Do not redo Phase 0 or rebuild the Agent/history/access owners.

Next coherent work:
implement the remaining external host Agent lifecycle/creation shell over the existing owners,
starting with the SPEC §28.3 agent.create policy/defaulting fields:
  from?, project, title?, task?, access=private|inherit|custom, sandbox=auto|ref
and compose existing:
  create_agent_from_history_ref
  resources::create_in_workspace_tx
  agent_access_list/set
  title/lifecycle/close/admission owners
  existing replay identity/fingerprint
Do not create a second constructor/publication path.

Keep implementation turns around 30 minutes and stop at the next coherent green checkpoint.
Prefer simple owner-local Rust primitives, zero-cost/static abstractions and easy hot-path wins.
Avoid granular fixture/evidence overengineering.

Do not create:
- TransactionManager/OperationManager
- global replay ledger
- universal revision clock
- generic Jobs+Events traversal engine
- distributed Event router
- public spawn-pool framework
- second Process/Event/Project/Session/Source owner
- unused registry implementations.

At each checkpoint:
- update CURRENT_HANDOFF + plan TODO factually;
- run focused real integration + strict relevant Clippy/check/fmt/diff;
- make one signed private-index expected-parent commit over exact owned paths;
- verify signature;
- pause;
- push only when separately requested.
```

---

## 28. Definition of a good future handoff

A future handoff is accurate only if it keeps these distinctions explicit:

- sealed design vs implemented source;
- implemented source vs executed validation;
- local commit vs published remote commit;
- AW current main vs AW revision pinned by dotlink;
- source/test green vs unavailable platform evidence;
- Agent owner vs Session backing owner;
- Agent rev vs access/security generation;
- group rev vs Agent rev;
- projection candidate vs authoritative row;
- replay identity vs current disclosure;
- immutable disclosure fact vs current permission;
- HistoryBase vs invocation cache key;
- history ancestry vs delegation/Sandbox ancestry;
- Process exit vs StopRecord vs Occupancy vs output completeness;
- user authorization to implement vs authorization to commit/push/release.

Never compress those into a vague “complete” claim.
