# AW + dotlink + DL-200 R6 comprehensive handoff — 2026-11-21

Prepared: 2026-11-21
Owner: @pvl
Machine/source authority: L5 only
AW repo: /home/pvl/spaces/abird/src/aw
dotlink repo: /home/pvl/spaces/abird/src/dotlink
Read-only 0.17 reference: /home/pvl/spaces/abird/src/dotlink-017-mounts

## 0. Purpose and status

This is the current comprehensive context handoff for DL-200 after the November 20–21
design/review sequence and the R6 simplification reconciliation.

It is a navigation/context document, not a second normative specification.

Current design status:

- R6 is the sealed and authoritative DL-200 normative revision.
- R1–R5 are retained as historical design provenance.
- R6 keeps the public resource model, public API and seven implementation phases unchanged.
- R6 simplifies implementation mechanics and documentation; it does not add a new public
  resource, authority service, distributed transaction layer or eighth phase.
- R6 was explicitly approved/resealed on 2026-11-21 after the final live-owner review.
- Implementation was separately promoted on 2026-11-21 and Phase 0 remains in progress.
- Do not claim R6 source behavior is implemented merely because the R6 plan is written.
- No commit, push, tag, release or deployment is authorized by this handoff.

The sealed R6 design is the result of reconciling the repeated R1–R5 correctness reviews
with the final simplification reviews. The guiding stopping criterion is now: fewer repeated
mechanisms and owners, without collapsing semantic distinctions that matter. Broad
architecture iteration should stop after R6 unless implementation evidence proves a real
problem.

## 1. Authority order and mandatory read order

Source/tests/executed results outrank prose.

For current DL-200 work read in this order:

1. /home/pvl/spaces/abird/src/aw/AGENTS.md
2. /home/pvl/spaces/abird/src/aw/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
3. /home/pvl/spaces/abird/src/aw/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
4. /home/pvl/spaces/abird/src/aw/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
5. /home/pvl/spaces/abird/src/aw/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
6. this handoff
7. /home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md
8. /home/pvl/spaces/abird/src/aw/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. /home/pvl/spaces/abird/src/aw/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. /home/pvl/spaces/abird/src/dotlink/AGENTS.md
11. /home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md
12. live source/tests for every owner actually touched

Historical/forensic context, when needed:

- /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md
- /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md
- /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md
- the immutable DL-150 reference under .agents/plans/dotlink-tool-port/reference/

Do not treat old comprehensive handoff wording as current authority when it conflicts with the
sealed R6 design or live source.

## 2. Non-negotiable operating constraints

