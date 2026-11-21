# AW + dotlink + DL-200 full comprehensive handoff — 2026-11-20

Author: coordinating design/implementation agent
Owner: @pvl
Source authority host: **L5 only**

AW source: `/home/pvl/spaces/abird/src/aw`
dotlink source: `/home/pvl/spaces/abird/src/dotlink`
Read-only release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

This is the canonical handoff for the complete AW + dotlink execution/Jobs/runtime
convergence **plus the new DL-200 design work** for unified Jobs, Agents, Events,
history/forks, host input planning, access/groups and checkpoints.

It supersedes earlier handoffs for **current state, current design direction and
next-agent instructions**. The previous canonical AW+dotlink comprehensive handoff is
embedded verbatim later in this file so no prior implementation chronology, review
finding, 0.17 work item, architectural decision or qualification evidence is lost.

When facts conflict, use this authority order:

1. direct user instruction in the current conversation;
2. live source/tests on L5;
3. the current DL-200 spec;
4. active owning implementation plans/TODOs;
5. current-state sections at the top of this handoff;
6. the embedded historical comprehensive handoff;
7. older dated handoffs and release implementations.

DL-200 **R5 is SEALED / IMPLEMENTATION PROMOTED / PHASE 0 IN PROGRESS** (R1 sealed 2026-11-20; R2-R5 amended/resealed 2026-11-21; R5 authoritative). Process is one supervised execution scope with immutable pre-cleanup `Tree`; strong PID-1 reporting is supervisor-authored over a private unidirectional channel. `agent_state_current` activates through one exclusive Agent+ACL migration. Multi-resource construction durably claims Prepared before effects; SourceDriver capture/restore share one bounded non-secret recovery contract (sources default 128/hard 1024), live/unresolved driver dependencies are validated at store-open, external fences are immutable or held through the AW commit, and final store-owned authorization is fenced with publication. Replay compacts safely. Event consumers use authority-scoped/gap-free EventStore affinity with source-first authorization; visible-source external dependencies require immutable/fenced evidence, `authorization_unavailable` is candidate-oblivious/non-advancing, Agent delivery is same-transaction-store only, and migration sets `delivery_cutover_rev = current next_event_rev` while accepted legacy Inputs receive conservative disclosure/provenance sidecars. DisclosureManifest structurally shares provenance while roots identify live owners to recheck. Jobs pagination is store-local/path-reauthorized with exact `(publication_position DESC, canonical_ref ASC)` ordering and swap-time head catch-up. Leaf registries stage atomically, dotlink reuses the shared Process owner, and nested Agents remain allowed while nested batch is denied. Current source still needs the direct-Unix negative->Unknown correction and real dotlink shared-owner compilation; concurrent Phase 0B work must be preserved.

Phased implementation authority:
`../plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md`.

---

## LATEST DL-200 design refinement — 2026-11-20

This section supersedes older DL-200 design summaries in PART I/PART II where they
conflict. PART III remains preserved historical chronology. Older API sketches below are
kept intentionally as design history and may contain superseded spellings such as
`agent.fork`, singular `source=...`, public `cache=...`, public HistoryBase `from`, or
checkpoint `rev`; **never implement those when they conflict with the current SPEC/TODO or
this top section**.

- One public host Agent constructor: `agent.create(from=Agent|settled Turn)`; there is no
  host `agent.fork`. Model ancestry uses `agent(..., from=...)`. Trusted in-process library
  code may use typed HistoryBaseRef only as a lower primitive.
- Fork remains a semantic immutable-history relation; exact forks structurally share
  HistoryBase/HistoryGrant and preserve inherited provenance.
- `checkpoint.restore` remains first-class because it restores exact history **plus**
  an exact plural named typed `SourceSet` into a new Agent + new Sandbox.
- Git is the mandatory first SourceDriver; dirty Git retains the private-index synthetic
  commit/tree design and **both clean and dirty live checkpoint anchors get private
  retention refs**. Versioned/content-addressed object/bucket manifests can implement
  the same fail-closed SourceDriver contract; there is no generic archive fallback.
- The canonical Project resource tree now has two sibling branches:
  `/a:<agent>/t:/j:/n:` for ExecutionRef and `/s:<sandbox>` for SandboxRef.
- Named source attachment is owned by only `sandbox.sources.list/set`; SourceRef is a
  provider/driver identity and never contains credentials.
- Sandbox is logical mutable execution state, not a persistent Bubblewrap namespace. An
  Agent chooses its Sandbox at creation and does not live-rebind in v1. Agents may share
  one, derive an isolated child Sandbox, or start fresh by creating a new Agent; every
  Process still gets fresh physical containment.
- Same Sandbox does not imply same authority. Sandbox/tool/Vault authority remains
  Agent-sensitive and revalidated.
- VaultProvider/VaultEntryRef/VaultGrant project selected secret/service capabilities as
  broker/socket/short-lived credential/ephemeral-file/env access. History, Sandbox
  lineage, ACL inheritance and checkpoint restore never implicitly grant Vault authority.
- Bare Event names are system-owned forever; custom publication is structurally
  `#name`, e.g. `e:#ready`. No reserved-name registry or `aw_` prefix remains.
- Events have stable opaque `Event.id`; host Event surface also includes bounded
  `events.subscriptions.list`.
- One plain `Extension` registers through one typed `ExtensionRegistry` composition root.
- Typed subregistries include Tool/Hook/Action/Resource/View/EventType/EventHandler/
  Provider/ProviderHook/ProcessHook/UI/Setting/State/Compactor plus native SourceDriver/
  SandboxDriver/VaultProvider registries.
- Native Rust registration preserves concrete domain types and returns typed handles;
  stable Extension/local IDs + schema versions own durable/wire identity, never Rust
  TypeId. Encoding/erasure happens only at heterogeneous registry internals or real
  model/web/IPC/storage boundaries.
- One Extension registration publishes atomically across all subregistries.
- Runtime callbacks still use scoped `ctx.tools`, `ctx.state`, `ctx.ui`, `ctx.models`, etc.
- Histate can therefore register fully typed Actions/Views/EventHandlers/UI/Tools/Hooks
  while keeping its semantic graph in Histate's own typed store.
- Dynamic Extensions reuse/generalize the existing `aw-extensions` child-process ABI: any explicitly selected executable can speak one bounded full-duplex versioned stdio protocol projecting the same registries and scoped `ctx.*` capabilities.
- TypeScript/JavaScript is the first ecosystem SDK; Node/Bun/Deno run directly. AW does not own npm/pip/cargo/package-marketplace lifecycle in v1.
- Do not embed a JS engine or Wasmtime for the first implementation. WASI may run through an external `wasmtime run ...` process today; embedded Wasm remains an optional future ExtensionHost behind the same registry contract.
- Histate owns Builds, product Sessions, Decisions/Evidence/summaries/semantic graphs,
  commit classification and product fork UX. A Histate Session may reference many Agents
  + zero/many Sandboxes and a product checkpoint may reference one/more AW Checkpoints,
  without resurrecting AW public Session. Cross-store mutation uses product-owned
  idempotent saga/outbox reconciliation rather than an AW distributed transaction.
- The frozen 017 sandbox/mount/config-v13 implementation plan remains frozen. DL-200
  defines logical Sandbox/Vault composition only.

The canonical detailed authority is the current DL-200 SPEC/TODO.

The fresh full pre-seal architecture audit completed clean. It rechecked namespace/token
encoding, mutation/authority/revision domains, lineage vs history identity, cache identity,
constructor replay + atomic publication, multi-source capture/retention/restore,
SourceRef/SandboxGrant/Vault separation/revocation, transitive disclosure manifests,
Event routing/causal persistence/commit-log compatibility, typed registry activation/
reload/State CAS/View isolation, dynamic process bounds and Histate composition.

---

## 0. Non-negotiable operating rules

- Use **dotlink---l5 only**.
- Do not use A1.
- Do not create another worktree.
- Preserve all unrelated dirty/staged work exactly.
- Do not inspect or touch AW `.agents/plans/ux/**`.
- Keep implementation turns around 30 minutes and stop at coherent checkpoints.
- Do not commit/push/tag/release/deploy unless the user explicitly asks.
- Signed history remains required when commits are authorized.
- Revalidate refs/status/staging before each implementation phase.
- Live source/tests outrank prose.
- Portable semantics belong in AW; dotlink remains concrete host/transport/deployment
  consumer.
- Prefer clean/simple owner boundaries over speculative abstraction.
- Do not add a second JobManager, PolicyStore, event service, scheduler, process Guard,
  result fitter, progress scheduler or replay ledger.
- Do not resurrect old `job_*` APIs or parallel foreground/background engines.
- **Sandbox/mount/config-v13 remains frozen** until explicitly promoted.
- Native Windows/macOS runtime qualification and trusted-selector Bubblewrap smoke
  remain external gates unless the user explicitly asks to pursue them.
- DL-200 is not yet an implementation plan. Treat its storage/migration questions as
  design work until sealed.

---

## 1. Exact current Git/worktree state

### AW

At handoff creation:

```text
HEAD        f035a00031b8f5879f4d77f3fd14bfe02b89305c
origin/main f035a00031b8f5879f4d77f3fd14bfe02b89305c
```

Recent published checkpoints:

```text
f035a00 Record published dotlink repin and issuer trust
b8a46a9 Complete v36 runtime admission and process semantics
4234447 Record final shared 0.17 implementation pin
1a5d0f6 Relax shared MCP protocol dependency ranges
7c7de6c Fix shared 0.17 projection boundaries
eb325b3 Complete shared 0.17 runtime convergence
```

Reusable AW implementation consumed by dotlink remains:

```text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
Complete v36 runtime admission and process semantics
```

At the start of this handoff task, the only AW working-tree addition was the untracked
DL-200 plan directory. This handoff task then added the canonical DL-200 handoff,
copy-ready prompt, plan README/TODO/queue entry and CURRENT_HANDOFF pointer.

No implementation source is dirty; all current AW worktree changes are documentation/
planning material created by this design/handoff task.

### dotlink

```text
HEAD        bbd29c13904b7255eb28e2ed6c776c3590ac122a
origin/main bbd29c13904b7255eb28e2ed6c776c3590ac122a
```

Recent published checkpoints:

```text
bbd29c1 Record AW repin and Remote Start trust cutover
648519a Repin AW and complete process-control hardening
20b6f04 Record fresh 0.17 review remediation
d32a319 Harden Jobs replay and response budget boundaries
8b77bae Record final 0.17 equivalence checkpoint
fa01e18 Complete D4-D7 shared consumer convergence
```

dotlink implementation source is clean. This handoff task updates only
`.agents/docs/CURRENT_HANDOFF.md` to point at the new canonical DL-200 handoff.

All direct/transitive AW Git packages in dotlink remain pinned to:

```text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
```

with no path override and no registry package/version drift.

### Release reference

Do not modify `/home/pvl/spaces/abird/src/dotlink-017-mounts`.

Published compatibility baseline remains:

```text
origin/release/0.17 = v0.17.5
3995448d7bf33794ef84c7bb51150d77cc260ada
```

Current read-only local behavioral reference is:

```text
release/0.17 = v0.17.6
6cd056672d9390b17ac591786253af5f04bfeef1
```

The v0.17.6 delta was forensically reviewed and its reusable Process-settlement semantics
are now sealed in DL-200 section 25.11 / implementation Phase 0. The release worktree
remains reference-only.

---

## 2. Current implementation checkpoint before DL-200

All D0-D7/shared-consumer/0.17 equivalence work is complete and published. Do not reopen
it without a real regression.

The complete current implementation state is documented in the embedded historical
handoff later in this file. The most important published facts are:

### AW v36

Published AW `b8a46a94...` includes:

- replay-before-current-authority for policy admission and Remote Start;
- v36 indexed current projections replacing lifetime audit scans;
- shared Session participant policy routing without privilege union;
- durable Agent/Turn source event ownership;
- durable/restart-safe wake admission;
- indexed canonical Agent lookup;
- index-backed Job listing;
- source revision notifications replacing 10 ms Agent `jobs_get` polling;
- shared process outcome/output-completeness projection;
- trusted Remote Start issuer evidence and issuer-aware leases.

### dotlink

Published dotlink includes:

- D4-D7 typed JobRef/shared receipts/raw-page/result/protocol/progress convergence;
- replay-before-projection and bounded response envelope hardening;
- strict raw-byte delivery/ACK truth;
- immediate reasonless descendant cleanup after ordinary child exit;
- bounded post-exit output drain preserving factual child exit;
- no private stop/Guard taxonomy;
- canonical shared-owner cancel linearization;
- stdin authority/revision acceptance linearized with byte admission;
- trusted AW Remote Start OAuth client/epoch enforcement;
- one product mapper/lifecycle log projection;
- duplicate static MCP instructions removed.

Latest fully repinned dotlink qualification recorded in current handoff:

```text
main binary: 706 passed / 0 failed / 1 intentional ignored
integrations: 42/42
strict Clippy -D warnings: green
rustfmt/diff: green
Alejandra: green
Markdown: green
```

Externally SIGKILLed combined Cargo invocations were re-run serially; they are host
noise, not source regressions.

---

## 3. Read order for the next agent

Read these in order before changing design or implementation:

