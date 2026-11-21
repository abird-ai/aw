# AW + dotlink + DL-200 R6 full comprehensive handoff — 2026-11-21

Prepared: 2026-11-21, after the local shared-Process consumer cut + Phase-0B closure
Owner: @pvl
Machine/source authority: **L5 only**

AW repo: `/home/pvl/spaces/abird/src/aw`
dotlink repo: `/home/pvl/spaces/abird/src/dotlink`
Read-only release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

---

## 0. Purpose, authority and exact status

This is the **canonical comprehensive DL-200 handoff** for the next agent. It exists so a new
agent can recover the complete design lineage, current implementation reality, validation evidence,
remaining gates and operating constraints without reconstructing the project from chat history.

It is a **navigation/context handoff**, not a second normative specification. The sealed normative
contracts remain in the DL-200 README/SPEC/IMPLEMENTATION/TODO listed below. Live source, tests and
executed results outrank prose if anything conflicts.

### Current status in one paragraph

DL-200 **R6 is sealed and authoritative**, including the later shared Process/release-correction
amendment imported from dotlink v0.17.7–v0.17.9. The **AW corrective Process source is implemented
locally and uncommitted**; the **dotlink v0.17.9 product-policy and async controlled-Process consumer
source are also implemented locally and uncommitted**; **Phase 0B is locally complete**. Dotlink's
manifest intentionally still pins the older published AW runtime `7c16708...`, so the current
combined dotlink source **does not compile under its pinned Git dependency** until AW is published
and dotlink is repinned. The consumer source was already validated successfully through temporary
local-AW dependency substitution, and Cargo.toml/Cargo.lock were restored afterward. Remaining
pre-Phase-1 work is therefore: explicit publication authorization -> publish AW -> repin dotlink ->
re-diff release/0.17 from exact v0.17.9 -> rerun the already-green consumer gate under the real Git
pin -> real Bubblewrap worker-retirement qualification on a host whose outer sandbox permits bwrap
mounts -> Windows-native qualification -> review -> Phase 1.

No commit, push, tag, release or deployment is authorized merely by this handoff.

### Design status

- R6 is sealed and authoritative as of 2026-11-21.
- R1–R5 remain historical design provenance, not competing normative revisions.
- R6 keeps the public resource model/API/seven implementation phases unchanged and simplifies
  private mechanics/ownership/documentation.
- The v0.17.7–v0.17.9 Process/release-correction amendment is part of sealed R6.
- Broad architecture redesign should stop unless implementation evidence proves a real blocker.
- Phase 0B replay/disclosure/provider/tombstone/lifetime work is locally complete.
- Phase 1 has **not** started.

## 1. Mandatory read order / authority order

Read in this order before changing source:

1. `AW/AGENTS.md`
2. **this handoff**
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

For forensic chronology, not current authority:

- `AW/.agents/docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
- `AW/.agents/docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md`
- `AW/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md`
- older AW/dotlink 0.17 handoffs under `.agents/docs/`
- immutable DL-150 reference under `.agents/plans/dotlink-tool-port/reference/`

Do not use an old handoff to override live source or the sealed R6 SPEC.

## 2. Non-negotiable operating constraints

- Use **dotlink---l5 only**.
- Do not use A1.
- Do not create another worktree.
- Preserve unrelated/concurrently owned dirty work.
- **Never inspect, list, stage, reset, stash or modify `AW/.agents/plans/ux/**`.**
- Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless explicitly
  promoted by the user.
- `/home/pvl/spaces/abird/src/dotlink-017-mounts` is read-only reference.
- Do not reopen published 0.17 D0–D7/shared-runtime/process-control/issuer work without a concrete
  regression.
- Keep implementation turns around 30 minutes and stop at the next coherent checkpoint.
- Prefer simple, elegant, composable owner-local primitives. Use zero-cost/static Rust abstractions
  and easy hot-path wins where they do not create maintenance burden.
- Do not over-engineer tests/evidence. Prefer grouped owner/failure-boundary gates over fixture
  proliferation or evidence pinning.
- Do not commit/push/tag/release/deploy unless explicitly asked.

## 3. Exact current repository state

### 3.1 AW

Current working-tree HEAD:

```text
0d2dc10e20b62ebf0f064cdbc43615ae2d728ad7
```

Current published reusable runtime still consumed by dotlink:

```text
7c1670821b15082821a0b9621cd1caccbe62f144
```

Published historical AW checkpoints that remain useful provenance:

- `7c3800c` — seal DL-200 R6 architecture
- `7c16708...` — published shared Process runtime consumed by current dotlink manifest
- `59d67ba...` — later published main with focused drain regression only
- `132bfd7...` — R1 DL-200 baseline
- `f035a00...` — earlier implementation/design base

Current AW status:

```text
## main...origin/main
 M .agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md
 M .agents/docs/AW-DL200-R6-NEXT-AGENT-2026-11-21.md
 M .agents/docs/CURRENT_HANDOFF.md
 M .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
 M .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
 M .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
 M .agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
 M Cargo.lock
 M crates/aw-agent-core/src/accounts.rs
 M crates/aw-platform-unix/src/lib.rs
 M crates/aw-platform-unix/src/pid_identity.rs
 M crates/aw-platform-win/src/windows.rs
 M crates/aw-policy/src/lib.rs
 M crates/aw-process/Cargo.toml
 M crates/aw-process/src/lib.rs
 M crates/aw-store/src/policy_admission.rs
 M crates/aw-store/src/remote_start.rs
 M crates/aw-store/src/subagents.rs
 M crates/aw-store/tests/store.rs
 M crates/aw-tools-adapter/src/job_host.rs
 M crates/aw-tools-adapter/src/process.rs
 M crates/aw-tools-adapter/src/shell.rs
 M crates/aw-tools/src/filesystem/runner.rs
 M src/actor_owner.rs
 M src/child_actors.rs
 M src/main_full.rs
 M src/policy_runtime.rs
 M src/subagents.rs
?? crates/aw-process/src/linux_spawn.rs
```

Important: the dirty AW tree is authoritative for the new corrective source. It includes the
sealed Spawn Facility/release corrections, provider/payer work, replay disclosure, policy
admission tombstones and related tests/docs. Preserve all of it.

### 3.2 dotlink

Current working-tree HEAD:

```text
fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94
```

Current dotlink status:

```text
## main...origin/main
 M .agents/docs/CURRENT_HANDOFF.md
 M src/jobs.rs
 M src/limits.rs
 M src/main.rs
 M src/mcp.rs
 M src/mcp/tools/graph.rs
 M src/mcp/tools/jobs.rs
 M src/mcp/tools/mod.rs
 M src/mcp/tools/shared_native.rs
 M src/mcp/tools/shell.rs
 M src/mcp/tools/test_support.rs
 M src/remote_start.rs
 M src/setup.rs
 M src/transports/mod.rs
 M src/transports/openai.rs
```

Current manifest/lock still intentionally pins AW:

```text
7c1670821b15082821a0b9621cd1caccbe62f144
```

Current dotlink source is **ahead of that pin**: it contains the v0.17.9 product policy and the new
async `aw_process::Process::spawn` + `SpawnControl` consumer source. Therefore an ordinary pinned
`cargo check --locked --tests` currently fails with the expected old-API errors (`SpawnControl`
missing, `SpawnOptions.control` missing, sync Process::spawn not awaitable, pre-spawn error variants
missing). Do **not** "fix" source back to the old API. Publish/repin AW first when explicitly
authorized.

The consumer source has already been validated successfully via temporary local-path substitution
to the dirty AW checkout. Cargo.toml/Cargo.lock were restored byte-for-byte afterward.

### 3.3 read-only release/0.17 reference

Current exact reference:

```text
576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2
v0.17.9
```

Current release worktree state:

```text
## release/0.17...origin/release/0.17
```

The exact audited release is **v0.17.9 / `576d0a1`**. Its `81ef891..v0.17.9` diff SHA-256 is
`1aa632e35d9c...`, matching the candidate audited before the tag was cut. Re-diff any later
release/0.17 movement from this exact tag before the final dotlink repin/consumer qualification.

---

## 4. Historical design lineage

### 4.1 DL-150 and shared Tool/Jobs foundations

DL-150 established the lower filesystem/Graph/result/paging/mutation/replay/Process contracts
that DL-200 inherits unless explicitly superseded.

Important inherited principles:

- one shared host-neutral Tool/Job semantics layer;
- finite Jobs/Graph identity;
- exact replay and current-authority separation;
- output/receipt budgets and raw-page vs serialized-result separation;
- ACK safety and bounded paging;
- physical Process exit, semantic stop and output coverage are different facts;
- no blind retry of Unknown external effects;
- exact authority and revocation at the effect boundary.

The immutable historical reference remains under:

    .agents/plans/dotlink-tool-port/reference/

### 4.2 November 9–14 unified Jobs + Agent runtime

The unified runtime work established the model that DL-200 later simplifies rather than
discarding:

- Jobs as the consolidated work/control plane;
- Agent/Process/Graph as the meaningful Job kinds;
- Agent Turn/history ownership distinct from execution;
- Agent-mediated subagents allowed under bounded delegation;
- nested Graph/batch prohibited while active Graph ownership is retained;
- current policy/grant checks at new-effect boundaries;
- shared lower execution/runtime primitives and a hard pre-1.0 cutover philosophy.

See:

    .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md

### 4.3 November 15–20 shared AW/dotlink and 0.17 convergence

The D0–D7/0.17 sequence converged AW and dotlink on:

- shared Process/control semantics;
- typed refs and shared receipts;
- replay-before-projection and replay-before-current-authority for exact committed effects;
- Jobs read/get/ACK/output fitting;
- deterministic progress/heartbeat/result projection;
- Remote Start trusted issuer and replay semantics;
- participant-aware policy routing;
- scalable Agent source/event ownership and wake admission;
- process containment/cleanup, physical exit and output-drain distinctions.

Do not reopen this published work merely because R6 improves the implementation boundary.
R6's goal is to consume it once, not recreate two implementations.

### 4.4 DL-200 R1 — public-model simplification

R1 established the core public north star:

- one canonical public hierarchy;
- Agent, not Session, is the public collaboration/security/history/execution envelope;
- no public Session coordinate/API;
- model Jobs exactly six methods;
- host-staged Agent input plan;
- immutable history/forks and checkpoint semantics;
- explicit principals/groups/ACL;
- Events and Signals collapsed into one Event concept;
- NATS-style Event subject projection/wildcards;
- one host/model Agent constructor shape;
- Checkpoint restore creates a new Agent/Sandbox rather than mutating existing state.

Published R1 plan baseline:

    132bfd7416a0a83e915ab18ffc10de9f6d3e2a2b

### 4.5 R2–R5 — correctness hardening

The repeated external reviews were deliberately absorbed without proliferating public
resources. The complete historical design-resolution checklist is:

    .agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md

That checklist is design history, not implementation evidence.

Major R2–R5 hardening themes included:

- Process becomes one supervised execution scope;
- Sandbox and Process Scope are orthogonal;
- strong Linux target is a tiny PID-1 supervisor, Bash/main command PID 2;
- immutable pre-cleanup Process occupancy semantics;
- fail-closed strong-containment requirement;
- authoritative agent_state_current with exclusive Agent+ACL activation;
- one transaction commit as constructor publication point;
- Prepared-before-external-effect construction and replay recovery;
- compact replay tombstones and disclosure-safe replay responses;
- authority-scoped Event-store affinity and gap-free Event projection;
- EventInput admission as one authorization/dedupe/causal transaction;
- exact subscription re-enable/backlog semantics;
- source-owner delivery_cutover_rev for Event migration;
- scalable DisclosureManifest DAG and live authority-root rechecks;
- stable store-local Jobs pagination with ancestor-gate reauthorization;
- recoverable SourceDrivers with schema pinning and store-open compatibility validation;
- non-secret bounded RecoveryDescriptors and bounded SourceSets;
- typed registry leaf staging from first use;
- no nested batch under active Graph ownership;
- same-store Event→Agent Input delivery in v1;
- cross-store ordered/federated delivery explicitly deferred.

### 4.6 R6 — simplification candidate, not a new public architecture

R6 reconciles the final simplification reviews.

R6 does not change the six public concepts or seven implementation phases.

Public concepts remain:

- Agent
- Job
- Sandbox
- Event
- Checkpoint
- Extension

R6 simplifies implementation around one architectural law:

> One authoritative source for each semantic truth.
> Mutable truth has one Owner and one linearization point.
> Immutable truth has one stable Fact identity.
> Uncertain external work has one Operation custody.
> Derived state is a Projection/Cursor/adapter, never competing truth.

Global canonical refs are identity only. Transactional consistency, total ordering,
projection completeness and cursor high-water semantics belong to one authoritative Store.

## 5. Current public north star

### 5.1 canonical namespace

    /d:<domain>/i:<principal>/p:<project>
      /a:<agent>/t:<turn>/j:<job>/n:<node>

Sandbox is a sibling logical resource under the Project namespace, not an ExecutionRef.

Principal namespace i: contains users and groups.

Canonical names do not grant authority or global transaction/order semantics.

### 5.2 model-facing Jobs

Exactly:

- jobs_list
- jobs_get
- jobs_read
- jobs_send
- jobs_ack
- jobs_cancel

jobs_send is only:

- substantive directed Agent input; or
- Process stdin / EOF.

No public queue CRUD, pause-after-turn, steer modes, wake modes or Jobs-specific subscribe.

### 5.3 host Agent lifecycle/input

Host Agent construction:

    agent.create(from=...)

Model constructor:

    agent(task, from?)

Fresh and exact-history construction use the same lower constructor.

Host staged input API remains:

- agent.inputs.list
- agent.inputs.add
- agent.inputs.edit
- agent.inputs.delete
- agent.inputs.send

Actual Send crosses Agent Input admission.

### 5.4 Agent vs Session

Agent is public identity/security/history/collaboration/execution envelope.

Legacy SessionStore/session_id may remain internal Turn/history persistence/recovery machinery.

After migration:

- agent_state_current is authoritative durable Agent-envelope Owner state;
- SessionStore remains the Turn/history owner;
- lookup/list/exposure indexes are rebuildable projections;
- Agent rev/lifecycle/lineage/ownership are never re-derived from legacy audit.

### 5.5 Events

Events and Signals are one Event concept.

Canonical Event subjects project canonical resource tokens into dot-separated NATS-like
subjects:

    d:<domain>.i:<principal>.p:<project>.a:<agent>
      [.t:<turn>][.j:<job>][.n:<node>].e:<event>

Wildcard rules:

- * = exactly one token
- > = terminal one-or-more suffix

Custom Agent events use #name, for example:

    e:#ready

System/reserved event names do not use #.

Patterns grant no authority. Every concrete Event is authorized.

Host/API concepts:

- events.read
- events.subscribe
- events.publish
- bounded subscription listing

Model:

- events_subscribe
- events_publish

No Signals API, topics/channels or second Event bus.

### 5.6 checkpoints, history and forks

Checkpoint is:

- immutable HistoryBase/head;
- plus optional exact immutable captured named SourceSet.

Git dirty checkpoints may use hidden AW-pinned synthetic Git objects.

checkpoint.restore:

- never mutates an existing Agent/Sandbox;
- creates a new Agent and Sandbox;
- restores exact history + exact captured SourceSet;
- uses the same lower replay-safe prepare/publish/reconcile constructor as agent.create.

Exact forks structurally share immutable history/provenance.

Settled Turn boundaries are reusable fork/cache seed points.

History ancestry, delegation lineage and Sandbox derivation are different relations.

### 5.7 access/groups

Agent ACL is allow-only over direct user/group principals.

Groups are first-class explicit principals.

No implicit group nesting in v1.

Owner-local rev, ACL/security generation and store publication positions remain distinct.

### 5.8 Sandbox, Scope and Vault

Sandbox is a durable logical mutable execution environment.

Process Scope is one execution lifetime.

They are orthogonal:

    Sandbox = where/how execution may run
    Scope   = what belongs to this Process lifetime

Unsandboxed execution remains first-class.

A Sandbox/launcher may provide stronger containment, but does not own Process lifetime.

Vault capabilities are explicit grants and are never checkpoint contents/Sandbox files.

Secrets remain provider/Vault-owned; checkpoints and RecoveryDescriptors contain no live
credentials, bearer tokens or raw secrets.

### 5.9 Extensions

ExtensionRegistry is a typed composition/publishing root.

Typed registries include Tools, Hooks, Views, EventHandlers, State, Actions, Resources,
Settings, drivers, providers, etc., but concrete families are implemented only when a real
consumer exists.

Installed definitions do not grant runtime authority.

Dynamic V1 extension host is Process-based; WASM is deferred until it gives real value.

The design intentionally supports product extensions such as Histate without making Histate
state part of AW's core resource model.

## 6. R6 private architecture

### 6.1 six internal roles — vocabulary, not a framework

Owner:
- sole mutable authority + commit boundary;
- examples: agent_state_current, Agent Input owner.

Fact:
- immutable authoritative evidence;
- examples: HistorySpan, commit-log Event, ProcessTerminal.

Operation:
- replay/recovery custody for uncertain effects;
- examples: Prepared/Unknown construction or remote Start receipt.

Projection:
- rebuildable accelerator/index;
- examples: Event projection index, Job exposure projection.

Cursor:
- private bounded observation/progress;
- examples: jobs_list page state, Event reader/consumer state.

Scope:
- one Process lifetime;
- private to the Process runtime.

Do not turn these into six services/traits/base classes.

### 6.2 one StoreId + one StorePos for store-local publication order

Reuse existing persistent store_meta.store_id.

Use one internal StorePos newtype backed by existing commit_log.seq.

StorePos is comparable only after the same StoreId is established.

StorePos may back:

- Event indexed-through position;
- Jobs publication/high-water position;
- projection build-through/head evidence.

Do not merge StorePos with:

- Agent/Process/Graph rev;
- source-local event_rev;
- security/group generations;
- inputs_rev;
- output byte offsets.

### 6.3 one owner-mutation discipline, no TransactionManager

Shared internal discipline:

    replay lookup
      -> recover existing outcome if present
      -> authorize genuinely new work
      -> acquire required external evidence/fences
      -> Prepared before an escapable/uncertain external effect
      -> revalidate/publish in the real Owner transaction
      -> record terminal Operation outcome
      -> notify only after commit

Store-only deterministic mutations do not need Prepared merely for uniformity.

Existing domain owners keep their own states/tables/transactions.

### 6.4 ExternalEvidence

One rule for externally mutable authority/effect evidence:

Store-local authority:
- check at the authoritative transaction/linearization point.

External authority:
- immutable/non-revocable proof; or
- provider-enforced held fence valid through linearization; otherwise
- Unknown/unavailable/fail closed.

Durable evidence stores only non-secret proofs/opaque handles. Live credentials remain
provider/Vault-owned.

This rule is shared by constructors/SourceDrivers, Event authorization and later
provider-backed paths.

### 6.5 one projection publication protocol

Event projection and Jobs exposure projection use the same private rebuild mechanics:

- build private generation;
- maintain contiguous complete safe prefix;
- record built_through StorePos;
- at final fence verify/catch up the current authoritative head;
- atomically activate replacement generation;
- never expose a cursor high-water across a projection hole.

Index/query schemas remain owner-specific.

### 6.6 cursor envelope, not one generic traversal engine

Share:

- opaque authenticated cursor envelope;
- caller/selector/Store affinity;
- generation/schema validation;
- StorePos/high-water validation;
- integrity/expiry/staleness helpers.

Do not unify actual scanning algorithms:

- Event scanning is ordered immutable-history consumption;
- Jobs scope=tree traversal is hierarchical high-water traversal with ancestor-gate
  reauthorization.

### 6.7 EventScan

One private EventScan engine is used by:

- events.read;
- Agent Event subscriptions;
- EventHandlers.

It owns:

- Event projection scan;
- StoreId/StorePos/indexed-through checks;
- authority-scoped placement affinity;
- source-first visibility gate;
- allow / proven-deny / blocked-unknown;
- DisclosureManifest dependency lookup;
- cursor advancement eligibility.

It does not own:

- Agent Input admission;
- causal-budget mutation;
- Handler effects/receipts.

Final owners reauthorize/commit their own effects.

### 6.8 DriverKey + per-artifact schema

One stable DriverKey identifies a built-in/Extension driver namespace + local driver ID only.

Persisted artifacts carry their own adjacent schema versions:

- SourceBinding schema;
- SourceAnchor schema;
- RecoveryDescriptor schema.

Runtime installation generation is a live fence, not durable historical identity.

Store-open resolves DriverKey + artifact schema through the typed registry.

Live bindings/anchors/unresolved descriptors pin compatible driver/schema support.

### 6.9 one typed registry core

Share mechanics:

- staged register/remove;
- collision validation;
- typed storage/lookup;
- local revision;
- atomic snapshot/root publication;
- typed handles;
- Extension ownership.

Keep family-specific validation/authority specialized.

Do not implement dormant registry families only because they appear in the spec.

### 6.10 one shared Process runtime

Target dependency boundary:

    AW host / dotlink product
            |
            v
    shared Agent-neutral Process owner
      spawn + pipes + Scope + wait/stop + drain + settlement
            |
            v
    aw-platform
      synchronous target-specific OS facts

Adapters own:

- policy/authorization;
- output persistence/spools/product transport;
- MCP/result/log projection;
- runtime deadlines/profile policy.

The dependency boundary is now explicit: **`crates/aw-process` is required in Phase 0**.
It is the one Agent-neutral async Process/Job lifetime implementation shared by AW and
dotlink; it is not a wrapper around two pre-existing runtimes.

Hosts prepare authorized executable/Sandbox/environment choices and pass the prepared command
by value. `aw-process` owns containment finalization, stdio, spawn, exact child custody,
wait/stop/reap, pipe drain and `ProcessTerminal`. Product output storage/retention, policy,
MCP/result/log projection and runtime-profile deadlines remain at the edges. AW's
`ProcessReceipt` and dotlink's JobCompletion remain product envelopes around the shared Fact;
do not create a generic shared receipt.

`aw-terminal` is intentionally outside this ownership cut because Terminal/PTY source
release/adoption may transfer custody. That reviewed Terminal resource is not a second
DL-200 Process Job runtime and must not force release/disarm into ordinary Process Scope.

### 6.11 Tree -> Occupancy

R6 proposes the source rename:

    Tree -> Occupancy
    ProcessTerminal.tree -> ProcessTerminal.occupancy

Values remain:

- Empty
- Live
- Unknown

Meaning:

- immutable pre-cleanup Process-scope occupancy;
- cleanup never rewrites it;
- Empty means the full semantic Process scope is proven empty;
- weak direct-Unix negative group evidence is Unknown when escaped descendants cannot be
  excluded.

Dotlink retains its product/wire compatibility field:

- Occupancy::Live -> tree="leftovers"
- Occupancy::Unknown -> tree="unknown"
- Occupancy::Empty -> omit tree

This rename is not yet implemented.

## 7. Critical R5 contracts R6 must preserve

R6 simplifies mechanics only. It must preserve the hardened R5 semantics.

### 7.1 authoritative Agent migration

One exclusive Agent+ACL store migration:

- no concurrent legacy Agent-envelope writes;
- complete seed under write/schema exclusion;
- activate new owner/schema epoch atomically;
- no v1 online dual-write/catch-up;
- rollback leaves legacy owner active.

### 7.2 construction / checkpoints / SourceDrivers

Before first external effect:

- claim existing replay/operation owner as Prepared;
- include fingerprint/candidate IDs/recovery descriptors;
- capture and restore use the same prepared-driver operation invariant.

Final publication:

- revalidate all AW-store-owned source/destination/grant/ACL/lifecycle authority inside
  the final transaction;
- keep AW-owned source fences through commit;
- external evidence is immutable/non-revocable or provider-held through the whole commit;
- transition Prepared -> Applied in the same public commit;
- revoke/replace-first denies; publish-first remains factual.

Source limits:

- default max_sources_per_sandbox = 128;
- hard absolute ceiling = 1024;
- hosts may configure lower only;
- reject over-limit work before external effects.

RecoveryDescriptors:

- stable operation-owned identity before effect;
- exact DriverKey + recovery schema;
- non-secret opaque IDs/digests/proofs/handles only;
- no bearer tokens, credential URLs or serialized credentials;
- unresolved/live dependencies are validated at store-open;
- incompatible/missing driver puts affected resources into recovery-required fail-closed
  state.

### 7.3 replay

Same replay identity never repeats an effect.

Effect/outcome identity is stable; response payload bytes need not live forever.

Full receipts may compact in the same existing owner to minimal never-retry tombstones.

Constructors retain created canonical identities for replay lifetime.

Withheld replay returns only the minimal/opaque non-retryable acknowledgement permitted by
current disclosure.

TTL alone never makes a consumed durable identity executable again.

### 7.4 DisclosureManifest

Persistent content-addressed bounded-fanout immutable set/DAG.

Keep exact provenance facts distinct from deduplicated authority roots.

AuthorityRoot:

- immutable provenance/maximum-ceiling identity;
- identifies which live ACL/policy/group/provider owners to recheck;
- never caches current permission except an explicitly immutable export grant contract.

Structural sharing bounds rewrite/union work; per-request work is bounded.

### 7.5 Event store and authorization

V1 ordered Event consumers are store-local.

Event cursors/subscriptions/Handlers bind:

- StoreId;
- authority-scoped placement generation;
- StorePos/progress.

Hidden Domains must not perturb caller-visible staleness.

Projection consumers never advance beyond the contiguous indexed-through prefix.

Authorization order:

1. exact source visibility first;
2. proven hidden/nonmatch skips privately and does not evaluate deeper provider/manifest
   dependencies;
3. only for an already-visible source evaluate current audience + DisclosureManifest;
4. external mutable dependencies require immutable proof or provider-held fence through
   admission/read authorization;
5. unresolved visible-source authority returns candidate-oblivious
   authorization_unavailable and does not advance.

Privacy guarantee is direct, not constant-time:
- no direct hidden Event identity/subject/exact count/private position disclosure;
- coarse aggregate timing/traffic inference from bounded scans/waits is not promised away.

### 7.6 Event delivery / migration

Agent subscription delivery in v1 requires:

- Event/subscription/causal owner;
- recipient Agent Input owner;

to participate in the same authoritative transaction/store.

Cross-store Agent delivery is explicitly unsupported in v1.

EventInput admission transaction atomically rechecks:

- active generation/store affinity;
- current recipient/source/disclosure authority;
- deterministic delivery dedupe;
- causal budget for newly accepted input.

Legacy migration uses one source-owner cutover:

    delivery_cutover_rev = current next_event_rev

inside the serialized cutover transaction.

Whole event_rev belongs to one delivery path.

Already accepted legacy Inputs:

- keep original identity;
- are not re-admitted;
- gain immutable compatibility disclosure/provenance sidecar;
- incomplete reconstruction uses conservative legacy_protected so future audience expansion
  fails closed.

Only genuinely pending pre-cutover deliveries bridge once deterministically.

### 7.7 Events.subscribe enable semantics

Active same subject + enabled=true:
- no-op;
- preserve generation, baseline and backlog.

Absent/disabled + enabled=true:
- new generation/baseline.

Lagged + enabled=true:
- explicit rebaseline with new generation.

Disable:
- fence later admissions.

### 7.8 Jobs pagination/traversal

One jobs_list traversal is exposure-store local in v1.

Ordering:

    publication_position DESC, canonical_ref ASC

Cursor stores both tuple fields.

Page one captures publication high-water.

Later pages only consider rows at/below that high-water.

New work appears on a fresh listing.

scope=tree cursor frontier is navigation only.

On resume:
- reauthorize every Agent frontier/ancestor gate;
- hidden ancestor discards its whole queued subtree;
- direct jobs_get(descendant) may still independently succeed.

Exposure rebuild:
- private replacement;
- final write/swap fence verifies current head == built head or catches up intervening range;
- stale replacement never overwrites newer rows;
- no high-water is issued over a projection hole.

Cross-store recursive Jobs federation is explicitly unsupported in v1.

### 7.9 Graph ownership

Nested Agents are allowed within existing causal/depth/cost/authority limits.

Nested batch/Graph construction is transitively prohibited while an Agent or descendant is
owned by an active Graph node.

After ownership releases, a genuinely independently admitted later Turn regains ordinary
top-level batch eligibility.

### 7.10 strong Linux supervisor

Future strong backend:

- per-command PID namespace;
- tiny AW supervisor PID 1;
- main command/Bash PID 2;
- user command is never PID 1.

Private report channel:

- unidirectional supervisor -> host;
- command/descendants inherit no writable endpoint;
- bounded/versioned framing;
- expected launch/supervisor identity validation;
- exactly one terminal report;
- malformed/duplicate/unexpected report is failure;
- report loss before main-command exit -> ProcessExit::Unknown;
- namespace cleanup never fabricates command exit or rewrites Occupancy.

## 8. Product-extension / Histate seam

DL-200 intentionally leaves room for Histate-like products without turning product metadata
into AW core resources.

Histate is conceptually GitHub for AI work:
- fork sessions/decision trees, not just files;
- attach commits, summaries, decisions, product graphs and team/project metadata.

AW supplies:
- Agent/history/fork identities;
- Event stream;
- Checkpoints/Sources;
- typed registry/Extension seams;
- State/View/Resource/EventHandler-style definition points;
- immutable provenance/disclosure contracts.

Histate/product state remains typed extension/product ownership rather than becoming
Agent/Sandbox/Event core columns.

This is why Registry definitions, runtime state and core AW authority remain separate.

## 9. R6 proposals deliberately rejected or modified

Do not accidentally reintroduce these.

- Do not make `aw-process` a generic execution framework. The crate **is** required for
  DL-200 Process/Job execution, but it owns only lifecycle-sensitive command/pipe settlement;
  host policy/storage and transferable Terminal/PTY custody stay outside it.
- Do not create a universal TransactionManager/OperationManager/workflow engine.
- Do not create one generic Jobs+Events cursor/traversal engine.
- Do not merge all Unknown states into one enum.
- Do not merge Agent/Sandbox, Sandbox/Scope, Jobs/Events, HistoryGrant/DisclosureManifest,
  or rev/event_rev/StorePos/security generations.
- Do not implement all registry families upfront.
- Do not add cross-store Event delivery/federated Jobs traversal in v1.
- Do not promote frozen 017 sandbox/mount/config-v13 work merely because Process/Sandbox
  composition references a future stronger launcher.


## 10. Current implementation reality — source, not aspiration

### 10.1 Published shared baseline before the corrective amendment

The published R6 shared baseline at AW `7c16708...` and dotlink source `16475e4` established:

- `ProcessTerminal = exit + Option<StopRecord> + Occupancy + per-stream output facts`;
- `crates/aw-process` as the sole Agent-neutral async Process/Child/stdin/pipe/wait/stop/reap/drain
  owner;
- AW adapter retains policy/storage/receipt ownership only;
- `aw-terminal` remains separate PTY custody;
- dotlink deletes its parallel `shell_process` lifetime owner and consumes `aw-process`;
- compatibility `tree=leftovers|unknown` remains a dotlink wire projection over Occupancy;
- weak direct-Unix negative membership remains Unknown rather than overclaiming Empty;
- bounded drain and interactive-stdin cancellation/deadline behavior are shared.

Historical v0.17.6 executable evidence remains useful for unchanged semantics, but later SIGKILL
interruptions are no longer confidently attributed to an external host because v0.17.7 identified
a concrete parent-thread lifetime bug.

### 10.2 Sealed shared Process/release-correction amendment — implemented locally in AW

#### Origin: v0.17.7 parent-thread SIGKILL fix

Bubblewrap `--die-with-parent` uses Linux `PR_SET_PDEATHSIG`, whose parent relationship is tied to
the **forking thread**. Spawning from a retireable Tokio worker can therefore SIGKILL an otherwise
healthy sandbox when that worker exits while the host process remains alive.

R6 sealed the fix at the shared AW Process boundary, not by restoring dotlink's old local runtime.
The implemented local AW shape is:

- one private logical Linux Spawn Facility inside `aw-process`;
- stable process-lifetime Spawn Lane threads are O(1), **N=1 initially**;
- Processes/Jobs remain O(n) lightweight `Send` futures free to migrate across Tokio workers;
- only bounded async admission + `Command::spawn` + exact `DirectScope` attachment run on the lane;
- private RAII `Spawned { DirectScope, Child, pid }` transfers complete custody;
- vanished receiver after spawn cleans the exact scope/child; no orphan/retry;
- lanes never retire/shrink while host lives;
- request panic poisons new launch admission but keeps the lifetime-anchor thread alive;
- `spawn_blocking`/`block_in_place` are explicitly not lifetime anchors;
- N>1 is benchmark-driven internal tuning only, not public config.

#### v0.17.9 stale launch + one execution deadline

The runner establishes one **absolute execution deadline** at the canonical Running/Start boundary.
Pre-Running queue/preparation/output admission age does not consume execution budget; Spawn-Lane
queueing and Process execution do.

Lane-side cancellation/expiry is rechecked immediately before fork through a private linearizable
launch state:

- stale before `Launching` => no child; no ProcessTerminal; no StopRecord; no fake kill evidence;
- `Launching` wins => any real child follows normal RAII/Process settlement;
- Process wait reuses the same absolute deadline rather than starting a fresh timeout.

#### strict Linux child-terminal proof

Owned main-child cleanup eligibility uses strict parent-owned non-reaping `waitid(... WNOWAIT)`.
Pidfd readiness/SIGCHLD is only a wake hint. The owned main-child path does not downgrade ECHILD to
pidfd readability to authorize a group sweep.

#### stop/exit causality

Stop operation evidence and physical Process exit remain independent facts until reconciled. A
physical status incompatible with the delivered termination wins; cleanup of leftovers never
rewrites the main ending. Product `stopped_by`/stop-reason wording derives from the reconciled
terminal fact, not StopRecord alone.

#### Windows corrections

Local AW source also carries:

- `CREATE_SUSPENDED | CREATE_NO_WINDOW` for non-PTY managed launch;
- private termination marker shared by Job termination and leader fallback;
- bounded post-exit Job accounting settle so transient console infrastructure does not become false
  Occupancy::Live;
- settle is cancellation/deadline-preemptible: Empty already proven => Empty; query failure or
  interrupted nonzero => Unknown; nonzero for the full uninterrupted bound => Live;
- no process-name/conhost exemptions;
- no blocking Tokio-worker sleep.

Windows-native/cross execution is **not** qualified on L5 because only the Linux Rust target is
installed.

### 10.3 dotlink v0.17.9 product policy — implemented locally

Dotlink owns turn/product policy, not AW:

- one small `RuntimeSurfacePolicy` derived from active transport constraints;
- bounded surface: 35-minute turn horizon;
- 10-minute report/hand-back reserve;
- therefore 25-minute / 1500-second maximum new command ceiling, further narrowed by configured
  shell policy;
- local-only/unbounded surfaces keep configured Process limit and no turn advice;
- effective command ceiling participates in **fresh-effect** authority;
- current advice capability is rebound per machine construction and never rewrites retained Job
  timeout truth;
- AW receipt profile supplies factual `ts` only; dotlink sets `checkpoint_minutes=None` so generic
  AW checkpoint prose cannot conflict with the exact 25/35/10 policy;
- `created` remains Job-age/`elapsed_ms` only;
- absolute `execution_deadline` is established at canonical Running/Start;
- queued/preparation/output-admission age does not consume execution timeout;
- `jobs_get.next` counts down from `execution_deadline - now`, never `timeout_secs - elapsed_ms`;
- advice appears only on the hand-back-capable surface for Running/non-cancelling Jobs inside
  `min(timeout/5, 5m)`;
- list/read/ack/cancel and terminal/unknown/cancelling projections stay quiet;
- no generic renderer plugin, AW turn clock, rendered-text parsing or duplicate lifecycle runtime.

### 10.4 dotlink shared-Process consumer cut — implemented locally, pin still old

The sole dotlink shared Process launch now:

- awaits the async `aw_process::Process::spawn`;
- passes the exact canonical Running deadline through `SpawnControl`;
- passes a cloned level-triggered Job cancellation probe;
- maps `CancelledBeforeSpawn` to cancelled Job semantics with no ProcessTerminal;
- maps `TimedOutBeforeSpawn` to timed-out Job semantics with no ProcessTerminal;
- retains definite spawn refusal as factual no-Process Failed;
- retains setup uncertainty as Unknown;
- generalizes the old `spawn_failed` boolean into factual `process_not_started`, so Graph/shared
  settlement cannot fabricate terminal Process facts for a Process that never existed.

The consumer regression uses the **real global Spawn Lane**, not a mock: one launch holds the lane
briefly, a second Job reaches Running and queues behind it, cancellation wins before fork, the
command marker is never created, status becomes Cancelled, caller attribution survives and no
ProcessTerminal appears. The artificial lane hold was reduced to ~250 ms after the first ~1.4 s
version interfered with unrelated parallel timing tests; the original <900 ms assertions then pass
unchanged.

Validation used temporary local-path dependency substitution to the dirty AW tree and restored
Cargo.toml/Cargo.lock exactly afterward. The current manifest still pins `7c16708`, so the ordinary
pinned build correctly fails until publication/repin.

### 10.5 real Bubblewrap and Windows gates

A real Bubblewrap smoke was explicitly enabled. It reached installed `bwrap` but L5's outer tool
sandbox rejects:

```text
bwrap: Can't mount proc on /proc: Operation not permitted
```

Therefore real Bubblewrap worker-retirement qualification is **unavailable on this host**, not
passed and not a product failure. AW's generic PDEATHSIG parent-thread lifetime regression is green.

Windows-native qualification is also unavailable on L5 and remains a separate platform gate.

### 10.6 Phase 0B — complete locally

#### replay disclosure / no-rerun

AW preserves immutable replay/no-rerun identity and checks current Session View + ToolBinding +
Actor Envelope. Dotlink independently checks current issuer trust plus target-side filesystem,
shell or Job authority before exposing cached content. Withheld replay returns one minimal generic
non-retryable acknowledgement and never reopens the effect.

There is no generic cross-host preflight service or Tool hook. Concrete authority stays at the
machine that actually owns the resource.

#### participant Provider/payer

`ProviderAccounts::select_exact(actor, captured_identity)` reselects current authority and requires
full InvocationIdentity equality: actor, account owner, payer, custody/version, executor, model and
grant revision. There is no Session-owner fallback.

For paid participant policy delegation:

- Alice remains Session/delegation/child-Session/journal/lifecycle resource owner;
- Bob retains his exact provider/payer/model/tool identity;
- current Submit is rechecked transactionally;
- participant delegate path is top-level, non-owner, no child routing/constructor replay;
- Store model attempts bind to the child relation's stored InvocationIdentity;
- lifecycle transitions authorize the resource owner rather than conflating provider identity.

#### AW compact replay tombstones

`policy_admission_current` remains the hot indexed replay owner. Immutable replay identity, actor,
canonical target, request digest and revision remain in indexed columns. After a consumed effect
crosses a durable/effect recovery boundary, current-row detail compacts best-effort to a versioned
<256-byte never-retry tombstone. Immutable audit retains the full evidence.

Important boundary:

- first consume returns the full fact to the executor;
- legacy Consumed-without-Start remains full until Remote Start commits;
- local/delegated/remote-read effects compact only after the owning executor returns/commits;
- compaction failure leaves the full row and never changes the effect result;
- a re-entrant store-lock deadlock discovered during validation was fixed by reusing the already-held
  store connection for post-commit Remote Start compaction.

Grouped qualification proves a new admission still succeeds after **8,205 unrelated audit rows**;
there is no Session-history admission scan.

#### dotlink Remote Start tombstones

Dotlink's existing in-memory `remote_starts` owner now retains authorization IDs beyond ordinary
full-receipt retention:

- full `Mcp`/`JobMcp` receipts compact to payload-free `RemoteStartReceipt::Compacted`;
- `Unknown` remains a tombstone;
- claim lookup stays HashMap O(1);
- full receipts awaiting TTL compaction use a FIFO, avoiding a lifetime O(n) tombstone scan;
- compact/Unknown tombstones do not consume the 4,096 active/full receipt budget;
- semantic hash collision remains enforced after compaction;
- shell/filesystem may return only a generic compacted acknowledgement under current disclosure;
- Job/Graph control without a retained exact Job ref returns the opaque withheld acknowledgement.

Remaining R6 capacities are live-resource guards, not history ceilings: per-Turn admissions,
currently active Event subscriptions, and current reserved/published finite Job exposures.

## 11. Implementation phase map and status

### Phase 0 — shared Process convergence

Historical shared cut is published/accepted. Corrective AW source + dotlink consumer source are
implemented locally. **Publication/repin + real-Bubblewrap + Windows-native qualification remain.**

### Phase 0B — replay/admission/disclosure/lifetime

**Complete locally.** Do not redo it unless new evidence finds a concrete regression.

### Phase 1 — Agent identity/history/access foundation

Not started. Next after the Process publication/repin/platform gate is reviewed.

Planned work:

- `StoreScope`/`StoreId` + `StorePos(commit_log.seq)` cursor/projection helpers;
- exclusive Agent+ACL owner activation;
- exact indexed lookup;
- immutable history/HistoryGrant/DisclosureManifest owners;
- shared disclosure DAG with live authority-root rechecks;
- restore-generation stale-position semantics.

### Phase 2 — generic Events substrate

- one EventScan;
- gap-free projection publication;
- Event read/subscription/Handler integration;
- authority-scoped store affinity;
- same-store Event -> Agent Input transaction;
- source-first proof/deny/auth-unavailable semantics;
- legacy delivery/provenance cutover;
- EventType leaf registry on shared typed staging core.

### Phase 3 — Sandbox/Sources/Checkpoints/Vault

- DriverKey + per-artifact schemas;
- bounded SourceBindings/SourceSet;
- symmetric Prepared capture/restore;
- non-secret RecoveryDescriptors;
- store-open driver compatibility;
- Sandbox/Checkpoint/Vault owners;
- publication authority/external-effect fences.

### Phase 4 — public Agent + Jobs cutover

- unified public Agent envelope;
- Jobs use store-local position/cursor/projection helpers;
- exact high-water paging;
- ancestor-fenced tree traversal;
- Graph ownership/no nested batch;
- final six-method model Jobs surface.

### Phases 5–6 — typed Extensions

- one typed registry staging/storage core;
- instantiate only real leaf registries;
- ExtensionRegistry composes leaf owners;
- dynamic Process Extensions;
- TypeScript SDK;
- same shared Process owner.

### Phase 7 — retirement/cleanup

- remove compatibility runtime/state only after proof;
- retire legacy Event/Session/Job compatibility semantics;
- fold legacy sidecars into normal DisclosureManifest ownership;
- grouped cross-repo final qualification;
- final status/handoff sync.

## 12. Full chronology / implementation ledger

This section is intentionally chronological so the next agent can understand **why** the current
architecture looks the way it does.

### A. DL-150 foundations

DL-150 established the lower shared Tool/Job/filesystem/Graph/result/paging/mutation/replay/Process
contracts. Core lessons carried forward: one shared host-neutral semantic owner; finite Job refs;
exact replay separate from current authority; raw-page vs serialized-result budgets; ACK safety;
physical Process exit != semantic stop != output/occupancy; no blind retry of Unknown external
effects; exact authority at the effect boundary.

### B. 2026-11-09 through 2026-11-14 — unified Jobs + Agent runtime

The runtime converged toward Jobs as the work/control plane, Agents as persistent public work
owners, Agent-mediated subagents, Graph ownership, current authority at new-effect boundaries, and
one shared lower execution layer. This is preserved in
`UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`.

### C. 2026-11-15 through 2026-11-17 — shared Toolset/output/ACK/replay

AW/dotlink converged shared tool schemas, result budgets, exact paging/ACK safety, replay-before-
projection, typed media boundaries and product-only vs shared ownership. Dotlink D4–D7 later built
on this instead of inventing a second protocol.

### D. 2026-11-18 through 2026-11-20 — 0.17 convergence and process/control hardening

The release audit through v0.17.0–v0.17.6 refined:

- typed finite Job refs;
- shared receipt grammar;
- RawPageBytes vs SerializedResultBytes;
- final decorated-response fitting;
- heartbeat/progress;
- exact replay before projection;
- process stop evidence and terminal facts;
- one finalizer/cleanup owner;
- early cancel CAS and stdin effect linearization;
- trusted Remote Start issuer;
- Agent/source/event scaling/index work.

The 2026-11-20 continuation ledger in the prior comprehensive handoff records exact D4–D7 commit
history and review findings.

### E. DL-200 R1 — public-model simplification

R1 simplified the public model while preserving lower contracts:

- one public Agent envelope; public Session removed, internal Session storage retained;
- exactly six model Jobs methods: `jobs_list/get/read/send/ack/cancel`;
- Events and Signals collapse into one Event concept;
- NATS-style Event subject wildcards;
- Session remains internal structural organizer only;
- global principal/resource namespace;
- Checkpoint/history/fork/cache/access plan;
- host Agent input-plan API;
- Agent ACL/groups and multi-user semantics.

### F. R2–R5 — correctness hardening

Repeated reviews hardened:

- immutable history/fork attribution;
- HistoryGrant/DisclosureManifest separate from Agent ACL;
- live authority-root rechecks;
- Event authorization/delivery migration/cutover;
- gap-free projection and subscription enable semantics;
- Jobs traversal/pagination authority;
- SourceSet/Checkpoint restore and driver recovery;
- logical Sandbox/Vault capability projection;
- typed Extension registries and dynamic Process extensions;
- Graph ownership / nested Agent allowed / nested batch denied;
- strong Linux supervisor contract;
- replay, Prepared construction and external-effect custody.

The exact historical checklist is `DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md`.

### G. R6 — final architectural simplification and seal

R6 reconciled the simplification reviews without changing public API/resources/phases. It introduced
six descriptive internal roles (Owner, Fact, Operation, Projection, Cursor, Scope), one StoreId +
StorePos vocabulary, one owner-mutation discipline, ExternalEvidence, one projection publication
protocol, authenticated cursor envelope, EventScan for Events only, DriverKey/per-artifact schema,
one typed registry staging core, one shared Agent-neutral Process runtime, and Tree -> Occupancy.

R6 explicitly rejects universal TransactionManager/OperationManager, generic Jobs+Events traversal,
global replay ledger, universal revision clock, distributed Event router and eager unused registry
implementations.

### H. Phase 0A — shared Process source convergence

AW implemented Occupancy and `aw-process`; dotlink deleted its local shell Process lifetime owner and
consumed shared Process facts. Output/drain/stdin/cancel semantics were consolidated and grouped
checks passed.

### I. v0.17.7 correction — retiring Tokio worker SIGKILL

The release proved Bubblewrap PDEATHSIG follows the forking thread. R6 was amended/sealed around a
private process-lifetime Spawn Facility rather than one thread per Process. This also corrected
strict parent waitid semantics, stop-vs-exit causality and Windows no-window launch behavior.

### J. high-concurrency Spawn Facility refinement

The design was explicitly refined so stable OS threads are O(1), Processes/Jobs remain O(n)
lightweight/migratable tasks, admission is async/bounded, custody transfers through RAII, lanes
never shrink, panic poisons admission without killing the anchor, and N>1 remains an internal
benchmark-only optimization.

### K. v0.17.8/v0.17.9 imports

Dotlink release work added:

- 35m turn horizon;
- 10m report reserve;
- 25m/1500s max new-command cap on hand-back-capable surfaces;
- deadline advice on `jobs_get` only;
- actual execution deadline separate from Job age;
- Windows Job-accounting settle for transient console infrastructure;
- interruptible settle;
- stale queued launch cancellation/expiry immediately before fork.

Shared factual/process semantics were imported into AW; product policy remained dotlink-local.

### L. Phase 0B — replay disclosure

Replay identity and payload disclosure were split cleanly. AW checks current Session audience +
ToolBinding/Actor Envelope; dotlink checks current issuer/resource authority. Withholding never
re-enables an effect. The briefly-considered AW-local generic Tool replay preflight was rejected
because AW cannot authorize remote dotlink resources.

### M. Phase 0B — participant Provider/payer

One shared `ProviderAccounts::select_exact` primitive became the fresh paid-authority check. The
implementation explicitly separated durable Session/delegation resource ownership from participant
provider/payer identity. Store lifecycle/model-attempt authority was corrected accordingly.

### N. shared Process corrective implementation

Local AW source implemented stable Spawn Facility, strict waitid, one absolute execution deadline,
pre-spawn cancellation/timeout, stop/exit reconciliation and Windows source changes. Validation is
listed below.

### O. dotlink product-policy + Process consumer cut

Dotlink main locally implemented RuntimeSurfacePolicy and actual Running-deadline advice, then
adapted its sole shared Process consumer to the new AW async API using temporary local dependency
substitution for validation. The checked manifest was deliberately restored to the published pin.

### P. Phase 0B compact tombstones/lifetime closure

AW policy consumed rows now compact only after durable/effect recovery boundaries, preserving
legacy recovery and full immutable audit. A re-entrant store-lock deadlock found during validation
was fixed owner-locally. Dotlink Remote Start full receipts now compact after TTL to payload-free
claim tombstones instead of deleting the authorization identity; FIFO compaction avoids lifetime
O(n) scanning and compact tombstones do not consume active/full receipt capacity.

Phase 0B is locally complete.

## 13. Documentation architecture after R6 simplification

Use each document for one job.

README:
- developer mental model;
- public API/resource inventory;
- roles/mechanics/crate map;
- phase overview.

SPEC:
- normative contracts only;
- one authoritative definition per rule;
- section 0.1–0.3 owns shared mechanics/vocabulary;
- detailed domain sections own domain semantics.

IMPLEMENTATION:
- phase order;
- concrete work slices;
- grouped acceptance gates;
- current implementation notes.

TODO:
- current queue and approval gates.

DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST:
- historical R1–R5 design-resolution checklist only.

This handoff:
- chronology/current state/owner map/next-agent workflow.

Old comprehensive handoffs:
- historical forensic chronology only.

Avoid re-copying normative contracts into multiple docs after R6.

## 14. Non-regression checklist

Do not regress any of the following:

- one public Agent envelope; no public Session resurrection;
- exactly six model Jobs methods;
- Jobs send is directed Agent input or Process stdin/EOF only;
- Events and Signals stay one Event concept;
- no second Event log/runtime/scheduler;
- patterns never grant discovery/authority;
- same-store atomicity/order only;
- no hidden cross-store cursor merge;
- replay-before-current-authority for exact committed effect;
- current authority for genuinely new effects;
- Unknown external effect is inspected, never blindly replayed;
- one constructor publication linearization point;
- Prepared before escapable external effect;
- no live secrets in Checkpoints/SourceRefs/RecoveryDescriptors;
- Sandbox != Scope;
- History ancestry != delegation lineage != Sandbox derivation;
- HistoryGrant/DisclosureManifest != Agent ACL;
- Disclosure authority roots identify live owners to recheck, not cached permission;
- physical Process exit != semantic stop != occupancy != output completeness;
- weak direct Unix never overclaims full-scope Empty;
- nested Agent allowed; nested batch denied under active Graph ownership;
- Extension definitions != runtime grants/activation;
- no generic distributed transaction framework;
- no universal revision clock;
- no mandatory implementation of unused registry families;
- no broad architecture redesign after R6 without implementation evidence.


## 15. Current validation matrix

### AW shared Process correction

Green on L5:

- `aw-process`: **7/7**;
- `aw-platform-unix`: **18 executed + 1 explicit cross-UID ignore**;
- `aw-tools`: **126/126**;
- adapter Process: **27/27** using the real Nix Bash path;
- focused foreground shell;
- foreground shell cancellation;
- Graph ShellNode cancellation;
- Graph nonzero exit;
- `aw-cli --tests` type-check;
- strict affected/all-target/all-feature Clippy;
- rustfmt + `git diff --check`.

The stable-parent regression proves a PDEATHSIG child survives the submitting OS/Tokio runtime
thread ending.

### AW Phase 0B

Green:

- policy-admission grouped tombstone + >8K audit-history test;
- Remote Start owner: **5/5**;
- policy-runtime replay: **4/4**;
- account publication/revoke/regrant owner: **1/1**;
- Store participant delegate lifecycle owner: **1/1**;
- strict root/all-target Clippy + test type-check + fmt/diff.

### dotlink local product-policy / consumer source

Validated against the dirty local AW checkout via temporary path substitution:

- shell: **86/86**;
- Graph: **13/13**;
- queued real-Spawn-Lane cancellation regression: green;
- pre-spawn timeout/no-terminal mapping: green;
- canonical spawn-refusal settlement: green;
- Jobs group: **121/121**;
- shared-native: **15/15**;
- bounded-surface constants/instructions/deadline tests: green;
- queued time does not consume execution timeout: green;
- full test graph check: green;
- strict all-target/all-feature Clippy: green;
- fmt + diff-check: green.

### dotlink Phase 0B target replay

- consolidated `remote_start` filter: **16/16**;
- full receipt -> compact tombstone -> semantic collision retained: covered;
- strict all-target/all-feature Clippy/test-graph/fmt/diff green.

### Expected pinned-build failure before repin

Current dotlink source intentionally targets the new local AW API while Cargo still pins
`7c16708`. A normal pinned build currently fails with the expected old-API errors. This is a
**publication ordering state**, not a source regression. Do not revert the consumer source.

### External/platform gates not available on L5

- real Bubblewrap test reaches installed bwrap but outer sandbox rejects `/proc` mount with
  `Operation not permitted`;
- Windows Rust target/native host not available;
- therefore real Bubblewrap worker-retirement and Windows-native qualification remain open.

## 16. Exact remaining work / promotion gate

Do **not** start Phase 1 yet.

Remaining sequence:

1. **Explicit user authorization required:** commit/publish the dirty AW corrective source.
2. Repin all dotlink AW Git dependencies atomically to that new AW revision.
3. Re-diff read-only `release/0.17` from exact v0.17.9 before touching consumer source again.
4. Run ordinary dotlink build/tests under the real Git pin; the already-implemented consumer source
   should now compile without path substitution.
5. Rerun the grouped shell/Graph/stale-launch consumer gate under the real pin.
6. Run real Bubblewrap worker-retirement qualification on a host where bwrap mounts are permitted.
7. Run Windows-native qualification for no-window launch, private termination marker and
   interruptible Occupancy settling.
8. Review the publication/platform gate.
9. Begin Phase 1.

Do not redo Phase 0B unless a concrete regression appears.

## 17. Historical provenance map — nothing is intentionally discarded

This handoff summarizes the active line, but all detailed prior work remains preserved:

- `AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md` — full pre-R6 DL-200 + inherited 0.17
  chronology, D4–D7, review findings, v36 admission/source work and older 2026-11-19 handoff;
- `DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md` — exact R1–R5 design-resolution checklist;
- `AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md` — detailed 0.17 release/process lineage;
- `AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md` — shared AW/dotlink implementation
  chronology before DL-200 R6;
- `UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md` — unified Jobs/Agent runtime origin;
- sealed DL-200 `SPEC.md` — the full normative contract, including every public API/security/
  history/checkpoint/sandbox/vault/extension/event/process rule;
- `IMPLEMENTATION.md` — exact phased source plan and grouped gates;
- `TODO.md` — current queue/status;
- immutable DL-150 reference — lower shared semantics.

If forensic detail appears missing here, **read those preserved sources rather than inventing a new
contract**. This document is the current entrypoint and owner/status map; the SPEC remains normative.

## 18. Copy-ready starting prompt for the next agent

```text
Use dotlink---l5 only.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect/list/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless explicitly promoted.
Do not commit/push/tag/release/deploy unless explicitly asked.

Read first:
1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md
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

Current live refs at handoff creation:
  AW HEAD:      0d2dc10e20b62ebf0f064cdbc43615ae2d728ad7
  dotlink HEAD: fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94
  release ref:  576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

Published dotlink AW pin remains:
  7c1670821b15082821a0b9621cd1caccbe62f144

Important current state:
- R6 + shared Process/release-correction amendment are sealed and authoritative.
- AW corrective Process source is implemented locally/uncommitted and green.
- dotlink v0.17.9 product-policy + async controlled-Process consumer source are implemented
  locally/uncommitted and green only when validated against the dirty local AW source.
- dotlink Cargo.toml/Cargo.lock intentionally still pin published AW 7c16708; ordinary pinned build
  therefore currently fails on the new async API. Do not revert source to the old API.
- Phase 0B is locally complete: replay disclosure, participant Provider/payer, AW policy tombstones,
  dotlink Remote Start tombstones and lifetime-history qualification are done.
- release/0.17 reference is exact v0.17.9 / 576d0a1 and read-only.
- real Bubblewrap worker-retirement is unavailable on L5 because the outer sandbox rejects bwrap's
  /proc mount; Windows-native qualification is also unavailable here.

Do not redo already-complete source cuts.

If the user asks to continue implementation without authorizing publication, only work on tasks that
are genuinely unblocked and do not require pretending the old AW pin has the new API.

If the user explicitly authorizes commit/publish/repin, the next sequence is:
1. revalidate both dirty trees and exact release ref;
2. commit/publish AW corrective source first;
3. atomically repin all dotlink AW Git dependencies to that AW revision;
4. re-diff release/0.17 from exact v0.17.9;
5. run ordinary pinned dotlink build + the already-green shell 86/86, Graph 13/13, stale-lane
   cancellation/pre-spawn-timeout/spawn-refusal consumer gates;
6. run real Bubblewrap worker-retirement on a suitable host and Windows-native qualification;
7. review; then begin Phase 1.

Phase 1 starts only after that publication/platform gate is accepted. Phase 1 is the StoreScope /
StoreId + StorePos / Agent migration-history-disclosure foundation described in IMPLEMENTATION.md.

Keep implementation turns around 30 minutes and checkpoint at the next coherent green point.
Prefer simple owner-local Rust primitives, zero-cost/static abstractions and easy hot-path wins.
Do not create a generic TransactionManager, global replay ledger, universal revision clock,
generic Jobs+Events traversal engine, distributed Event router, public spawn-pool framework or
unused registry implementations.
```

## 19. Definition of a good future handoff

A future handoff is accurate only if it distinguishes:

- sealed design vs implementation evidence;
- published Git boundary vs dirty local source;
- AW local corrective API vs the older AW revision still pinned by dotlink;
- source validated through temporary local dependency substitution vs ordinary pinned build;
- Process factual ownership vs dotlink product projection;
- replay identity vs current disclosure;
- resource owner vs provider/payer identity;
- full replay receipt vs compact never-retry tombstone;
- live capacity bounds vs lifetime-history ceilings;
- unavailable platform qualification vs product failure;
- user authorization to implement vs authorization to commit/push/release.

Never compress these distinctions into one vague "complete" statement.