- Use dotlink---l5 only.
- Do not use A1.
- Do not create another worktree.
- Preserve unrelated and concurrently owned work.
- Never inspect, list, stage, reset, stash or modify AW .agents/plans/ux/**.
- Sandbox/mount/config-v13 and the 017 sandbox convergence implementation remain frozen
  unless explicitly promoted.
- The release worktree /home/pvl/spaces/abird/src/dotlink-017-mounts is read-only reference.
- Do not reopen published 0.17 D0–D7/shared-runtime/process-control/Remote-Start issuer work
  without a concrete regression.
- Keep implementation turns around 30 minutes and stop at the next coherent checkpoint.
- Prefer clean/simple design first; use zero-cost/static abstractions and easy hot-path
  performance where they do not create maintenance burden.
- Do not over-engineer tests/evidence. Prefer a small grouped failure-boundary gate over
  fixture explosion or evidence pinning.
- Do not commit/push/tag/release/deploy unless explicitly asked for that action.

## 3. Exact current repository state

### 3.1 AW

Current implementation source checkpoint before this handoff sync:

    c7cc948 — Checkpoint DL-200 Phase 0 owner hardening
    branch: main

The immediately preceding R6 seal/docs checkpoint is:

    7c3800c — Seal DL-200 R6 architecture

The older published R1 baseline remains `132bfd7416a0a83e915ab18ffc10de9f6d3e2a2b`.

The DL-200 TODO still records the older implementation/design base:

    f035a00031b8f5879f4d77f3fd14bfe02b89305c

The reusable AW implementation currently consumed by dotlink remains:

    b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b

The R6 plan/docs are checkpointed in `7c3800c`. The pre-R6 Phase-0A Process work and
independent Phase-0B store/policy/exposure work are checkpointed together in `c7cc948` after
focused compilation and owner-boundary tests. They remain conceptually separate slices even
though the repository checkpoint preserves both. Do not casually rewrite either slice while
performing the R6 `aw-process` cutover.

### 3.2 dotlink

Current implementation source checkpoint before its handoff sync:

    89ff3fb — Harden Remote Start replay admission
    branch: main

Current AW Git dependency pin in dotlink Cargo.toml/Cargo.lock:

    b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b

The Remote Start/replay hardening previously dirty in `src/jobs.rs`,
`src/mcp/tools/shell.rs` and `src/remote_start.rs` is checkpointed in `89ff3fb`. It is
Phase-0B/replay hardening, **not** evidence that the R6 shared-Process consumer cutover has
begun. The real dotlink `aw-process` migration remains pending.

Published dotlink lineage immediately below current head includes:

- bbd29c1 — Record AW repin and Remote Start trust cutover
- 648519a — Repin AW and complete process-control hardening
- 20b6f04 — Record fresh 0.17 review remediation
- d32a319 — Harden Jobs replay and response budget boundaries
- 8b77bae — Record final 0.17 equivalence checkpoint
- fa01e18 — Complete D4-D7 shared consumer convergence
- 36d6533 — Repin AW and complete D1-D3 process convergence

### 3.3 read-only release/0.17 reference

Current local release reference:

    94ae8bf2757a279dc5b16b50b010ee1d278c3b44
    branch: release/0.17
    local state: ahead of origin/release/0.17; treat as read-only

Relevant 0.17.6 forensic/process sequence includes:

- 6718cd5 — release 0.17.6
- 6bf8b6d — run path owns the sweep/final cleanup
- 64e7a0d — one finalizer
- 63ad3f3 — kill proves nothing about what was running
- 4731b1d — report command process tree
- c26293b — Darwin empty-group cleanup correction
- additional qualification commits through 94ae8bf

The R6 Process design consumes lessons from this reference but does not mutate this worktree.

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

## 10. Current implementation reality

### 10.1 implemented Phase-0A work

AW source checkpoint `c7cc948` contains the pre-R6 Process work:

- ProcessTerminal has physical exit, StopRecord, Tree and per-stream output completeness;
- ProcessReceipt stores ProcessTerminal as terminal truth;
- private async Scope owns Child + platform DirectScope;
- Linux DirectScope uses pidfd when available plus direct process-group/procfs observation;
- Windows uses exact duplicated leader HANDLE + Job Object;
- settlement ordering closes remaining scope before output drain;
- direct Unix/Darwin/Windows process observations and stop semantics were focused-tested.

Important: this is pre-R6 source and still spells Tree.

Historical focused evidence recorded in CURRENT_HANDOFF includes:

- affected shared crates compile, including aw-platform-win;
- DirectScope tests 4/4;
- aw-tools Process runner tests 4/4 with full;
- adapter lifecycle regressions for descendants, inherited pipes, external signals,
  cancellation/deadline and terminate-once;
- strict affected Clippy green;
- git diff --check clean at that checkpoint.

This evidence does not qualify the R6 Occupancy/shared-owner/dotlink cutover.

### 10.2 known Phase-0 gaps

Before Phase 0 can be accepted under R6:

- approve/reseal R6 first;
- rename shared Rust Tree/.tree -> Occupancy/.occupancy;
- correct weak direct-Unix semantic negative observation so it cannot claim Occupancy::Empty
  when setsid escape cannot be excluded;
- extract one Agent-neutral Process lifetime owner at a dependency-clean boundary;
- make the real dotlink consumer compile/use that owner without importing the full
  Agent-specific adapter solely for Process lifetime;
- remove dotlink parallel guard/wait/reap/tree-classification semantics;
- preserve dotlink wire tree projection;
- qualify real AW+dotlink behavior together.

### 10.3 Phase-0B concurrent work

A separate concurrent source slice is active in aw-store/policy/runtime files.

The plan records outstanding grouped gates around:

- original ToolCall replay across writer/policy/parent changes;
- actual participant ToolPolicy/payer authority;
- current disclosure on cached receipts;
- compact replay tombstones;
- exact opaque/minimal withheld replay result;
- indexed audit admission beyond prior lifetime ceilings.

Do not conflate this concurrent work with Phase-0 Process acceptance.

### 10.4 dotlink current state

dotlink is still pinned to AW b8a46a... and does not yet consume the R6 shared Process owner.

Current dirty dotlink source is not R6 implementation evidence.

The current dotlink shell already contains several 0.17.6-era product behaviors such as
non-login Bash arguments and tree/result projection, but R6 requires the lifetime semantics
to move to the one shared Process owner rather than preserving a second implementation.

## 11. R6 implementation plan — no eighth phase

### Phase 0

- R6 approval/reseal first.
- Tree -> Occupancy source cutover.
- one shared Agent-neutral Process owner.
- AW cutover.
- dotlink cutover.
- preserve dotlink wire projection.
- one grouped AW+dotlink Process behavior gate.

### Phase 0B

Use small shared private replay/effect/ExternalEvidence helpers inside the existing policy,
guardian and Remote Start owners.

No global replay ledger/manager.

Finish the outstanding replay/actor/disclosure/lifetime-audit gates.

### Phase 1

- StorePos + cursor/projection helpers from existing commit_log.seq/store_meta.store_id;
- exclusive Agent+ACL owner activation;
- exact indexed lookup;
- immutable history/HistoryGrant/DisclosureManifest owners;
- structurally shared disclosure DAG with live authority-root rechecks.

### Phase 2

- one EventScan;
- gap-free Event projection publication;
- Event read/subscription/Handler integration;
- authority-scoped store affinity;
- same-store Event->Agent Input transaction;
- source-first authorization and authorization_unavailable;
- legacy cutover/provenance migration;
- EventType leaf registry on shared typed staging core.

### Phase 3

- DriverKey + per-artifact schemas;
- bounded SourceBindings/SourceSet;
- symmetric prepared capture/restore;
- non-secret RecoveryDescriptors;
- store-open driver compatibility validation;
- Sandbox/Checkpoint/Vault owners;
- final publication authority/external fence semantics.

### Phase 4

- unified Agent public cutover;
- Jobs use StorePos/cursor/projection helpers;
- exact store-local high-water paging;
- ancestor-gated tree traversal;
- Graph ownership/no nested batch;
- Jobs send/read/cancel/ack final model surface.

### Phases 5–6

- one typed registry staging/storage core;
- instantiate concrete registry families only with real consumers;
- ExtensionRegistry composes existing leaf owners;
- dynamic Process Extensions;
- TypeScript SDK;
- dynamic extensions reuse the same Process owner.

### Phase 7

- remove compatibility runtime/state only after proof;
- fold legacy Event Input sidecars into ordinary DisclosureManifest ownership;
- retire delivery_cutover_rev and old delivery state when recurrence is impossible;
- remove duplicate Session/Job/Event compatibility semantics;
- final status/handoff synchronization;
- grouped cross-repo qualification.

Documentation deduplication is already an R6 design gate; do not defer another normative
cleanup project to Phase 7.

## 12. Current R6 review/approval queue

Completed design work:

- owner/locality law and six roles;
- Occupancy proposal;
- StoreId/StorePos;
- replay/Prepared/ExternalEvidence semantics;
- projection publication protocol;
- cursor envelope;
- EventScan;
- DriverKey/per-artifact schema split;
- typed registry core/staging;
- shared Process runtime design;
- phase integration;
- README simplification;
- SPEC normative-index/history cleanup.

Still open:

- explicit user approval/reseal of R6;
- all R6 implementation slices;
- Phase-0 real AW+dotlink gate;
- Phase-0B grouped outstanding gates;
- later Phase 1–7 implementation/qualification.

Do not mark R6 sealed simply because this handoff exists.

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

## 15. Exact next action

The next agent should first revalidate:

1. AW/dotlink/release heads and dirty paths;
2. R6 README/SPEC/IMPLEMENTATION/TODO status;
3. that R6 remains sealed and authoritative;
4. that UX and frozen 017 sandbox/mount work remain untouched;
5. that concurrent Phase-0B files remain preserved.

Then follow the user's current instruction.

If asked to continue design review:
- review the sealed R6 design for contradictions/minimality only;
- prefer deleting/reusing mechanisms over adding abstractions;
- do not reopen public API/resource model without a real blocker.

R6 is already resealed. Begin/continue Phase 0 only at coherent source boundaries and do
not start Phase 1 before the grouped Phase-0 AW+dotlink gate is accepted.

If asked to implement Phase 0:
- keep each turn around 30 minutes;
- checkpoint at the next coherent green point;
- preserve unrelated files;
- no commit/push unless separately requested.

## 16. Copy-ready starting prompt

Use this prompt for the next agent:

~~~text
Use dotlink---l5 only.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 and the 017 sandbox implementation remain frozen unless
explicitly promoted.
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
9. dotlink/AGENTS.md
10. dotlink/.agents/docs/CURRENT_HANDOFF.md
11. live source/tests for every owner touched

Current implementation source checkpoints after reseal:
  AW source      c7cc948
  AW R6 seal     7c3800c
  dotlink source 89ff3fb
  dotlink AW implementation pin:
               b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
  read-only 017 reference:
               94ae8bf2757a279dc5b16b50b010ee1d278c3b44

DL-200 status:
  R6 = sealed and authoritative as of 2026-11-21.
  R1-R5 = historical design provenance.
  Implementation was separately promoted on 2026-11-21; Phase 0 is in progress under R6.
  Do not claim R6 implementation merely because the design is sealed.

R6 public model is unchanged:
  Agent, Job, Sandbox, Event, Checkpoint, Extension.
  Model Jobs remains exactly:
    jobs_list jobs_get jobs_read jobs_send jobs_ack jobs_cancel

R6 simplification north star:
  one authoritative source per semantic truth;
  mutable truth -> one Owner + linearization point;
  immutable truth -> one Fact identity;
  uncertain external effect -> one Operation custody;
  derived state -> Projection/Cursor/adapter;
  global identity but store-local atomicity/order.

Shared private mechanics:
  StoreId + StorePos
  replay/Prepared semantics
  ExternalEvidence
  projection safe-prefix/head-catch-up/atomic swap
  authenticated cursor envelope
  EventScan for Event consumers only
  DriverKey + per-artifact schema
  typed registry staging core
  one Agent-neutral Process lifetime owner

R6 proposes:
  Tree -> Occupancy
  ProcessTerminal.occupancy = immutable pre-cleanup Process-scope occupancy.
  Dotlink wire tree=leftovers|unknown remains compatibility projection.

Do not collapse:
  Agent vs Sandbox
  Sandbox vs Process Scope
  Jobs vs Events
  HistoryGrant/DisclosureManifest vs ACL
  rev vs event_rev vs StorePos/security generations
  different Unknown semantics

Current implementation reality:
  AW `c7cc948` preserves the pre-R6 Phase-0A Process work and Phase-0B owner hardening;
  it still uses Tree and has not performed the R6 shared-runtime extraction.
  Direct Unix still needs the R6 weak-negative -> Occupancy::Unknown semantic correction.
  Dotlink `89ff3fb` preserves Remote Start/replay hardening; the real R6 shared-Process
  consumer cutover has not begun.

Before implementation, revalidate current refs/status because source outranks this handoff.
R6 is already sealed. Continue Phase 0 only:
    Occupancy rename
    one shared Agent-neutral Process owner
    AW + real dotlink cutover
    preserve dotlink wire projection
    one grouped AW+dotlink behavior gate

Keep implementation turns around 30 minutes and stop at the next coherent checkpoint.
Prefer zero-cost/static Rust abstractions and simple owner-local logic; do not create a
generic TransactionManager, cursor engine, distributed Event router, global replay ledger,
universal revision or unused registry implementations.
~~~

## 17. Definition of a good next handoff

A future handoff is accurate only if it distinguishes:

- sealed design vs candidate design;
- design-resolution checklist vs implementation evidence;
- published head vs dirty working-tree implementation;
- AW implementation head vs the older AW revision pinned by dotlink;
- current shared Process source vs historical 0.17 release behavior;
- R6 mechanics vs existing semantic owners;
- temporary migration state vs forever architecture;
- current-user authorization to implement/commit/push/release.

Never compress those distinctions into a single "complete" statement.