1. `/home/pvl/spaces/abird/src/aw/AGENTS.md`
2. **this handoff**
3. `/home/pvl/spaces/abird/src/aw/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md`
4. `/home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md`
5. `/home/pvl/spaces/abird/src/aw/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
6. `/home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md`
7. `.../DESIGN.md`
8. `.../AUDIT.md`
9. `.../TODO.md`
10. `/home/pvl/spaces/abird/src/dotlink/AGENTS.md`
11. `/home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md`
12. dotlink 017 plan README/TODO/source-cutover/equivalence docs
13. live source/tests for every owner being touched.

Do **not** inspect AW `.agents/plans/ux/**`.

For historical 0.17 chronology, the entire prior canonical handoff is embedded at the
end of this file.

---

# PART I — DL-200 DESIGN EVOLUTION AND CURRENT NORTH STAR

## 4. Why DL-200 exists

The 0.17/AW-v36 implementation work exposed a mature shared runtime, but subsequent
design review showed that the **model-facing and host-facing APIs could be simplified
substantially**.

The discussion started from the existing maximum model-facing toolset and the presence
of SDK-only tools such as:

```text
queue_enqueue
queue_update
queue_delete
agent_pause_after_turn
process_close_stdin
process_terminate
agent / queue / process observer tools
```

The goal became:

> keep the strong authority, replay, CAS and no-retry invariants behind those tools,
> but eliminate duplicate model vocabulary by moving the semantics into the correct
> owners and a simpler Jobs/Agent/Event surface.

The design evolved iteratively; the final/current decisions below supersede intermediate
variants.

---

## 5. Final model-facing Jobs API

DL-200 Jobs is deliberately only **six** tools:

```text
jobs_list
jobs_get
jobs_read
jobs_send
jobs_ack
jobs_cancel
```

Durable event subscription is generic `events_subscribe`, not a Jobs operation.

There is no public:

```text
jobs_control
jobs_subscribe
pending mode
queue mode
steer mode
interrupt mode
pause_after_turn
queue CRUD
process_close_stdin
process_terminate
agent/queue/process observer tools
```

### 5.1 `jobs_list`

```text
jobs_list(
    ref   = ".",
    scope = "children" | "tree" | "turns",
    limit?,
    cursor?
)
```

Defaults:

```text
ref   = "."
scope = "children"
```

Natural direct children:

- Agent -> child Agents + Processes + Graphs
- Turn -> work spawned by that Turn
- Graph -> Nodes
- Process -> empty

Turns are intentionally not mixed into the default Agent work list.

`scope="tree"` recursively traverses the natural work tree.

`scope="turns"` explicitly lists an Agent timeline.

Do not add initial `kind` or `state` filters. Rows carry `kind` and `state`
facts; callers can filter bounded pages locally.

### 5.2 `jobs_get`

```text
jobs_get(
    ref=".",
    rev?,
    wait_ms?,
    tail?
)
```

Compact status/read/wait convenience.

`rev + wait_ms` remains a useful model-facing shorthand for "I already observed this
owner version; wait until it changes and reread." It should ultimately use the shared
Events/wake substrate internally.

### 5.3 `jobs_read`

Detailed/paged natural read surface.

Typical views:

- Agent/Turn: result/messages/tool_calls/tool_outputs/trace
- Process: stdout/stderr/result
- Graph: aggregate result
- Graph Node: natural output/result

### 5.4 `jobs_send`

```text
jobs_send(
    ref=".",
    input={text} | {eof:true}
)
```

Semantics:

- Agent -> ordinary directed Agent input;
- Process -> stdin/EOF;
- Turn/Graph/Node -> reject.

The model does **not** choose queue/steer/interrupt/pending.

Agent scheduling/admission belongs to the host/Actor owner.

### 5.5 `jobs_ack`

Explicit exact target required.

Retained finite-output ACK remains separate from observation and uses the existing
raw-byte delivery authority, not generic Agent `rev`.

### 5.6 `jobs_cancel`

Explicit exact target required.

Ordinary model-facing cancel does not require a generic revision CAS; exact identity,
authoritative current state and replay/idempotence own correctness.

---

## 6. Revision model

Keep revisions simple and owner-scoped.

```text
Agent       rev
Turn        uses Agent rev

Process     rev

Graph       rev
Graph Node  uses Graph rev

Agent input plan  inputs_rev
Group             rev
```

`rev` is:

- a public-state change token;
- a cache/watch baseline;
- an optional host optimistic-concurrency fence.

It is not:

- immutable history identity;
- authorization;
- event revision;
- replay identity;
- globally comparable.

Agent `rev` should advance once per logical mutation that changes public/control state:

- lifecycle;
- active Turn creation/state/settlement;
- committed input custody/admission;
- steer/interrupt acceptance;
- Agent ACL/config changes;
- Agent-owned subscription change;
- work-tree membership attach/retire.

Do not bump Agent rev for:

- reads;
- child Process stdout growth;
- Graph-node internal progress;
- cursor reads;
- host-only staged input reorder;
- unrelated legacy store metadata.

The current Session-audit-derived revision is a bridge only. DL-200 wants a true
owner-local Agent revision.

---

## 7. Host Agent input plan

Model-facing `jobs_send` is deliberately simple. Rich operator control belongs to a
host-owned editable input plan.

Only five host operations:

```text
agent.inputs.list
agent.inputs.add
agent.inputs.edit
agent.inputs.delete
agent.inputs.send
```

### Staged intents

```text
followup(after = optional exact Turn)
steer(turn = exact active Turn)
interrupt(turn = exact active Turn)
```

The target Turn is captured exactly. A staged steer/interrupt never silently retargets a
later Turn.

### Staging boundary

Before `agent.inputs.send`:

- editable;
- reorderable;
- deletable;
- not yet runtime/effect state.

After Send:

- immutable runtime custody;
- host scheduler owns admission/steering/interruption;
- correction is a new input, never history rewrite.

### API

```text
agent.inputs.list(ref=".")

agent.inputs.add(
    ref=".",
    items=[...],
    rev?
)

agent.inputs.edit(
    ref=".",
    id,
    text?,
    intent?,
    before? | after?,
    rev
)

agent.inputs.delete(
    ref=".",
    ids=[...],
    rev
)

agent.inputs.send(
    ref=".",
    ids?,   // omitted = all staged
    rev
)
```

One coarse `inputs_rev` protects the staged plan; no per-item revision initially.

Interrupt means:

> stop/redirection of the current Turn at the earliest **safe factual authority
> boundary**, suppressing not-yet-started work where possible while already-started
> paid/external/effectful work still reaches a factual receipt.

It does **not** mean "kill every tool immediately."

---

## 8. Checkpoints

A checkpoint is an immutable **cross-domain anchor**:

```text
Checkpoint
├── immutable Agent HistoryBase/head
└── optional immutable source snapshot
```

It is never identified by `rev`.

Host API:

```text
checkpoint.list
checkpoint.get
checkpoint.create
checkpoint.restore
checkpoint.delete
```

### Create

```text
checkpoint.create(
    ref=".",
    source="auto" | "none",
    note?,
    rev?
)
```

`rev` is only an optional capture fence.

### Git source anchor

Clean workspace:

```text
source = exact Git commit OID
```

Dirty workspace:

- use a private temporary Git index;
- write blobs/tree;
- create a synthetic commit object parented at the real base;
- do not move user's HEAD/branch/index;
- pin with an AW private ref such as
  `refs/aw/checkpoints/<id>` so Git GC cannot prune it.

The AW checkpoint record remains authoritative.

### Restore

```text
checkpoint.restore(
    id,
    project=<Project ref>,
    title?,
    task?,
    access="private" | "inherit" | [{principal,permissions}, ...],
    cache="prefer" | "require" | "off"
)
```

Restore is **non-destructive**.

It never rewinds an existing Agent, Git branch or worktree in place.

It creates a new Agent from the checkpoint's exact immutable history/source anchors,
using the same lower HistoryFork mechanism as `agent.fork`.

For a dirty synthetic Git snapshot, reconstruct a new isolated worktree so the normal
user branch/base remains the original base while the worktree matches the checkpoint
tree. The hidden synthetic commit is storage, not automatically user-visible history.

---

## 9. HistoryBase, forks and cache-friendly subagents

Committed history is immutable and structurally shareable.

A fork is:

```text
HistoryBase H
    +
new Agent-local immutable divergent tail
```

Do **not** copy all prior Turns.

A fork never copies:

- source Agent rev;
- active Turn state;
- staged input plan;
- committed-but-not-historical input;
- Process/Graph runtimes;
- ephemeral watches.

### Exact fork boundaries

Model/host fork source may be:

- Agent ref -> latest committed immutable head;
- settled Turn ref -> exact boundary after that Turn;
- checkpoint -> exact history/source anchors;
- HistoryBase ID -> exact immutable prefix.

An unsettled Turn is not an exact fork boundary.

Inherited Turn ordinals are preserved. If the base contains `t:1..t:N`, the new
Agent's first divergent Turn is `t:N+1`.

### Cache prefix

Logical correctness never depends on provider caching.

Use a transport-neutral content identity:

```text
PrefixContentKey =
    hash(
        exact serialized model-visible prefix bytes,
        serialization version
    )
```

Backend compatibility may additionally include:

```text
model/profile
provider/cache domain
tool/request-prefix profile
```

Do **not** include in PrefixContentKey:

- Agent ref;
- ACL/principal list;
- internal backing IDs;
- access generations;
- Agent rev.

Security metadata must not perturb the model prefix unless explicitly model-visible.

### Seed-Turn fanout

First-class optimization target:

```text
new Agent
  t:1 common initialization
       │
       ▼
  HistoryBase H1
  PrefixContentKey P1
       │
       ├── fork -> t:2 security role
       ├── fork -> t:2 performance role
       └── fork -> t:2 API role
```

The role pivot should be the first divergent input, not a rewritten system prompt when
cache compatibility is desired.

Local inference may share/refcount KV pages copy-on-write.

Remote providers at least receive exact compatible prefix serialization, maximizing
their own prompt-cache opportunities.

No `fork_many` API is needed; repeated/batched forks naturally share the same base/key.

---

## 10. Historical attribution across forks

Exact forks preserve inherited identity, content and provenance.

Hard invariant:

```text
fork(F).identity   == F.identity
fork(F).content    == F.content
fork(F).provenance == F.provenance
```

A private fork does **not** anonymize/re-author the past.

Historical provenance should point to the original canonical Agent ref and Turn, e.g.:

```text
HistoryProvenance {
    author
    original_agent: AgentRef
    original_turn
    created_at
    source?
}
```

A new Agent's inherited `t:7` remains authored by the original Agent/user. New divergent
Turns are attributed to the fork Agent.

If content or attribution must be redacted/anonymized, that is a derived
**projection/import** producing a new HistoryBase and potentially a new PrefixContentKey,
not an exact fork.

Full provenance may remain out-of-band from the model-visible prompt; UI/audit can
retain rich attribution without breaking cache reuse across differently secured forks.

---

## 11. Public Session disappears; Agent is the public envelope

DL-200 public identity is:

```text
/d:<domain>/i:<principal>/p:<project>/a:<agent>
```

Publicly:

> **Agent = security/collaboration envelope + durable history continuation + live
> execution owner.**

There is no public Session coordinate and no `session.*` API.

Internally AW may keep `session_id`, branch rows and SessionStore/journal tables as
persistence/migration machinery. Those are implementation details and need not be
renamed immediately.

Public receipts/provenance should use canonical Agent refs.

### Principal namespace

`i:<principal>` is a principal namespace, not specifically a user.

A principal may be a user or explicit group.

A group-owned Agent naturally lives under the group's Project:

```text
/d:acme/i:compiler-team/p:runtime/a:reviewer
```

An Alice-owned Agent:

```text
/d:acme/i:alice/p:runtime/a:reviewer
```

Creation/fork APIs therefore choose a destination **Project ref**, not a redundant
`owner=` field. The Project's `i:` principal is the canonical owner.

---

## 12. Agent ACL and groups

Agent access is:

```text
owner = canonical i:<principal>

access:
    PrincipalRef -> PermissionSet
```

Principal:

```text
user:<id>
group:<id>
```

Groups are explicit reusable user sets; do not create implicit groups for ad-hoc lists.

Keep groups simple in v1:

```text
Group {
    principal
    owner_user
    members: Set<User>
    rev
}
```

No nested groups initially.

An Agent may be group-owned. That means every active group member has Agent-owner
authority; use only for genuine team co-ownership.

For ordinary team access, use a user-owned Agent with a group ACL grant.

Permissions:

```text
view
contribute
submit
control
derive
share
manage
```

Allow-only effective permissions:

```text
owner rights
UNION direct user grant
UNION every matching group grant
```

No deny entries in v1.

`derive` permits forks that do not broaden disclosure.

`share` permits disclosure to new principals subject to inherited dependency policy.

Group membership changes that alter effective Agent access advance the Agent's observable
access state and emit a `changed` Event.

Host group API stays small:

```text
group.list
group.get
group.create
group.edit
group.delete
```

`group.edit(members=[...], rev)` replaces separate member add/remove verbs.

---

## 13. Agent create/fork/access API

Host Agent surface:

```text
agent.list
agent.get
agent.create
agent.fork
agent.close
agent.admission
agent.access.list
agent.access.set
```

plus `agent.inputs.*`.

### Create

```text
agent.create(
    project=<Project ref>,
    title?,
    access="private" | [{principal,permissions}, ...],
    source?,
    input?
)
```

Creates a fresh Agent with no inherited HistoryBase.

### Model constructor

```text
agent(
    task,
    title?,
    background=false,
    fork?=<Agent ref | settled Turn ref>
)
```

The model never chooses destination Project/owner/ACL/source authority/cache domain.

Host/delegation policy chooses the destination profile.

### Fork

```text
agent.fork(
    from=<Agent | settled Turn | checkpoint | HistoryBase>,
    project=<Project ref>,
    title?,
    task?,
    access="private" | "inherit" | [{principal,permissions}, ...],
    source="auto" | "history-only",
    cache="prefer" | "require" | "off",
    rev?
)
```

`private` -> destination owner only.

`inherit` -> copy source Agent ACL principal entries into fresh destination ACL entries.
Group principals stay live group principals; direct ACL edits diverge independently.

Explicit list -> exact destination ACL.

The destination Project's `i:` principal owns the new Agent.

There is no public `session.fork`.

### ACL

```text
agent.access.list(ref=".")

agent.access.set(
    ref,
    principal,
    permissions=[...] | null,
    rev
)
```

Array creates/replaces a complete grant; null removes it.

The canonical owner `i:` is not changed by ACL edits.

---

## 14. Global namespace

One canonical hierarchy:

```text
/d:<domain>
  /i:<principal>
    /p:<project>
      /a:<agent>
        /t:<turn>
          /j:<job>
            /n:<node>
```

Concrete execution/control refs remain slash-delimited.

Intermediate Domain/Principal/Project refs may be used by host/event observation.

Child-Agent delegation is a typed relationship, not necessarily path nesting. A child
Agent can be a sibling under the chosen Project while `^` and Jobs lineage preserve
parent/child semantics.

---

## 15. Events: one concept, NATS-style subject projection

DL-200 collapses prior `signals` vs `events` split.

There is only:

> **Event**

Some Event names are reserved/system-emitted:

```text
changed
result
settled
attention
```

Agents may publish bounded custom Event names:

```text
ready
tests_passed
review_requested
candidate_found
phase_complete
```

Agents cannot forge reserved system names.

### Subject projection

Concrete resource identity stays slash-delimited.

Event routing projects the same typed namespace into NATS-style dot tokens and appends
the Event token:

```text
d:<domain>.i:<principal>.p:<project>.a:<agent>[.t:<turn>][.j:<job>][.n:<node>].e:<event>
```

Examples:

```text
d:acme.i:alice.p:core.a:2.e:changed
d:acme.i:alice.p:core.a:2.e:ready
d:acme.i:alice.p:core.a:2.t:7.e:settled
d:acme.i:alice.p:core.a:2.t:7.j:test.e:result
```

NATS wildcard semantics:

```text
*    exactly one dot-delimited token
>    one or more trailing tokens; terminal only
```

Because every Event has trailing `e:<event>`, `a:2.>` includes Agent 2's own Events
as well as Turn/Job/Node descendant Event subjects.

Examples:

```text
a:2.>
a:*.e:ready
a:*.>
p:*.a:*.e:ready
p:*.a:*.>
i:*.p:*.a:*.>
d:acme.i:*.p:*.a:*.>
d:*.i:*.p:*.a:*.>
```

Patterns grant no access.

For every concrete matched Event:

- authorize actual source;
- authorize disclosure;
- authorize recipient/read;
- then deliver/project.

Wildcard subscriptions are dynamic and include future matching resources.

Do not expand a wildcard subscription into one stored rule per current source.

The cursor/stream projection must not leak hidden source counts or unauthorized gaps.

---

## 16. Events API

### Host observation

```text
events.read(
    subject,
    after="now" | cursor,
    limit?,
    wait_ms?
)
```

The subject may be concrete or wildcard.

`after="now"` atomically captures the authorized projected stream tail.

Reusing the returned opaque cursor catches up without a poll race.

If cursor retention expires, return `cursor_expired`; caller rereads authoritative
snapshots and resumes from now.

Events are factual/invalidation hints. Reread authoritative mutable state before acting.

### Host durable subscription

```text
events.subscribe(
    subject,
    to=<AgentRef>,
    enabled=true
)
```

`enabled=false` removes that exact recipient+pattern subscription idempotently.

No queue/steer/interrupt mode.

The recipient Agent input owner decides later scheduling.

Durable subscriptions are:

- restart-safe;
- future-only from committed subscription baseline;
- subscription-generation/revisioned;
- retry-safe using immutable Event delivery identity;
- source disclosure revalidated per Event;
- recipient authority revalidated at delivery;
- bounded by pattern/subscription/event/delivery budgets.

### Model subscription

```text
events_subscribe(
    subject,
    enabled=true
)
```

Recipient is implicitly current Agent.

### Host publish

```text
events.publish(
    ref=<exact Agent>,
    event,
    data?
)
```

Custom publish source must be one exact Agent.

### Model publish

```text
events_publish(
    event,
    data?
)
```

Source is implicitly current Agent.

No topic/channel creation lifecycle.

Custom payload is small bounded JSON metadata; large substantive results remain in
Jobs/history and can be referenced.

Communication split:

```text
jobs_send         directed substantive input
events_publish    source-scoped factual Event
events_subscribe  decoupled reaction/fanout
events.read       host/SDK observation/catch-up
```

Self-delivery is suppressed in v1, including matches through broad wildcards.
Cross-Agent cycles are bounded by the shared Event causal hop/delivery budgets.

Cross-Agent/Project/Domain delivery is an explicit disclosure path and cannot bypass
exact source, recipient or Domain/federation authority.

### Shared implementation

Use the existing immutable `commit_log` as the one Event history/order owner. Split the
current SourceEventBook responsibilities into source-local Event revision plus
recipient-owned pattern subscriptions/cursors, and deliver through deterministic
replay-safe Agent input admission.

Do not add a second durable Event log or copied per-subscriber backlog.

Local `tokio::watch` may remain only as an ephemeral wake accelerator.

---

## 17. Current DL-200 architectural invariants

1. Model-facing Jobs remain exactly six tools.
2. Durable subscription is generic `events_subscribe`, not `jobs_subscribe`.
3. System and Agent custom notifications are one Event concept.
4. Events route over a NATS-style projection of the canonical resource namespace.
5. Event patterns are typed separately from concrete execution refs and can never reach
   mutation APIs.
6. Agent is the public security/history/execution envelope; there is no public Session
   resource or `session.*` API.
7. Internal SessionStore/session_id may remain as persistence/migration machinery.
8. Exact forks structurally share immutable history; they do not copy prior Turns.
9. Exact forks preserve inherited identity/content/provenance.
10. Redaction/anonymization creates a new projected HistoryBase.
11. Agent access uses one allow-only ACL over user/group principals.
12. Groups are explicit reusable user sets; no implicit groups and no nesting in v1.
13. Group-owned Agents are explicit team co-ownership.
14. ACL/security metadata stays outside PrefixContentKey unless model-visible.
15. Cache reuse is optimization, never authority.
16. Agent forks mint fresh Agent ACL/security/runtime state.
17. Active Turns, finite runtimes, watches and staged inputs are never copied to a fork.
18. Checkpoint identity is immutable history + immutable source, never `rev`.
19. Checkpoint restore is non-destructive and creates a new Agent.
20. Settled Turn boundaries are reusable fork/cache seed points.
21. Fanout needs no special API.
22. Recursive subagents use the same fork primitive and delegation limits.
23. `jobs_send` is directed input; Events are coordination/fanout, not bulk transport.
24. One lower Agent/history/Event owner model serves model APIs, host APIs and recursive
   delegation.

---

## 18. DL-200 resolved design checkpoint before seal

All pre-implementation design questions are resolved in the current
`dl200-unified-jobs-agent-events-2026-11-20/SPEC.md`. The design remains unsealed.

The closed decisions are:

1. immutable `HistorySpan/HistoryBase/HistoryGrant` links over the existing
   Session/journal recovery owner, with lazy legacy sealing and reachability retention;
2. persisted owner-local Agent `rev`; direct ACL edits bump Agent rev, referenced group
   membership changes only group rev + derived `access_generation`;
3. Git-native clean/dirty SourceAnchors, private-index synthetic commits, isolated
   restore, fail-closed filters/partial clones and explicit non-Git history-only fallback;
4. model fork default = source Project + inherited source ACL, with trusted-host policy
   allowed to narrow but never silently broaden;
5. one-time migration seed from current Session-audit revision into
   `agent_state_current.rev`, after which audit is evidence only;
6. existing immutable `commit_log` as the one Event history/order owner, with
   source-local Event revision and recipient-owned pattern subscriptions;
7. concrete Event grammar/payload/read/subscription/causal bounds plus exact
   per-resource authorization and no separate EventLink service;
8. opaque caller+pattern Event cursors, bounded replay retention and explicit
   `cursor_expired` / subscription `lagged`;
9. legacy observer/queue/process/pause/control spellings converged onto canonical
   Jobs/Events/Agent-input/admission surfaces without weakening lower safety facts;
10. minimal Domain/Principal/Project `list/get` metadata reads, with Project remaining
    a principal-scoped view over existing Workspace backing;
11. one Domain principal-handle registry across user/group kinds, with kind as metadata.

The later fresh full pre-seal architecture audit recorded at the top of this handoff and
in the canonical SPEC/TODO completed clean. The only remaining design gate is explicit
user approval to seal. Seal authorizes writing an
implementation plan, not implementation itself.

---

# PART II — NEXT AGENT WORKFLOW

## 19. If continuing DL-200 design

Do not touch implementation first.

Read the complete current DL-200 spec and TODO. The 2026-11-20 end-to-end design review
was clean; re-run these checks only if new design edits or a concrete contradiction
appears, rather than reopening the already-closed questions by default:

1. one authoritative owner per mutable concept and no duplicate Event/history/Project
   service;
2. revision-domain separation: Agent `rev`, `inputs_rev`, group/security generations,
   Event `event_rev` and immutable history identity never substitute for one another;
3. HistoryBase/HistoryGrant disclosure, attribution, cache identity and legacy Session
   recovery remain composable;
4. wildcard Event patterns never grant authority or enter mutation APIs, and cursors /
   durable subscriptions do not leak hidden counts or replay unauthorized intervals;
5. checkpoint/live-fork source semantics remain factual and fail closed at unsafe
   boundaries;
6. the final model/host API surfaces are minimal, orthogonal and free of legacy aliases.

The naming/semantic, typed ExtensionRegistry/API, dynamic-host/minimality, fresh full
pre-seal architecture and 0.17.6 Process-settlement reviews are clean. DL-200 is sealed and
`IMPLEMENTATION.md` is prepared. Do not edit implementation source until the separate
explicit implementation-promotion gate is satisfied.

---

## 20. If implementation work is explicitly promoted

Before editing:

```bash
cd /home/pvl/spaces/abird/src/aw
git status --short
git diff --cached --name-status
git rev-parse HEAD
git rev-parse origin/main
git diff --check

cd /home/pvl/spaces/abird/src/dotlink
git status --short
git diff --cached --name-status
git rev-parse HEAD
git rev-parse origin/main
git diff --check
```

Expected current published heads at this handoff:

```text
AW      f035a00031b8f5879f4d77f3fd14bfe02b89305c
dotlink bbd29c13904b7255eb28e2ed6c776c3590ac122a
```

The DL-200 plan is currently untracked design material and should be preserved.

Keep each implementation turn bounded to one coherent owner-focused checkpoint and run
batched qualification rather than over-granular test loops.

---

## 21. Things never to regress

Do not:

- invent physical Process signals from semantic timeout/cancel;
- merge `NoTarget` with `Unknown`;
- erase terminal facts after later infrastructure failure;
- restore dotlink-private Guard/stop models;
- parse arbitrary rendered receipt prose as trusted identity;
- mutate raw offset/EOF/ACK truth while fitting response envelopes;
- turn typed MCP facts into text;
- mint new replay timestamps;
- let progress backpressure delay operation completion;
- pull full `aw-mcp` into dotlink;
- implement frozen sandbox/config-v13 work without explicit promotion;
- make cache identity into authority;
- make `rev` immutable-history identity;
- copy active Turn/runtime state into a fork;
- rewrite attribution in an exact fork;
- let Event wildcard patterns enter mutation APIs;
- let wildcard subscriptions grant visibility;
- use Events as an unbounded bulk message transport;
- create a second Event bus/runtime alongside the existing source-event machinery;
- expose legacy internal Session IDs as DL-200 public identity.

---

## 22. Copy-ready next-agent starting prompt

The short prompt is also stored separately in
`.agents/docs/AW-DL200-NEXT-AGENT-2026-11-20.md`.

Use:

```text
Use dotlink---l5 only.

Continue in:
  AW: /home/pvl/spaces/abird/src/aw
  dotlink: /home/pvl/spaces/abird/src/dotlink
  read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve unrelated work.
Do not inspect/touch AW .agents/plans/ux/**.
Sandbox/mount/config-v13 remains frozen unless explicitly promoted.

Read first:
1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
4. AW/.agents/docs/CURRENT_HANDOFF.md
5. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
6. AW 017 shared-runtime README/DESIGN/AUDIT/TODO
7. dotlink/AGENTS.md
8. dotlink/.agents/docs/CURRENT_HANDOFF.md
9. live source/tests for every owner touched

Current published heads:
  AW      f035a00031b8f5879f4d77f3fd14bfe02b89305c
  dotlink bbd29c13904b7255eb28e2ed6c776c3590ac122a

All 0.17 D0-D7/shared-runtime/process-control/Remote-Start issuer work is
published and qualified. Do not reopen it without a real regression.

The active work is DL-200 DESIGN ONLY unless I explicitly promote implementation.

DL-200 current north star:
- public Agent is the security/history/execution envelope; no public AW Session API;
- canonical Project tree has Agent execution and Sandbox sibling branches:
    /d:/i:<principal>/p:/a:/t:/j:/n:
    /d:/i:<principal>/p:/s:<sandbox>;
- i: is a user-or-group principal namespace;
- exact forks structurally share immutable HistoryBase and preserve attribution;
- one host constructor `agent.create(from=...)`; model uses `agent(..., from=...)`;
- model-facing Jobs remains exactly:
    jobs_list/jobs_get/jobs_read/jobs_send/jobs_ack/jobs_cancel;
- Agent scheduling/queue/steer/interrupt is host-owned;
- host staged input API is list/add/edit/delete/send;
- checkpoint is exact immutable history + plural named typed SourceSet;
- checkpoint.restore is non-destructive and creates a new Agent + Sandbox with every
  captured source restored;
- Sandbox ancestry is independent from history ancestry; share/branch/fresh are
  host/delegation choices and same Sandbox never implies same authority;
- Vault grants are Agent-sensitive provider-backed capability refs and never inherit from
  history/Sandbox/checkpoint/ACL state;
- Events are one concept, not signal+event;
- bare Event names are system-owned and custom Events are `#name`, e.g. `e:#ready`;
- model events:
    events_subscribe(subject, enabled=true)
    events_publish(event="#...", data?)
- host events:
    events.read(subject, after=now|cursor, ...)
    events.subscribe(subject, to=AgentRef, enabled=true)
    events.publish(ref=exact Agent, event="#...", data?)
    events.subscriptions.list(to=AgentRef, ...)
- wildcard Events never grant access or enter mutation APIs;
- immutable commit_log is the one Event history/order owner; durable EventHandlerRegistry
  keeps only private handler positions in AW and uses stable Event.id;
- ViewRegistry owns typed read facets over already-authorized resources, not product data;
- ExtensionRegistry/subregistries are typed views over existing authoritative owners, not
  a second runtime/authority/database;
- Histate product Session/Decision/summary/graph state stays product-owned;
- rev is owner-local mutable Agent state, not history identity or security authority;
- Project remains a principal-scoped view over existing Workspace backing;
- PrefixContentKey excludes Agent/ACL/security identity unless model-visible.

The broad/Pi-parity/naming reviews and the focused typed-registry/API review are clean.
The design is sealed and the phased implementation plan exists. Do not begin implementation
until the separate explicit user promotion.

Keep turns around 30 minutes and stop at the next coherent checkpoint.
Do not commit/push/tag unless explicitly asked.
```

---

# PART III — PRESERVED HISTORICAL CHRONOLOGY

The complete previous canonical AW+dotlink comprehensive handoff follows **verbatim**.
It is preserved so every prior 0.17 design/implementation/review decision remains
available in one file.

Where it conflicts with PART I/PART II above, the newer current-state/DL-200 sections
above are authoritative.

---

# AW + dotlink full comprehensive handoff — 2026-11-20

Author: coordinating implementation agent
Owner: @pvl
Source authority host: **L5 only**

AW source: /home/pvl/spaces/abird/src/aw
dotlink source: /home/pvl/spaces/abird/src/dotlink
Read-only release reference: /home/pvl/spaces/abird/src/dotlink-017-mounts

This is the canonical cross-repo handoff for the AW + dotlink execution, Jobs,
Agent, replay, admission and 0.17.x convergence workstream.

It supersedes all earlier handoffs for current state, Git refs, pending work and
next-agent instructions. The complete 2026-11-19 handoff is embedded verbatim
later in this file as historical chronology so no design lineage or prior work
item is lost.

When facts conflict, use this authority order:

1. direct user instruction in the current conversation;
2. live source and live tests on L5;
3. active owning plan README / DESIGN / AUDIT / TODO;
4. the current-state sections at the top of this file;
5. the inherited historical handoff embedded later;
6. older dated handoffs and release implementations.

Portable semantics belong in AW. Dotlink should remain the concrete
host/transport/deployment consumer and must not grow parallel shared runtimes.

---

## 0. Non-negotiable operating rules

- Use **dotlink---l5 only**.
- Do not use A1.
- Do not create another worktree.
- Preserve unrelated dirty/staged work exactly.
- Work primarily in AW and dotlink paths above.
- Treat dotlink-017-mounts as read-only behavioral reference.
- **Do not inspect or touch AW .agents/plans/ux/**.
- Keep implementation turns around 30 minutes and stop at coherent checkpoints.
- Do not commit, push, tag, release, deploy or publish unless explicitly
  authorized.
- Signed history is required; do not disable commit signing.
- Revalidate refs/status/staging before each implementation phase.
- Live source/tests outrank prose.
- Keep reusable factual semantics in AW.
- Keep transport, product wording, logging, deployment and release ownership in
  dotlink.
- Prefer zero-cost / monomorphized / Copy / transparent-newtype shared values on
  hot paths.
- Prefer clean/simple ownership over speculative abstraction.
- Do not add a second JobManager, PolicyStore, event service, scheduler,
  process Guard, result fitter, progress scheduler or replay ledger.
- Do not resurrect old job_* APIs or separate foreground/background engines.
- **Sandbox/mount/config-v13 remains frozen** until explicitly promoted.

---

## 1. Exact current Git and release state

### AW

The reusable shared **source pin** consumed by dotlink is signed and published:

~~~text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
Complete v36 runtime admission and process semantics
~~~

That source commit contains the complete v36 admission/source/query-index batch,
issuer-bearing Remote Start wire, and corrected shared process-status projection.
This document reconciliation is docs-only on top of that reusable source pin;
dotlink intentionally stays pinned to `b8a46a94...`, not to later AW docs-only
commits.

Preserve unrelated AW working-tree material exactly. In particular, do not
inspect/absorb `.agents/plans/ux/**` or the pre-existing untracked
`.agents/plans/dl150-jobs-host-input-extension-2026-11-20/` tree.

### dotlink

Published dotlink main:

~~~text
source checkpoint 648519add3d174b9e13de81aeca62431591dac6d
                  Repin AW and complete process-control hardening

docs/main tip     bbd29c13904b7255eb28e2ed6c776c3590ac122a
                  Record AW repin and Remote Start trust cutover
~~~

Every direct/transitive AW Git package in dotlink `Cargo.lock` resolves exactly
to:

~~~text
b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b
~~~

No path override or registry package/version drift was introduced.

The published source includes the post-equivalence process/control hardening,
direct consumption of shared `ProcessTerminal::status()`, and target-side trusted
AW Remote Start issuer enforcement with monotonic per-client OAuth trust epochs.

### Release reference

The read-only release worktree is clean and was not modified in this workstream.
Its local branch has advanced independently beyond the published release
baseline:

~~~text
local release/0.17  a2be5098e990f78656db746e10f5652a494347c2
ahead/behind origin 28 / 0

origin/release/0.17 3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.5             3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.4             fd1cdb6429a341a4e3d27805cae0bec288fc26df
~~~

Treat local `release/0.17` as a **read-only behavioral reference** and
`origin/release/0.17` / `v0.17.5` as the published compatibility baseline.

---

## 2. Read order for the next agent

1. AW AGENTS.md
2. this file
3. AW .agents/docs/CURRENT_HANDOFF.md
4. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
5. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
6. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
7. AW .agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
8. AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
9. dotlink AGENTS.md
10. dotlink .agents/docs/CURRENT_HANDOFF.md
11. dotlink 017 README
12. dotlink 017 TODO
13. dotlink D4-D7-SOURCE-CUTOVER-MAP.md
14. dotlink V0174-EQUIVALENCE.md
15. dotlink DL-060 TODO
16. live source/tests for every owner being touched.

Do not read AW .agents/plans/ux/**.

---

## 3. Current architectural north star

Five ownership rules explain nearly every completed and pending correction:

~~~text
Replay identity
    = immutable semantic invocation identity only

Authority
    = checked at new-effect linearization boundaries

Durable state
    = indexed current projection + immutable audit history

Process lifecycle
    = one waited command owns one managed process-tree lifetime

Observation
    = never grants effect / replay / ACK / authority by itself
~~~

Corollaries:

- recover immutable replay before mutable current-authority checks;
- current authority may block a new effect but must not erase historical fact;
- audit history is history, not the operational replay database;
- source events are owned by the source and receiver wake authority is
  independently revalidated;
- jobs_get/tail/status are observation only;
- jobs_read delivery proof is raw-byte authority and later response decoration
  must not change it;
- reasonless cleanup is housekeeping, not terminal semantic stop;
- rendered prose is never trusted semantic identity.

---

## 4. Completed 0.17 shared-runtime architecture

The historical body later in this file contains full chronology. Current compact
state:

### Physical process facts

~~~rust
ProcessExit::{Code(i32), Signal(i32), Unknown}

StopReason::{Timeout, Cancelled, RuntimeError}
StopEvidence::{Stopped, NoTarget, Unknown}
StopRecord { reason: Option<StopReason>, evidence }
~~~

Only Some(reason)+Stopped becomes terminal semantic stop.

NoTarget and Unknown remain distinct.

Reasonless StopRecord is housekeeping and never becomes stopped_by.

### Shared process containment

AW ManagedProcessGuard owns reusable containment semantics.

Dotlink does not own a parallel stop model.

### Task terminal detail

Typed TaskTerminalDetail preserves exit/stop facts behind later
spool/projection/internal failure.

### Finite Job identity

Shared bounded flat:

~~~text
j:<handle>[/n:<coordinate>]
~~~

uses shared JobRef.

Hierarchical ExecutionRef remains ownership/Agent-routing identity.

### Receipt grammar

AW owns shared typed finite receipts and compact factual renderers.

Dotlink must never parse rendered receipt prose to recover semantic identity.

### Raw page vs serialized result

~~~rust
RawPageBytes
SerializedResultBytes
~~~

jobs_read raw offsets/EOF/delivery proof are determined before MCP response
projection.

Only the exact serialized raw prefix receives delivery credit.

### MCP projection

Lightweight aw-mcp-protocol owns complete-result validation, bounded metadata,
stored replay timestamp, visible timestamp sidecar, text-only fitting and the
generic heartbeat wrapper.

Dotlink owns transport ceiling, clock, wire errors and product logging.

### Progress

Heartbeat is advisory, token-gated and unknown-total.

Progress backpressure cannot own operation completion.

---

## 5. Dotlink D4-D7 and review hardening

D4-D7 implementation, 0.17.5 semantic equivalence and later review hardening
are complete and published; latest signed dotlink source is `648519ad...`,
pinned to published AW `b8a46a94...`.

### Canonical replay before projection

Current invariant:

~~~text
effect completes
→ stamp canonical typed result
→ cache canonical replay result
→ derive this request's projection from canonical result
~~~

A narrow first response cannot poison later replay.

### Final decorated-result fit

Jobs fit/preflight decisions measure the final D6-decorated MCP result,
including receipt metadata and visible timestamp.

For irreversible stdin:

~~~text
prove complete timestamped minimal receipt fits
→ only then accept stdin
~~~

### jobs_read ACK safety

D5 raw page text was exhaustively checked through D6 decoration for every
ceiling 1..=1024 where a page can be produced.

The credited page text is byte-identical after decoration.

### jobs_get Process-tail fitting

Published production API:

~~~rust
JobManager::standalone_process_tail_fitted(..., fits)
~~~

Jobs owns factual candidate generation; host supplies zero-cost final-envelope
fit predicate.

Private search:

- returns the exact candidate it proved;
- never re-reads a winner;
- logarithmic fast path for ASCII;
- bounded exact descending fallback for non-ASCII/lossy-UTF8 boundaries;
- bounded by public 1024-byte combined-tail maximum.

The UTF-8 512/513-byte adversarial boundary is regression-tested.

### Recorded 0.17 validation

~~~text
main binary 701 passed / 0 failed / 1 ignored
integrations 42/42
all-target/all-feature check green
strict Clippy green
rustfmt/diff green
Markdown green
real NixOS module gate green
available Linux native gates green
~~~

Do not rerun old equivalence wholesale unless a new change touches that
boundary.

---

## 6. Validated external-review findings

### Resolved and published in AW `b8a46a94...`

#### Policy replay-before-authority

Replay now precedes mutable Turn/writer/participant evidence checks.

Immutable invocation/subject identity determines replay.

New effects still require current authority.

#### Policy/Remote Start audit lifetime caps

Operational state no longer scans lifetime audit history.

v36 current projections:

~~~text
policy_admission_current
remote_start_current
~~~

Audit remains immutable history.

Per-Turn safety caps remain as indexed counts.

#### Shared-session participant policy routing

Session resource owner and initiating participant are distinct.

PolicyAdmissionSubject captures initiating actor plus membership/audience epochs.

Production routing uses:

~~~text
Session owner -> resource/writer authority
participant   -> current Submit + exact tool/payer authority
~~~

No privilege union.

Alice-owner/Bob-initiator regression covers Bob's own grant, tool revocation and
Session participation revocation.

#### Agent-source jobs_subscribe

The hard-disable is removed.

Durable Agent/Turn source ownership reuses shared SourceEventBook.

Agent root emits result per newly attached Turn and settled on actual close.

Exact Turn emits result+settled at one source checkpoint.

Attention remains fail-closed until a real attention-capable source exists.

#### Agent jobs_get polling

The specific Agent Job 10 ms polling loop is removed.

A bounded per-Agent watch is now a wake hint; after wake, current participant
View authority is revalidated and the authoritative Store is reread.

#### Restart-safe wake admission

Source subscriptions/events and receiver wake admissions are durable.

A fresh Router can reconstruct the opaque wake receipt only when current
participant epochs and exact tool/capability authority still match.

Revoke/regrant does not revive stale refs.

### Cross-repo completion

Remote Start trusted issuer and Agent scaling work are complete:

- `RemoteStartIssuer { client_id, trust_epoch }` is the shared issuer identity;
- dotlink ordinary OAuth and AW-bridge trust are separate capabilities;
- only an explicitly trusted bridge client with the current trust epoch may
  create a new Start claim;
- exact existing replay remains recoverable after issuer revocation;
- canonical Agent lookup is indexed and rechecks current Session authority;
- mixed Agent/finite listing is index-backed/keyset-paged under the existing
  cursor contract.

No second Job/Agent registry was added.

---

## 7. Current AW v36 source design

Schema version:

~~~text
LATEST_SCHEMA_VERSION = 36
~~~

Migration:

~~~text
crates/aw-store/migrations/036_runtime_admission_indexes.sql
~~~

New projections:

~~~text
policy_admission_current
remote_start_current
agent_job_event_sources
agent_job_wake_admissions
~~~

### policy_admission_current

Stores stable admission identity, Session/branch/Turn, call id, canonical target,
initiating actor, request digest and current factual admission JSON/revision.

Projection revisions are monotonic:

- create at revision 1;
- exact idempotent same-revision rewrite allowed;
- otherwise only current+1.

### remote_start_current

Keyed by stable semantic attempt key.

Provides O(1) replay and indexed per-Turn capacity.

### agent_job_event_sources

One durable source owner per canonical Agent/Turn ref.

Stores SourceEventBook, baseline/settled state and exact pending deliveries.

Subscriptions baseline current source facts and never retro-fire old Turns.

### agent_job_wake_admissions

Bounded current receiver projection keyed by recipient+actor with unique opaque
authority ref.

Regrant replaces current receipt instead of growing indefinitely.

---

## 8. Agent event ownership and crash semantics

SourceEventBook is serializable and validates persisted generation/rule state.

Turn event commitment happens in the journal attachment transaction:

~~~text
provider result persisted
→ attachment transaction
→ attach settled run
→ Agent root result event
→ exact Turn result+settled event
→ durability event
~~~

Subscribers never wake before public Turn settlement.

Stable Agent root settled is committed in the same transaction as authoritative
Agent/subagent close through settle_agent_job.

Exact JobEventDelivery values remain pending until ACK.

Receiver denial/conflict/limit is terminal suppression for that delivery and
does not rewrite source history.

Storage/protocol failure leaves pending state for later drain.

There is no detached event scheduler.

---

## 9. Current AW validation evidence

### Admission/replay

Focused green:

~~~text
policy_admission_claim_guardian_ask_and_replay_are_single_owner_fenced
completed_guardian_replay_survives_policy_change_without_reauthorizing_effect
policy_operation_router_local_and_delegate_share_one_factual_executor
~~~

### Agent source/event

Focused green:

~~~text
source_event_book_round_trips_without_losing_generations_or_rules
attached_turn_commits_root_and_exact_turn_events_until_exact_ack
jobs_get_wait_wakes_on_authoritative_agent_revision_without_polling
revoked_shared_participant_cannot_receive_committed_wake_or_reuse_history_cursor
~~~

The participant test includes fresh-Router durable wake-admission recovery before
revocation.

Final relevant green checks:

~~~text
cargo check -p aw-store --all-targets --all-features
cargo check -p aw-cli --lib --all-features
cargo check -p aw-cli --bin aw --all-features

cargo clippy -p aw-store --lib --all-features -- -D warnings
cargo clippy -p aw-cli --lib --all-features -- -D warnings
cargo clippy -p aw-cli --bin aw --all-features -- -D warnings

cargo fmt --all -- --check
git diff --check
~~~

L5 intermittently externally SIGKILLs rustc/Cargo without Rust diagnostics.
Incomplete tails were rerun independently and completed green.

---

## 10. Dotlink process/control batch — complete and published

The formerly pending process/control batch is implemented and fully qualified on
L5. The design below is retained as the as-built contract.

### P1: close managed process ownership on ordinary child exit

Former behavior could let inherited background writers hold pipes until the
original command deadline.

As-built design:

~~~text
waited command exits
→ capture physical ExitStatus
→ immediately terminate remaining managed process group / Windows Job Object
   as reasonless housekeeping
→ bounded short post-exit output drain
→ return factual top-level command result
~~~

Do not wait to the original execution deadline before containment cleanup.

If an unsandboxed setsid-escaped writer still owns an FD after grace:

- preserve top-level exit;
- mark output incomplete using existing facts;
- stop drain tasks;
- do not turn a successful command into internal failure solely for escaped FD.

Sandbox namespace teardown remains stronger.

### Lifecycle semantics

Keep current shared model:

~~~text
ProcessExit                       physical waited-command result
StopRecord(reason=Some(...))     semantic command-stop sequence
StopRecord(reason=None, ...)     containment housekeeping
~~~

Reasonless descendant cleanup never projects stopped_by.

Keep one logical lifecycle projection; do not emit both stop and outside-exit
for the same command.

### P2: early non-shared cancel CAS gap

Required invariant:

~~~text
model-visible revision-fenced Job
⇒ canonical shared Job owner exists
~~~

During admission, model `jobs_cancel(rev=...)` is unavailable until shared
attachment exists. The second public local CAS authority was removed rather than
strengthened. A negative regression proves owner verification failure does not
signal the runner.

### P2: stdin authority/effect linearization

Implemented host callback boundary:

~~~rust
standalone_send_process_input_checked(
    ...,
    verify_authority: impl FnOnce(ProcessInputTarget) -> JobResult<()>,
)
~~~

Flow:

~~~text
reserve bounded channel capacity
→ acquire per-Job transition gate
→ reread exact target/input custody
→ synchronously revalidate current jobs_send + interpreter/launcher authority
→ compare-and-bump revision
→ enqueue input
→ release transition gate
→ await runner/write
~~~

No global mutex. No lock across actual ChildStdin write.

### P3: duplicate MCP server instructions

The duplicate static tool-handler instruction copy is removed.
`get_info().server_info.instructions` is the single instruction owner and the
one-occurrence regression is green.

### Qualification

The final L5 host-available gate uses
`AW_BASH="$(command -v bash)"` and is green:

- main binary **706 passed / 0 failed / 1 intentional ignored**;
- integrations **42/42**;
- strict all-target/all-feature Clippy `-D warnings`;
- rustfmt and `git diff --check`;
- Alejandra;
- Markdown **0 issues / 86 files**.

Known physical exit and output coverage are now orthogonal in shared settlement:
dotlink stores the existing typed `ProcessTerminal` through
`ProcessSettlement` and projects output coverage through
`Status.incomplete`. An escaped `setsid` writer therefore cannot turn known
`Code(0)` into `Unknown` merely by retaining an inherited FD past the 250 ms
post-exit grace.

The published AW shared owner has the same rule directly in
`ProcessTerminal::status()`: `Primary` derives only from stop/exit evidence and
`incomplete = !output.complete`. The exact full-feature runner regression is
green, the full `aw-tools --all-features --lib` suite is **359/359**, and strict
all-target/all-feature Clippy is green after one diagnostic-free L5 kill/retry.
Published AW `b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b` carries that
shared projection. Dotlink source `648519add3d174b9e13de81aeca62431591dac6d`
is repinned to it and has removed the temporary `shared_process_status`
compatibility shim.

---

## 11. Remote Start trusted issuer — complete cross-repo

AW-side issuer identity/wire/replay semantics are published in `b8a46a94...`.
Dotlink source `648519ad...` consumes that exact pin and implements target-side
issuer trust in existing private OAuth state.

Final direct bridge design:

~~~text
Remote Start semantic identity
→ existing exact replay? recover fact
→ new claim?
    require trusted RemoteStartIssuer capability
    require current lease/generation/tool/resource authority
    admit effect
~~~

Issuer revocation blocks new Starts but does not erase existing replay.

Add signature/MAC only if future evidence must cross an untrusted intermediary
or be independently verifiable offline.

Qualified trust/replay behavior:

~~~text
unauthenticated lease request                         -> 401
authenticated ordinary OAuth client                   -> 403 lease
trusted AW bridge current epoch                       -> issuer-bearing lease
revoked issuer + genuinely new Start                  -> denied
revoked issuer + exact already-committed Start replay -> original factual replay
re-trusted client                                     -> new epoch; old lease stale
~~~

---

## 12. Agent scaling — complete and published

Completed in published AW source `b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b`:

- durable Agent/Turn source subscriptions;
- durable wake receipt and restart recovery;
- revision-watch `jobs_get` wake;
- indexed canonical Agent lookup owned by SessionStore with current Session
  authorization recheck;
- query-only `job_exposure_current` projection derived from immutable commit-log
  positions;
- index-backed keyset pagination for mixed Agent/finite Job listing under the
  existing cursor contract.

No second Agent/Job registry was created.

---

## 13. P9 documentation reconciliation

Do not reimplement D4-D7 because of stale AW plan checkboxes.

Factual state:

~~~text
D4-D7 implementation            complete/published
0.17.5 semantic equivalence     complete
post-review dotlink hardening   complete/published
process/control hardening       complete/published
trusted Remote Start issuer     complete/published
AW v36 admission/Agent batch    complete/published at b8a46a94
dotlink source checkpoint       648519ad
dotlink published main          bbd29c13
Windows/macOS native gates      external/open
trusted-selector Bubblewrap     host-gated
sandbox/config-v13              explicitly deferred
~~~

Active AW/dotlink 017/current handoff prose is reconciled at this checkpoint.

---

## 14. DL-060 state

DL-060 implementation/in-repo maintenance is complete.

Current shell timing semantics:

~~~text
timeout_secs
    hard Process execution deadline bounded by host policy

omitted timeout_secs
    host max_shell_timeout_secs

max_output_bytes
    host ceiling

omitted ordinary shell output
    min(256 KiB, host ceiling)

authenticated response_timeout
    synchronous response/handoff window only
    never rewrites Process execution lifetime
~~~

Focused current-head contract tests were 6/6 green.

Remaining DL-060 work is owner/external or optional and should not be silently
started while active runtime work is dirty.

---

## 15. Non-regression invariants

### Process

- never invent signal from timeout/cancel;
- physical exit is not lifecycle;
- reasonless cleanup is not terminal stop;
- NoTarget is not Unknown;
- later infrastructure failure must not erase known terminal facts.

### Replay

- lookup immutable replay before mutable authority checks;
- exact replay never reruns effect;
- current authority controls new effects/disclosure, not fact existence;
- request fitting never mutates canonical replay state;
- replay timestamp is immutable.

### Job/output

- one JobManager;
- one finite Job identity;
- observation grants no ACK credit;
- only serialized raw prefix grants delivery credit;
- ACK never crosses unread gaps;
- source registration never grants recipient authority.

### Agent events

- source owns event revision/subscription generation;
- recipient owns wake authority;
- source stores opaque recipient authority ref only;
- committed delivery survives restart until ACK/suppression;
- revoke/regrant cannot revive stale admission;
- do not fabricate Attention.

### MCP

- structured/media/result metadata are immutable;
- only text shrinks;
- explicit limit error if immutable facts cannot fit;
- fit proof includes receipt/timestamp overhead.

### Progress invariants

- advisory only;
- no fake total/message;
- no detached generic task;
- progress cannot delay completion.

---

## 16. Current definition of done

### AW source criteria — complete and published at `b8a46a94...`

- v35 -> v36 migration/open coverage and legacy replay are green;
- replay/participant/source/restart regressions remain green;
- AW issuer identity/wire/replay semantics are implemented;
- indexed canonical Agent lookup is implemented;
- mixed listing is index-backed/keyset-paged;
- relevant host-available checks/clippy/fmt/diff are green.

### dotlink source criteria — complete and published at `648519ad...`

- ordinary child exit closes the managed group/Job Object immediately as
  reasonless housekeeping;
- post-exit output grace is 250 ms and independent of execution deadline;
- escaped-FD retention preserves command exit and marks output incomplete;
- public revision-fenced cancel has no non-shared CAS authority;
- stdin authority is checked at the per-Job acceptance boundary;
- duplicate server instruction ownership is removed;
- full repinned L5 gate is green at **706/0/1 + 42/42** with strict Clippy,
  fmt/diff, Alejandra and Markdown lint;
- trusted OAuth AW-bridge issuer enforcement and replay-after-revoke semantics
  are qualified and published.

### Cross-repo / external criteria still open

- trusted issuer cutover is complete and published;
- native Windows/macOS runtime validation remains external;
- sandbox/config-v13 remains untouched unless explicitly promoted;
- no duplicate runtime/store/event service/scheduler;
- commit/push/repin/release only with user authorization.

---

## 17. Exact next implementation order

There is no remaining source implementation in the AW v36 / dotlink
process-control-trust cutover that should be started implicitly.

1. Preserve published source pins and unrelated working-tree material.
2. Native Windows/macOS validation remains external.
3. Trusted-selector Bubblewrap smoke remains host-gated.
4. Do not implement sandbox/config-v13 unless explicitly promoted.
5. Do not tag/release/deploy without separate explicit authorization.

Current plan/current-handoff documentation is reconciled through this
checkpoint.

---

## 18. L5 infrastructure caveat

L5 intermittently externally SIGKILLs Cargo/rustc with no Rust diagnostic.

Rules:

- do not classify external SIGKILL as code/test failure;
- do not rerun already-passed portions unnecessarily;
- ACK connector jobs;
- rerun only incomplete tails where practical;
- avoid parallel Cargo commands fighting for target/build locks;
- prefer serial warmed qualification.

---

## 19. Copy-ready next-agent prompt

A standalone prompt is stored at:

~~~text
.agents/docs/AW-DOTLINK-NEXT-AGENT-2026-11-20.md
~~~

Use that file verbatim for the next agent.

---

<!-- markdownlint-disable MD024 MD025 MD048 -->

## Historical complete handoff inherited from 2026-11-19

**Important:** everything below this line is preserved for complete chronology.
Its date-stamped current-state, refs, queues and next-action sections are
historical and are superseded by sections 0–19 above.

The historical body is included verbatim so no original design decision,
implementation phase, validation checkpoint or work item is lost.

---

# AW + dotlink 0.17.x full comprehensive handoff — 2026-11-19

Author: coordinating implementation agent
Owner: @pvl
Source authority host: **L5 only**
AW: `/home/pvl/spaces/abird/src/aw`
dotlink: `/home/pvl/spaces/abird/src/dotlink`
release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

This is the superseding end-to-end handoff for the AW/dotlink shared-runtime
0.17.x convergence workstream. It is intended to let a new agent become
productive without reconstructing the skipped conversation history.

It incorporates:

- the original AI-native filesystem/Graph/Jobs design lineage;
- unified AW Jobs + Agent runtime ownership;
- shared Toolset/output/ACK/replay convergence;
- typed MCP media;
- pre-0.17 shared convergence;
- the v0.17.0–v0.17.5 release audit;
- the stop-provenance redesign and simplification;
- AW P1–P8 implementation and fresh-review fixes;
- final AW publication/pin;
- dotlink pre-repin hardening;
- atomic repin and D1–D3 process convergence;
- the exact remaining D4–D7 cutover plan;
- frozen sandbox/config-v13 scope;
- current validation evidence and external gates.

When this file conflicts with live source/tests or the active owning plan, the
live source/tests and active plan win.

---

## 0. Authority order

Use this order whenever facts conflict:

1. direct user instruction in the current conversation;
2. live source and live tests on L5;
3. active owning plan README / DESIGN / AUDIT / TODO;
4. this handoff;
5. older dated comprehensive handoffs and archived plans;
6. release-line implementation only as behavioral reference, never as automatic
   architectural authority.

Portable semantics belong in AW. Dotlink should consume/project those semantics
and keep only product/transport/deployment policy.

---

## 1. Non-negotiable operating instructions

- Use **dotlink---l5 only** for this workstream.
- Do not use A1.
- Do not create another worktree.
- Continue primarily in:
  - AW: `/home/pvl/spaces/abird/src/aw`
  - dotlink: `/home/pvl/spaces/abird/src/dotlink`
- Preserve unrelated dirty/staged work exactly.
- AW `.agents/plans/ux/**` is reserved by another owner:
  - do not read it;
  - do not list it;
  - do not inspect its staging;
  - do not stage/reset/stash/commit it.
- Keep implementation turns around 30 minutes and stop at the next coherent
  checkpoint.
- Do not commit, push, tag, publish, release or deploy unless explicitly
  authorized for that action.
- Signed history is required; never disable commit signing.
- Revalidate Git refs/status/staging before each new implementation phase.
- Live source/tests outrank prose.
- Keep AW as the reusable semantic owner wherever practical.
- Prefer zero-cost / `Copy` / transparent newtype shared values on hot paths.
- Prefer clean/simple APIs over speculative abstraction.
- Optimize obvious hot paths only where the simpler design remains maintainable.
- Keep product strings/logging outside canonical shared state.
- Do not reintroduce old `job_*` APIs, a second JobManager, a second process
  Guard, a second result fitter, or a second progress scheduler.
- The sandbox/mount/config-v13 scope is **frozen** until the user explicitly
  promotes it. Reading the design is allowed; implementation is not.

---

## 2. Exact current Git / release state

### 2.1 AW

Current AW:

```text
HEAD        4234447612421069a746c925c76a25e3ba27774a
origin/main 4234447612421069a746c925c76a25e3ba27774a
```

Current HEAD:

```text
4234447 Record final shared 0.17 implementation pin
```

Authoritative reusable implementation pin consumed by dotlink:

```text
AW_IMPL_SHA =
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
Relax shared MCP protocol dependency ranges
```

Important AW commits in the 0.17 shared-runtime sequence:

```text
796225e  Separate process signal identity from lifecycle
63f4056  Plan shared-first 0.17 runtime convergence
021b8ce  Add shared process stop evidence types
eb325b3  Complete shared 0.17 runtime convergence
7c7de6c  Fix shared 0.17 projection boundaries
1a5d0f6  Relax shared MCP protocol dependency ranges
4234447  Record final shared 0.17 implementation pin
```

The final dotlink pin is **1a5d0f6**, not eb325b3 or 7c7de6c.

AW working tree is clean at handoff creation, excluding the opaque reserved UX
subtree.

### 2.2 dotlink

Current dotlink:

```text
HEAD        4c3b96fd5ba9e6772bc159f10a3ad893c9729887
origin/main 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
```

Latest checkpoints:

```text
46215fe  Harden pre-repin MCP and shell boundaries
0aea1f4  Record fresh 0.17.x consumer cutover
36d6533  Repin AW and complete D1-D3 process convergence
4c3b96f  Record repinned D1-D3 convergence checkpoint
```

D0 repin and D1–D3 are complete, signed and pushed.

Current direct AW Git dependencies:

```text
aw-tools
aw-tools-fs-unix
aw-tools-fs-win
aw-platform
aw-mcp-protocol
```

All direct/transitive AW Git packages in `Cargo.lock` resolve to exactly:

```text
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
```

The final repin caused **no registry package/version churn** and uses no
sibling/path override.

Dotlink working tree is clean at handoff creation.

### 2.3 Release reference

Current published release reference:

```text
release/0.17        3995448d7bf33794ef84c7bb51150d77cc260ada
origin/release/0.17 3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.5             3995448d7bf33794ef84c7bb51150d77cc260ada
v0.17.4             fd1cdb6429a341a4e3d27805cae0bec288fc26df
```

Important release references:

```text
c185558  v0.17.4 canonical stop/provenance implementation
74820ff  bound caller-controlled Job IDs in diagnostics
06d5548  raw page limit != serialized response envelope
30099aa  Nix module-owned environment hardening
5b27784  runtime-health/backport docs
8874013  ask platform for signal label in tests
f9537bb  v0.17.5 outside-signal lifecycle record
a1f8505  v0.17.5 hard execution-deadline wording
3995448  release 0.17.5
```

The release lane is behavioral reference. Do not wholesale cherry-pick its
runtime model back into current main.

---

## 3. Read this next-agent material in this order

Before editing:

1. AW `AGENTS.md`
2. AW `.agents/docs/CURRENT_HANDOFF.md`
3. this file
4. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md`
5. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md`
6. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md`
7. AW `.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md`
8. dotlink `AGENTS.md`
9. dotlink `.agents/docs/CURRENT_HANDOFF.md`
10. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md`
11. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md`
12. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/D1-D3-SOURCE-CUTOVER-MAP.md`
13. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md`
14. dotlink
    `.agents/plans/017-main-shared-consumer-backport-2026-11-18/V0174-EQUIVALENCE.md`
15. live source/tests for each owner being touched.

Historical deep context:

- `.agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md`
- `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
- `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
- the superseded
  `.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md`

Use those for history, not current refs/status.

---

## Part I — chronology and design lineage

### 4. AI-native filesystem / Graph / Jobs foundations

The workstream did not begin with 0.17. The architectural base was established
during the AI-native filesystem and Job Graph design work in late October /
early November.

The design converged around:

- canonical native filesystem tools;
- Graph-native batch execution;
- durable Jobs;
- finite compact model-facing refs;
- compact token-efficient receipts;
- one execution path for foreground/background;
- immutable Graph nodes;
- explicit authority and policy;
- exact output ownership and delivery proof;
- no duplicate product Job runtimes.

Representative historical dotlink commits:

```text
c6f0e1c  spec AI-native filesystem tool surface
9b7d4b4  refine AI-native filesystem limits and edit
6c1cb16  specify filesystem status and batch grammar
f28ce2c  design batch execution graph
ebb5b84  seal AI-native filesystem design
8a3e7bb  unify durable batch controls with jobs
a156142  make jobs canonical graph runtime
bec528f  finalize canonical job graph design
81a54ce  seal reviewed AI-native job graph design
c34ea5a  seal tool design edge cases
```

Important lasting decision:

> `background` is a disposition of the same durable execution, not a separate
> execution engine.

This is why current main must never resurrect separate foreground/background
runners or old `job_output`/line-tail APIs.

---

### 5. Unified AW Jobs + Agent runtime — November 9–14

Cross-repo design moved reusable Job/Graph/Agent/session/replay/authority
semantics into AW.

Important AW checkpoints:

```text
406bfa5  Resolve unified Graph, replay, authority, and integration design
890e9a3  Resolve shared Job, replay, wake, and Graph ownership design
9e9ab04  Implement unified Jobs and Agent runtime checkpoint
0a6c6a0  Complete unified Jobs and Agent runtime
88cd965  Record finalized unified runtime pins
```

Important dotlink integration checkpoints:

```text
886e861  Align dotlink Start and Job contracts with AW design
3f59bb1  Cut over standalone Jobs to shared AW runtime
c90a660  Finalize dotlink unified Jobs bridge
```

Core ownership decision:

- AW owns reusable JobManager/runtime semantics.
- Dotlink is the concrete finite Process/Graph host.
- Dotlink must not keep a second independent Job runtime.
- Graph nodes are immutable.
- replay is identity/effect preserving;
- authority checks remain layered and explicit;
- output owner/delivery proof is correctness state, not mere presentation.

The finalized unified runtime handoff is:

```text
AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
```

---

### 6. Workspace/remote-view work — separate but protected context

AW also completed major Workspace/remote-view work in parallel:

```text
c9aaf0e  Complete delegated browser authority and surface lifecycle
aaab161  Complete workspace views UX and remote trust
62b8da6  Complete remote Workspace target integration
7d09d5f  Advance workspace views, remote resources and handoff
```

This is not the active 0.17 workstream.

Do not regress it while editing shared tools/runtime.

The reserved `.agents/plans/ux/**` subtree remains opaque to this workstream.

---

### 7. Shared Toolset / output / ACK / replay convergence — November 15–17

AW progressively absorbed portable finite-Job/tool semantics:

```text
0d2b4e3  Build configurable toolset and harden shared Job control
be6d48d  Harden shared Job output and journal uncertainty
26a8c78  Preserve strict output ACK replay across compaction
beef1bc  Harden Job writer quiescence
7e6fe20  Align shared Job list metadata
```

Important invariants established here:

- Toolset is immutable after build.
- metadata is presentation/configuration, not authority.
- finite `jobs_get/jobs_read/jobs_ack` share the canonical JobManager.
- output delivery proof is exact-owner aware.
- ACK Through advances only actually delivered bytes.
- omitted-offset Finish releases owned streams under the correct owner.
- already-committed ACK replay remains valid after another owner compacts the
  retained physical prefix.
- a peek/tail grants no delivery/ACK credit.
- no foreign-owner ACK.
- writer quiescence prevents terminal Job pruning while an OutputWriter remains
  live.
- `jobs_get` wait is settlement/event aware.
- `jobs_read` wait is byte/EOF aware.

Dotlink adoption checkpoints:

```text
d0a3922  Adopt reviewed AW Job output runtime
4f343c3  Complete AW shared-tools convergence
5381729  Complete shared Toolset metadata adoption
```

Historical full detail:

```text
AW .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md
```

---

### 8. B6 shared Toolset metadata convergence

B6 centralized canonical tool metadata/defaults:

- filesystem tool title/description/schema facts;
- shell/PowerShell/Batch descriptor facts;
- finite Job operation metadata;
- one Job profile drives advertised/runtime values;
- jobs_list lower bound lives in AW;
- list_tools/get_tool/inspection use the same canonical projection.

Product-owned dynamic facts remain product-owned, especially:

- shell execution timeout ceilings;
- output ceilings;
- profile/environment/deployment policy.

Toolset metadata is never execution authority.

---

### 9. Typed MCP media boundary

AW added a bounded raw MCP result boundary:

```text
d6009de  Preserve typed MCP media results
30dc55a  Record typed MCP media checkpoint
```

The MCP boundary preserves:

- Text;
- Image;
- Audio;
- embedded Resource;
- ResourceLink;
- structured content.

It bounds content block count / payload / metadata and fails closed on unknown
future content kinds.

Ordinary Agent `ToolOutput` remains text-only.

Real cross-repo stdio qualification proved:

```text
PNG -> image/png
WAV -> audio/wav
PDF -> application/pdf embedded resource
```

Provider/model media delivery, durable media history and consent remain separate
future work.

---

### 10. Pre-0.17 shared convergence — November 18

A semantic audit of the pre-v0.17 release line reduced missing reusable work to:

1. scoped result/receipt metadata + timestamps;
2. shared `jobs_read` page default/max;
3. dotlink synchronous shell omitted-output default.

AW checkpoints:

```text
723ec24  Add scoped receipt metadata and Job page profiles
792f0b3  Allow scoped extended Agent tool results
```

Key receipt design:

- receipt selection is explicit semantic scope;
- AW owns no wall clock;
- host supplies timestamp;
- immutable exact replay reuses stored timestamp;
- fresh `jobs_list/get/read` observation gets fresh timestamp;
- timestamps are not process start time / mtime / settlement time;
- default shared receipt timestamp policy is off.

Job read profile:

```text
default raw page = 256 KiB
shared hard max  = 8 MiB
```

Schema/runtime derive from the same profile.

Trusted Host Agent result budget may be larger than ordinary tools, but
external sources cannot self-widen it.

Dotlink pre-0.17 convergence checkpoint:

```text
4f60bc3 Complete pre-0.17 shared convergence
```

That work introduced current receipt timestamp/checkpoint behavior, current
jobs_read default behavior and the product's roughly 30-minute implementation
workflow guidance.

Important:

> 30 minutes is human/agent workflow guidance, not an execution timeout,
> transport wait, Job TTL or cancellation deadline.

---

### 11. v0.17.x release audit

The release lane was audited semantically rather than cherry-picked.

#### 11.1 v0.17.0 / v0.17.1

Already satisfied/superseded by current main/shared AW:

- single durable background shell path;
- explicit ACK/Finish ownership;
- compact Job payloads;
- token-gated progress;
- `--no-tunnel`;
- strict schemas;
- `utf8_lossy`;
- receipt count units;
- old `job_output`/line-tail surface.

Do not re-port those release-local mechanisms.

#### 11.2 v0.17.2 / v0.17.3

These are the sandbox/config-v13/mount bundle.

Representative release commits:

```text
21de8b3
e7db299
349a7d0
cb7418a
c71f426
6f48703
ba947c6
ae4d623
d855672
0983d39
```

Disposition: **deferred**.

The reusable design is frozen under AW's 017 sandbox plan. Do not implement it
without explicit promotion.

#### 11.3 v0.17.4

Canonical stop/provenance reference:

```text
c185558
```

Hardening references:

```text
74820ff  bounded caller-controlled Job diagnostics
06d5548  raw page != response envelope
30099aa  Nix module-owned environment
5b27784  runtime-health/backport docs
8874013  platform signal labels in tests
```

The audit concluded that reusable stop/process/Task/Job/MCP facts belonged in
AW, not in a permanent dotlink-local release compatibility layer.

#### 11.4 v0.17.5

New release behaviors:

```text
f9537bb  outside-signal lifecycle record
a1f8505  hard execution deadline wording
3995448  release 0.17.5
```

Current main now satisfies D1–D3/v0.17.5 process lifecycle semantics.

---

## Part II — stop/process redesign and final shared architecture

### 12. Physical exit is not lifecycle

The first shared process correction was:

```text
796225e Separate process signal identity from lifecycle
```

Canonical physical exit:

```rust
pub enum ProcessExit {
    Code(i32),
    Signal(i32),
    Unknown,
}
```

Rules:

- only reaped/host-observed OS status produces `Signal(n)`;
- timeout/cancel/shutdown never manufactures a signal number;
- cancellation/deadline/revocation/failure are not physical exit shapes.

Shared platform fact:

```rust
aw_platform::exit_signal(&ExitStatus) -> Option<i32>
```

Signal naming is presentation:

```rust
aw_platform::signal_name(i32) -> Option<&'static str>
```

No dotlink-local signal-name table.

---

### 13. Stop vocabulary evolution

The initial 021b8ce checkpoint intentionally proved invariants but had an
overloaded taxonomy. It went through discussion around:

- cancellation origin;
- stop cause;
- attempt reason;
- outcome;
- TargetGone vs Unknown;
- cleanup vs Drop.

The user explicitly asked for a simpler library-level design.

Final canonical shared vocabulary became:

```rust
pub enum StopReason {
    Timeout,
    Cancelled,
    RuntimeError,
}

pub enum StopEvidence {
    Stopped,
    NoTarget,
    Unknown,
}

pub struct StopRecord {
    pub reason: Option<StopReason>,
    pub evidence: StopEvidence,
}
```

Meaning:

- `Some(reason)`: semantic reason the process was meant to stop;
- `None`: housekeeping, not a semantic terminal cause;
- `Stopped`: platform has evidence the stop acted on a live target;
- `NoTarget`: positive knowledge that no live containment target remains;
- `Unknown`: resulting target state cannot be established.

Never collapse `NoTarget` into `Unknown`.

Why:

```text
NoTarget -> positive absence -> disarm -> no retry
Unknown  -> unresolved       -> may remain armed -> fail-safe retry
```

Cleanup/Drop are execution paths, not stop reasons.

Caller/parent/runtime/authority provenance is useful context but is not part of
canonical process-stop state.

---

### 14. StopTracker

Unknown retries need to preserve the original semantic reason without growing
the canonical stop enums.

Shared zero-cost state:

```rust
StopTracker
```

Behavior:

- initial `Cancelled + Unknown` remembers Cancelled;
- later housekeeping/fail-safe retry that proves Stopped yields
  `Cancelled + Stopped`;
- Stopped/NoTarget concludes/reset the sequence;
- later unrelated cleanup is reason-less.

This avoids synthetic `Dropped`/cleanup reasons and keeps the state small.

---

### 15. Terminal attribution

Only:

```text
StopEvidence::Stopped + Some(StopReason)
```

may become terminal semantic stop.

Examples:

```text
Timeout      + Stopped  -> terminal.stop = Timeout
Cancelled    + Stopped  -> terminal.stop = Cancelled
RuntimeError + Stopped  -> terminal.stop = RuntimeError

Some(reason) + NoTarget -> no terminal stop
Some(reason) + Unknown  -> no terminal stop
None + any evidence     -> no terminal stop
```

Canonical terminal:

```rust
ProcessTerminal {
    exit: ProcessExit,
    stop: Option<StopReason>,
    output: ProcessOutputMeta,
}
```

Product fields such as `stopped_by`, signal labels and prose do not belong
here.

---

### 16. Managed process-tree Guard

Platform ownership:

- Unix:
  - successful process-group signal -> Stopped;
  - ESRCH -> NoTarget;
  - other error -> Unknown.
- Windows Job Object:
  - proven empty -> NoTarget;
  - proven live + successful termination -> Stopped;
  - failed/unprovable query/termination -> Unknown.

Shared platform Guard:

```text
ManagedProcessGuard
```

NoTarget / Stopped disarm.
Unknown stays armed for final fail-safe retry.

Dotlink's previous private process-group/Job Object Guard semantics are gone
after D1.

The unsandboxed Unix `setsid` descendant-escape limitation remains a real
containment limitation; do not rewrite platform evidence merely to hide it.

---

### 17. Lifecycle events vs final logical stop

These are deliberately separate.

Attempt-level diagnostic event:

```rust
ExecutionLifecycleEvent::ProcessStop { record }
```

One logical stop may emit multiple attempt observations:

- graceful;
- forced;
- cleanup;
- fail-safe retry.

Products must not render one operator line per attempt event.

The adapter also exposes the final factual logical record:

```rust
ProcessReceipt::stop_record() -> Option<StopRecord>
```

That is the correct product-facing stop decision.

---

### 18. v0.17.5 product projection

Dotlink retains only product context required for compatibility:

```text
CancelOrigin::{Caller, Connector}
```

This context lives beside cancellation state and is not promoted back into
AW's canonical StopReason.

One product mapper drives sync result / jobs_get / Task failure / lifecycle
projection.

`stopped_by` mapping:

```text
shared Timeout      -> "timeout"
shared Cancelled:
  caller            -> "cancel"
  connector teardown-> "shutdown"
shared RuntimeError -> "failed"
```

Only proven terminal stop attribution may emit `stopped_by`.

Lifecycle operator records are mutually exclusive:

```text
reasoned final StopRecord
  -> exactly one
     LOG job stop id=<id> reason=<product reason>
                  outcome=<delivered|gone|failed>

ProcessExit::Signal(n)
+ no reasoned stop sequence
  -> exactly one
     LOG job exit id=<id> signal=<name-or-number> from=outside
```

`from=outside` means only:

> dotlink did not enter a semantic stop sequence.

It does **not** identify or blame the sender.

A reasoned stop sequence suppresses `from=outside` even when evidence is
NoTarget/Unknown, because direct-child fallback may have delivered the physical
signal.

---

### 19. Task terminal detail

Later infrastructure failure must never erase already observed process facts.

Compact shared failure detail:

```rust
TaskTerminalDetail {
    exit: ProcessExit,
    stop: Option<StopReason>,
}
```

Examples that must survive later spool/journal/internal failure:

```text
exit=Signal(9), stop=Some(Timeout)
exit=Code(1),   stop=None
exit=Unknown,   stop=Some(Cancelled)
```

The full `ProcessTerminal` remains in Job/process state.

---

## Part III — shared Job, receipt, budget and MCP architecture

### 20. Flat finite Job refs

Model/product-facing finite ref:

```text
j:<handle>
j:<handle>/n:<positive-coordinate>
```

Shared typed owner:

```rust
aw_tools::execution_ref::JobRef
```

Important separation:

- finite `JobRef` is the model/product surface;
- hierarchical `ExecutionRef` remains internal ownership/Agent-routing
  identity.

Do not flatten internal AW hierarchy into the external finite grammar.

Fields are private behind validated constructors/accessors.

---

### 21. Shared receipt grammar

Shared receipt owner:

```rust
aw_tools::receipt::{
    Receipt,
    AckMode,
    NextAction,
    render_plain,
    render_handoff,
}
```

Shared receipt types cover:

- JobStatus;
- NodeStatus;
- Handoff;
- Cancel;
- ACK Through;
- ACK Finish.

Shared rendering distinguishes Job lifecycle:

```text
running
cancelling
revoking
settled
cancelled
revoked
```

Process terminal detail is retained separately:

```text
exit=<...>
stop=timeout|cancelled|runtime_error
```

Handoff remains same Job + `no-rerun`; it is never a second process verdict.

`jobs_send` intentionally remains a tiny dotlink product receipt. Do not add
an AW Send receipt merely to remove a few product strings.

---

### 22. Bounded diagnostics

Shared generic diagnostic truncation:

```rust
aw_tools::diagnostic::bounded_diagnostic
```

Use this after D4 for generic Job diagnostics.

Product MCP `ErrorData` serialized-envelope fitting remains dotlink-owned; it
is a transport boundary, not a generic Job diagnostic.

Never reflect hostile caller-controlled Job refs into error bodies.

---

### 23. Raw page vs serialized result budget

These are different dimensions:

```rust
RawPageBytes
SerializedResultBytes
```

A one-byte source page may still require a normal-size MCP/JSON envelope.

Current pre-D5 compatibility behavior already proves:

- `limit=0` rejected;
- `limit=1` valid;
- absolute offsets remain raw-byte offsets;
- delivery proof tracks bytes actually returned;
- ACK Through uses delivered proof;
- response fitting must not mutate retained source bytes / EOF / offsets.

Shared runtime resolver:

```text
JobProfile::resolve_read_page(...)
```

D5 must make raw source reading independent from the serialized envelope
internally, not only at the tool boundary.

---

### 24. Receipt policy extraction

Receipt selection is pure copyable policy:

```rust
aw_tools::receipt_policy::{
    ReceiptCategory,
    ReceiptScope,
    ReceiptProfile,
    ReceiptMetadata,
}
```

It is feature-neutral.

Important semantics:

- host supplies clock timestamp;
- AW/protocol layer owns policy and result shaping;
- stored timestamp wins on replay;
- fresh observation gets a fresh timestamp when policy selects it.

The MCP protocol decorator takes `ReceiptProfile` directly, not a full
Toolset/catalog.

---

### 25. aw-mcp-protocol

Generic reusable MCP model projection is in:

```text
aw-mcp-protocol
```

It deliberately does not depend on the full AW MCP client adapter.

Direct dependency boundary:

- `aw-tools`, default features disabled, **no aw-tools feature enabled**;
- rmcp model types, default features disabled;
- `serde_json`;
- Tokio timing/macros.

It does **not** pull:

- aw-agent-core;
- aw-agent-kernel;
- reqwest;
- jsonschema;
- clap;
- MCP client/server transport stacks.

Full `aw-mcp` depends on and re-exports this protocol layer.

Do not add full `aw-mcp` to dotlink.

---

### 26. MCP result/receipt invariants

Shared helpers:

```text
stored_receipt_timestamp
has_visible_receipt_timestamp
set_stored_receipt_timestamp
validate_complete_result
fit_complete_result
decorate_and_fit_complete_result
```

Metadata rules:

- result `_meta` <= 16 KiB serialized;
- content-block metadata overhead bounded independently;
- timestamp mutation merges into existing
  `_meta["io.abird/receipt"]`;
- malformed non-object reserved receipt metadata fails transactionally;
- unrelated/reserved receipt fields survive timestamp mutation.

Fitting rules:

- actual serialized `CallToolResult` bytes determine fit;
- only text may shrink;
- truncation always carries an explicit marker;
- typed Image/Audio/Resource/ResourceLink remain immutable;
- structured JSON remains immutable;
- block order/metadata remains immutable;
- result metadata/type/error class remains immutable;
- if immutable facts + minimum marker cannot fit -> explicit limit error.

Timestamp sidecar logic:

1. fit against full envelope first;
2. if visible stored `ts=` survives, return with no sidecar reserve;
3. otherwise append marker;
4. if marker needs reserve, shrink the already-fitted body further;
5. do not clone whole media/structured results merely to test marker fit.

At the 32-block limit:

- existing text block may carry the marker;
- all-typed full block set fails explicitly.

---

### 27. MCP heartbeat/progress

Shared protocol API:

```rust
ProgressHeartbeatConfig
ProgressHeartbeat
ProgressHeartbeatSummary
ProgressHeartbeatObserver
await_with_progress_heartbeat
semantic_progress_notification
notify_semantic_progress_bounded
```

Semantics:

- no progress token -> sender is never polled;
- heartbeat ticks monotonically 1,2,3,...;
- heartbeat has unknown total and no fake message;
- product defaults remain:
  - interval 10 s;
  - per-send budget 100 ms;
- operation future remains polled while send is backpressured;
- no detached task/queue;
- if operation completes first, pending notification is dropped;
- expired delivery deadline does not poll sender;
- observer receives only `{attempted, delivered}`.

Real semantic progress uses the existing AW known/unknown total; never infer
percent from heartbeat count, time, revision or output bytes.

---

### 28. Transport replay boundary

AW owns:

- semantic Job replay;
- Job/effect identity;
- output ownership/delivery proof;
- Remote Start authorization;
- no-rerun/uncertainty inner semantics.

Dotlink still owns:

- OpenAI request IDs;
- serialized hosted response body caching;
- HTTP delivery leases;
- tunnel namespace/horizon;
- outer RequestReplayLedger transport policy.

Do not move the outer hosted replay ledger into AW in D4–D7.

---

## Part IV — implemented dotlink convergence

### 29. Product-only pre-repin hardening

Before the final AW repin, dotlink hardened its existing product boundaries so
the shared cutover could be behavior-preserving.

Signed commits:

```text
8a3ed9d  Harden Nix service environment ownership
a36e976  Make lifecycle logging best effort
690cfb5  Harden pre-repin runtime compatibility
8e14ee2  Record pre-repin 0.17 consumer checkpoint
46215fe  Harden pre-repin MCP and shell boundaries
0aea1f4  Record fresh 0.17.x consumer cutover
```

#### Nix

Module-owned environment keys are rejected generically:

```text
DOTLINK_PROFILE
DOTLINK_CONFIG_FILE
DOTLINK_API_KEY_FILE
NGROK_AUTHTOKEN_FILE
DOTLINK_STDIO
DOTLINK_COLOR
DOTLINK_SUPERVISOR
DOTLINK_SSH_AGENT_SOCKET
```

Allowed unrelated env remains supported, including:

```text
DOTLINK_VERBOSITY
RUST_LOG
XDG_CONFIG_HOME
XDG_STATE_HOME
```

#### Logger

Structured lifecycle/TOOL/REQ writes:

- lock stderr per record;
- best-effort/fallible;
- BrokenPipe never panics;
- safe when invoked during Drop while another panic is unwinding.

`LogConfig::lifecycle` is always visible regardless of verbosity.

#### Pre-repin D4–D7 compatibility

Before consuming shared replacements, current main locked behavior with tests:

- 32 KiB hostile Job refs do not inflate stdio/local HTTP response;
- `jobs_read(limit=0)` rejected;
- one-byte jobs_read pages work with independent response envelope;
- one-byte delivery -> ACK Through(1) -> offset 1 continuation;
- typed Image/Audio/Resource survives stdio + hosted path;
- structured content preserved at decorator boundary;
- receipt metadata merge is transactional;
- visible timestamps are replay stable;
- no duplicate visible `ts=`;
- text truncation never silently drops its marker;
- immutable typed/structured facts fail explicitly;
- progress send backpressure never delays completed operation;
- no detached/post-return progress send.

These tests are compatibility oracles for D4–D7.

---

### 30. AW publication and atomic repin

Fresh-review AW source converged through:

```text
7c7de6c Fix shared 0.17 projection boundaries
1a5d0f6 Relax shared MCP protocol dependency ranges
4234447 Record final shared 0.17 implementation pin
```

Why the 1a5d0f6 follow-up exists:

The initial repin attempt exposed that exact Tokio/serde_json requirements in
`aw-mcp-protocol` would downgrade dotlink's already compatible runtime
dependency set.

The shared package manifest was relaxed before final repin.

Final result:

- all AW Git packages pin `1a5d0f65...`;
- no registry package/version churn;
- no path override;
- full `aw-mcp` client adapter is absent from dotlink.

---

### 31. D1–D3 completed process convergence

Signed/pushed source:

```text
36d6533 Repin AW and complete D1-D3 process convergence
```

Docs checkpoint:

```text
4c3b96f Record repinned D1-D3 convergence checkpoint
```

#### D1

Removed/reduced duplicate process semantics:

- private process Guard semantics replaced by `aw_platform::ManagedProcessGuard`;
- private `stop_proven` model removed;
- synthetic Cancelled/DeadlineExceeded physical ProcessExit model removed;
- physical exit uses shared `ProcessExit`;
- semantic stop uses shared `StopRecord / StopReason / StopEvidence`;
- shared `StopTracker` preserves reason across Unknown retry;
- shared `exit_signal` extracts physical Unix signal;
- local signal-name table gone.

Dotlink retains only product cancellation context:

```text
CancelOrigin::{Caller, Connector}
```

This is intentionally not reintroduced into canonical AW stop state.

#### D2

Lifecycle operator logging uses final result, not attempt event.

Reasoned final stop:

```text
LOG job stop id=<id> reason=<reason> outcome=<delivered|gone|failed>
```

Outside physical signal with no reasoned stop sequence:

```text
LOG job exit id=<id> signal=<name-or-number> from=outside
```

Mutually exclusive.

Real binary tests cover quiet/default/verbose.

#### D3

One product mapper now drives:

- sync shell result;
- `jobs_get`;
- Task failure detail;
- lifecycle prose.

Later spool/internal/journal failure cannot erase a previously observed signal
or semantic stop.

Shared `TaskTerminalDetail` is projected into Task failure data.

Compact handoff remains a Job handle, not a second process verdict.

---

## Part V — current remaining implementation queue

### 32. Current queue summary

Only D4–D7 shared-consumer replacement remains active.

Order:

```text
D4 typed refs + shared receipts + shared diagnostics
-> focused qualification
D5 raw page vs serialized result budget separation
-> focused + combined D4/D5 qualification
D6 aw-mcp-protocol receipt/result projection
-> typed-media/metadata/tight-budget qualification
D7 shared heartbeat/progress
-> full Rust/Nix/equivalence qualification
```

Do not skip ahead by deleting all local helpers at once.

Replace one semantic area, run its compatibility oracle, then delete the old
generic implementation.

Exact map:

```text
dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/
  D4-D7-SOURCE-CUTOVER-MAP.md
```

---

### 33. D4 — typed refs

Current duplicate surface:

```text
src/mcp/tools/jobs.rs
  StandaloneRef
  parse_ref(...)
  local ExecutionRef/ExecutionSegment flat parsing
```

Replace with:

```rust
aw_tools::execution_ref::JobRef
```

Rules:

- raw transport string parsed once;
- never echo hostile ref into errors;
- typed JobRef carries trusted finite identity;
- render to string only at model/product boundary;
- checked conversion from node coordinate where host API needs `usize`;
- internal hierarchical AW ExecutionRef remains separate.

Current `JobManager::standalone_ref(...)` string construction should become
typed construction + final Display.

No second flat grammar.

---

### 34. D4 — shared receipts

Current duplicate surfaces include:

```text
bounded_job_status
bounded_job_handoff
bounded_replayed_job_handoff

ControlReceipt
bounded_control_success
bounded_control_replay
```

Shared replacement:

```rust
Receipt::{JobStatus, NodeStatus, Handoff, Cancel, Ack}
AckMode::{Through, Finish}
render_plain
render_handoff
```

For shared-backed Jobs, use authoritative shared Snapshot/NodeState/terminal.

Do not locally reclassify:

```text
running
cancelling
revoking
settled
cancelled
revoked
```

Do not parse rendered receipt prose back into trusted semantic identity.

Replay rules:

- carry typed JobRef through the call;
- validate replay identity against the trusted ref;
- original cached result/metadata remains authoritative;
- narrow response projection never replaces cache or reruns effect.

`jobs_send` stays product-owned and tiny; AW has no generic Send receipt.

---

### 35. D4 — diagnostics

Current:

```text
src/jobs.rs
  MAX_JOB_ERROR_BYTES
  bounded_job_error(...)
```

Replace generic diagnostic truncation with:

```rust
aw_tools::diagnostic::bounded_diagnostic
```

Keep dotlink product MCP `ErrorData` envelope fitting.

Mandatory regressions:

- hostile 32 KiB ref over stdio;
- hostile 32 KiB ref over local HTTP;
- narrow replay same Job/effect;
- no-rerun;
- no rendered-prose semantic parsing.

---

### 36. D5 — raw source page vs serialized response

Current behavior is correct at the outer boundary, but two internal paths still
couple raw source read to serialized payload ceiling.

Goal:

```text
RawPageBytes          = source/read semantic limit
SerializedResultBytes = response envelope limit
```

Do not use one as the other.

Preferred internal request shape:

```text
OutputPageRequest {
    requested_bytes: RawPageBytes,
    response_ceiling: SerializedResultBytes,
    ...
}
```

If changing the struct creates needless churn, unwrap exactly once at the
lowest renderer boundary; still keep the dimensions separate in APIs.

Correct sequence:

1. source window bounded by `RawPageBytes`;
2. raw offset/retained length/EOF fixed independently;
3. serialized projection may return a smaller delivered prefix;
4. only bytes actually present in validated response get delivery credit;
5. ACK proof follows that delivered prefix.

Never mutate retained source offsets/EOF to satisfy response envelope.

Use:

```text
JobProfile::resolve_read_page
```

for runtime limit validation.

Schema and runtime must derive from the same profile.

Mandatory regressions:

- `limit=0` rejected;
- `limit=1` valid;
- one-byte delivery + ACK + absolute offset continuation;
- ACK proof unchanged under response fitting;
- EOF/raw offsets remain source facts.

---

### 37. D6 — shared MCP projection

Delete local generic copies only after shared parity.

Current local helpers in `src/mcp.rs` include:

```text
fitted_text_blocks
fit_call_tool_response_to_budget
stored_receipt_ts
has_visible_receipt_ts
set_stored_receipt_ts
ensure_visible_receipt_ts
stamp_receipt_metadata
LocalMachine::decorate_and_fit_tool_response
```

Shared owner:

```rust
aw_mcp_protocol::{
    stored_receipt_timestamp,
    has_visible_receipt_timestamp,
    set_stored_receipt_timestamp,
    validate_complete_result,
    fit_complete_result,
    decorate_and_fit_complete_result,
}
```

Keep a tiny dotlink wrapper only to:

- convert product/transport result ceiling to `SerializedResultBytes`;
- choose `OperationId`;
- pass `self.shared_toolset().receipt_profile()`;
- supply host UTC timestamp;
- map shared protocol error to dotlink `McpError`;
- leave non-complete response variants unchanged.

Cached Remote Start:

- host decides when to obtain clock timestamp;
- use shared metadata mutation;
- do not fit cached state to one request's response envelope;
- immutable stored timestamp wins on replay.

Keep product-owned:

```text
unix_ts_now
fit_mcp_error_to_budget
product ErrorData envelope fitting
```

Mandatory D6 gates:

- PNG exact identity;
- WAV exact identity;
- PDF Resource exact identity;
- structured-content identity;
- unrelated metadata preserved;
- fields in reserved receipt object preserved;
- malformed reserved receipt object rejected transactionally;
- stored timestamp wins replay;
- visible timestamp survives tight fit;
- no unnecessary sidecar reserve;
- explicit truncation marker;
- immutable typed/structured result fails explicitly if impossible to fit.

---

### 38. D7 — shared heartbeat/progress

Current duplicate scheduler:

```text
src/mcp/tools/mod.rs
  PROGRESS_HEARTBEAT_INTERVAL
  PROGRESS_SEND_BUDGET
  deliver_heartbeat_bounded
  ProgressHeartbeat
  heartbeat_tick
  await_wait_or_delivery
  await_with_heartbeat
```

Replace with:

```rust
aw_mcp_protocol::{
    ProgressHeartbeatConfig,
    ProgressHeartbeat,
    ProgressHeartbeatSummary,
    ProgressHeartbeatObserver,
    await_with_progress_heartbeat,
    semantic_progress_notification,
    notify_semantic_progress_bounded,
}
```

Dotlink still owns request context:

```text
context.meta.get_progress_token()
context.peer.clone()
```

Product send closure:

```text
peer.notify_progress(param).await.is_ok()
```

Per-tick product logging may remain in closure.

Final attempted/delivered summary moves into a product observer that implements:

```rust
ProgressHeartbeatObserver::completed(summary)
```

and delegates to existing `progress_done(...)`.

No detached progress task.

Shell retains its real execution deadline when wrapping waits.

Real semantic progress only uses actual AW cumulative progress facts; never
invent percentages.

Mandatory D7 gates:

- no token -> sender never polled;
- monotonic ticks;
- no fake total/message;
- backpressure cannot delay ready operation;
- pending send dropped when operation wins;
- expired deadline does not poll sender;
- wrapper cancellation still emits attempted/delivered summary;
- hosted `jobs_get` progress remains correct;
- shell progress forwarding remains correct.

---

### 39. Final post-D4–D7 qualification

After all four slices:

- locked all-target/all-feature cargo check;
- main binary full tests;
- all integration tests;
- strict all-target/all-feature Clippy;
- rustfmt;
- `git diff --check`;
- Markdown lint;
- NixOS module checks;
- D1–D3 real lifecycle tests;
- D4 hostile refs/replay/receipt tests;
- D5 zero/one-byte/ACK tests;
- D6 typed media/metadata/timestamp/result-fit tests;
- D7 progress/backpressure/deadline tests;
- full v0.17.x equivalence matrix through published v0.17.5.

Do not claim final equivalence until this rerun is green.

---

## Part VI — product-only/deferred boundaries

### 40. Dotlink product ownership

Intentionally dotlink-owned:

- OpenAI/HTTP/stdio delivery policy;
- request replay transport ledger;
- profile/CLI/environment syntax;
- NixOS/systemd deployment;
- product wire wording;
- `stopped_by`;
- `cancel` vs `shutdown`;
- `LOG job stop ...`;
- `LOG job exit ... from=outside`;
- operator logging destination/verbosity;
- host wall clock;
- MCP ErrorData transport fitting;
- release/version bookkeeping.

Do not move product strings/policy into AW merely to reduce line count.

---

### 41. Frozen sandbox/mount/config-v13 work

AW frozen plan:

```text
.agents/plans/017-sandbox-mount-convergence-2026-11-18/
```

Dotlink deferred pointer plan exists separately.

Do not implement without explicit promotion.

Future designed AW ownership includes:

- generic SandboxMountSpec;
- MountAccess;
- required/optional sources;
- target-keyed merge/conflict validation;
- reserved environment validation;
- pinned source capabilities;
- final bind-time validation;
- descriptor-backed Bubblewrap RO/RW binds.

Future dotlink ownership includes:

- config v13;
- profile mounts;
- `DOTLINK_MOUNTS`;
- repeatable `--mount`;
- developer cache discovery;
- precedence;
- Nix integration;
- product diagnostics/docs.

This scope is not part of D4–D7.

---

## Part VII — non-regression invariants

### 42. Process facts

Never regress:

- physical exit != Job lifecycle != semantic stop;
- no requested stop invents a signal;
- NoTarget != Unknown;
- only Stopped + semantic reason populates terminal.stop;
- external signal does not produce stopped_by;
- reasoned connector stop does not become from=outside;
- unattributed signal does not lose its outside-exit record;
- later infrastructure failure does not erase observed terminal facts;
- attempt lifecycle events are not final product stop records.

---

### 43. Job/output facts

Never regress:

- one canonical shared JobManager;
- exact owner delivery proof;
- ACK Through only delivered bytes;
- Finish only correct owner;
- replay-safe committed ACK after compaction;
- no peek/tail ACK credit;
- no foreign owner ACK;
- no new ACK into compacted-away undelivered bytes;
- writer quiescence before terminal pruning;
- same Job/effect under narrow replay;
- handoff is no-rerun continuation, not rerun permission.

---

### 44. Budget facts

Never regress:

- raw page and serialized envelope are independent;
- source offset/EOF remain raw facts;
- tiny raw page still gets normal factual response envelope;
- response fitting may reduce delivered prefix, not retained source truth;
- only actually delivered bytes receive ACK proof.

---

### 45. MCP result facts

Never regress:

- typed media stays typed;
- structured content stays structured;
- result metadata/error class/type/block order remain factual;
- only text shrinks;
- truncation is explicit;
- malformed reserved metadata fails transactionally;
- stored receipt timestamp wins replay;
- fresh observation may get fresh timestamp;
- no receipt timestamp double append;
- no hidden full aw-mcp client dependency in dotlink.

---

### 46. Progress facts

Never regress:

- token-gated only;
- monotonic heartbeat;
- no guessed total/message;
- bounded send;
- operation completion owns completion;
- no detached queue/task;
- observer summary on cancellation/drop;
- semantic progress comes from real AW cumulative progress.

---

### 47. Authority/replay facts

Never infer authority from:

- labels;
- paths;
- tabs;
- presentation metadata;
- browser auth state without the correct authority chain.

Outer hosted replay remains product-owned.

Do not re-run an effect merely because a compact projection could not fit.

---

## Part VIII — validation history

### 48. Important completed validation checkpoints

Historical examples:

#### Shared Toolset / Job runtime

Repeated full AW core/adapter suites, strict Clippy, minimal/no-default builds,
downstream MCP/host/CLI, and full dotlink binary/integration qualification.

#### Typed MCP media

Real stdio PNG/WAV/PDF + synthetic typed-result bounds.

#### Pre-0.17 convergence

AW and dotlink full suites, strict Clippy, receipt timestamp assertions over
stdio and hosted EmbeddedMcp.

#### Stop/process convergence

AW:

- physical signal identity;
- final minimal stop vocabulary;
- platform Guard evidence;
- ProcessTerminal/Task detail;
- StopTracker;
- final ProcessReceipt stop record;
- receipt stop/revocation projection;
- exit_signal platform fact.

#### Final shared MCP protocol review

Latest AW shared validation before publication included:

```text
aw-tools                  357/357
aw-tools-adapter          255/255
aw-mcp-protocol            12/12
aw-mcp                     20/20
aw-platform-unix           13/13
live PTY                     4/4
cross-UID                    1 intentional ignore
aw-terminal                20/20
```

Plus:

- minimal/no-default checks;
- host-side Windows compile;
- host/CLI/full workspace;
- strict Clippy;
- rustfmt/diff;
- dependency guard proving aw-mcp-protocol enables no aw-tools feature and
  imports no Agent/reqwest/jsonschema/clap transport stack.

#### Post-repin D1–D3 dotlink

Current qualification:

```text
main binary       701 passed / 0 failed / 1 intentional ignored
integrations       42/42
```

Integration breakdown recorded in current handoff:

```text
child-env                 1/1
exit-status              15/15
OAuth live rotation       1/1
private generations      13/13
stdio tools              10/10
real lifecycle exit/stop  2/2
```

Also green:

- locked all-target/all-feature cargo check;
- strict all-target/all-feature Clippy;
- rustfmt/diff;
- Markdown lint;
- NixOS module check.

Real-binary tests prove outside SIGKILL and timeout stop semantics at
quiet/default/verbose.

---

### 49. Native Windows gate

The Windows Job Object / native exit-signal tests are authored.

L5 does not have the Windows Rust std target, so native Windows execution remains
an external gate.

Host-side Windows package compilation is green.

Do not mark native Windows execution complete from L5.

---

## Part IX — current authoritative plans / documents

### 50. AW owning plan

```text
AW .agents/plans/017-shared-runtime-convergence-2026-11-18/
  README.md
  DESIGN.md
  AUDIT.md
  TODO.md
  PUBLISH-REPIN-PREFLIGHT.md
```

P1–P8 source/publish are complete.
P9 D1–D3 is complete.
P9 D4–D7 remains open.
Final cross-repo equivalence remains open.
Native Windows remains external.

---

### 51. Dotlink owning plan

```text
dotlink .agents/plans/017-main-shared-consumer-backport-2026-11-18/
  README.md
  TODO.md
  D1-D3-SOURCE-CUTOVER-MAP.md
  D4-D7-SOURCE-CUTOVER-MAP.md
  V0174-EQUIVALENCE.md
```

Current status:

```text
D0 repin   COMPLETE
D1         COMPLETE
D2         COMPLETE
D3         COMPLETE
D4         OPEN
D5         OPEN
D6         OPEN
D7         OPEN
D8         COMPLETE
D9 docs    COMPLETE; final equivalence rerun OPEN
sandbox    DEFERRED
```

---

## Part X — exact next action

### 52. Do not re-open D1–D3

D1–D3 is implemented, signed, pushed and qualified.

Do not redesign stop provenance again unless a real failing regression or a new
user requirement appears.

Do not restore:

- stop_proven;
- synthetic DeadlineExceeded/Cancelled physical exits;
- private Guard semantics;
- local signal-name table;
- lifecycle-event-per-log-line behavior.

---

### 53. Start D4

Before editing:

```bash
cd /home/pvl/spaces/abird/src/dotlink
git status --short
git diff --cached --name-status
git rev-parse HEAD
git rev-parse origin/main
git diff --check
```

Expected:

```text
HEAD == origin/main == 4c3b96fd5ba9e6772bc159f10a3ad893c9729887
working tree clean
nothing staged
```

Revalidate AW lock entries:

```text
all AW Git packages -> 1a5d0f65208e4a05b737c804a55a3aaad3e2fa59
```

Then execute only D4:

1. replace raw finite ref parsing with `JobRef`;
2. convert trusted internal ref construction to typed refs;
3. switch shared-backed status/handoff/control rendering to shared receipts;
4. keep tiny product `jobs_send` receipt;
5. replace generic Job diagnostic truncation;
6. stop reparsing rendered receipt prose as trusted replay identity;
7. run D4 focused tests;
8. run strict Clippy/fmt/diff;
9. update TODO/handoff;
10. pause.

Do **not** begin D5 in the same turn unless D4 is truly small and completely
green.

---

### 54. D4 focused compatibility oracle

At minimum retain:

```text
stdio_huge_job_ref_is_rejected_without_reflection
local_http_huge_job_ref_is_rejected_without_reflection

remote_start_duplicate_graph_replays_receipt_without_second_effect
remote_cancel_replay_precedes_revision_recheck
```

Also add/update tests for shared receipt grammar and typed ref construction.

---

## Part XI — definition of done

### 55. Active non-sandbox workstream is complete when

1. D4 uses typed JobRef/shared receipts/shared diagnostics.
2. D5 separates raw-page and serialized-result budgets internally.
3. D6 consumes aw-mcp-protocol and removes local generic fitter/receipt helpers.
4. D7 consumes shared heartbeat/progress and removes local scheduler.
5. All D1–D7 compatibility regressions pass.
6. Full dotlink Rust/integration/Clippy/fmt/Markdown/Nix gates pass.
7. v0.17.x equivalence matrix is fully green through published v0.17.5.
8. No duplicate reusable semantic owner remains in dotlink.
9. Native Windows gate is either executed on an eligible host or explicitly
   remains external/unclaimed.
10. Sandbox/config-v13 remains untouched unless separately promoted.

---

## Part XII — historical references

### 56. Older handoffs that explain how we got here

Use only when deeper historical detail is needed:

```text
AW .agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
AW .agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md
AW .agents/docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md
AW .agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md
```

This 2026-11-19 handoff supersedes their state/next-action claims for the active
0.17.x convergence thread.

---

## Part XIII — copy-ready starting prompt

### 57. Starting prompt for the next agent

Copy the prompt below exactly or use the companion NEXT-AGENT file.

```text
Use dotlink---l5 only.

Continue primarily in:

/home/pvl/spaces/abird/src/dotlink

Cross-repo AW:

/home/pvl/spaces/abird/src/aw

Read-only release reference:

/home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1. Do not create another worktree. Preserve all unrelated work.
Do not inspect or touch AW .agents/plans/ux/**. Do not implement the frozen
sandbox/mount/config-v13 plan unless I explicitly promote it.

Keep each implementation turn to roughly 30 minutes. Aim for the next coherent
checkpoint, update durable docs, then pause.

Read first, in this order:

1. /home/pvl/spaces/abird/src/aw/AGENTS.md
2. /home/pvl/spaces/abird/src/aw/.agents/docs/AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md
3. /home/pvl/spaces/abird/src/aw/.agents/docs/CURRENT_HANDOFF.md
4. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/README.md
5. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/DESIGN.md
6. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/AUDIT.md
7. /home/pvl/spaces/abird/src/aw/.agents/plans/017-shared-runtime-convergence-2026-11-18/TODO.md
8. /home/pvl/spaces/abird/src/dotlink/AGENTS.md
9. /home/pvl/spaces/abird/src/dotlink/.agents/docs/CURRENT_HANDOFF.md
10. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/README.md
11. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/TODO.md
12. /home/pvl/spaces/abird/src/dotlink/.agents/plans/017-main-shared-consumer-backport-2026-11-18/D4-D7-SOURCE-CUTOVER-MAP.md
13. live source/tests for every owner you touch.

Current expected refs:

AW HEAD == origin/main
4234447612421069a746c925c76a25e3ba27774a

AW_IMPL_SHA consumed by dotlink
1a5d0f65208e4a05b737c804a55a3aaad3e2fa59

dotlink HEAD == origin/main
4c3b96fd5ba9e6772bc159f10a3ad893c9729887

release/0.17 == origin/release/0.17 == v0.17.5
3995448d7bf33794ef84c7bb51150d77cc260ada

D0 repin and D1-D3 are complete, signed, pushed and qualified. Do not redesign
or reopen them without a real regression.

Remaining active implementation is D4-D7 only:

D4 typed JobRef + shared receipts + bounded diagnostics
-> D5 RawPageBytes / SerializedResultBytes internal separation
-> D6 aw-mcp-protocol receipt/result fitting
-> D7 shared heartbeat/progress
-> full v0.17.5 equivalence.

Start with D4 only.

D4 requirements:

- replace local raw finite Job ref parser with aw_tools::execution_ref::JobRef;
- use typed JobRef as trusted replay identity instead of parsing rendered prose;
- use shared Receipt::JobStatus/NodeStatus/Handoff/Cancel/Ack for shared-backed
  Jobs;
- delete local ControlReceipt only after parity;
- keep the tiny product-owned jobs_send receipt;
- replace generic bounded_job_error with aw_tools::diagnostic::bounded_diagnostic;
- preserve hostile-ref compactness and same-Job/no-rerun replay behavior.

Before editing, revalidate status/staging/HEAD/origin/diff and verify all AW Git
lock entries still resolve to 1a5d0f65....

Run focused D4 tests, strict Clippy/fmt/diff, update the owning TODO/handoff, and
pause at that coherent checkpoint.

Do not start D5 until D4 is green.
```

---

<!-- markdownlint-enable MD024 MD025 MD048 -->

## 2026-11-20 continuation ledger

This section provides a chronological delta after the inherited 2026-11-19
handoff.

## A. Dotlink D4-D7 and final equivalence

- D4 typed JobRef/shared receipt/bounded diagnostic cutover completed.
- D5 RawPageBytes/SerializedResultBytes cutover completed.
- D6 lightweight aw-mcp-protocol receipt/result fitting completed.
- D7 shared heartbeat cutover completed.
- full 0.17.5 semantic equivalence completed.
- signed local checkpoints:
  - fa01e18e3bb023dfe1d60b8ed45fd0ce53bfc0f7
  - 8b77bae48633d8c609b44ccec7291e5e20ab5624
  - d32a3196ec18ada7f85176a9feed42667e5f2d6b
  - 20b6f045f79fa7725f286e05b40e9262b1c9e4d3
- later tail-envelope/exact-tail hardening remains uncommitted.

## B. Fresh-review hardening

Reviews found and fixed:

1. canonical replay cache had to precede request projection;
2. Jobs fit/preflight had to measure final timestamped MCP envelope;
3. irreversible jobs_send needed final receipt preflight;
4. jobs_get optional tail had to fit final decorated result;
5. winner had to be the exact proven candidate, not a live re-read;
6. lossy UTF-8 boundaries required exact fallback for non-ASCII tails;
7. D5/D6 ACK safety had to hold at tiny response ceilings.

## C. Validated external review findings

Confirmed:

- policy replay-before-current-authority bug;
- audit lifetime caps/O(history);
- Session owner/participant conflation;
- Agent-source subscription gap;
- Remote Start issuer trust gap;
- early non-shared cancel CAS gap;
- stdin authority/effect linearization gap;
- Agent lookup/list/wait scaling costs;
- ordinary child-exit process containment latency bug.

Rejected as stale/superseded:

- old private StopCause::Teardown taxonomy;
- duplicate stop+exit lifecycle projection;
- runtime-wide 2100-second hosted cap;
- checkpoint minutes changing execution lifetime;
- old Windows sh test issue;
- older contradictory stopped_by wording.

## D. AW v36 admission/replay and query-index work

Implemented locally/uncommitted:

- v36 schema;
- policy_admission_current;
- remote_start_current;
- replay-first policy identity;
- Session owner / participant separation;
- participant epoch evidence;
- new-effect current authority;
- factual guardian settlement after revocation;
- projection revision monotonicity;
- legacy audit backfill;
- indexed Remote Start replay/counts;
- explicit v35 -> v36 open/migration regression with projection backfill and
  exact issuer-less v35 replay;
- `RemoteStartIssuer { client_id, trust_epoch }` plus
  `RemoteStartLease { target, issuer }`;
- issuer-bearing v2 Start authorization digest while pre-v36 issuer-less facts
  retain the exact v1 replay digest;
- issuer-independent exact Start recovery by immutable attempt+semantic
  identity before a new issuer/lease is obtained;
- indexed canonical Agent binding lookup over the existing authoritative
  Workspace/Session/branch rows, with current Session authorization rechecked;
- query-only `job_exposure_current` projection derived atomically from
  immutable commit-log positions and rebuilt at v36 migration;
- bounded keyset pagination for the mixed Agent/finite Job list under the
  existing position cursor contract; no new Agent/Job runtime or lifecycle
  registry was introduced.

Validation at this checkpoint:

- migration/open, Remote Start replay, participant, source, restart/wake and
  mixed-list focused regressions are green;
- aw-store lib and AW lib/bin checks are green;
- strict `-D warnings` Clippy is green for aw-tools/aw-mcp all targets,
  aw-store lib, and AW all targets (`--no-deps`);
- repeated aw-store all-target check/Clippy attempts are externally SIGKILLed
  by L5 while compiling aw-store without a compiler diagnostic and therefore
  are not classified as code failures;
- final fmt/diff checks are recorded in CURRENT_HANDOFF after durable-doc edits.

Issuer trust boundary:

- AW-side identity/wire/replay semantics are complete locally;
- dotlink still consumes published AW `1a5d0f65...`, so target-side trusted
  OAuth issuer issuance must wait for an explicitly authorized AW publish/repin;
- do not use a temporary path dependency and do not duplicate the issuer wire
  in dotlink merely to bridge the uncommitted revision.

## E. AW Agent/Turn source-event work

Implemented locally/uncommitted:

- serializable SourceEventBook;
- agent_job_event_sources;
- root result per attached Turn;
- exact Turn result+settled;
- root settled on actual Agent close;
- exact pending delivery persistence;
- agent_job_wake_admissions;
- fresh Router recovery;
- current authority revalidation;
- revoke/regrant stale-ref fencing;
- revision-watch jobs_get wake.

## F. Current pending order

Both active local source batches are now at coherent **uncommitted** checkpoints:

- AW v36 admission/Agent/source/query-index work is implemented and qualified;
- dotlink post-equivalence process/control + trusted issuer hardening is
  published and qualified at **706/0/1 + 42/42** with strict Clippy,
  fmt/diff, Alejandra and Markdown lint;
- dotlink consumes `RemoteStartLease`, requires explicit trusted OAuth bridge
  authority for new claims, binds authenticated `client_id` + trust epoch,
  rejects ordinary OAuth issuer self-construction, removes the temporary process
  status shim, and preserves exact committed replay after later trust revocation.

No further source change should cross one of the guarded boundaries implicitly.

P9/D4-D7/v0.17.5 equivalence, process/control hardening and trusted issuer
cutover are complete.
Native Windows/macOS runtime/deployment validation and trusted-selector
Bubblewrap remain external gates.

Sandbox/config-v13 remains frozen. No commit/push/repin/release/deploy without
explicit user authorization.

---

End of canonical handoff.
