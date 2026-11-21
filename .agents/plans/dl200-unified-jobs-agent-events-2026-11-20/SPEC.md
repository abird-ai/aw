# DL-200 — unified Jobs, Agents, Events, History and Checkpoints

Author: Prasanna Loganathar
Opened: 2026-11-20
Design revision: **R6 — architectural simplification, SEALED 2026-11-21, including the sealed shared Process/release-correction amendment**
Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION AMENDMENT SEALED / IMPLEMENTATION PROMOTED / PHASE-0 CORRECTIVE IMPLEMENTATION GATE PENDING**

## 0. Relationship to DL-150

DL-200 is the successor design layer for unified Jobs, Agents, Events, immutable
history/forks, collaboration/access, host input planning and source-aware checkpoints.

It **extends rather than rewrites** DL-150. The sealed DL-150 filesystem, Graph, result,
paging, mutation, replay and Process contracts remain inherited from:

- `.agents/plans/dotlink-tool-port/reference/DL150-LIVE-CA89F959-README.md`
- the preserved `DL150-SEALED-*` snapshots
- later shared Jobs/runtime implementation plans and handoffs

Where DL-200 explicitly changes a later Jobs/Agent surface, DL-200 is authoritative for
that surface. Everything else remains inherited.

DL-150 remains an immutable historical design anchor; DL-200 is a separate specification
so the evolution is reviewable and does not retroactively rewrite prior decisions.

### 0.1 One truth source, one mutation boundary

The architectural law is precise rather than metaphorical:

> **One authoritative source for each semantic truth. Mutable truth has one Owner and one
> linearization point; immutable truth has one stable Fact identity; an uncertain external
> effect has one Operation custody. Everything derived is a Projection/Cursor/adapter, never
> competing truth.**

**Global identity; authoritative-store-local consistency.** Canonical Domain/Principal/
Project/Agent/Turn/Job/Node/Sandbox refs may identify resources across the fleet. A ref
does not imply a global transaction, ordered cursor, revision clock or execution scheduler.

Within one authoritative store, an owner serializes and publishes its own mutations.
Atomic admission, Store positions, contiguous projection coverage, cursor high-waters and
ordered Event delivery are store-local. In v1, cross-store atomic mutation, ordered
`jobs_list(tree)`, Event→Agent Input delivery and merged Event cursors fail explicitly.
Product-owned multi-store workflows may use the existing explicit idempotent saga/outbox
pattern. Never turn a local success into an implicit global-consistency claim.

The common internal vocabulary describes **roles**, not six traits or services:

| Role | Invariant | Examples |
|---|---|---|
| **Owner** | Sole mutable authority and linearization point | Agent state, Job owner, Sandbox, Agent Input |
| **Fact** | Immutable authoritative evidence | HistorySpan, commit-log Event, ProcessTerminal |
| **Operation** | Replay/recovery custody of an effect | Existing Prepared constructor/remote Start receipt |
| **Projection** | Rebuildable read/index accelerator, never truth | Event subject index, Job exposure lookup |
| **Cursor** | Private bounded observation/progress, never authority | Jobs page, Event reader/consumer |
| **Scope** | One Process lifetime, independent of Sandbox state | Process child + platform containment |

One resource can reference several roles without creating competing owners. In particular
`agent_state_current` is **Owner** state after exclusive activation, not a projection;
SessionStore remains the Turn/history owner. Commit-log Events are Facts, not a second
Event runtime. Driver and registry definitions do not grant source/Tool/secret authority.

### 0.2 Small reusable implementation mechanics (private)

These are shared **mechanics**, not a generic framework, public API or new persistence
service. Preserve every existing semantic owner; implement a helper when it removes real
duplication. Existing typed owners may call the same helper with different policy and
transition rules. Crate placement is governed by actual dependency direction, not this
conceptual grouping.

**Store identity and position.** Reuse `store_meta.store_id` as the persistent
`StoreId`, and the existing `store_meta.restore_generation` as its incarnation fence:

```text
StoreScope = { StoreId, RestoreGeneration }
StorePos(u64) = position in this scope's immutable commit_log.seq
```

Do not mint Event-/Jobs-specific Store IDs, restore epochs or global sequence clocks.
Event indexed-through, Jobs publication/high-water and projection build-through use the
same zero-cost `StorePos`, comparable **only within one StoreScope**. A same-store SQLite
backup restore preserves `store_id` but advances `restore_generation`; a cursor, durable
consumer position or projection generation from the prior incarnation must fail stale or
be explicitly rebuilt/rebased from restored authoritative facts, never silently resume
against reused numeric `commit_log.seq` values. Persist scope affinity with private
ordered cursors and projection heads, not as a new public resource identity. Keep
Agent/Process/Graph `rev`, source `event_rev`, staged `inputs_rev`, group/security
generations and output byte offsets distinct. The `StoreScope` guard is a safety type,
not a distributed transaction or universally comparable revision.

**One owner-mutation discipline.** This is a review rule/helper sequence, not a generic
TransactionManager:

```text
recover exact prior operation/result if this identity already exists
-> for genuinely new work, preflight current Store-owned authority
-> derive a stable operation-owned recovery identity (pure planning, no external effect)
-> if anything external may have an uncertain effect, durably claim Prepared FIRST
-> acquire external evidence/fence and execute exact prepared effect using that identity
-> inside the real Owner transaction, revalidate Store-owned authority and external
   immutable/held-through-commit evidence; publish Owner state + Facts + outcome atomically
-> after commit, notify/wake/refresh reconstructible accelerators and release fences
```

Side-effect-free immutable evidence reads can precede `Prepared`. But issuing a provider
lease, capability or reservation **may itself be an effect**; if so, it happens only after
a durable Prepared claim with enough identity to inspect/reconcile it following failure.
A provider `plan()` that itself has effects is not a pure recovery-planning step. Simple
Store-only deterministic mutations skip Prepared/external-evidence ceremony. Pure
read-only disclosure may use fenced evidence without a Prepared mutation claim. Each
Agent/Input/policy/Start/constructor/Job owner keeps its real transaction and
specialized transitions; common helpers encode only identity, evidence and factual
outcomes.

**One effect/replay discipline.** Authenticated original operation identity + canonical
original-request fingerprint identifies the attempt. Exact replay returns retained
factual identity or a disclosure-safe non-retry acknowledgment; changed semantics conflict.
For an effect that may escape the Store, persist `Prepared` **before** that effect and
recover by its stable operation/driver identity after uncertainty. `Applied`, `Rejected`
and `Unknown` describe the shared effect outcome vocabulary, not a compulsory generic
state machine replacing `PolicyAdmissionState`, guardian phases, Job statuses or
`StopEvidence`. Store-only atomic mutations need no Prepared ceremony. The original
policy/Start/constructor/Job owners retain replay and recovery truth. No universal
OperationManager, global ledger or workflow engine is permitted.

**One external-evidence rule.** Store-owned authority is verified inside the owner's
final transaction. Externally mutable authority must instead supply either an immutable
non-revocable/versioned proof or a provider-enforced live fence held **through the actual
linearization/read decision**. Otherwise the result is Unknown/unavailable and the effect
is not published, disclosed or retried blindly. Separate the *durable*, bounded, non-secret
evidence/identity from the *live*, provider-owned fence handle; persisted descriptors never
contain reusable credentials or pretend a pre-commit `valid()` call is a fence. Acquire
the fence before a short Store transaction; do not perform unbounded provider I/O inside
SQLite write locks. This rule governs prepared SourceDriver publication, Event read/Input
disclosure, provider-backed projections and later Vault/provider integrations without
becoming a second authorization engine.

**One private projection-publication protocol.** A rebuild records its originating
`StoreScope`, projection generation and `built_through: StorePos`, populates a private
replacement and
under the final authoritative write/swap fence either proves the publication head
unchanged or catches up the intervening suffix before activation. A consumer never
advances above a contiguous safe prefix or scans a half-built generation. Event indexing
may publish incremental gap-free prefixes; Job exposure may build then atomically swap.
Share only the Store helpers for generation, prefix, head/fence verification and swap:
indexes, materialized columns, retention and query algorithms remain owner-specific.

**One opaque cursor envelope, distinct algorithms.** Reuse private authenticated cursor
identity, `StoreScope` affinity (StoreId + restore generation), selector/anchor binding,
projection/placement generation checks, expiry and progress fencing. An Event cursor stores an ordered log position and separate disclosure
checks; a Jobs cursor stores publication high-water, exact tuple and optionally a private
tree frontier/ancestor-gate chain. Current authority is always revalidated at read/commit;
a cursor is not a grant. Do not invent one generalized cursor traversal engine.

**One Event scanning core.** `events.read`, recipient subscriptions and EventHandlers
scan the same immutable Event projection through a bounded, source-first,
authority-scoped, placement- and indexed-through-fenced scanner. The private scanner
returns a candidate plus preliminary evidence, proven skip, blocked/unknown, stale or end;
it does **not** commit EventInputs, call Handlers or publish progress. Proven-deny/nonmatch
can advance privately; unavailable authority cannot. A public blocked response is
candidate-oblivious. Recipient Input admission rechecks the candidate in its own
same-store transaction with subscription generation, audience/manifest, causal budget
and deterministic dedupe. Handler execution remains at-least-once with its own receipt
and acknowledgment. Read response projection remains a separate pure consumer. This
shares authorization/scanning correctness without a second delivery runtime.

**One durable DriverKey.** Persist one stable typed key for built-in or Extension driver
identity (`namespace/extension + local id`) in SourceBindings, SourceAnchors and
RecoveryDescriptors. Persist the **artifact-specific schema beside that key** (`binding`,
`anchor`, `recovery`, etc.); do not overload DriverKey with one version that accidentally
couples unrelated formats. Runtime installation generation is a separate *live* fence and
is never historical identity. Store-open/recovery resolve `DriverKey + artifact schema`
through the existing registry compatibility check. This gives one durable driver identity
while allowing anchor/recovery/binding schemas to evolve independently.
Unresolved descriptors pin a compatible implementation until safe reconciliation. No
separate driver directory/authority service.

**One typed registry staging core.** The final leaf registries reuse small typed-key,
collision, local-revision, staged snapshot and atomic-publication mechanics from their
first consumer. An Extension registration stages several leaf changes and publishes
one coherent root snapshot; no leaf is replaced during Phase-5 composition. Family-specific
validation, durable pinning and activation/authority rules stay with their real owners.
Do not preimplement dormant registries merely to fill out a taxonomy.

**One dedicated `aw-process` crate.** This is the canonical, reusable, Agent-neutral
**async Process runtime** for AW, dotlink and future embedding hosts. It owns exactly one
private Scope and the actual child lifetime: prepared launch into containment, exact main
child identity/readiness, stdin/EOF, stdout/stderr pipe readers, wait, graceful/forced
stop, one bounded retry while the target is safe, pre-cleanup occupancy, bounded drain and
one `ProcessTerminal`. A host cannot retain a parallel wait/reap/stop/pipe-drain manager.

Crate direction is explicit:

```text
aw-tools                    ProcessTerminal / Occupancy / StopRecord facts (no OS runtime)
aw-platform                 synchronous OS-specific DirectScope / containment operations
       ^                        ^
       |________________________|
                  |
aw-process                  async Scope + pipes + stop/wait/drain/settle
       ^                        ^
       |                        |
AW tools/host adapter        dotlink shell/MCP adapter
```

`aw-process` depends downward only on the minimal `aw-tools` semantic feature set,
`aw-platform` and required async/OS primitives; it must **not** depend on AgentCore,
MCP, SessionStore, ToolPolicy or another process lifecycle owner. The AW adapter and
dotlink prepare already-authorized executable/Sandbox/environment choices and transfer
one prepared command **by value** into `aw-process`; the shared runtime then owns the
lifecycle-sensitive finalization: containment setup, stdio ownership, spawn, exact child
custody, read/drain lifetime, wait/stop/reap and terminal settlement. This keeps sealed
executable/Bubblewrap preparation, Tool/Sandbox authorization and runtime-profile policy
at the host edge without permitting `spawn -> attach later` to become the generic contract.

The AW adapter and dotlink also retain product transport, operator logs, deadlines and
output **storage/retention**. One small sink interface accepts pipe chunks and reports
factual success/failure; the shared runtime owns bounded backpressure, cancellation and
drain deadlines. A blocked or failed sink can never hold containment cleanup indefinitely;
dropped/unretained bytes are represented as truncation/incomplete evidence and are never
reported as fully captured output.

`ProcessTerminal` is the canonical cross-host terminal Fact. AW's existing
`ProcessReceipt` remains the single AW adapter envelope around that Fact plus AW-specific
snapshot/spool access; dotlink retains its product JobCompletion/result envelope around the
same Fact. Do **not** create a generic `SharedProcessReceipt` or move JobManager/ACK/quota/
paging ownership into `aw-process`.

`aw-process` owns DL-200 Process/Job execution lifetimes. The existing `aw-terminal`
Terminal/PTY subsystem is a distinct custody resource with reviewed source-release/adoption
semantics that may intentionally transfer child/PTY custody. It remains separate rather
than forcing transferable release/disarm semantics into ordinary Process Scope. A tiny PTY
compatibility adapter is not a second DL-200 Process Job runtime.

This crate is **required in Phase 0**, not conditional on whether an existing module
could technically hold the implementation. Create it to replace both AW's current
`aw-tools-adapter::process` lifetime machinery and dotlink's guard/wait/reap path; only
remove the old paths after real AW+dotlink compilation and behavioral parity prove the
same owner. Do not wrap two pre-existing runtimes and call it shared.

### 0.3 Non-collapsible distinctions and adoption

Preserve: Agent ≠ Sandbox ≠ Scope; Agent owner ≠ internal Session journal; history ancestry
≠ delegation lineage; disclosure provenance ≠ live grants; Event subscription ≠ Handler;
definition ≠ activation; `rev` ≠ `event_rev` ≠ `StorePos`; Process physical exit ≠ stop
evidence ≠ occupancy ≠ output completeness. These distinctions carry correctness.

The shared primitives above land **inside the existing seven implementation phases**.
No R6-only implementation phase, public API, second persistence owner or release gate is
introduced. See `IMPLEMENTATION.md` for exact placement and grouped acceptance. R5
remains the most recent sealed revision until this R6 proposal is explicitly accepted.

---

## 1. Design goals

1. **One small model-facing Jobs vocabulary.**
2. **One authoritative owner per mutable execution object.**
3. **Agent scheduling belongs to the host/Actor owner, not to `jobs_send`.**
4. **Committed history is immutable; mutable heads/control state are cheap projections.**
5. **Forks structurally share immutable history and receive independent live owner state.**
6. **Multi-user concurrency is explicit without turning every object into a miniature CAS domain.**
7. **Model API and host/operator API share lower owners but are separate policy surfaces.**
8. **No second Job registry, queue scheduler, event bus, replay ledger, or runtime.**

---

## 2. Public execution model

The normal public work tree is:

```text
Agent
├── Agent
├── Process
└── Graph
    └── Node
```

Turns are an Agent timeline, not ordinary work-tree children:

```text
Agent
├── Turn 1
├── Turn 2
└── Turn N
```

Kinds remain factual result metadata:

```text
agent | process | graph | turn | node
```

They are **not** initially exposed as a `jobs_list` query filter.

---

## 3. Global resource namespace

DL-200 has one canonical **hierarchical ownership/execution resource tree** for public
addressing and Event routing. Execution is one branch of that tree; Sandbox is a
Project-level sibling resource:

```text
/d:<domain>
    /i:<principal>
        /p:<project>
            ├── /a:<agent>
            │       /t:<turn>
            │           /j:<job>
            │               /n:<node>
            └── /s:<sandbox>
```

The `s:` branch is deliberately **not** an `ExecutionRef`. Jobs/control continue to
operate only on the Agent/Turn/Job/Node branch; Sandbox operations use a distinct typed
`SandboxRef`.

`i:` is an **identity principal namespace**, not specifically a user. A principal may
be a user or group; principal handles are unique within one Domain across principal
kinds.

The Agent ref is the public durable identity of the collaboration/history/execution
object. There is no public Session coordinate.

### 3.1 Concrete execution refs

Compact contextual forms remain:

```text
.                       current Agent
^                       parent Agent

a:2                     another Agent in current Project
t:7                     Turn 7 of current Agent
a:2/t:7                 Turn 7 of Agent 2

j:<id>                  finite Process or Graph Job
j:<id>/n:<n>            Graph Node

p:core/a:2
i:alice/p:core/a:2
d:remote/i:alice/p:core/a:2
```

Canonical concrete execution form:

```text
/d:<domain>/i:<principal>/p:<project>/a:<agent>[/t:<turn>][/j:<job>][/n:<node>]
```

Concrete Jobs/control APIs accept concrete refs only. Wildcards are never valid
`ExecutionRef` values.

`.` and `^` are contextual Agent aliases, not filesystem traversal syntax. Do not
add `..` or compose `^/t:...`.

### 3.2 Intermediate resource refs

The same namespace also names intermediate and sibling resources for host/Event
observation:

```text
/d:acme
/d:acme/i:alice
/d:acme/i:alice/p:core
/d:acme/i:alice/p:core/s:dev
```

Canonical Sandbox form is:

```text
/d:<domain>/i:<principal>/p:<project>/s:<sandbox>
```

This lets Domain, Principal, Project and Sandbox changes live in the same Event address
space as Agents and Jobs without inventing a second routing namespace or admitting
Sandbox refs to Jobs mutation APIs.

Do **not** force every immutable/content/provider object into this path tree. These remain
typed opaque IDs/refs with their own owner relationship:

```text
CheckpointId
HistoryBaseId / history_fact_id
Event.id
SourceRef / SourceAnchor
VaultEntryRef
ExtensionId / registration key
```

They may appear inside `ResourceRef` for typed library/View composition, but they are not
new ExecutionRef segments. When such an object's lifecycle needs observation, the Event
is emitted from its owning hierarchical resource (for example checkpoint-list change from
the source Agent) rather than inventing a `c:`/`h:` routing subtree.

### 3.3 Event subjects use NATS wildcard semantics

Events use a dot-delimited subject projection of the same typed resource namespace, with
the event name as the final token. The slash-form ExecutionRef handle grammar is preserved
as-is; Event projection **encodes** literal handle payloads so a valid `.` inside a handle
cannot become a NATS token separator. Any concrete resource prefix may emit an Event:

```text
d:<domain>.e:<event>
d:<domain>.i:<principal>.e:<event>
d:<domain>.i:<principal>.p:<project>.e:<event>
d:<domain>.i:<principal>.p:<project>.s:<sandbox>.e:<event>
d:<domain>.i:<principal>.p:<project>.a:<agent>
    [.t:<turn>][.j:<job>][.n:<node>].e:<event>
```

The typed hierarchy is never skipped in a concrete subject: a Project Event has
`d/i/p/e`, a Sandbox Event has `d/i/p/s/e`, an Agent Event has `d/i/p/a/e`, and a
Job Event has the complete `d/i/p/a/t/j/e` prefix.

Examples:

```text
d:acme.i:alice.p:core.s:dev.e:changed
d:acme.i:alice.p:core.a:2.e:changed
d:acme.i:alice.p:core.a:2.e:#ready
d:acme.i:alice.p:core.a:2.t:7.e:settled
d:acme.i:alice.p:core.a:2.t:7.j:test.e:result
```

This is an Event routing representation, not an `ExecutionRef`; concrete Jobs/control
APIs continue to use the slash-delimited canonical refs from section 3.1.

Literal handle payloads use one canonical reversible percent encoding inside Event tokens.
With the current ASCII Handle grammar, ordinary alphanumeric/`_`/`-`/`~` bytes remain
literal and `.` is encoded as uppercase `%2E` (with `%` itself reserved/encoded if ever
admitted by a future Handle grammar). Decoding must reject malformed/non-canonical escapes.
For example:

```text
ExecutionRef project handle  foo.bar
Event token                  p:foo%2Ebar
```

The typed parser decodes literal payloads back to the exact Handle before authorization.
`*` is never decoded as data: `p:*` is the one-token typed wildcard. `>` remains a whole
terminal wildcard token. This keeps matching unambiguous without changing existing public
resource handles.

Use NATS wildcard **cardinality and terminal semantics** exactly while preserving the
typed resource token:

```text
a:* / s:* / p:* / i:* / d:* / e:*   matches exactly one token of that declared kind
>                             matches one or more trailing tokens and must be terminal
```

In ordinary NATS the one-token wildcard is the whole `*` token. DL-200's subject
projection keeps the resource kind in the same token, so `a:*` is the canonical typed
spelling of that one-token wildcard. It may match `a:2`, never `p:2`. This is the
only intentional syntax specialization; `>` keeps NATS's terminal one-or-more rule.

Contextual subjects may omit already-known leading Domain/Principal/Project tokens and
are resolved against the current Agent before matching.

Examples:

```text
a:2.>                       every Event from Agent 2 and its Turn/Job/Node subtree
a:*.e:#ready                #ready custom Events from Agent roots in current Project
a:*.e:*                     every direct Agent-root Event, system or custom
a:*.>                       every Event under every Agent in current Project
s:*.e:changed                changed Events from authorized Sandboxes in current Project

p:*.a:*.e:#ready            #ready custom Events across Projects for current Principal
p:*.a:*.>                   every Agent-subtree Event across current Principal's Projects

i:*.p:*.a:*.>               Domain-wide authorized Agent-subtree Events

d:acme.i:*.p:*.a:*.>        explicit Domain-wide selector
d:*.i:*.p:*.a:*.>           cross-Domain selector when policy explicitly permits it
```

Because every Event subject has at least the trailing `e:<event>` token,
`a:2.>` naturally includes Events emitted directly by Agent 2 as well as Events from
its namespace descendants while preserving NATS's one-or-more `>` rule.

Rules:

- no partial-token globs or regex; in particular there is no `e:#*` custom-only glob;
- `e:*` matches exactly one Event-name token, whether system or custom;
- `>` is terminal only;
- Event patterns use a distinct typed `EventSubject` / `EventPattern` parser and can
  never reach mutation APIs such as send/cancel;
- wildcard subscriptions are dynamic and may match future resources/events;
- authorization is evaluated for every concrete matching Event at read/delivery time;
- patterns never grant discovery or access by themselves;
- selector/subscription complexity and delivered Event volume are bounded; broad
  patterns are indexed/projection-driven rather than expanded into per-source rules;
- model-facing cross-Project/Principal/Domain patterns require corresponding current
  source visibility and host-granted Event capability;
- **v1 cross-Domain patterns are supported only for Domains indexed by the same authoritative
  Event store**. The exact concrete Domain, Principal, source and recipient policies are still
  checked per Event. Domains whose Events reside in another store/host are not silently
  federated into the local cursor; such a cross-store request is explicitly unavailable,
  not a partial result disguised as a complete subscription/read. A future federated
  cursor/merge transport needs its own reviewed owner contract, not a hidden aggregator.

The namespace tree, Agent delegation relation and immutable-history ancestry are three
distinct things. A child Agent is a new globally addressable Agent, normally a sibling
under the destination Project rather than a syntactic descendant of its parent Agent.

`parent_agent` means immutable creator/delegation lineage only. It is assigned once at
child creation and never retargeted. It powers `^`, Jobs `children/tree`, depth/budget
accounting and parent lifecycle facts. `from=` / HistoryGrant ancestry is a
separate history-source relation and never defines `^` by itself. Therefore a model Agent
may create a child whose `parent_agent` is the current Agent while `from=` points at a
different authorized Agent/Turn. A host-created historical fork outside an Agent
delegation context has no parent unless a trusted creator context supplies one.

---

## 4. Final model-facing Jobs surface

Exactly six Jobs tools:

```text
jobs_list
jobs_get
jobs_read
jobs_send
jobs_ack
jobs_cancel
```

Durable event subscription is generic `events_subscribe`, not a Jobs operation.

Model Tool availability and resource authority are always **conjunctive**. Possessing a
`jobs_*` Tool grant authorizes that operation shape; it never grants visibility/control of
a canonical ref by itself. Every call reauthorizes the exact target and current actor/
authority generation:

- `jobs_list/get/read` -> current target `view`/read authority;
- `jobs_send` to Agent -> current `submit` plus exact Tool capability; Process stdin/EOF ->
  the exact Process input/control capability inherited from DL-150;
- `jobs_cancel` -> current target `control`/cancel capability; Graph Node cancel remains
  rejected in v1;
- `jobs_ack` -> exact retained-output release authority/proof from DL-150, not Agent `rev`.

A broad Jobs Tool registration can therefore never become cross-Agent discovery or
control authority.

There is no public:

```text
jobs_control
pending send mode
queue / steer / interrupt send mode
pause_after_turn
queue CRUD
process_close_stdin
process_terminate
agent/queue/process observer tool
```

The useful invariants behind those historical SDK tools move into the authoritative
owners/capabilities; the duplicate model-facing vocabulary does not survive.

---

## 5. `jobs_list`

Conceptual schema:

```text
jobs_list(
    ref    = ".",
    scope  = "children" | "tree" | "turns",
    limit  = <bounded>,
    cursor = <opaque>
)
```

Defaults:

```text
ref   = "."
scope = "children"
```

### 5.1 `children`

List the anchor's natural direct public children.

- Agent -> child Agents + live/retained Processes + Graphs owned by that Agent
- local Turn -> concrete Jobs spawned by that Turn
- inherited Turn -> **no destination-local Job children are fabricated**; its historical
  Process/Graph/tool/effect facts are read through the Turn's `messages/tool_calls/
  tool_outputs/trace/result` history views and retain original provenance
- Graph -> Nodes
- Process -> empty

Turns are deliberately excluded from an Agent's normal child listing. Exact history forks
share immutable historical work facts but never mint ghost `j:`/`n:` runtime identities in
the destination Agent.

### 5.2 `tree`

Recursive natural work tree.

Turns remain excluded from the work tree. A Graph's Nodes are part of the Graph subtree.

### 5.3 `turns`

Explicit Agent timeline view. Valid only for an Agent anchor. For structurally inherited
Turns, rows use the destination Agent timeline ref while also exposing the stable
`history_fact_id` and authorized original provenance/ref needed to show that this is the
same immutable inherited fact rather than a copied/re-authored Turn.

### 5.4 Traversal never grants authority

`children` / `tree` lineage is navigation, not access. Every candidate row is authorized
against the exact concrete target before it is returned. An Agent child that the caller
cannot view is omitted and its Agent subtree is not recursively traversed; parent/delegation
lineage therefore cannot reveal a private child's existence, Project or descendants.

Pagination cursors retain hidden scan/traversal state privately and are bound to the
authenticated caller, exact anchor ref and normalized scope. The first page also captures
an immutable **publication high-water position** from the existing authoritative exposure/
publication ordering. Every later page considers only identities published at or below that
high-water mark, with canonical ref as the deterministic tie-breaker. New children/work
published afterward appear only in a fresh listing; no expensive database snapshot is
promised.

The canonical ordering is exactly (the position is one store-local `StorePos`
projected from the existing `commit_log.seq`, not a second global sequence):

```text
publication_position DESC, canonical_ref ASC
```

The opaque cursor retains **both** tuple fields; a page boundary inside one shared
publication position therefore cannot skip or repeat tied rows.

For `children`/`turns`, the cursor keeps the private last scanned ordering tuple. For
`scope=tree`, traversal is deterministic pre-order with each sibling set ordered by that
same tuple, and the opaque cursor retains the traversal frontier/child scan positions plus
the private Agent ancestor-gate chain needed to resume each queued subtree.

A stored frontier is navigation state, never retained traversal authority. Before resuming
or descending through a frontier Agent, reauthorize that Agent **and every still-relevant
Agent ancestor gate on the queued path** under current authority. If any gate is no longer
viewable, discard that entire queued subtree even if a descendant would be directly
viewable through `jobs_get`; tree traversal cannot tunnel through a now-hidden ancestor.
Hidden child rows are omitted and their subtree is not traversed. Gaining access later does
not rewind already-scanned cursor state.

Use the existing immutable publication/timeline position appropriate to the authoritative
owner; if an exposure projection needs that key, store the owner's existing commit/
publication position as projection data rather than minting another global sequence.

One `jobs_list` traversal is **exposure-store local in v1**. Reuse the common
private authenticated cursor envelope and `StoreScope + StorePos` provenance, but
keep Jobs high-water/tree-frontier traversal separate from Event scanning. The opaque
cursor binds the existing Store ID **and restore generation**; restoring an earlier
same-Store backup makes pre-restore pages stale, even if their numeric position exists. parent/delegation edges used for recursive
Jobs traversal must have traversal/publication evidence in that same store. A cross-store
recursive selector is explicitly unsupported/fails closed rather than merging independent
publication positions. The globally shaped namespace does not imply a federated Jobs
cursor.

Exposure-index rebuilds must never expose holes below a cursor high-water. Authoritative
Agent/Process/Graph publication writes the exposure row transactionally when possible. A
full rebuild constructs a replacement exposure index privately through a captured
publication head `H`, but **must close the swap race** before activation: under the store's
final write/swap fence, either verify `current_publication_head == H`, or catch the
replacement up for `(H, current_head]` under that same fence, then atomically switch the
active generation. If the head cannot be reconciled safely, abandon/retry the replacement;
never swap a stale `H` generation over newer active rows. Consumers never scan a partially
rebuilt generation. If a future implementation uses incremental online repair instead, it
must provide the equivalent contiguous safe-prefix fence before issuing a high-water.
This prevents duplicates/misses under concurrent publication/rebuild while keeping the
cursor cheap and rebuildable.

Direct `jobs_get/read/send/cancel` still reauthorize the exact target independently.

### 5.5 No initial `kind` / `state` filters

Rows contain compact `kind` and `state` facts. The bounded caller can filter locally.

Do not begin with a generic database-query surface. Add server-side filters only if
measured workloads prove they are necessary.

---

## 6. `jobs_get`

Conceptual schema:

```text
jobs_get(
    ref     = ".",
    rev     = <optional observed owner revision>,
    wait_ms = <bounded>,
    tail    = <bounded Process convenience>
)
```

Purpose:

- compact status
- owner revision observation
- efficient wait-for-change
- small Process tail convenience where supported

If `rev` equals the current owner revision and `wait_ms > 0`, wait until that owner
revision changes or the deadline/cancellation fires, then reauthorize and reread.
**Efficient wait is mandatory:** subscribe to the existing owner-local revision notifier,
recheck the authoritative revision after registration to close the lost-wakeup race, then
await a change/deadline/cancellation without a fixed-interval database polling loop. A
notification is only a hint; every returned snapshot and its disclosure are freshly
validated by the real owner. Owner restart/notification loss falls back to one authoritative
reread, not a second revision truth or notification service.

`rev` is discussed normatively in section 12.

---

## 7. `jobs_read`

Conceptual schema remains the natural detailed read surface:

```text
jobs_read(
    ref      = ".",
    view     = ...,
    filter   = ...,
    cursor   = ...,
    offset   = ...,
    limit    = ...,
    encoding = ...,
    wait_ms  = ...
)
```

Natural views depend on target owner:

- Agent / Turn: result, messages, tool_calls, tool_outputs, trace
- Process: stdout, stderr, result
- Graph root: synthesized result
- Graph Node: natural node output/result

Detailed host input-plan state is **not** exposed through model-facing Jobs.

---

## 8. `jobs_send`

Conceptual model-facing schema:

```text
jobs_send(
    ref   = ".",
    input = { text: ... } | { eof: true }
)
```

No public mode.

### Agent

Deliver text to the target Agent's authoritative input owner.

Jobs does not choose queueing, steering, interruption, priority, or Turn scheduling.

### Process

Deliver stdin bytes/text or EOF to the exact Process owner.

### Turn / Graph / Node

Model-facing `jobs_send` is rejected.

New Agent input targets the Agent. Host/operator steering and interruption are separate
host input intents described below.

---

## 9. `jobs_cancel`

Conceptual schema:

```text
jobs_cancel(ref = <required>)
```

Exact refs carry the target identity:

- Agent -> close/cancel lifecycle where policy allows
- Turn -> cancel exact active Turn
- Process -> terminate exact Process
- Graph -> cancel Graph
- Graph Node -> reject in v1; cancel the owning Graph or an exact concrete child Process/Agent when that is the intended target

Ordinary model-facing cancellation does **not** require a revision fence. The owner
atomically checks exact target identity and current state. Repeated cancellation is
idempotent/unchanged where factual state permits.

A trusted host API may optionally support `if_rev` for strict "only if unchanged since
I observed it" UX.

---

## 10. `jobs_ack`

Explicit ref required.

Retained finite output ownership remains distinct from observation. Reading bytes does
not grant their release.

This continues to use delivered-offset / retained-output proofs rather than a generic
Agent revision.

---

## 11. Event subscription is generic

DL-200 removes `jobs_subscribe` from the Jobs namespace.

Model-facing durable subscription is:

```text
events_subscribe(subject, enabled=true)
```

The current Agent is the implicit recipient and the host owns later scheduling.

The shared Events API, event revisions and durable delivery rules are defined in section
26.

---

## 12. Owner revision model

### 12.1 What `rev` is

`rev` is an **owner-local monotonic public-state revision**.

It is:

- a change token
- a wait/watch baseline
- an optional host optimistic-concurrency fence

It is **not**:

- committed-history identity
- a security token
- a participant permission epoch
- an event revision
- a replay identity
- globally comparable across Agents/forks

### 12.2 One revision per authoritative owner

```text
Agent       -> Agent rev
Turn        -> uses owning Agent rev

Sandbox     -> Sandbox rev

Process     -> Process rev

Graph       -> Graph rev
Graph Node  -> uses owning Graph rev
```

No separate Turn or Node revision is needed. Sandbox revision is independent from every
Agent bound to it; a shared Sandbox mutation never fans out synthetic Agent revisions.

### 12.3 What advances Agent rev

Advance exactly once per logical transaction that changes state **owned by that Agent**,
for example:

- lifecycle transition;
- active Turn creation/state/settlement;
- committed input custody/admission transition;
- steer/interrupt authority transition;
- Agent-owned subscription transition;
- direct child/delegation relationship attachment when a new child Agent is created;
- direct Agent ACL/configuration transition, including VaultGrant binding changes.

Do not advance merely because:

- a read occurred;
- a child Process emitted more stdout;
- a child Graph node made internal progress;
- a cursor/page was read;
- host-only staged input text was added/edited/reordered before Send;
- a referenced group's membership changed;
- a user/group security generation changed;
- unrelated legacy store metadata changed.

A referenced group owns its own revision. Group membership changes can alter effective
authorization without mutating every Agent that references the group. Security-sensitive
waits/effects revalidate the Agent ACL generation plus the exact referenced principal/group
generations and fail closed when those dependencies change. That derived
`access_generation` is authority evidence, not Agent `rev`.

The current implementation's legacy Session-audit-derived revision is a useful bridge
but is too broad for the final semantic contract. Final implementation persists an
Agent-owner revision rather than equating it with a backing-store audit clock.

### 12.4 Model-facing use

The main model-facing use is:

```text
jobs_get(ref, rev=N, wait_ms=T)
```

meaning: "I have already observed owner version N; wait efficiently until it changes."

Do not require `rev` on ordinary additive `jobs_send`, exact-target cancellation, or
ACK. Exact refs, owner state checks and replay/idempotence are the correctness boundary.

### 12.5 Host-facing use

Trusted host mutations may expose optional/required `if_rev` where the user's intent
is explicitly snapshot-relative.

This is particularly valuable for shared editable input plans and destructive operator
controls.

---

## 13. Immutable history and forks

Committed conversation/history nodes are immutable and identified by their exact
head/node identity. The mutable branch/Agent owner points at that immutable history.

Forking therefore does **not** copy or continue the parent's owner revision.

Conceptually:

```text
Parent Agent
  immutable head = H
  rev = 84

fork at H
       │
       ├── Parent continues from H    rev=85...
       │
       └── New fork starts at H       rev=1...
```

The two owners share immutable ancestry structurally but have independent mutable live
state.

A revision from one fork is meaningless on another fork.

For public "fork what I saw", use the exact **settled TurnRef** returned by the observed
history snapshot. That boundary remains valid even if the source Agent advances
concurrently. Trusted in-process code may use a typed HistoryBaseRef directly, but there
is no public opaque-head or `if_rev` fork mode.

Staged host input plans are mutable uncommitted intent and are **never copied by the v1
fork constructor**. A new fork starts with an empty staged plan. A trusted host that wants
the same draft text may separately recreate selected drafts through the ordinary
`agent.inputs.*` API with fresh destination provenance; that is not fork inheritance.

Committed-but-not-yet-historical live inputs remain owned by the original live Agent
and are likewise not implicitly duplicated into a fork anchored at an earlier
immutable head.

---

## 14. Multi-user semantics

A shared Agent has **one Agent revision**, not one revision per participant.

Example:

```text
Alice observes a:2 rev=41
Bob   observes a:2 rev=41

Alice performs a meaningful owner transition
a:2 -> rev=42

Bob's strict host action if_rev=41
-> stale/conflict -> reread
```

For additive model-facing input, Bob need not carry the revision; the owner serializes
both accepted inputs and records provenance.

Authorization remains independent:

- membership_epoch = participant grant generation
- audience_epoch = disclosure generation
- writer/owner generations = current mutation authority
- rev = public mutable state change token

Never treat `rev` as a substitute for those authority epochs.

Each Agent has its own revision. Child-Agent execution does not bubble every transition
into parent Agents; only relationship/membership changes actually owned by the parent
advance the parent's revision.

**Session/Agent owner and initiating actor are different identities.** An Alice-owned
Agent may accept Bob's policy-routed ToolCall only after AW validates Bob's current
participation/ACL, writer/Turn claim, Bob's own source/ToolPolicy/provider/payer ceilings
and the destination resource authority. Persist both the immutable resource owner and
Bob's authenticated subject/participation epochs on the original invocation claim. The
Session owner remains the storage/writer coordinator, never an implicit grant of Alice's
provider/tools or disclosure to Bob. Revoking Bob prevents fresh effects and private
receipt disclosure; it does not rewrite factual results of already admitted effects.

---

## 15. Host Agent input-plan API

The host gets an editable input plan above the immutable runtime boundary. Keep the API
small:

```text
agent.inputs.list
agent.inputs.add
agent.inputs.edit
agent.inputs.delete
agent.inputs.send
```

`list` is a pure snapshot read. Input-plan change observation uses the shared Events API;
there is no separate `get` or `watch`.

### 15.1 `agent.inputs.list`

```text
agent.inputs.list(ref=".")
```

Returns the staged plan plus one coarse `inputs_rev`.

Input-plan changes are observed through the shared Events API.

### 15.2 Staged intent kinds

```text
followup(after = optional exact Turn)
steer(turn = exact active Turn)
interrupt(turn = exact active Turn)
```

The exact Turn is captured when the item is created. A staged steer/interrupt never
silently retargets a later Turn.

### 15.3 `add`

```text
agent.inputs.add(
    ref   = ".",
    items = [...],
    rev?
)
```

Blind append may omit `rev`. Inserting relative to existing items requires the observed
plan revision.

The server assigns opaque host input IDs such as `q:<id>`. They are not Jobs.

### 15.4 `edit`

One operation edits both content and position:

```text
agent.inputs.edit(
    ref = ".",
    id,
    text?,
    intent?,
    before? | after?,
    rev
)
```

This replaces separate update/move APIs. Editing intent may move the item between the
current-Turn control lane and follow-up lane.

### 15.5 `delete`

```text
agent.inputs.delete(
    ref = ".",
    ids = [q:...],
    rev
)
```

Only staged items are deletable.

### 15.6 `send`

```text
agent.inputs.send(
    ref = ".",
    ids? = <selected staged IDs; omitted means all>,
    rev
)
```

Once Agent lifecycle enters `closing`, the staged input plan is frozen: `list` remains
readable but `add/edit/delete/send` reject. This preserves the exact terminal audit/UI
snapshot and avoids mutable drafts accumulating on an Agent that can never admit them.

This is the irreversible boundary:

```text
staged
  editable / reorderable / deletable
        │
        ▼ send
runtime custody
  immutable / scheduler owned
```

Correction after Send is another input, never history rewrite.

### 15.7 Permissions and forks

The staged plan is shared only where Agent access permits it. Every item retains
immutable creator/source provenance plus mutable last-editor metadata.

- `view` -> observe allowed staged content;
- `contribute` -> add/edit/delete the caller's permitted staged follow-ups;
- `submit` -> commit permitted follow-ups;
- `control` -> stage/send steer or interrupt;
- `manage` -> manage other users' staged items and admission controls.

Normative storage/transaction semantics:

- the plan has one persisted owner-local `inputs_rev`; every successful
  add/edit/delete/reorder/send transaction advances it exactly once;
- blind append allocates the next order key transactionally; relative insertion/edit is
  CAS-fenced by `inputs_rev`;
- implementation may use sparse integer order keys and atomically rebalance them when a
  gap is exhausted; rank representation is never public;
- staged text uses the existing private payload-custody/encryption primitive rather than
  plaintext audit rows;
- `send(ids...)` validates the complete selected set first, then transfers every item
  to the existing Agent queue/control owners atomically with deterministic request IDs;
  no partial Send is allowed;
- a stale exact-Turn `steer` or `interrupt` makes the whole Send fail and leaves all
  selected items staged; it never silently retargets a later Turn;
- a `followup(after=t:N)` is an ordering lower bound and may still be admitted later
  after N has settled.

Staged changes advance `inputs_rev` and emit `changed` with `data.aspect="inputs"`; they do not
advance Agent `rev` until Send crosses into authoritative runtime custody.

Staged drafts are mutable host state, not immutable history, and are never inherited by
a fork by default.

---

## 16. Host scheduling

All committed Agent input enters one authoritative Agent input owner.

The host decides when an input is:

- queued
- admitted as a new Turn
- applied as a steer at a safe authority boundary
- applied as an interrupt at the earliest factual safe boundary

These dispositions are **not** model-facing Jobs parameters.

Interrupt is not "kill every tool immediately". Already-started paid/external/effectful
work must reach a factual safe receipt; not-yet-started work may be suppressed and the
new authority segment applies at the earliest safe boundary.

---

## 17. Revision summary

Keep the revision domains intentionally small:

```text
Agent owner       rev
Sandbox owner     rev
Process owner     rev
Graph owner       rev
Host input plan   inputs_rev

Turn              uses Agent rev
Graph Node        uses Graph rev
Input item        uses inputs_rev
```

Immutable history uses exact head/node identity instead of these revisions.

Event streams use their own event revisions.

Security/participation uses its own epochs.

This separation is normative.

### 17.1 One mutation replay envelope, not per-API request fields

DL-200 semantic method signatures stay small; do **not** add an ad-hoc `request_id` field
to every create/set/send operation. The model/tool adapter or host RPC boundary supplies
one authenticated bounded mutation identity from its existing request/tool-call envelope.
The lower owner records/reuses it where replay could otherwise duplicate an effect.
All owners reuse the private replay/evidence rules in section 0.2; the common mechanics
never replace owner-specific `PolicyAdmissionState`, guardian/approval transitions, stop
records or resource-specific receipt schemas.

Conceptually:

```text
MutationReplayKey = authenticated caller/scope + semantic operation + request identity
request_fingerprint = canonical hash of the complete semantic request
```

Rules:

- same replay key + same fingerprint -> **never repeat the mutation**. Effect/outcome identity
  is stable; exact payload/detail is replayable only while retained and currently
  disclosable. Constructors retain the exact created canonical resource identity/identities
  in the resource/tombstone/replay owner for the full replay-identity lifetime, so replay
  still names the same Agent/Sandbox/Checkpoint/Principal. Effectful operations whose full
  receipt was compacted return the canonical compact non-retryable acknowledgement carrying
  only the retained terminal fact/disclosure-safe detail;
- same replay key + different fingerprint -> conflict/denied as request-identity reuse;
- model calls naturally use their durable tool-call identity; host transports use their
  existing authenticated RPC/request identity;
- adapters that cannot provide a durable identity may execute a mutation once but must not
  claim safe automatic replay after a lost response;
- owner-local idempotent setters/cancel/close may additionally return unchanged current
  facts when semantics permit, but that does not weaken request-fingerprint checks;
- non-idempotent external effects keep the inherited `Prepared/Applied/Rejected/Unknown`
  and no-blind-retry rules. A replay key never turns an uncertain effect into success;
- the existing owner/operation/receipt stores remain the replay truth. Replay evidence is
  durable owner provenance, not an expiring in-memory dedupe cache: constructors bind the
  request identity/fingerprint to the created/tombstoned resource or immutable operation
  receipt; Event publication binds it to the persisted Event; input/control uses the
  existing queue/control facts. Logical close/delete/tombstone does not make the same
  request identity available to create a different resource later;
- **replay lookup precedes contextual-ref resolution and current effect admission**.
  Authenticate the original caller/request identity; find the claim by stable origin,
  operation and ToolCall/request ID, then compare the *original* request fingerprint,
  including its literal requested ref. A matching replay retains the stored canonical
  target/accepted decision even after `^` reparenting, writer change or Policy revision;
  it never creates another claim for a changed resolved target. Current disclosure is
  checked **separately** before returning a cached payload, while factual success/Unknown
  remains stored. Fresh invocations alone resolve contextual refs and revalidate current
  grants, writer, payer, guardian budget and policy before effects;
- policy admission and remote Start use their existing **indexed current replay owners**
  (`policy_admission_current` and `remote_start_current`) backed by immutable audit. No
  lifetime audit-event count or O(Session-history) JSON scan may determine whether an
  otherwise valid **new** invocation is accepted. Per-active-Turn work remains bounded.
  Replay retention has two representations in the **same owner**, never a second ledger:
  a full recovery/receipt record while payload/detail is useful, then a compact replay
  tombstone containing only the replay key, original semantic fingerprint, terminal
  disposition (`Applied|Rejected|Unknown` or equivalent), and minimal integrity evidence
  sufficient to guarantee that identity can never execute again. Large payloads/provider
  details/current-authority snapshots may be dropped independently. The compact tombstone
  may disappear only when that replay identity itself can no longer be legally presented;
  if durable ToolCall/request identity lives with retained Agent/history indefinitely, the
  compact tombstone lives with that identity rather than using a TTL that could re-enable
  an old effect. Replay lifetime is explicit per identity namespace: a model ToolCall tied
  to retained Agent/history keeps its tombstone with that retained identity; a host RPC
  namespace may retire a tombstone only after its authenticated transport/owner guarantees
  that old request IDs from that namespace are permanently rejected. If no such irreversible
  presentation boundary exists, retain the compact tombstone. A TTL by itself never makes a
  consumed identity executable again. Startup/migration backfill may scan audit once, but
  ordinary admission cannot.
- when an exact replay matches but current disclosure no longer permits its protected
  content, return only a minimal **non-retryable replay acknowledgement**. It may reveal no
  target ref, Job ID, provider details, output metadata, timestamps or protected payload.
  Where even effect existence is protected, collapse this further to an opaque
  prior-invocation/non-retryable response. Withholding content never erases replay identity
  and never permits a replacement effect. Apply the same rule to policy Tool effects,
  Remote Start and paid guardian receipts.
- do not create a second generic replay ledger merely for DL-200.

This applies uniformly to resource construction and additive mutations that can otherwise
duplicate on retry, including `agent.create`, `sandbox.create`, `checkpoint.create`,
`group.create`, staged input append/Send, custom Event publication and Extension Actions
that invoke those canonical owners. A replayed custom Event returns the same persisted
`Event.id`; a replayed constructor returns the same Agent/Sandbox/Checkpoint/Principal.

### 17.2 Prepared construction and atomic public publication

Constructors that span AW rows plus filesystem/provider materialization use one shared
prepare/publish discipline. The private lower construction primitive exists **before** the
first caller that needs multi-resource publication (`checkpoint.restore`); later public
`agent.create` and model `agent(...)` expose/admit the same primitive rather than creating
another constructor. Do not expose a half-created resource, add a restore-specific
constructor, or require a cross-provider distributed transaction.

```text
1. replay lookup + reserve/mint operation-owned candidate IDs privately
2. preflight current authority, disclosure and quota constraints
3. BEFORE the first external/materializing effect, durably claim the existing replay/
   operation owner with {replay key, semantic fingerprint, candidate IDs, cleanup/recovery
   identity, state=Prepared}
4. perform external/materialized preparation under non-public operation custody
5. revalidate prepared external evidence and enter/retain any source-mutation fences needed
   for publication
6. COMMIT one AW store transaction that **revalidates all AW-store-owned source/destination/
   grant/ACL/lifecycle authority at the publication boundary**, publishes all authoritative
   current rows/grants/relations, initial queued input where requested, commit-log Events and
   transitions the same operation receipt to Applied — this commit is the **single public
   linearization point**
7. after commit, wake schedulers/waiters, refresh reconstructible caches/accelerators and
   release temporary barriers; reconcile abandoned preparation asynchronously
```

Before step 6, canonical refs are private/non-addressable. Immediately after the step-6
commit succeeds, the Agent/Sandbox/Checkpoint and canonical refs are authoritative,
discoverable and dispatchable subject only to ordinary current admission. There is no
second visibility flag/publication owner. Post-commit wakes, caches and accelerators are
reconstructible from committed state and may never control visibility.

AW-store-owned authorization/freshness checks in step 6 are transactionally fenced with
publication: revoke/source-replace/destination-close committed first denies publication;
publication committed first retains the original factual result. Existing source-mutation
barriers/fences acquired for an AW-owned source stay held through this commit.

For externally owned sources that cannot participate in the AW transaction, the reviewed
SourceDriver contract must provide immutable-version/source-fence authorization evidence
bound to the RecoveryDescriptor. Step 5 validates that evidence and step 6 publishes only
if the corresponding prepared evidence is still acceptable under current AW authority. If
external freshness/ownership cannot be confirmed safely, leave the original operation
`Prepared`/`Unknown` for inspection/reconciliation rather than publishing or repeating the
external effect. No cross-provider distributed transaction is implied.

The step-3 claim is **not** a public resource lifecycle or a second construction owner; it
is the existing mutation/operation replay receipt below the resource. A crash during
external preparation therefore leaves a durable `Prepared` claim meaning "the external
effect may have started". Restart/replay inspects/recovers that exact candidate/cleanup
identity and never blindly repeats preparation. A definitively failed prepare may transition
the same claim to `Rejected`; an uncertain effect transitions/remains `Unknown` until
inspection/reconciliation proves a safe continuation. The final public commit transitions
the same claim to `Applied` atomically with resource publication.

Prepared objects are **not** public Sandboxes/Agents/checkpoints and cannot be addressed by
Jobs/Events/Views while preparation is incomplete. A crash before step 6 leaves no
published AW resource; a reconciler identifies operation-owned prepared worktrees/provider
materializations/temporary retention roots that are not reachable from a committed current
row and cleans them idempotently.

Concrete consequences:

- `agent.create(sandbox="auto")` may prepare a fresh/derived Sandbox, but publishes the new
  Sandbox + SourceBindings/SandboxGrant evidence + Agent + ACL/HistoryGrant + parent
  relation/revisions + optional initial `task` in one logical AW commit. The task enters
  the child input owner/runtime custody atomically with publication; actual Turn execution
  starts only afterward.
- `checkpoint.restore` prepares **all** SourceDriver materializations first, then publishes
  the fresh Sandbox/SourceBindings + Agent/ACL/HistoryGrant + optional initial `task` and
  replay receipt together. Failure never silently falls back to history-only and never
  exposes a partially restored Sandbox/Agent.
- `sandbox.create(from=...)` prepares all derived bindings all-or-nothing and publishes one
  Sandbox only after every required source is ready.
- `checkpoint.create` may mint its ID and temporary driver retention roots/private Git refs
  before the row commit; those roots remain operation-owned until the single Checkpoint
  publication transaction succeeds and are reconciled away if it does not.
- simple store-only constructors such as `group.create` still use the same replay identity
  but need no external preparation phase.
- if `agent.create` is given an **existing explicit Sandbox ref**, that Sandbox is caller-
  owned/pre-existing and is never closed, rolled back or otherwise mutated merely because
  Agent construction later fails.

No user-visible `preparing` Agent/Sandbox lifecycle is added in v1; preparation is an
operation/receipt fact below the public resource lifecycle.

---

## 18. Host checkpoint API

A checkpoint is an immutable cross-domain **history+source reproducibility anchor**:

```text
checkpoint = exact immutable Agent history + immutable SourceSet
```

`SourceSet` may be empty for an explicit history-only checkpoint. The history side is
already immutable; the checkpoint's additional value is that it binds one exact history
boundary to the exact immutable snapshots of the mutable execution sources that mattered
at that boundary.

It is not an Agent `rev`, a live Sandbox, a credential snapshot, or a promise to recreate
the exact historical toolchain/network/secret environment. It guarantees exact history
and captured source state; restore derives the currently permitted execution environment.

Keep the host API to five operations:

```text
checkpoint.list
checkpoint.get
checkpoint.create
checkpoint.restore
checkpoint.delete
```

### 18.1 Identity, SourceSet and retention

```text
Checkpoint
├── history_base = exact immutable HistoryBase
├── history_head = exact settled immutable head inside that base
└── sources      = immutable named SourceSet, possibly empty
    ├── code     -> SourceAnchor
    ├── infra    -> SourceAnchor
    └── data     -> SourceAnchor
```

Conceptually:

```rust
Checkpoint {
    id,
    source_agent_ref,
    history_base_id,
    history_head,
    sources: SourceSet,
    source_coverage,
    note: Option<String>,
    created_by,
    created_at,
}

SourceSet {
    entries: [{
        name,
        source_identity,
        anchor,
        access,               // captured SourceBinding maximum: ro | rw
        checkpoint_policy,    // required | ephemeral at capture time
    }]
}
```

The durable storage shape is additive and immutable per checkpoint:

```text
checkpoints
    id
    source_agent_ref
    history_base_id
    history_head
    source_coverage = auto_all_required | selected | none
    lifecycle = live | retired
    note / creator / time

checkpoint_sources
    (checkpoint_id, source_name) PRIMARY KEY
    driver_kind
    source_identity
    captured_access = ro | rw
    captured_checkpoint_policy = required | ephemeral
    immutable anchor payload/digest
    retention metadata
```

`checkpoint_sources` contains immutable anchor descriptors only, never live credentials,
leases or mutable Sandbox authority. Driver-specific retention state may be reconciled
from these rows but is not a second checkpoint identity.

A source `name` is stable within the Agent/Sandbox execution model (`code`, `infra`,
`dataset`, ...), using `[a-z][a-z0-9_-]{0,63}` in v1. It is not a host pathname. Physical placement is mutable execution
configuration and must not become durable source identity.

Backing `session_id`/branch IDs may be retained as internal locator evidence but are not
part of the public checkpoint identity. Agent `rev` may fence capture, but is never
stored as checkpoint identity.

A live checkpoint is a retention root for its HistoryBase and every SourceAnchor in its
SourceSet. Checkpoints have no implicit TTL in v1; only explicit `checkpoint.delete`
retires one. Retention is reconciled by reachability from retained Agents/checkpoints/history
grants/source anchors, not by crash-sensitive reference counters.

### 18.2 Typed source anchors and drivers

AW does **not** define "source" as "one Git checkout." A Sandbox may bind multiple
execution-relevant mutable sources: several Git codebases, a versioned object/bucket
dataset, generated assets, or another source type whose host can prove an immutable
snapshot and later materialize it.

The reusable contract is typed:

```text
SourceBinding
    stable SourceRef / source identity + current mutable materialization
    access = ro | rw
    checkpoint policy = required | ephemeral

SourceDriver
    // one private prepared-operation primitive is shared by capture and restore
    plan(op = Capture(binding) | Restore(anchor), prepare_id) -> RecoveryDescriptor
    execute(op, descriptor) -> Capture(SourceAnchor) | Restore(SourceBinding)
    inspect(op, descriptor) -> not_started | completed(result/evidence) | unknown
    cleanup(op, descriptor)

    retain/release/reconcile(anchor)

SourceAnchor
    driver kind + immutable driver-specific identity/provenance

RecoveryDescriptor
    DriverKey + recovery_schema_version
    bounded driver-specific recovery/cleanup identity, created before any external effect
    immutable prepared source/version identity
    publication evidence = immutable/non-revocable fact
                         | opaque provider fence handle held through AW publication commit
    // durable descriptor is non-secret: IDs/digests/version proofs/opaque handles only
```

Persist one stable internal **DriverKey** across SourceBinding, SourceAnchor and
RecoveryDescriptor: built-in or Extension namespace + stable local driver ID **only**.
Each persisted artifact carries its own adjacent schema (`binding_schema`, `anchor_schema`,
`recovery_schema`, etc.). A runtime registry generation is a separate admission fence and
is not historical driver identity. Store-open/replay resolves `DriverKey + that artifact's
schema` against the same typed registry's compatible implementation; unresolved descriptors
pin the required recovery schema until reconciliation. No driver-key database or second driver catalog is introduced.

A source driver is valid only if it can fail closed and provide stable immutable capture/
restore semantics. **Capture and restore are symmetric for crash recovery**: any path that
can create an external snapshot/version/ref/object/materialization is executed through the
same prepared-operation contract and receives a stable operation-owned preparation identity
before the effect begins. Thus `checkpoint.create` is protected just like restore; Git
synthetic commits/private refs and provider snapshot creation cannot be blindly repeated
after a lost response. The exact Rust API may use a typed enum/generic helper or separate
thin typed methods, but there is one recovery invariant/owner. After restart AW must inspect
the exact attempted operation and clean/recover it rather than issue a second external
effect blindly.

For multi-source checkpoint/restore, the one existing operation receipt carries the bounded
set of per-source RecoveryDescriptors; they are evidence owned by that operation, not a new
coordinator/resource. V1 applies one host/runtime `max_sources_per_sandbox` limit to named
SourceBindings and therefore to every derived/captured/restored SourceSet and descriptor set.
Default implementation policy is **128 sources per Sandbox**, with a hard absolute ceiling of
**1024**; hosts may choose a lower configured limit but never exceed the hard ceiling. This
is admission/runtime policy, not a new per-request wire knob. Operations whose complete
SourceSet would exceed the effective limit fail before external preparation begins. Each descriptor carries enough immutable-version/fence identity to
validate that the exact captured/prepared materialization is still publishable.

Publication evidence has only two acceptable shapes:

1. an immutable/non-revocable provider/source fact whose validity cannot change during the
   AW commit; or
2. a provider-enforced fence/lease/capability that is **held and guaranteed valid through the
   complete AW publication transaction**, then released afterward.

A pre-commit `lease.valid()` observation is not enough. If the provider/driver cannot make
one of those guarantees, publication stays `Prepared`/`Unknown` for reconciliation rather
than racing the external authority boundary.

A durable RecoveryDescriptor is **never secret custody**. It may contain opaque provider
operation/fence IDs, content/version digests, immutable proofs and references to provider/
Vault-held capability state, but never bearer tokens, signed credential URLs, raw secrets,
private keys, serialized provider credentials or reusable fence credentials. If holding a
publication fence requires privileged material, the provider/driver owns that material and
the descriptor persists only its opaque recovery handle.

Git can encode private worktree/ref paths plus exact OIDs/source fence; cloud/object/provider
drivers can encode idempotency/object/materialization IDs plus immutable version or a held
provider fence. A driver that cannot provide both recovery identity and publication-fence
evidence cannot participate in effectful automatic capture/restore.
There is deliberately no generic "tar whatever is at this path" fallback.

A SourceBinding is `required` by default. `ephemeral` is an explicit trusted-host
declaration for disposable caches/generated state that is not part of reproducible source
state. A required binding may never be silently skipped by automatic checkpoint or
Sandbox-derivation operations.

The mandatory built-in source driver for the first implementation remains Git. Other
drivers, such as versioned object/bucket manifests, may be added through the same typed
contract without changing Checkpoint identity or Agent history semantics. Requesting a
source whose driver cannot safely capture it fails rather than silently omitting it.

For an object/bucket source, an acceptable anchor is an immutable provider version or a
content-addressed/version-addressed manifest whose referenced objects are retained by the
driver. A mutable bucket name/path alone is never a checkpoint.

### 18.3 Git source anchors

For a clean Git source, the SourceAnchor records the exact committed HEAD OID. The
checkpoint must still retain that object independently of the user's branch: a later
reset/rebase may otherwise make the exact clean commit unreachable and eligible for Git
GC.

For a dirty Git source, AW creates a hidden immutable snapshot with a **private temporary
index**:

```text
base commit A
    │
    └── hidden AW synthetic commit S
            tree = captured Git-relevant working tree T
```

Every live Git checkpoint SourceAnchor is pinned by one private retention ref, whether
clean or dirty:

```text
refs/aw/checkpoints/<checkpoint-id>/<source-name> -> H   // clean exact commit
refs/aw/checkpoints/<checkpoint-id>/<source-name> -> S   // dirty synthetic commit
```

The user's HEAD, branch and real index are never moved or rewritten. The AW checkpoint
row is authoritative; private refs are only object-retention machinery. Pinning S also
retains its parent/base commit and tree closure.

Dirty capture semantics are intentionally simple:

- seed the private index from committed HEAD;
- apply the final tracked working-tree state, including deletions;
- include non-ignored untracked regular files;
- exclude ignored files, `.git` internals and AW state outside the repository;
- do not follow untracked symlinks/reparse points; tracked Git symlinks remain ordinary
  Git tree entries;
- collapse staged-vs-unstaged distinction into the final captured tree in v1;
- require a committed base HEAD.

This is an exact **Git-relevant source tree**, not a byte-for-byte backup of ignored build
outputs, caches or arbitrary files outside Git's source view.

Capture reuses the existing `aw-git` fail-closed policy: repositories with executable
clean/smudge/process filters or partial-clone/promisor semantics are not snapshotted by
AW v1. That intentionally makes Git-LFS/filter-dependent dirty capture unsupported
rather than executing conversion code behind the checkpoint boundary.

Submodules remain gitlinks. The superproject Git SourceAnchor captures the exact gitlink
OID, not the nested repository's object database/working tree. An initialized dirty
submodule makes dirty superproject capture fail closed. Restore never automatically
fetches or initializes submodules.

If nested submodule contents themselves must be retained/reproducible, the host binds that
submodule repository as its own named required SourceBinding (and it receives its own Git
SourceAnchor/private retention ref). Otherwise the checkpoint's exact claim is only the
superproject gitlink identity; unavailable nested material later fails explicitly rather
than changing checkpoint identity or silently fetching it.

### 18.4 `create`

Checkpoint creation never mints source/history authority. The host call requires an
explicit checkpoint-create capability plus current `view` authority on the source Agent.
If `sources != "none"`, the caller/host must additionally hold current capture/read
authority for every selected SourceBinding and the mutation-barrier authority needed to
fence it. Agent ACL permissions alone do not imply filesystem/bucket capture authority.

```text
checkpoint.create(
    ref      = ".",
    sources  = "auto" | "none" | [<source name>, ...],
    note?
)
```

`sources="auto"` captures every **required** source binding in the Agent's effective
Sandbox and fails if any required binding cannot be captured safely. Explicitly
`ephemeral` bindings are omitted by design. If the Sandbox has no required source
bindings, the result is history-only.

`sources="none"` explicitly requests history-only capture.

An explicit list captures exactly those named sources and fails if any selected source is
missing, unauthorized, unsupported or unsafe to capture. This is an intentionally
selected/partial SourceSet: restore reconstructs exactly those captured bindings and does
not pretend omitted required sources were part of the checkpoint. `checkpoint.get`
reports whether source coverage was `auto_all_required`, `selected` or `none`.

A history-only checkpoint always anchors the latest settled immutable history head.
Checkpoint creation deliberately has no public Agent/Sandbox revision fence: the operation
captures one internally fenced current settled boundary and returns the exact history head
and SourceSet it captured. A client that needs a previously observed historical boundary
must already have a checkpoint for that boundary; a mutable owner `rev` is not a substitute
for source-state identity.

A source checkpoint is stricter. Capture requires a settled Agent boundary (no active
unresolved Turn) and no unresolved write-capable Process/effect against any selected
source. The host then:

1. fences the exact Agent/history head;
2. acquires the source-mutation barrier for the complete selected SourceSet;
3. captures every immutable anchor;
4. revalidates every source identity/authority and capture fingerprint;
5. commits one Checkpoint row only if the entire set is valid;
6. releases the barriers.

This is an all-or-nothing **logical** checkpoint. AW-owned writers are serialized across
the selected set; external stores/drivers must provide their own immutable-version or
stable-capture guarantee. A same-user adversarial writer outside AW confinement remains
outside the trust boundary.

Git dirty capture still validates repository identity/HEAD and produces the private-index
tree twice; both passes must yield the same tree/status fingerprint before publication.

Any mutation in flight, unresolved write hazard, source identity change, unstable capture
or unavailable required source returns conflict; no partial checkpoint row is published.

Successful checkpoint create/delete may emit the source Agent's `changed` Event with
`data.aspect="checkpoints"` and the opaque checkpoint ID. Checkpoint-list membership is a
separate resource collection and does not by itself advance Agent `rev`.

### 18.5 `list` / `get`

```text
checkpoint.list(ref=".", limit?, cursor?)
checkpoint.get(id, include?)
```

Return the immutable history anchor, named SourceSet and bounded presentation metadata.
The core read requires current `view` authority on the checkpoint's source/history. Source
names/kinds/layout facts may be shown, but provider/source identities and anchor details
that would disclose protected repository/bucket metadata are redacted unless the caller
also has current metadata-read authority for that SourceRef. Reads never fetch/materialize
missing objects. Namespaced product Views may be requested through the section 25.6
`include` seam.

### 18.6 `restore`

```text
checkpoint.restore(
    id,
    project = <Project ref; default checkpoint source Agent's Project>,
    title?,
    task?,
    access = "private" | "inherit" | [{ principal, permissions }, ...]
)
```

Restore is deliberately **non-destructive** and has a stronger semantic contract than a
plain history fork. It never rewinds an existing Agent, Sandbox, Git branch, index or
worktree in place. Host restore defaults to `access="private"`; cache selection is an
internal optimization, not a restore mode.

Authorization is conjunctive rather than inherited across layers:

- checkpoint/history visibility and ordinary HistoryGrant derive/share rules authorize
  the immutable conversation prefix;
- every captured SourceAnchor is independently authorized for current read/materialize
  disclosure into the destination Project;
- destination Project authority authorizes creation of the new Agent/Sandbox;
- none of those checks grants Vault/network/device authority.

A cross-Project restore therefore cannot move source contents merely because history is
shareable. `access="inherit"` means the checkpoint source Agent's **current** direct ACL at
restore time; checkpoints deliberately do not snapshot ACL/security state. If that source
Agent/ACL is no longer available or authorized, `inherit` fails and the caller must use
`private` or an explicitly authorized ACL.

Restore creates:

```text
new Agent
    history = exact checkpoint HistoryBase/head

new Sandbox
    sources = fresh materializations of every checkpoint SourceAnchor
```

Each SourceDriver restores its own anchor into a fresh mutable binding. Restore preserves
the captured binding's source name, `ro|rw` maximum and checkpoint-policy metadata; these
are state/layout facts, not authority. The destination Agent's actual source authority is
re-derived and may narrow the captured maximum. For a clean Git
anchor, the host creates an isolated managed worktree at the exact commit. For a dirty
synthetic Git anchor, the host creates an isolated worktree at the original base commit
and materializes the synthetic tree while leaving the index based on the base commit.
The restored dirty delta therefore appears as ordinary unstaged/untracked work; the
hidden synthetic commit never becomes the user's visible HEAD.

The target paths/materialization locations are host-selected. Missing required
repository/object/submodule/provider material is an explicit failure; history-only
restore is never silently substituted for a checkpoint that contains sources.

Source coverage determines Sandbox reconstruction:

- `auto_all_required` -> restore every captured required source exactly; explicitly
  ephemeral bindings may be recreated only from current destination Project policy;
- `selected` -> restore exactly the selected SourceSet; omitted required bindings are
  not silently filled from the current Project baseline;
- `none` -> make no source-state restoration claim and start a fresh destination
  Project Sandbox baseline, equivalent to fresh source environment selection rather than
  fabricated historical source state.

The restore response reports the checkpoint's `source_coverage` and the resulting
Sandbox ref.

`checkpoint.restore` intentionally remains a public recovery-oriented convenience even
though the lower Agent constructor is shared with `agent.create(from=...)`.

Checkpoint restore does **not** restore or inherit Vault secrets, credentials, live
leases, network/device authority, active Processes, staged inputs or other mutable
security state. Current destination Sandbox/Vault authority is derived independently
under section 25.4/25.5.

### 18.7 `delete` and crash recovery

```text
checkpoint.delete(id)
```

Delete requires current `manage` authority on the source Agent or an explicit Project
retention/admin capability; it is not implied by source read authority.

Delete first retires/tombstones the checkpoint in the authoritative store, then releases
every driver-specific retention root/private Git ref when no other live SourceAnchor
needs it. A reconciler repairs either side after a crash: retired rows may leave orphan
retention roots to remove, while a live row with missing required retained material is
reported as damaged and never fabricated.

Once no applicable Agent/checkpoint/grant/source retention root reaches an object,
ordinary driver/store GC may reclaim it. Immutable history is never rewritten by
checkpoint deletion.

Staged host input drafts, active Turns, finite Job runtime state, Sandbox grants and
Vault grants are never checkpoint contents.

---

## 19. Unified Agent construction and immutable forks

AW uses one Agent construction concept for both fresh Agents and exact immutable-history
forks. **Fork remains a semantic/history relation, not a second host constructor.**

Model-facing concept:

```text
agent(
    task,
    title      = ...,
    background = false,
    from       = <optional Agent ref | settled Turn ref>
)
```

Examples:

```text
agent(task="Investigate the parser")
    -> fresh Agent history

agent(task="Try an alternative implementation", from=".")
    -> fork current Agent's committed immutable history prefix

agent(task="Explore another approach", from="a:2")
    -> fork the visible source Agent's committed immutable history prefix
```

There is no separate model `fork_agent` tool and no host `agent.fork` operation.
Host callers use `agent.create(from=...)`; `checkpoint.restore` remains the explicit
history+SourceSet recovery operation.

### 19.1 Exact semantics

Omitting `from` creates a fresh Agent with no inherited HistoryBase.

Supplying `from` means:

1. resolve and authorize the exact public source Agent/settled Turn (or an already-
   authorized typed HistoryBaseRef only in the lower in-process constructor);
2. resolve the exact immutable boundary selected by that source: latest committed head for
   an Agent ref, the exact post-Turn boundary for a settled Turn ref, or the supplied
   internal HistoryBaseRef;
3. create a fresh destination Agent envelope under the caller surface's trusted policy:
   section 19.9 for the model constructor, section 22 for the direct host constructor;
4. attach its history to that immutable base;
5. append the supplied `task` as the new Agent's first divergent input;
6. give the destination fresh Agent security state, revision and input plan.

An in-flight source Turn is not copied as mutable state. If construction is invoked
during a source Turn from the Agent root, the fork base is the latest committed immutable
head. The delegated task is the fork's first divergent input.

History ancestry and Sandbox ancestry are deliberately independent. `from=a:2/t:7`
selects history only; it does not by itself grant or select the source Agent's mutable
Sandbox, mounts, Vault entries, credentials, devices or network authority. The trusted
host/delegation profile chooses whether the destination shares a Sandbox, branches one,
or starts fresh under section 25.4. For an exact reproducible history+source boundary,
use `checkpoint.restore`.

Never copy merely because history is forked:

- source Agent `rev`;
- active Turn state;
- staged host inputs;
- committed-but-not-historical in-flight input;
- ephemeral watches;
- Process/Graph runtime objects;
- Sandbox/Vault authority.

### 19.2 Immutable history base is independent from Agent security

The final architecture must not bind immutable conversation history ownership to one
Agent ACL/audience.

Separate:

```text
HistoryBase
    immutable committed model-visible history prefix

Agent envelope
    canonical Agent ref
    owner principal + ACL
    Agent rev
    Sandbox binding
    staged input plan
    local divergent history tail
```

A fork creates a new Agent envelope that **references** an immutable `HistoryBase`; it
does not copy the prefix.

Conceptually:

```text
                        immutable HistoryBase H
                       /          |           \
                      /           |            \
            inherited Agent   private Agent   custom shared Agent
```

All three may physically share the same history objects and exact model prefix while
having unrelated mutable lifecycle and ACL state.

Storage may implement this as a global immutable history DAG or an immutable
cross-Agent base-link/manifest. The normative requirement is structural sharing, stable
immutable identity, bounded ancestry lookup and independent destination authorization;
copying every prior Turn into each destination Agent is not the fork model.

A destination Agent's effective history is:

```text
immutable HistoryBase H
        +
destination-local immutable divergent tail
```

The destination's own history tail never mutates H.

### 19.3 History disclosure grant

Cross-Agent structural sharing uses an explicit durable disclosure grant:

```text
HistoryGrant {
    base,
    source_agent,
    destination_agent,
    created_by,
    disclosure_manifest,
}
```

The grant authorizes one destination Agent to resolve the exact immutable prefix without
copying it. It is not an ACL grant and grants no provider, tool, filesystem or execution
authority.

Fork creation validates the complete disclosure/provenance closure of H before the
destination becomes visible.

That closure is transitive. Whenever a destination-local immutable span is sealed into a
new HistoryBase, its dependency manifest is the union of:

- the inherited HistoryGrant/base dependencies; and
- every committed local history fact's `disclosure_dependencies`, including EventInputs
  or other externally sourced facts that carried protected content.

An exact fork/HistoryGrant may preserve or further restrict that dependency closure but
can never drop it. Queued/staged/uncommitted inputs are not part of HistoryBase and are not
fork-inherited, so their dependencies enter the closure only if/when they become committed
history. Redaction/materialization that intentionally changes those dependencies is a new
projection/base, not an exact fork.

Once committed, the destination Agent's direct ACL evolves independently from the
source Agent's direct ACL. Shared Principal objects (for example the same group) remain
live references, so later group membership changes intentionally affect every Agent
granting that group.

Source ACL revocation does not rewrite already-disclosed destination history.
Materialized/external facts with their own export/revocation policy remain subject to
that policy and may make a requested exact fork invalid.

The history objects remain retained while any retained Agent/checkpoint/current grant
reaches them.

#### Storage shape

Cross-Agent history is an additive immutable link layer over the current Session journal,
not a replacement journal:

```text
HistorySpan
    exact immutable local backing span
    {source Agent ref, backing session, local root, exact head, content/provenance digest}

HistoryBase
    id
    parent_base?       // inherited prefix, if any
    local_span?        // newly sealed local divergent span
    total_turns
    next_turn
    history_content_digest
    manifest_digest

HistoryGrant
    id
    base
    source_agent
    destination_agent
    disclosed_principals[]   // user/group principals, not expanded group members
    dependency_manifest      // transitive inherited + committed local disclosure dependencies
    created_by
    supersedes?
```

`HistorySpan`, `HistoryBase` and `HistoryGrant` rows are immutable. A destination
Agent's mutable envelope points at its current inherited `HistoryBase` and current
`HistoryGrant`, while new local Turns continue through the existing Session
journal/recovery owner.

The same disclosure model also applies to destination-local facts that carry content from
another protected resource. An immutable/queued fact may therefore retain bounded
`disclosure_dependencies` such as:

```text
DisclosureDependency {
    source_fact                   // exact provenance/audit identity
    authority_root               // stable disclosure-policy/source-authority root
    already_disclosed_principals[]   // user/group principals, groups not expanded
    source_policy/provenance evidence
}

DisclosureAuthorityRoot {
    stable source/disclosure-policy identity
    immutable dependency/provenance + maximum-ceiling evidence
    // NOT a current authorization grant/result
}

DisclosureManifest {
    immutable content-addressed root id/digest
    persistent bounded-fanout immutable set/DAG nodes
    exact provenance fact refs
    deduplicated authority_root refs
}
```

`DisclosureManifest` is an internal immutable provenance/authorization-closure structure,
not another ACL/grant service or public resource API. Keep **exact fact provenance** and
**authorization ceiling identity** distinct: many protected facts may share one stable
`authority_root` when they are governed by the same effective disclosure/source-authority
ceiling. Audit/provenance retains every exact source fact; ordinary expansion/fork/share
checks deduplicate *which live authority owners must be consulted* over unique roots.

An `authority_root` is **never cached current permission**. It identifies immutable
provenance/maximum ceiling and the live policy/ACL/group/provider owners that must be
re-resolved. Disclosing to a principal not already covered by immutable prior disclosure
always rechecks current source policy/share authority and current referenced security/
provider generations. Only an explicitly immutable export/disclosure grant whose own
reviewed contract survives later revocation can avoid that live recheck. Manifest DAGs and
indexes accelerate authorization dependency lookup; they never answer authorization by
themselves.

The manifest is represented as a persistent content-addressed set/DAG with bounded-fanout
immutable nodes. A union reuses prior nodes and adds only newly introduced fact/authority
roots; it never rewrites or recopies the whole historical closure. HistoryGrant, Event,
committed local facts and trusted projections may reference the same root digest. A small
rebuildable projection/cache may accelerate unique-authority-root checks, but exact facts
and immutable DAG nodes remain authoritative. Retention follows immutable facts/grants/
Events that reference the manifest.

For example, an EventInput received from another Agent records the recipient disclosure
principals to whom that fact was validly disclosed at admission. Later source ACL
revocation does not claw back that already-disclosed audience. A later destination ACL
expansion validates only newly added principals against current source `share`/equivalent
policy, while explicitly revocable external-material policies may still impose their own
stronger contract. This does not create another grant service; it is provenance used by
the ordinary Agent audience/share validator.

#### Disclosure dependency propagation

The same metadata propagates conservatively through AW-mediated model/history boundaries
so protected content cannot be laundered merely by changing representation:

- EventInput carries its concrete source Event/resource dependency;
- a SourceRef-backed Resource/typed Tool result admitted into durable/model-visible context
  carries its exact known source disclosure dependency;
- a generic Process/result inherits the union of protected SourceBindings and raw Vault
  projections it had capability to read during that invocation, unless a narrower trusted
  Tool/driver contract can prove a smaller set;
- a View/Hook/extension-provided model-visible value with stricter product visibility
  carries that product/source dependency;
- a raw Vault file/env projection is potentially disclosive: Process/Tool output produced
  under that raw secret exposure carries the Vault disclosure ceiling unless the provider
  supplies a stronger safe brokered-result contract;
- combining protected inputs unions dependencies rather than overwriting them; union is
  incremental/structurally shared and deduplicates authorization work at stable authority
  roots while preserving every exact fact-provenance edge;
- an Assistant/model output inherits the union of disclosure dependencies from the
  model-visible context/tool results that could have influenced it, because AW cannot
  prove the model did not paraphrase or reproduce them.

This is conservative **capability/provenance propagation**, not byte-level dynamic taint
analysis. AW does not claim to infer arbitrary information flow through code, files or
network side effects inside a Process; Sandbox/Vault/source capability separation remains
the primary control for those effects. Dependencies may be removed only by an explicit
trusted redaction/declassification/materialization operation producing a new immutable
projection/base with its own provenance. Ordinary Hooks, model generation, summaries and
exact forks never silently declassify.

Scalability is structural rather than an arbitrary tiny hard cap: manifest DAG nodes have
bounded fanout/encoded size and per-request union/authorization work is bounded. If a
single operation would exceed those safety budgets it fails/defers rather than flattening a
giant JSON/ACL mirror. Long-lived history may contain many exact provenance facts while the
unique authority-root closure stays much smaller and structurally shared.

A source with no new divergent local content reuses the exact same `HistoryBase`.
When local history has diverged, the exact immutable local prefix is sealed as one
`HistorySpan` and a new base node references the prior base plus that span. The content
is never recopied.

Sealing is **idempotent structural memoization**, not an Agent-owner mutation. The store
uses a canonical internal seal key over the exact backing history boundary + parent base/
local-span manifest so concurrent fork/checkpoint sealers converge on the same sealed
prefix (or an equivalent existing row) rather than minting parallel semantic bases. Lazy
sealing/flatten-index creation does not advance Agent `rev` and emits no Agent `changed`
Event because no user-visible history/content/lifecycle fact changed.

A rebuildable flattened closure/index may cache base ancestry for fast projection; it is
not authoritative state.

The effective immutable history is therefore:

```text
flatten(HistoryBase spans) + destination-local immutable tail
```

`history_content_digest` covers the canonical immutable history facts/provenance manifest,
not one Agent's system prompt, Context Hook output, Tool visibility or provider request
profile. Those model-visible invocation choices belong to the derived cache/prefix
projection in section 21, not to HistoryBase identity.

Inherited Turn ordinals/next-turn state come from the base manifest.

A HistoryGrant records disclosure to principal identities, not current expanded group
membership. If `group:team` is granted, later changes to that group's members follow
the same live principal grant. Expanding a destination ACL to a principal not covered by
the current disclosure manifest requires `share` validation and mints a new immutable
superseding HistoryGrant. Narrowing access does not rewrite old grants/history.

Redaction/anonymization never points a new Agent at the exact base. It materializes a new
immutable HistorySpan/Base with new content identity and provenance links to the
authorized source facts.

#### Retention and migration

Use reachability, not mutable refcounts. **Retained Agent** base pointers (including
closed Agents in v1), live checkpoints and current HistoryGrants are GC roots. A periodic/store-open reconciler marks reachable
bases/spans and only then permits unreachable compatibility history to be reclaimed.
Historical grant/audit rows may retain hashes/provenance without remaining retention
roots. `agent.close` is not history deletion/retirement and therefore never removes the
Agent's history-retention root. DL-200 v1 intentionally has no public destructive Agent
purge; a future retention/archive API requires a separate design.

Existing immutable Session nodes remain the physical facts they are today. Migration
does **not** rewrite them:

1. existing Agent bindings continue with their current Session/branch local history;
2. the first DL-200 fork/checkpoint that needs a reusable prefix seals the exact existing
   head into a HistorySpan/Base;
3. a new fork receives a fresh backing Session/branch for its local tail plus a base/grant
   link;
4. old same-Session branches may remain in place as compatibility backing; public
   isolation comes from the Agent envelope, not from requiring an eager history copy;
5. Session/journal/effect recovery remains authoritative for local Turns and unresolved
   effects.

This is the only cross-Agent history layer. There is no second conversation store.

### 19.4 Every fork creates a new Agent envelope

DL-200 uses one public fork lifecycle:

> every Agent fork creates a **new globally addressable Agent** referencing the exact
> immutable history base.

The destination ACL chooses its security shape:

```text
private
inherit
explicit ACL
```

#### `private`

New owner-only Agent referencing the exact permitted immutable prefix.

#### `inherit`

New Agent whose source ACL principal entries are copied into fresh destination ACL
entries. Shared group principals remain shared groups; direct ACL edits are independent.

#### explicit ACL

New Agent with an exact trusted-host-selected owner Project/principal and ACL.
Principals may be direct users, groups, or both, and may be completely different from
the source.

All exact forms structurally share immutable history and preserve attribution.

If multiple forks should continue to track the same team's live membership, grant the
same group principal to each Agent. No shared Session identity is needed.

There is no separate public/private history representation; only the destination Agent
security envelope differs.

### 19.5 Derive vs reshare authority

Viewing an Agent is not automatically authority to export its history into another
security envelope.

Fork policy therefore distinguishes:

```text
derive
    create an independent fork whose destination audience is no broader than
    the history the caller is authorized to derive

share
    disclose the exact inherited prefix to destination viewers not already
    covered by the source disclosure authority
```

These are host/security actions, not model-facing Jobs tools.

Every fork creates a new disclosure/security envelope and therefore requires current
derive authority.

If the destination adds viewers beyond the currently authorized audience, every current
content dependency must permit the expanded audience. This closure includes inherited
HistoryGrant dependencies and destination-local/queued externally sourced facts such as
EventInputs. For a viewer who does not already have source visibility, the actor performing
the expansion must hold current `share` (or the source resource's equivalent disclosure
authority), and the source policy must permit that disclosure.

The implementation may maintain a rebuildable bounded disclosure-closure index for fast
preflight, but authoritative access changes validate the exact dependency facts. The
index is not another ACL or grant owner.

Do not use Agent `rev`, cache identity or possession of a history hash as disclosure
authority.

### 19.6 Exact fork boundary

The model-facing `from` selector may resolve either:

```text
Agent ref
settled Turn ref
```

Agent ref means the source Agent's latest committed immutable head.

A settled Turn ref means the exact committed history boundary immediately after that
Turn. This enables deliberately reusable seed prefixes such as:

```text
agent(task="security analysis", from="t:1")
agent(task="performance analysis", from="t:1")
agent(task="API review", from="t:1")
```

An active/unsettled Turn is not a valid exact fork boundary.

The public host constructor uses only Agent or settled Turn refs. Trusted in-process
library code may use an already-authorized typed `HistoryBaseRef` as a lower construction
primitive, but `HistoryBase` is not a wire-level host mutation target. Checkpoints use the
stronger `checkpoint.restore` operation instead.

### 19.7 Fresh mutable state

Every fork, regardless of audience mode, receives:

```text
fresh Agent owner rev
fresh Agent runtime
fresh input-plan rev
empty staged input plan
no inherited active Turn
no inherited Process/Graph runtime
no inherited watches
fresh Agent access/security state
```

Only immutable history/source lineage is shared.

### 19.8 Inherited Turn ordinals and provenance

A structural fork preserves the inherited immutable Turn timeline.

If the base contains Turns 1..N:

```text
fork base
  t:1
  t:2
  ...
  t:N

new divergent input
  -> t:N+1
```

A fork from exact `t:1` therefore naturally starts its first divergent work as `t:2`.

`TurnRef` is an Agent-timeline coordinate; immutable inherited Turn identity is the
underlying opaque `history_fact_id`. `jobs_list(scope="turns")` for a fork may therefore
return destination timeline refs `t:1..t:N` that structurally resolve to inherited facts.
Rows/read results expose the stable history fact identity and original Agent/Turn
provenance where needed; this does not create a second public history namespace.

Inherited Turn authorship/provenance remains the original immutable provenance. A new
Agent does not relabel inherited content as authored by its new owner/viewers.

Historical Process/Graph/tool records in the inherited prefix remain historical facts;
their old runtime objects are not resurrected into the new Agent and no destination-local
`j:`/`n:` refs are fabricated for them. Authorized callers read those records through the
inherited Turn/history views. Original canonical Job/Agent refs may appear only as factual
provenance where the HistoryGrant permits that provenance; they do not become live
mutation targets through the fork.

### 19.9 Model constructor cannot choose audience

Model-facing:

```text
agent(task=..., from=...)
```

may choose only the history source permitted by its current Agent/delegation policy.

It does **not** choose:

- destination owner;
- participants;
- permission values;
- destination Project/owner/access policy;
- disclosure grants;
- Sandbox mode/ref;
- Vault grants or secret/service entries.

The normative model exact-history-fork default is:

```text
destination Project = source Agent's Project
access              = inherit source Agent's direct ACL entries
owner               = source Project's i:<principal>
```

This is the least-surprising zero-broadening default: the fork remains beside its source
and starts with the same principal grants, while the new Agent owns fresh independent
ACL rows. Group entries remain references to the same live group principals.

A trusted delegation profile may **narrow** that default (for example owner-only/private)
or select another already-delegated destination Project. Any redirect/broadening still
must pass the ordinary `derive`/`share` and destination-create authority checks; the
model cannot request or encode the broader audience itself. If those checks fail, fork
creation fails rather than silently choosing another Project.

This preserves one simple model constructor without granting the model sharing
authority.

---

## 20. Recursive Agent forks

Forking is compositional.

A forked Agent that is allowed to create Agents receives the same `agent(...)`
constructor. Therefore:

```text
root Agent
  └── forked Agent A
        └── forked Agent B
              └── ...
```

requires no separate recursive-fork API.

Inside Agent A:

```text
agent(task="Try another branch", from=".")
```

means "fork Agent A's latest committed immutable head."

All existing delegation-root depth, active-child, model-attempt, deadline, routing and
tool-authority ceilings continue to apply. Forking changes the history base; it does
not mint new budget or authority.

The durable child relation binds the canonical child Agent ref. Recursive forks use that
same Agent lifecycle; there is no second child-session form.

---

## 21. Cache-friendly fork contract

Logical fork correctness must not depend on provider caching. Cache reuse is an
optimization.

A fork naturally maximizes reusable prompt prefix because it shares the exact immutable
history prefix. Provider-side prefix-cache reuse is best when the following remain
compatible:

```text
provider/destination
account/cache domain
model
system prompt
history serialization
context projection
tool schema/profile
other request-prefix metadata
```

Changing any of these may reduce or eliminate provider cache reuse without changing
fork correctness.

### 21.1 Cache-compatible profile

For a requested cache-friendly fork, the host should prefer an **inherit profile**:

- same provider destination;
- same account/model where current authority still permits them;
- same system prompt;
- same history projection through the fork head;
- same stable tool schema/profile where policy permits.

The fork-specific task is appended only after that common prefix.

Tool/effect authority is still revalidated for the new Agent. Cache compatibility never
copies credentials or grants.

If host policy requires a different system prompt/tool schema/model, the fork remains a
valid logical history fork but cache compatibility is reported as degraded/best-effort.

### 21.2 Prefix identity and cache identity

Separate the content prefix from one backend's cache namespace.

```text
PrefixContentKey = hash(
    serialization version,
    effective system/context prefix,
    exact authorized HistoryBase/local-tail projection bytes,
    model-visible Context Hook/static-prefix output
)

BackendCacheKey = hash(
    PrefixContentKey,
    model/profile digest,
    backend cache-domain digest,
    tool/request-prefix digest
)
```

The content key deliberately excludes:

- Agent ref;
- Agent ACL/principal list;
- internal backing-store/session IDs;
- access/membership generations;
- Agent `rev`.

Those values are security/lifecycle metadata and must not perturb the model prefix merely
because an otherwise identical fork has a different audience.

`PrefixContentKey` is a derived invocation/cache projection keyed from immutable history
plus the effective model-visible profile. It is never stored as HistoryBase identity. The
same HistoryBase may therefore produce several PrefixContentKeys under different system
prompts, Context Hooks or serialization profiles without changing fork/history semantics.

Authorization is checked separately through the destination's valid `HistoryGrant`
and current Agent/provider/tool authority **before** any cached prefix/KV object may be
attached.

This allows two independently secured Agents that are both legitimately authorized for
the same immutable prefix to share physical prefix/token/KV data without treating cache
possession as disclosure authority.

Provider/account cache domains may still prevent backend sharing even when the
PrefixContentKey is identical.

### 21.3 Seed-turn fanout

A first-class optimization target is:

```text
new Agent
    │
    └── Turn 1: common system prompt + common initialization prompt
            │
            └── settled immutable prefix H1
                 /       |        \
                /        |         \
          specialist A specialist B specialist C
             Turn 2A      Turn 2B      Turn 2C
```

All specialists fork the exact post-Turn-1 history boundary and append their role/task
pivot as the **first divergent user input**.

Do not rewrite each child's system prompt merely to assign a role when cache reuse is a
goal. Prefer:

```text
shared system prompt
shared initialization Turn 1
-------------------------------- exact cached prefix
"For this fork, act as the security reviewer ..."
"For this fork, focus on performance ..."
"For this fork, challenge the API design ..."
```

The role pivot belongs after the shared prefix.

For local inference that supports paged/ref-counted KV state, AW may retain a
post-Turn prefix snapshot and let sibling Agents share those KV pages copy-on-write.

For remote providers, AW guarantees exact compatible prompt serialization but cannot
guarantee a provider cache hit. The first fork may populate a provider-side prefix cache;
later siblings may benefit. A lower provider adapter may internally use a provider-
specific cache handle/prime when safely supported, but no generic public cache-control API
is added and fork semantics never depend on it.

### 21.4 Fanout needs no new API

Do not add `agent.fork_many` or any other fanout constructor.

Multiple forks from the same exact base naturally share one HistoryBase. Under the same
effective model-visible profile they also derive the same PrefixContentKey. Callers can
issue repeated `agent.create(from=...)` operations or use existing Graph/batch machinery.

The lower owner/cache layer may resolve, retain and reuse the common immutable prefix
once for concurrent sibling forks. That optimization is invisible to the API.

This keeps fanout a performance property rather than another lifecycle primitive.

### 21.5 Source and Sandbox composition for forks

History sharing does not imply mutable source sharing.

For Agent/Turn history forks, trusted Sandbox policy determines whether current source
state is:

- **shared** — destination Agent binds the exact same Sandbox ref;
- **derived** — host creates an isolated child Sandbox from the source Sandbox's current
  state using source-driver/COW mechanisms where available;
- **fresh** — host starts from the destination Project/Workspace baseline.

An older settled Turn and an internal raw HistoryBase have **no historical source-state
implication**. Sharing/deriving the current Sandbox with such a history fork is an explicit
host policy choice, not a restoration claim. A Checkpoint instead carries an exact
SourceSet and is restored through `checkpoint.restore`.

The model constructor never chooses these modes directly. Trusted host/delegation policy
does. This prevents history access from becoming filesystem, credential or Sandbox
authority.

Git-backed derived Sandboxes naturally share the object database and can use managed
worktrees; other SourceDrivers may use immutable versions, manifests or COW snapshots.
Source correctness never depends on cache reuse.


---

## 22. Host Agent construction API

`agent.create` is the one public host Agent constructor. It creates either fresh history
or an exact immutable-history fork:

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

`from` omitted means fresh history and uses the caller's explicit/current destination
Project. `from` present selects immutable ancestry through an Agent's latest committed
head or an exact settled Turn boundary and defaults the destination Project to that source
Agent's Project. The destination Project's `i:` principal owns the new Agent.

Host construction defaults to `access="private"`. `inherit` is valid only when `from` has
a source Agent and copies that source Agent's current direct ACL after ordinary
`derive/share` validation. Public construction deliberately has no cache mode or
source-Agent revision fence: cache reuse is internal, and callers needing an exact
observed boundary use a settled Turn ref.

- `private` -> owner only;
- `inherit` -> copy source Agent ACL principal entries into fresh destination ACL
  entries; group principals remain live group principals;
- explicit list -> exact destination ACL.

Any `from` construction requires current source `derive` authority. Redirecting to another
Project or selecting an audience beyond the source disclosure closure additionally
requires destination-create plus current `share`/dependency authority; neither `inherit`
nor an explicit ACL is a bypass. With no `from`, only destination-create/access-management
authority applies because there is no inherited disclosure dependency.

`sandbox` is orthogonal to history:

- an exact Sandbox ref means **share that logical Sandbox state** subject to current
  destination authority; it must be `open` and in v1 must belong to the destination
  Agent's exact Project;
- `auto` lets the trusted host/delegation policy choose a fresh Project Sandbox or a
  derived Sandbox from the source's current Sandbox;
- a caller that wants an explicit isolated/fresh Sandbox creates it through
  `sandbox.create(...)` first and passes its ref.

For direct host construction, `sandbox="auto"` is deliberately conservative:

- fresh history -> fresh destination Project Sandbox;
- `from=<Agent>` / latest settled Turn -> isolated Sandbox derived from that Agent's
  **current** Sandbox state only when the source Agent is currently settled and the
  Sandbox passes the same quiescent mutation fence as `sandbox.create(from=...)`;
- if that latest-history source currently has an active Turn or conflicting source effect,
  `sandbox="auto"` fails with conflict rather than silently pairing committed history with
  uncommitted workspace state;
- `from=<older settled Turn>` -> fresh destination Project Sandbox, because AW has no
  historical source-state claim for that Turn unless a checkpoint exists.

A trusted delegation profile may deliberately choose shared/current-derived/fresh behavior
for model subagents during a live Turn, but that choice is collaboration/execution policy
and must not be presented as historical source restoration. Exact historical history+source continuation always uses
`checkpoint.restore`. The model itself never supplies a Sandbox/Vault selector.

A trusted in-process `HistoryBaseRef` lower constructor has no implied mutable source
environment, so its `sandbox="auto"` policy starts from the destination Project baseline.
That lower primitive is not part of the public host wire API.

A Checkpoint is intentionally not accepted by this method's `from` grammar.
`checkpoint.restore` owns the stronger "exact history **and** exact SourceSet" contract
and uses the same lower Agent constructor internally.

Repeated construction from the same immutable base naturally shares HistoryBase. When
the effective model-visible profile is also identical, the lower cache layer derives the
same PrefixContentKey and may coalesce the common prefix.
There is no public `session.fork`, `agent.fork`, or `agent.fork_many`.

---

## 23. Immutable attribution across forks

Forking changes the destination security/execution envelope. It never rewrites inherited
history provenance.

### 23.1 Hard fork invariant

For every immutable inherited history fact `F` in an exact fork:

```text
fork(F).history_fact_id == F.history_fact_id
fork(F).content         == F.content
fork(F).provenance      == F.provenance
```

The stable immutable fact identity is not the same thing as an Agent-local timeline
coordinate. A destination may expose the inherited fact at `a:<fork>/t:<ordinal>` while
that lightweight timeline coordinate resolves to the same immutable `history_fact_id`
and preserves the original canonical Agent/Turn provenance. Exact fork never copies or
re-authors the fact.

A transformation that changes content or attribution is a **projection/import**, not an
exact fork, and receives a new immutable HistoryBase identity.

### 23.2 Immutable provenance

History facts retain stable original provenance sufficient to answer:

```text
who authored/caused this fact?
under which original canonical Agent?
at which Turn?
when?
through which source/tool/effect lineage, when applicable?
```

Conceptually:

```rust
HistoryProvenance {
    author: HistoryAuthor,
    original_agent: AgentRef,
    original_turn,
    created_at,
    source: Option<HistorySource>,
}
```

`HistoryAuthor` may represent a stable user identity, Agent identity, host/system
source or factual tool/effect source.

A private fork from a shared Agent therefore still displays the original authors of
all inherited Turns. Removing an original viewer from the destination does not
remove or relabel that person's historical authorship.

Likewise, adding a new destination participant never makes that participant an author
of inherited history.

### 23.3 Identity vs display

Stable provenance identity and presentation metadata are separate.

The immutable provenance stores stable identity. UI projection may resolve a current
display name/avatar where authorized and may optionally retain a bounded historical
display snapshot for presentation.

Changing a user's current display metadata never mutates historical provenance.

### 23.4 Agent/tool attribution

Inherited Assistant Turns retain the original Agent/Turn attribution.

Historical Tool/Process/Graph/effect records retain their original factual invocation
and effect provenance. A fork does not resurrect their runtime objects and does not
re-author them under the destination Agent.

Example:

```text
source:
  t:7 assistant authored by a:2

fork:
  inherited t:7 -> still authored by original a:2
  new t:8       -> authored by fork Agent a:9
```

### 23.5 Exact forks vs privacy projections

A destination may sometimes be allowed to receive content but not the original
contributor identity.

That is not an exact fork.

Instead create a derived immutable projection:

```text
HistoryBase H
    │
    └── explicit redaction/anonymization policy
            ↓
         HistoryBase H'
```

`H'` retains provenance to the derivation operation but has its own model-visible
content and therefore its own PrefixContentKey.

Never silently anonymize or redact an exact `private` fork.

### 23.6 Model projection vs UI provenance

Full provenance need not be serialized into the model prompt.

The durable HistoryBase can retain:

```text
role/content + full provenance
```

while the model-visible serialization uses only the stable semantic role/content needed
for inference.

Principal/display names, Agent ACL generations, internal backing IDs and UI-only
attribution metadata stay out of the model prefix unless explicitly part of the
application semantics.

This maximizes exact prefix/cache reuse across differently secured forks while
preserving complete attribution for authorized UI/audit views.

---

## 24. Principals, groups and Agent access

The canonical Agent is the public collaboration/security object. Keep access around one
simple ACL keyed by first-class principals.

### 24.1 First-class principals

The canonical `i:<principal>` token contains one immutable principal handle. Principal
kind is metadata, **not syntax**:

```text
/d:acme/i:alice
/d:acme/i:compiler-team
```

Within one Domain the handle is unique across all principal kinds. The store resolves it
to exactly one of `user` or `group`; callers never address a second
`user:<id>`/ `group:<id>` namespace.

A user principal is backed by one authenticated person identity.

A group is an explicit reusable set of users. Groups are first-class principals for
Agent ownership and ACL grants, while individual authenticated users remain the factual
actors in audit/history.

Do **not** implicitly create a hidden group for every ad-hoc user list.

Use:

- direct user grants for one-off/fine-grained sharing;
- groups for stable teams/repeated sharing;
- both together where useful.

### 24.2 Groups

Keep v1 groups deliberately simple:

```text
Group {
    principal,
    owner_user,
    members: Set<User>,
    rev,
}
```

- no nested groups in v1;
- one user owns/manages group membership;
- group membership changes are revisioned;
- an Agent ACL grant to a group follows current group membership.

An Agent itself may be owned by either a user or group principal.

If a group owns an Agent, every active group member has Agent-owner authority. This is
intentionally strong and should be used only for genuine team co-ownership.

For a team that should share access but not full ownership, keep a user as Agent owner
and grant the group only the required permissions.

Group ownership of an Agent never grants authority to edit the group itself.

### 24.3 Agent ACL

Each Agent owns:

```text
owner: PrincipalRef

access:
    PrincipalRef -> PermissionSet
```

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

Semantics:

- `view` — read authorized Agent/history/result/Event projections;
- `contribute` — create/edit/delete permitted staged input drafts;
- `submit` — commit permitted new Agent input;
- `control` — steer/interrupt/cancel live execution where otherwise permitted;
- `derive` — fork the Agent/history without broadening inherited disclosure;
- `share` — disclose inherited or otherwise disclosure-dependent Agent content to
  principals outside the current audience;
- `manage` — manage Agent access/configuration subject to disclosure rules.

Every **stored ACL entry** containing any non-`view` permission must also contain `view`.
Do not rely on a union with some other group/direct grant to make an individually invalid
entry usable; this keeps each grant locally understandable and prevents surprising loss
of its prerequisite when group membership changes.

The owner principal has all Agent permissions. Provider credentials, tool authority,
source-workspace authority and delegation budgets remain separate.

### 24.4 Effective permissions

Authorization is allow-only:

```text
effective permissions =
    owner rights
    UNION direct user grant
    UNION every matching group grant
```

There are no deny entries in v1.

Example:

```text
principal engineering (kind=group) -> view + submit
principal alice       (kind=user)  -> view + control + manage

Alice, if a member of engineering:
    view + submit + control + manage
```

If deny semantics are ever needed, add them only with a separately reviewed precedence
model.

### 24.5 Revisions and groups

Direct Agent ACL/configuration changes are Agent-owned mutations and advance that
Agent's normal public `rev`. There is no public Session/access revision.

A group owns its own monotonic `rev`. **Changing group membership does not fan out a
write to every Agent that references the group and does not advance those Agents'
`rev`.** Instead, authorization through a group retains:

```text
Agent ACL revision
+ exact group principal
+ group rev
+ relevant principal/security epochs
```

The implementation derives one opaque `access_generation` from that dependency set.
It is revalidated at admission/effect/delivery boundaries and may be returned by
security-oriented host reads, but it is not a mutation fence and is never substituted
for Agent `rev`.

A direct Agent ACL edit emits the Agent's `changed` Event with `data.aspect="access"`.
A group membership edit emits `changed` with `data.aspect="members"` from the group
Principal resource.
There is deliberately no O(number-of-referencing-Agents) Event/revision fanout. A waiter
whose current authorization depends on the changed group is awakened internally and
must reauthorize; loss of authority returns denied rather than inventing an Agent
revision.

Adding a new principal grant to any Agent with existing disclosure-dependent content is a
disclosure expansion requiring the complete current dependency closure to remain valid.
For inherited history this includes `share` plus HistoryGrant dependency validation; for
externally sourced local facts it includes the corresponding source visibility/share
checks. A group is the disclosure principal:
later membership changes follow that already-granted group identity. Removing access
changes future visibility but never rewrites immutable history or attribution.

### 24.6 Host Group API

Groups are reusable principals, not a second collaboration runtime:

```text
group.list(limit?, cursor?)

group.get(ref)

group.create(
    name,
    members=[]
)
    -> server-minted immutable i:<principal> handle

group.edit(
    ref,
    name?,
    members?,
    rev
)

group.delete(
    ref,
    rev
)
```

Group changes are observed through the shared Events API; `group.get` is a pure snapshot
read. `group.list/get` are management/roster surfaces and return only groups the caller is
authorized to inspect under Domain policy; full membership requires the group owner or
Domain-admin equivalent. Merely seeing a group principal in an Agent ACL does not reveal
its member roster.

Group `ref` is the canonical Principal namespace ref (`/d:<domain>/i:<principal>`) and
must resolve to a group-kind principal; groups do not get a second global ID namespace.

`name` is mutable presentation metadata; it never changes the canonical principal
handle. If `members` is supplied to `edit`, it replaces the complete member set.
Revision fencing makes replacement safe without add/remove micro-verbs.

Deletion is refused while any **current authority-bearing reference** still requires the
group (for example Project/Agent ownership, Agent ACL, active execution/Vault disclosure
ceiling or another live grant that cannot safely become empty) unless that reference is
explicitly reassigned/removed first. Never silently orphan a current resource merely
because the group is being deleted.

Purely historical immutable provenance/retired grants do not block deletion. Group
deletion tombstones the principal and removes active membership authority; canonical
principal handles are never reused, so immutable history/grant/audit provenance cannot
later resolve to a different principal.

### 24.7 Principal storage and minimal metadata API

Use one additive principal registry per Domain to enforce the public one-handle namespace:

```text
domain_principals
    (domain_id, principal_handle) PRIMARY KEY
    kind = user | group
    user_id?          // user-kind backing only
    status
    display metadata

principal_groups
    same (domain_id, principal_handle) key
    owner_user_id
    rev

principal_group_members
    group principal key
    user principal key
    membership state/revision
```

A group member must resolve to a user-kind principal; nested groups fail validation.
Group rows extend the principal row rather than minting another externally addressable
ID. Existing `users` / `user_identities` remain authentication owners. Migration
backfills user-principal rows from existing Domain membership/Agent ownership using the
already-published stable user handle, so existing `i:<owner_user_id>` refs do not
silently change.

The public Project is a principal-scoped view over the existing Workspace owner:

```text
ProjectRef = /d:<domain>/i:<principal>/p:<project>
project backing = existing authorized workspace_id
```

Do not create a duplicate Projects database merely to rename Workspace. The principal
segment supplies ownership/collaboration namespace; Workspace remains the source/
execution-policy backing. A future richer Project object can add metadata behind this
same ref without changing Agent identity.

DL-200 needs only bounded host metadata reads:

```text
domain.list(limit?, cursor?)
domain.get(ref, include?)

principal.list(domain?, limit?, cursor?)
principal.get(ref, include?)

project.list(principal?, limit?, cursor?)
project.get(ref, include?)
```

These are snapshot reads, not model tools and not watch APIs; changes are observed
through Events. Existing provisioning/admin operations for Domains/Workspaces remain
their current owner during migration. If later renamed to `project.create/archive`,
that is a façade over the same Workspace owner, never a parallel service.

---

## 25. Public Agent envelope; Session becomes internal storage

DL-200 has no public Session resource.

The canonical Agent ref:

```text
/d:<domain>/i:<principal>/p:<project>/a:<agent>
```

is the durable public identity for:

```text
Agent {
    owner/access ACL
    public rev
    lifecycle + admission
    immutable history head/base
    Sandbox binding
    staged input plan
    parent/delegation lineage
    checkpoints/event source
}
```

Conceptually:

> **Agent = conversation/session security envelope + durable history continuation + live
> execution owner.**

There is no second public Session name, ID, ACL or API to keep synchronized.

### 25.1 Internal compatibility

The current store may retain an opaque `session_id`, branch row and Session-oriented
tables as internal persistence/migration machinery.

That is explicitly an implementation detail:

- one public Agent resolves to one backing Session/branch tuple for its local mutable
  continuation;
- legacy Agents may temporarily share one Session while using different branches; DL-200
  does not require an eager history rewrite just to make those public envelopes distinct;
- `session_id` is not a canonical resource address;
- callers do not pass Session IDs to DL-200 APIs;
- new public receipts/provenance use canonical Agent refs;
- internal tables can be renamed/normalized later without changing the DL-200 surface.

Add one authoritative durable Agent-state owner rather than teaching every legacy Session
table the new public semantics:

```text
agent_state_current
    canonical {domain, principal, project, agent} handles
    backing {session_id, branch_id}
    rev
    acl_rev
    lifecycle/admission
    history_base_id?
    history_grant_id?
    parent_agent_ref?      // delegation/creator lineage, not history ancestry
    sandbox_id?

agent_acl_current
    agent
    principal
    complete allow-only PermissionSet
    grant revision/provenance
```

`parent_agent_ref` is host-minted from the trusted Agent delegation/creator context and is
never inferred from HistoryGrant/source history. A model-created child records the current
Agent as parent even when its permitted `from=` history source is another Agent/Turn.
Host-created historical forks outside an Agent delegation context normally have no
parent.

The public owner principal in `agent_state_current` is authoritative. For a group-owned
Agent, the old `sessions.owner_user_id` may remain an internal storage custodian during
migration and is **not** public Agent authority.

#### Migration away from Session-audit revision

Backfill/activation is an **exclusive store migration boundary**, not an online eventually-
consistent copy. During the migration the store is under its existing schema/write exclusion:
no serving/concurrent legacy Agent-envelope writes are admitted. In one migration transaction,
backfill one `agent_state_current` + `agent_acl_current` state set for every existing public
Agent binding, freeze canonical handles, seed each initial `rev` from the exact value the
legacy bridge would return (`MAX(audit.seq)+1` for that backing Session), and publish the new
schema/internal Agent-owner epoch **only after the complete seed is durable**. Commit makes
the new owner active atomically; rollback leaves the legacy owner active.

V1 deliberately does not implement online dual-write/catch-up for this cutover. If a future
zero-downtime migration is required, it needs a separately reviewed protocol rather than
quietly running both owners. This avoids a lifecycle/ACL/rev transition racing between seed
and activation.

After `agent_state_current` is activated, it is **authoritative durable Agent state**, not a
rebuildable projection. Back it up/restore it as owner state. SessionStore remains the
backing Turn/history owner, not a second Agent-envelope owner. Only derived lookup/list/
exposure indexes over Agent state are rebuildable; they rebuild from `agent_state_current`
and other authoritative owners, never by re-deriving Agent rev/lifecycle/lineage/ownership
from legacy Session/audit state.

After activation:

- only logical Agent-owner transactions advance `agent_state_current.rev`;
- audit/commit-log rows remain immutable evidence but no longer define Agent `rev`;
- direct ACL mutation advances `acl_rev` and Agent `rev` in the same transaction;
- group/user security changes advance their own owners and affect only derived
  `access_generation`;
- `jobs_get(rev, wait_ms)` watches the persisted Agent revision, with separate internal
  security-dependency wakeups that force reauthorization without synthesizing a rev;
- compatibility APIs compare/return the new persisted rev rather than rescanning audit.

Existing Session participation grants can seed the first direct Agent ACL conservatively:
map only the permissions they already possess; do not infer new `derive` or `share`
rights for non-owners. Legacy `members.can_append` remains compatibility authority only.

Existing stored provenance may retain legacy internal Session IDs for audit
compatibility, but canonical original-Agent attribution is the public referent.

### 25.2 Global identity and ownership

The `i:<principal>` segment is the Agent's stable owner/namespace principal selected at
creation.

An Agent owned by a group therefore lives naturally under that group's identity
namespace:

```text
/d:acme/i:compiler-team/p:runtime/a:reviewer
```

An Agent owned by Alice:

```text
/d:acme/i:alice/p:runtime/a:reviewer
```

Ownership transfer is not an ACL edit in v1. If later supported, it is an explicit
namespace move/transfer operation with its own migration/alias semantics; ordinary
`agent.access.set` never changes the canonical `i:` owner.

### 25.3 Why keep no public Session object

With one Agent per security/history envelope, a separate Session would duplicate:

```text
identity
ACL
rev
history head
Sandbox binding
lifecycle metadata
```

and force callers to answer "am I acting on the Agent or its Session?" for no semantic
gain.

The lower store is free to keep Session abstractions where they are useful for journal,
recovery and migration, but DL-200 exposes only Agent identity.

### 25.4 Logical Sandbox resource

A `Sandbox` is a durable **logical mutable execution environment**, not one long-lived
Bubblewrap/container/VM namespace and not the owner of Process lifetime.

Keep Sandbox and Process Scope orthogonal:

```text
Sandbox    = where/how execution may run
Scope      = what belongs to one Process lifetime
```

A Process may run with no logical Sandbox, with a Sandbox that only constrains
filesystem/network/credentials, or with a Sandbox/launcher that can provide stronger
process containment. None of those choices changes Process semantics or `ProcessTerminal`.

Conceptually:

```text
Sandbox
    id / project / rev / lifecycle
    parent?                 // logical derivation lineage
    named SourceBindings    // mutable materializations
    execution layout facts
```

A Project/Workspace may provide a base/default Sandbox. Product layers may create derived
Sandboxes and bind multiple Agents to the same Sandbox. In v1 an Agent may bind only a
Sandbox under that Agent's exact destination Project; cross-Project source transfer is
performed through authorized SourceAnchors/checkpoint restore, not by sharing one mutable
Sandbox across Project authority boundaries. A product such as Histate may group a
primary/shared Sandbox plus any derived/fresh Sandboxes under its own product Session
without reintroducing an AW public Session object.

Sandbox identity is canonical under its Project:

```text
SandboxRef = /d:<domain>/i:<principal>/p:<project>/s:<sandbox>
```

A minimal authoritative persistence shape is:

```text
sandboxes_current
    canonical {domain, principal, project, sandbox} handles
    backing workspace_id
    parent_sandbox_id?
    rev
    lifecycle

sandbox_source_bindings_current
    (sandbox_id, source_name) PRIMARY KEY
    driver_kind
    source_ref / source_identity
    access = ro | rw
    checkpoint_policy = required | ephemeral
    mutable binding locator/digest
    binding_rev / provenance

agent_state_current.sandbox_id
    -> current Sandbox binding for that Agent

agent_sandbox_grants_current
    (agent, sandbox_id) PRIMARY KEY
    grant_rev
    sandbox_policy_ref / policy_digest
    authority_ref / authority_generation
    actor/principal ceiling evidence
    agent access-generation dependency
```

The Workspace remains the Project backing owner; `sandboxes_current` owns only logical
mutable execution-state instances and does not duplicate Workspace metadata or lower
mount-policy truth. `agent_sandbox_grants_current` stores only durable binding/authority
evidence and a policy reference/digest; the actual validated mount/network/device policy
remains owned by the existing Sandbox policy/grant layer and is revalidated at execution. Sandbox has no second user/group ACL in v1: creation/list/get/close
authorize through the owning Project plus explicit Agent-to-Sandbox binding authority.
Per-Agent execution rights remain in Sandbox/tool/Vault grants rather than a parallel
Sandbox audience model. Sandbox policy may constrain what launch environment/capabilities
are available, but the Process runtime remains the sole lifecycle/Terminal owner through
its private Scope. Sandbox lifecycle/binding changes and completed AW-owned write-capable source mutations
advance Sandbox `rev` once per logical mutation transaction and emit
`s:<sandbox>.e:changed` (for example `data.aspect="sources"`). Sharing Agents do not
receive synthetic Agent `rev` fanout. Sandbox `rev` is not a filesystem snapshot
identity and does not claim to detect adversarial/out-of-band same-user writes; exact
capture operations use SourceDriver fingerprints and mutation barriers instead.

An Agent's Sandbox binding is selected at Agent creation and is immutable for that Agent
in v1. There is no `agent.sandbox.set`/rebind operation. To continue the same history in a
different shared/derived/fresh Sandbox, create a new Agent from the desired history or
checkpoint. This keeps mutable execution-state lineage explicit and avoids live rebind
ambiguity. SandboxGrant authority for an existing binding may still narrow/revoke through
its own host authority generations.

Many Agents may share one Sandbox's mutable state:

```text
Agent A ─┐
Agent B ─┼── Sandbox S
Agent C ─┘
```

but **same Sandbox does not imply same authority**. Each Agent has independently
authorized effective `SandboxGrant`/tool/Vault authority.

Keep the host Sandbox surface small:

```text
sandbox.list(project?, limit?, cursor?)
sandbox.get(ref, include?)
sandbox.create(
    project = <Project ref; default current/from Sandbox Project>,
    from?   = <Sandbox ref>
)
sandbox.close(ref, rev?)

sandbox.sources.list(ref)
sandbox.sources.set(
    ref,
    name,
    binding={
        source=<SourceRef>,
        checkpoint="required" | "ephemeral",
        access="ro" | "rw"
    } | null,
    rev
)
```

`sandbox.list/get` are authorized Project/Sandbox snapshot reads. Sandbox lifecycle and
binding mutations are owned by current Project/Workspace host authority, not by the ACL of
an Agent that happens to be bound to the Sandbox. In particular:

- `sandbox.create` requires destination Project Sandbox-create authority; deriving from
  another Sandbox additionally requires current read/derive authority on that Sandbox and
  every required SourceBinding;
- `sandbox.sources.set` requires Sandbox-manage authority plus exact metadata/materialize
  authority for the new SourceRef (or remove authority for the old binding);
- `sandbox.close` requires Sandbox-manage authority and the quiescence conditions below.

Agent `control/manage` permissions never manufacture any of these Project/source
capabilities.

`sandbox.get` returns at least `ref`, `rev`, lifecycle, parent lineage and a bounded named
SourceBinding summary; protected SourceRef/provider details follow the same metadata-
disclosure rule as checkpoint reads.

`SourceRef` is an opaque typed provider/driver source identity, not a secret and not
another canonical execution namespace. A Git repository, versioned object collection or
other driver-backed source can expose such a ref. It must not embed passwords, bearer
tokens, credential-bearing URLs or equivalent secret material. Credentials needed to
materialize or use it come through current Vault/network authority, never through
SourceRef itself.

The binding's `access=ro|rw` is only its maximum Sandbox-level mode. One Agent's actual
source access is the intersection of SourceBinding access, current SandboxGrant/tool
authority and current host policy. Thus two Agents may share the same rw-capable Sandbox
while one is effectively read-only.

`sandbox.sources.set` is the one host attach/replace/remove operation for named source
bindings. It is valid only while Sandbox lifecycle is open and is a quiescent structural
mutation in v1: AW acquires the Sandbox/source mutation barrier and refuses while any
active AW-owned Process/effect could observe or mutate the affected execution layout. Do
not attempt live remount/rebinding semantics in v1.

After validation/materialization through the selected SourceDriver, publication advances
that binding's `binding_rev` and Sandbox `rev` once and emits `changed` with
`data.aspect="sources"`. `binding=null` removes the named binding. There are no separate
add/edit/delete/move verbs.

Source/Sandbox grants are pinned to the exact SourceRef/binding generation they authorize.
Replacing `code` with another repository or changing its access mode never causes an old
grant for the previous binding to follow the name; affected grants must be explicitly
revalidated/reissued before use.

Semantics:

- `sandbox.create(from omitted)` -> fresh mutable environment from the explicit/current
  destination Project/Workspace baseline;
- `sandbox.create(from=S)` -> S must be `open`; destination Project defaults to S's
  Project and a new isolated logical Sandbox is derived from S's current
  required source state. In v1 `S` must belong to the exact destination Project; cross-
  Project transfer uses authorized SourceRefs/checkpoints rather than mutable-Sandbox
  lineage. The host reuses the checkpoint capture fence: all participating bindings are
  mutation-barriered together, unresolved write-capable Processes/effects make the derive
  conflict, and derivation publishes all-or-nothing. A driver may implement this as native
  COW or as a transient immutable capture+restore. Git dirty state reuses the same
  private-index tree capture semantics as checkpoints, with child-Sandbox retention rather
  than a public checkpoint. Explicitly ephemeral SourceBindings are recreated/omitted
  according to destination Project policy rather than copied as reproducible state;
- sharing needs no clone operation: pass the existing Sandbox ref to `agent.create`.

A derived Sandbox owns/pins every lower backing dependency needed for its bindings. Parent
Sandbox close/GC can never invalidate a live child merely because a driver used COW or a
shared object database.

This gives subagents three host-selected modes:

```text
share    -> bind the parent's exact Sandbox ref
branch   -> sandbox.create(from=parent Sandbox)
fresh    -> sandbox.create(project=destination Project)
```

The model does not receive raw Sandbox/mount/network/device selectors in v1. A trusted
delegation profile chooses the mode.

Sandbox lifecycle is simply `open | closed` in v1. `sandbox.close` is an idempotent
terminal lifecycle operation, not force-delete. It refuses while any open/closing Agent is
bound, any AW-owned Process/effect is still using the Sandbox, or a capture/derive/source-
mutation operation is in flight. There is no asynchronous `closing` Sandbox state and no
force-close bypass in v1. Once closed and
unreferenced, mutable materializations may be reclaimed according to their SourceDriver
retention rules while the Sandbox identity/provenance tombstone remains bounded.

`SandboxGrant` is authority, not state. Merely binding an Agent to a Sandbox grants no
source/filesystem/network/device access. Every execution/source effect revalidates the
initiating actor against the current Agent `access_generation`, the exact SandboxGrant
`authority_ref/generation`, SourceBinding maximum access and current host policy.

A SandboxGrant carries its own principal/actor ceiling (direct principal or live group
reference as appropriate). Later Agent ACL/group broadening does not automatically widen
the grant; a newly authorized Agent participant may use a Sandbox capability only when the
independent SandboxGrant authority also covers that actor after revalidation.

SandboxGrant changes govern newly admitted effects/Processes. They do not pretend to
rewrite the mount/network/device namespace of an already-running Process. Immediate
revocation of already-realized capability requires ordinary factual Process cancellation
or a backend-specific revocation mechanism whose outcome is itself recorded.

Child authority is monotonic by default:

```text
effective child authority
    = Project/host ceiling
    ∩ creator delegation ceiling
    ∩ child-specific trusted policy
```

A child may lose network/GPU/write capability; it cannot gain authority merely because
its history or Sandbox was derived from a parent. A trusted host may independently grant
additional authority through the ordinary host authorization path.

A logical Sandbox also does not mean all commands share one PID/mount namespace. Each
actual Process execution resolves current Sandbox state, intersects current Agent grants,
pins exact capabilities and constructs a fresh platform containment plan. Today that can
be Bubblewrap; future container/VM/remote executors can implement the same contract.

This DL-200 section defines only the logical composition layer. The existing
`017-sandbox-mount-convergence-2026-11-18` plan remains frozen and continues to own the
lower shared mount/pinning/Bubblewrap convergence until separately promoted.

### 25.5 Vaults and secret/service capability projection

Secrets and privileged host services are a fourth orthogonal axis:

```text
History   = what the Agent knows
Sources   = reproducible execution-relevant mutable state
Sandbox   = where mutable work happens
Vault     = privileged secret/service capabilities available while working
```

A Vault is never mounted wholesale as a readable directory. The host grants selected
entries through a `VaultGrant`; each entry is projected into an actual Process in the
safest supported form:

```text
broker / short-lived credential
socket / signing or agent service
ephemeral file
environment value
```

Prefer broker/socket/short-lived projections over raw long-lived files or environment
variables whenever the target tool supports them.

The public `projection` kind does **not** let callers choose arbitrary environment names,
filesystem targets or socket paths. Each VaultProvider/host integration advertises the
safe projection kinds it supports for that typed entry and maps the chosen logical binding
name to reserved Sandbox integration points. Examples include an AWS credential bundle
projected as provider-defined short-lived environment values or an SSH identity exposed
through a host-owned ssh-agent/signing socket. File/socket targets live under reserved
host-selected Sandbox locations and participate in the lower mount/env collision rules.
If arbitrary target mapping is ever needed, it requires a separately reviewed capability
surface rather than overloading Vault binding.

Conceptually:

```text
VaultProvider
    host/product-owned secret backend
    (AW private store, OS keyring, HashiCorp Vault, 1Password, cloud secret manager, ...)

VaultEntryRef
    opaque stable entry identity; never the secret value

VaultGrant
    Agent + entry + allowed projection/scope
    authority_ref / authority_generation
    principal/actor ceiling + Agent access-generation dependency
```

AW core does not need to become a general secret-management product. Providers own
secret creation/rotation/storage. DL-200 needs only provider-neutral references,
grant/revalidation and execution projection.

Persist only Agent-owned grant references/evidence:

```text
agent_vault_grants_current
    (agent, name) PRIMARY KEY
    provider_id
    entry_ref
    projection
    bounded scope metadata
    grant_rev
    authority_ref / authority_generation
    actor/principal ceiling evidence
    agent access-generation dependency
```

No raw secret value is stored in this projection. A direct `agent.vault.set` transition
is an Agent-owned configuration mutation and advances Agent `rev` once; provider-side
secret rotation/revocation advances provider authority generations and forces
reauthorization without fanout-writing every referencing Agent.

Attaching a VaultGrant validates the intended actor/principal ceiling independently from
Agent ACL. Later Agent ACL/group broadening never makes the secret available to a newly
admitted participant by itself. Every actual broker/socket/file/env projection rechecks
the initiating actor, current Agent `access_generation` and VaultProvider authority. A
stale/broadened mismatch fails closed until the host explicitly regrants.

Changing/removing a VaultGrant governs **future** projections and broker operations. It
does not claim to erase a secret already materialized into an existing Process environment
or file. Provider-revocable leases/sockets should be revoked when supported; otherwise a
host requiring immediate revocation must separately terminate/replace affected Processes
through the ordinary factual Process lifecycle. AW never reports retroactive secret
revocation it cannot prove.

The minimal Agent binding surface is:

```text
agent.vault.list(ref=".")
agent.vault.set(
    ref=".",
    name,
    binding={
        entry=<VaultEntryRef>,
        projection="broker" | "socket" | "file" | "env"
    } | null,
    rev
)
```

Vault binding names use the same bounded logical-name shape as SourceBindings in v1
(`[a-z][a-z0-9_-]{0,63}`). All Vault metadata reads return references/presentation
metadata only, never secret bytes.

Vault authority is **Agent-sensitive**, even when Agents share a Sandbox:

```text
same Sandbox S
├── Agent A -> github + aws
└── Agent B -> github only
```

At Process launch the effective environment is the intersection of current Sandbox
state, initiating actor, current Agent/access generation, Agent SandboxGrant, Agent
VaultGrants, tool authority and current host/provider authority.

History fork, Sandbox sharing/branching, checkpoint restore and ACL inheritance never
implicitly copy Vault grants. Model-created children receive no broader Vault authority
than explicitly permitted by trusted delegation policy. Checkpoints contain no secret
values, live credential leases, sockets or VaultGrant authority.

### 25.6 ExtensionRegistry: typed registries under one registry

The extension vocabulary stays small and semantic:

```text
Extension   installed/composable unit
Registry    owns a validated named collection
Hook        intercepts an in-flight boundary
Handler     reacts to an already committed fact
Action      explicitly invoked host/user operation
Tool        model-callable operation
View        bounded read-only derived data
State       extension-local durable data
Resource    inert discoverable content
UI          presentation
Setting     typed inert configuration
Provider    external model provider/integration
Compactor   history-compaction implementation
Driver      pluggable low-level implementation
Policy      actual authorization/admission/narrowing decision
```

`Registry` is intentionally part of the public vocabulary because AW already has a real
`ToolRegistry` and the word has one precise meaning: a host-owned named collection with
validation, collision rules, revision/generation and a common lifecycle.

Avoid vague public synonyms such as module/layer/loadout/entry/backend/manager when one of
the roles above already describes the concept.

`Policy` remains reserved for things that actually answer may/deny/narrow/admit.
`ToolPolicy`, `ToolRevisionPolicy`, `TrustPolicy` and the existing
`CompactionPolicy` fit that rule. The implementation that performs compaction is a
`Compactor`.

`Driver` remains the word for interchangeable lower implementations such as
`SourceDriver` and `SandboxDriver`.

#### Extension and ExtensionRegistry

Reclaim the plain name `Extension` for the installed unit. The current Rust trait named
`Extension` is hook-only and should migrate to `Hook`.

Conceptually:

```rust
trait Extension: Send + Sync + 'static {
    fn id(&self) -> ExtensionId;
    fn revision(&self) -> ExtensionRevision;

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<()>;
}
```

`ExtensionRegistry` is the composition/publishing root and exposes typed subregistries.
The leaf registries are independent final owners and may be introduced before the root when
a lower phase first needs them. Phase ordering must never create temporary registry types:
`EventTypeRegistry`, `SourceDriverRegistry`, `SandboxDriverRegistry` and
`VaultProviderRegistry` introduced earlier are the exact owners later composed here.

The root exposes:

```text
ExtensionRegistry
├── ToolRegistry
├── HookRegistry
├── ActionRegistry
├── ResourceRegistry
├── ViewRegistry
├── EventTypeRegistry
├── EventHandlerRegistry
├── ProviderRegistry
├── ProviderHookRegistry
├── ProcessHookRegistry
├── UiRegistry
├── SettingRegistry
├── StateRegistry
├── CompactorRegistry
├── SourceDriverRegistry
├── SandboxDriverRegistry
├── VaultProviderRegistry
└── TrustPolicyRegistry       // operator/global Extensions only
```

These registries own or project **registered definitions**, never per-Agent grants or live
authority. They are not duplicate databases/authority engines. Composition under
`ExtensionRegistry` adds atomic extension-scoped staging/publication and root snapshot
lifecycle; it does not replace or migrate leaf registry identity/state.

For Tools specifically, `ToolRegistry` is the authoritative definition/catalog identity.
The same definition-vs-binding rule applies to Hooks and other executable contributions:
installation never means universal activation. Each Agent/runtime receives a generation-
pinned `ToolBindingView` (or equivalent
projection) combining those definitions with that Agent's ToolPolicy, Actor Envelope,
availability and model visibility. The current implementation's Agent-local cloned
ToolRegistry is a migration/implementation detail; it must not become independently
mutable definition truth. ProviderRegistry vs per-Agent ProviderAccounts/model selection
follows the same definition-vs-binding rule.

The author-facing shape is regular:

```rust
registry.tools().register(tool)?;
registry.hooks().register(hook)?;
registry.actions().register(action)?;
registry.resources().register(resource)?;
registry.views().register(view)?;
registry.event_types().register(event_type)?;
registry.event_handlers().register(handler)?;
registry.providers().register(provider)?;
registry.provider_hooks().register(hook)?;
registry.process_hooks().register(hook)?;
registry.ui().register(ui)?;
registry.settings().register(setting)?;
registry.state().register(state_schema)?;
registry.compactors().register(compactor)?;
registry.source_drivers().register(source_driver)?;
registry.sandbox_drivers().register(sandbox_driver)?;
registry.vault_providers().register(vault_provider)?;
```

The common registry lifecycle is typed and regular:

```text
register<T>(value: T) -> Handle<T>
remove<T>(handle: Handle<T>, expected_revision?)
get<T>(handle: Handle<T>) -> T / Arc<T>
list() -> bounded registration descriptors
revision() -> RegistryRevision
```

where the operation makes semantic sense. `list()` is descriptive/introspective; callers
that want to invoke a concrete native registration use its typed Handle rather than
recovering a value by downcasting a descriptor. Individual registries may expose
additional typed read helpers but should not invent different lifecycle verbs.

Every registration is owned by the current Extension namespace. An Extension cannot
remove/replace another Extension's registrations merely by naming them. Host/operator
administration may remove an entire Extension.

The registry implementation shares one private typed-key/snapshot/revision/stage core;
individual registries retain their unique schema, dependency, removal and activation
rules. The common core is not a Registry of registries, a new permission engine or a
reason to implement dormant registry families before their first consumer.

Every leaf registry uses the same private staged-mutation primitive from its **first**
implementation; published leaf state is never mutated piecemeal. Before the root exists,
Phase-2/3 users simply stage/validate/publish a one-leaf transaction. This may be implemented
as a small shared `RegistryStage`/`RegistryTxn` mechanism or equivalent internal snapshot
primitive; the name is not public API.

The `ExtensionRegistry` passed to `Extension::register` later coordinates those same leaf
stages as one extension-scoped **staging transaction**, not a set of immediately-mutating
registry references. All contributions from that call are validated together and published
atomically as one root generation. A collision, schema error, invalid authority declaration
or failed subregistry validation aborts the whole registration; no half-installed
Extension is visible. This composition adds no shadow registry and requires no migration of
earlier leaf ownership.

A later dynamic update uses the same extension-scoped, generation-fenced transaction
model with an expected registry revision. This covers Pi-style dynamic registration
without giving runtime callbacks unrestricted mutable access to global registries.

When the composition root exists, a multi-leaf register/remove/reload publishes one new
ExtensionRegistry generation as **snapshot/publication metadata**. Each leaf still owns its
local publication revision from first use. Typed Handle validity depends on that exact
registration identity/type/schema/local revision, never on an unrelated root generation.
Before Phase 5, one-leaf staged publication simply advances the leaf's local revision; no
placeholder root clock is required. In-flight work remains pinned to the exact published
registry snapshot + authority generation under which it was admitted; newly admitted work
sees the new snapshot.

Removing one registration or an entire Extension withdraws registered definitions but
**does not destroy durable extension State, EventHandler cursors, product data, source
anchors or secret custody**. Those records are retained/tombstoned according to their own
owner. Reinstalling the same stable Extension/local IDs and compatible schema may resume
them only after current authority/schema revalidation. Destructive data cleanup is a
separate explicit host maintenance operation, never a side effect of registry removal.

A definition registry may refuse removal when the implementation itself is required to
keep live durable state valid. In particular, SourceDriver removal/incompatible replacement
is refused while **any** of these depend on that exact driver implementation/recovery schema:

- live SourceBindings;
- live Checkpoint SourceAnchors/retention;
- unresolved `Prepared|Unknown` operation RecoveryDescriptors from capture/restore.

Each RecoveryDescriptor records the exact driver registration identity + recovery schema
version that can inspect/cleanup it. A replacement may take over an unresolved descriptor
only when it explicitly declares/validates compatibility with that original descriptor
schema and semantics in the same atomic registry generation. Otherwise the old driver stays
pinned until reconciliation reaches a terminal state. This prevents retaining perfect
recovery identity but losing the implementation required to use it.

The same dependency contract is validated **durably at store-open/startup**, not only on an
in-process `remove()`. Before serving/effectful source operations, compare every persisted
live SourceBinding, live Checkpoint SourceAnchor/retention root and unresolved
`Prepared|Unknown` RecoveryDescriptor against the installed SourceDriver registry. The exact
durable driver identity/recovery schema must be present or explicitly covered by a
compatible replacement that declares and validates support for that schema.

If a required driver/schema is absent or incompatible after binary/config upgrade, affected
source-dependent resources enter a fail-closed **degraded/recovery-required** state: no
execution/checkpoint/capture/restore/reconciliation effect may reinterpret, clean up or retry
that state through another driver. Read-only diagnostics/metadata may remain available where
safe. Recovery requires reinstalling the exact compatible implementation or an explicitly
compatible replacement, then normal inspection/reconciliation.

SandboxDriver removal is likewise refused while live execution depends on it. VaultProvider
removal may make grants unavailable/fail-closed but never deletes or exports provider-held
secret material.

Registering an Extension grants nothing by itself. **Installed/registered is not the same
as active/bound.** Discovery/manifest loading only publishes definitions; trusted host
composition chooses which definitions are active for which scope, preserving the current
`aw-extensions` rule that discovery never grants tools/hooks.

- Tool -> ToolRegistry definition + Agent/runtime ToolPolicy/Actor Envelope binding;
- Hook -> HookRegistry definition + explicit host-selected Agent/runtime Hook binding and
  allowed Stage/Observe|Transform|Gate capabilities;
- Action -> definition may be presented, but invocation always uses canonical host APIs
  and current caller authority;
- Resource -> Project/resource trust;
- View -> already-authorized resource read; no ambient source authority;
- EventType -> typed custom Event schema/key over the existing AW Event system;
  EventTypeRegistry owns no Event instances/history;
- EventHandler -> definition is inert until the host explicitly enables it with bounded
  service authority/pattern; delivery then reauthorizes every Event;
- Provider -> Provider definition + explicit account/model selection;
- UI -> surface host selects/render/binds the contribution;
- SourceDriver -> source capture/materialization implementation, never source authority;
- ProviderHook/ProcessHook -> definitions require explicit trusted host activation for
  their exact scope/capabilities;
- SandboxDriver -> implementation definition; Sandbox/Jobs authority remains separate;
- VaultProvider -> secret backend integration, never secret-use authority by registration;
- secret Setting -> Vault rather than ordinary Setting storage.

Native linked Extensions are trusted same-process Rust code. The typed API constrains
AW-mediated operations but is not an OS sandbox against malicious native code.
Untrusted/dynamic extension processes should run under host-selected Sandbox isolation
and receive only explicitly granted typed/wire capabilities.

Project-local Extensions load only after Project trust is established; they cannot decide
the trust that authorizes themselves. Only operator/global Extensions may register a
TrustPolicy.

#### Fully typed registration

Native Rust registration is **typed end to end**. The registry must not force a Histate
extension to convert its domain types to `serde_json::Value` merely to register them.

The common pattern is:

```rust
registry.actions().register::<ForkDecision>(ForkDecision::new(...))?;
registry.views().register::<DecisionView>(DecisionView::new(...))?;
registry.event_handlers().register::<DecisionIndexer>(DecisionIndexer::new(...))?;
```

Each role carries its Rust types through associated types/generic registration.

`ExtensionInput` / `ExtensionOutput` are marker bounds for safe owned Rust values
(`Send + Sync + 'static`) plus an explicit boundary codec/schema **only when** that
registration is exposed across a model/web/RPC/IPC/storage boundary. Purely local typed
registrations do not need to serialize themselves merely to exist.

Conceptually:

```rust
trait Action {
    type Input: ExtensionInput;
    type Output: ExtensionOutput;

    fn id(&self) -> ActionId;
    async fn run(
        &self,
        ctx: &ExtensionContext,
        input: Self::Input,
    ) -> Result<Self::Output>;
}

trait View {
    type Resource: ResourceKind;
    type Query: ExtensionInput;
    type Output: ExtensionOutput;

    fn id(&self) -> ViewId;
    async fn read(
        &self,
        ctx: &ViewContext,
        resource: TypedResourceRef<Self::Resource>,
        query: Self::Query,
    ) -> Result<Self::Output>;
}

trait EventHandler {
    fn id(&self) -> HandlerId;
    fn pattern(&self) -> EventPattern;

    async fn handle(
        &self,
        ctx: &ExtensionContext,
        event: &Event,
    ) -> Result<HandlerAck>;
}
```

`EventTypeRegistry` optionally registers typed custom Event definitions only; it is a schema/key registry, never an Event instance/history owner:

```rust
let decided: EventKey<DecisionAccepted> =
    registry.event_types().register("decision_accepted", schema_version(1))?;
```

At runtime:

```rust
ctx.events.publish(decided, DecisionAccepted { ... }).await?;
```

The stable wire subject remains the ordinary AW custom Event namespace. For an Extension
EventType the host derives the collision-safe extension-qualified token (for example
Histate local `decision_accepted` -> `e:#histate_decision_accepted`); registration adds a
typed adapter/schema, not another Event namespace or bus.

`EventKey<T>` provides type/schema identity, **not emitter authenticity**. Public/model
`events.publish` may still publish ordinary bounded `#name` JSON Events, including a
string that happens to equal an Extension-qualified subject. Typed publication validates and records the registered schema key/version in `Event.schema`
for typed decoding. A raw same-subject custom Event has no such matching tag and is handled
as a generic Event (or rejected by a typed-only Handler), never decoded as `T` merely from
the subject. A Handler that requires a trustworthy
assertion must also validate the immutable Event source/actor/tool/Extension provenance
and current authority; subject/schema alone are never a signature. A broad Handler that
intentionally observes heterogeneous AW Events may use the ordinary typed Event envelope.

Tool registration keeps the existing authoritative `Tool` trait. A
`TypedTool<Input, Output>` adapter may provide serde/schema conversion for extension
authors while the model/provider boundary remains JSON Schema as required by tool-call
protocols.

Typed registration returns typed handles. A handle carries the stable registration key,
type/schema evidence and the **specific registration revision** where executable-definition
identity matters; it does not use the root ExtensionRegistry generation as its ordinary
staleness key:

```rust
let fork: ActionHandle<ForkDecision> =
    registry.actions().register(ForkDecision::new(...))?;

let summary: ViewHandle<SummaryView> =
    registry.views().register(SummaryView::new(...))?;
```

Direct Rust callers that hold the typed handle can remain typed. `get(handle)` /
invocation revalidates that exact registration's current revision/type/schema; a removed
or replaced incompatible registration fails as stale rather than dispatching to a new
type with the old handle. An unrelated ExtensionRegistry generation change does **not**
stale the handle. A compatible replacement may explicitly re-resolve a fresh handle by
stable key.

The root ExtensionRegistry generation exists to publish/pin one atomic registry snapshot
for newly admitted work. It is not a global invalidation token for every typed handle.

The registry may erase the concrete implementation behind an internal object-safe
adapter for heterogeneous storage/dispatch, but that erasure is an implementation detail.

Rules for type identity:

- durable/wire registration identity is stable `ExtensionId + local registration id`;
- executable definitions own a local monotonic registration revision/fingerprint for
  stale-handle detection;
- durable keys such as StateKey/EventKey use stable identity + explicit schema version and
  can survive unrelated code/registry generations when schema-compatible;
- any encoded/persisted input/output/state/event schema has its own explicit schema
  version; pure in-process registrations need no fake schema version;
- Rust `TypeId` may be used as an in-process optimization/check but is never durable or
  wire identity;
- type/schema mismatch fails closed;
- a reload that changes a durable/transport schema requires an explicit schema version
  and migration/compatibility rule;
- JSON/CBOR/IPC encoding happens only at a transport/storage/model boundary, not as the
  author-facing Rust registration contract.

Local registration IDs are extension-local. Whenever a registration needs a global wire
name, the host derives a collision-safe name from `(ExtensionId, local_id)` rather than
requiring authors to self-prefix strings:

```text
View        histate + summary            -> histate.summary
Action      histate + fork_decision      -> histate.fork_decision
EventType   histate + decision_accepted  -> #histate_decision_accepted
Tool        histate + decision           -> host/model-safe extension-prefixed tool name
```

The adapter enforces each target protocol's length/character bounds and registration fails
on an unrepresentable name. Manual host/model custom Events such as `#ready` remain
ordinary unregistered Event names. This prevents two Extensions from colliding merely
because both use a natural local ID such as `status` or `ready`.

This gives Histate compile-time types without making the wire format Rust-specific.

#### Typed StateRegistry, runtime ctx.state

`StateRegistry` registers **state schemas/keys**, not mutable state values.

Conceptually:

```rust
let local: StateKey<HistateLocalState> =
    registry.state().register("local", schema_version(1))?;
```

At runtime the author uses the ergonomic scoped port:

```rust
let current: StateSnapshot<HistateLocalState> = ctx.state.get(local).await?;
ctx.state.append(local, next_state, expected_seq = current.seq).await?;
```

`get` returns the current typed value/projection plus the per-key `seq`. An append derived
from an observed snapshot uses `expected_seq` and conflicts if another writer advanced the
key. Blind append may omit the fence only for independently meaningful event/delta entries
whose schema does not depend on the previously read value. The store serializes each
append atomically and advances `seq` exactly once; there is no cross-key transaction DSL
in v1.

A registered state schema may choose snapshot-folded or append-log projection semantics;
the author-facing value remains typed either way.

Backing persistence may remain:

```text
agent_extension_state_log
    agent
    extension_id
    state_key
    schema_version
    seq
    bounded encoded typed value/delta
    turn/history-head provenance?
    created_at
```

Properties remain:

- durable across reload/restart of the same Agent;
- seq is the per-(Agent, Extension, state key) revision;
- not model-visible unless a Hook/Tool explicitly projects it;
- the stored State identity/seq/value is outside HistoryBase and Agent `rev`; it does not
  enter PrefixContentKey merely because it changed;
- if a Hook/Tool deliberately projects State-derived bytes into model-visible context,
  those exact projected bytes participate in PrefixContentKey/request-cache identity;
- never contains Vault/secret material;
- not inherited by `agent.create(from=...)` or checkpoint restore by default.

State spanning Agents/Projects/teams or participating in product fork/merge semantics
belongs in the embedding product's typed store, not Agent-local state.

#### Typed ToolRegistry, runtime ctx.tools

Keep `ToolRegistry` as the authoritative registered Tool collection. Do not add a
`ToolLoadout` type.

Extension registration:

```rust
registry.tools().register(HistateTool::new(...))?;
```

Runtime scoped access:

```text
ctx.tools.registered()       registered Tools visible to this Extension scope
ctx.tools.callable()         registered ∩ current ToolPolicy/Envelope authority
ctx.tools.visible()          callable ∩ currently model-declared
ctx.tools.set_visible(...)   choose a subset of callable Tools
ctx.tools.call(...)          exact currently-callable Tool invocation
```

`set_visible` never creates ToolPolicy authority. A visible Tool schema change is
model-visible invocation evidence and participates in PrefixContentKey/request-cache
identity.

Tool metadata/effect declarations supplied by an Extension are descriptive inputs, never
authority. In particular, an external process Tool's self-declared effect is not trusted
proof that its code cannot do more; v1 keeps the effective ToolEffect conservative
(`Unknown`) unless an independent trusted host policy supplies/verifies a narrower
classification. A manifest field can never downgrade required Tool/Sandbox/Vault
authority.

#### Hooks and EventHandlers

`Hook` means only in-flight interception.

Final Agent Hook stages:

```text
Turn
Step
Input
Context
Tool
```

Hook capabilities remain:

```text
Observe
Transform
Gate
```

Examples:

- Input Hook may transform/gate not-yet-committed input while preserving provenance;
- Context Hook may change model-visible projection of authorized immutable facts;
- Tool Hook may gate a proposal; argument revision still requires ToolRevisionPolicy;
- Tool-result Hook may alter only model-visible projection, never factual receipt.

A Context Hook cannot remove/rewrite non-overridable AW authority/safety layers,
provider-private signed evidence, factual Tool receipts or authoritative Tool declarations.
Any model-visible change participates in PrefixContentKey/request-cache identity and must
preserve/union the disclosure dependencies of facts it projects; an ordinary Hook is not
a declassification boundary.

Post-commit lifecycle/message/output/Event observation is not a Hook. Durable observation
uses AW Events + `EventHandlerRegistry`; presentation-only streaming observation belongs
to UI.

`ProviderHook` and final `ProcessHook` are typed specialized Hook roles because those
requests carry different sensitive fields. Do not confuse final `ProcessHook` with the
current `aw-extensions` implementation struct also named `ProcessHook`, which actually
means "Agent Hook hosted by an external process" and handles Prompt/Tool callbacks. That
legacy implementation type must be renamed (for example `RemoteHook` or
`ProcessHostedHook`) during convergence; it does not define the final public role.

Final roles:

- ProviderHook sees only host-granted provider fields; auth headers/secrets are hidden by
  default. Its effective chain digest is invocation evidence/cache identity when request
  bytes change.
- ProcessHook may observe/narrow/transform a prepared Process request but cannot widen
  Sandbox/Jobs/Tool/Vault/network authority.

#### Actions

`ActionRegistry` owns explicitly invoked host/user operations.

Conceptually:

```text
Action {
    typed Input
    typed Output
    title / description
    optional bounded completions
    run
}
```

Slash commands, keyboard shortcuts, web buttons/menus and RPC methods are bindings to the
same Action. Action metadata/completions are introspectable so help/palettes use the
ActionRegistry rather than a second command catalog.

A binding never grants an Action; invocation reauthorizes the caller and every effect.

Extension callbacks receive a role-scoped `ExtensionContext` with only permitted runtime
ports:

```text
agent
inputs
tools
models
process
events
ui
state
cancel/deadline
```

An Action submits Agent input only through the canonical input owner, preserving
extension/action/caller provenance. Agent creation/restore still use
`agent.create/checkpoint.restore`.

A display-only custom message belongs in `ctx.state` projected by UI; rendering does not
make it model input.

#### Resources, UI, Settings and Drivers

`ResourceRegistry` owns inert discoverable content such as skills, prompt templates and
themes. Resource values may be typed in Rust; transport encoding is a boundary concern.
Project Resources still pass Project trust/path/digest validation. Resource registration
grants no Tool/provider/execution authority. Merely registering/changing a Resource does
not perturb Agent history/cache identity; when its exact content is intentionally admitted
into model-visible context, those projected bytes/digest participate in PrefixContentKey
and carry their disclosure dependency.

`UiRegistry` owns semantic presentation contributions over the existing
`aw-ui-extension` foundation:

```text
notify
select / confirm / input
editor / autocomplete
status / widget / header / footer
message / state / tool rendering
Action bindings
```

TUI/web/desktop hosts implement those semantic contracts independently.

`SettingRegistry` owns typed inert configuration definitions and can return typed
`SettingKey<T>` handles. Runtime `ctx.settings.get(key)` returns the resolved `T`. Secret
settings resolve through VaultEntryRef/VaultGrant custody rather than ordinary Setting
storage. Setting changes do not affect history/cache identity merely by existing; if a
resolved setting changes model-visible prompt/context/tool/provider request bytes, the
resulting effective projection/request digest changes through the ordinary
PrefixContentKey/BackendCacheKey path.

`CompactorRegistry` owns implementations that derive a bounded summary/context
**projection** from authorized immutable history. A Compactor never rewrites/deletes
HistoryBase facts, changes attribution, or grants authority; the original immutable
history/provenance remains authoritative and retained under normal history policy.
`CompactionPolicy` remains separate because it decides whether/when such a projection is
allowed, and the existing tool-free SummaryExecutor/store CAS remains the paid/effect
boundary where applicable.

`SourceDriverRegistry` owns pluggable SourceDriver implementations such as Git and future
versioned object/bucket sources. Registration supplies implementation capability only;
SourceRef visibility/capture/materialization authority remains independently checked.

`VaultProviderRegistry` owns VaultProvider integrations such as AW private custody, OS
keyrings or external secret managers. Registration exposes no secret value and grants no
Agent permission to use an entry.

`SandboxDriverRegistry` owns pluggable Sandbox realizations such as Bubblewrap,
containers, VMs or remote workers. A Driver implements an existing logical Sandbox; it
does not own Sandbox authority **or Process lifetime**.

A SandboxDriver may supply an opaque host-only launch capability/factory that lets the
Process runtime choose a stronger private Scope backend (for example a Linux PID-namespace
supervisor or a remote/container execution scope). That capability is implementation, not
a second Process API. The Process runtime still owns wait/stop/Terminal semantics.

#### Registry vs runtime context

Registries answer **what is installed/registered**.

`ExtensionContext` answers **what this invocation may currently use**.

Therefore keep the ergonomic runtime ports even with registries:

```text
registry.tools()       registration/catalog owner
ctx.tools              current scoped/authorized Tool view

registry.state()       typed state-key/schema registration
ctx.state              current Agent-local state access

registry.ui()          registered UI contributions
ctx.ui                 current surface interaction port

registry.event_types() registered typed custom Event schemas/keys
ctx.events             current authorized Event publish/read port

registry.settings()    registered typed Setting definitions
ctx.settings           current resolved typed Setting values

registry.providers()       registered Provider implementations/routes
ctx.models                 current authorized model/provider choices; selection only when
                           the host grants the existing model-selection owner/capability

registry.source_drivers()  registered source implementation definitions
                            // no ordinary Agent ctx port; sources are host/Sandbox-owned

registry.vault_providers() registered secret-backend implementations
                            // runtime secret use remains through Agent VaultGrants
```

Do not replace these scoped runtime ports with direct mutable registry access.
`ctx.models.select(...)`, when granted, delegates to the existing host/provider-selection
owner; it is not Extension-owned state. If that owner persists an Agent-visible future-
Turn default, the mutation follows the ordinary Agent configuration/revision path.

### 25.7 ViewRegistry and EventHandlerRegistry: typed pull and push

The product-integration pair remains simple:

```text
ViewRegistry           registers typed pull/read facets
EventHandlerRegistry   registers durable push/background reactions
```

#### Views

`ResourceRef` remains the non-authority identity vocabulary:

```text
ResourceRef =
    Domain | Principal | Project | Agent | Turn | Job | Node
    | Checkpoint | HistoryBase | Event | Sandbox
```

A typed View chooses its exact Resource kind, query type and output type at registration.
Snapshot APIs still request views by their stable wire name:

```text
include=["histate.summary", "histate.decisions"]
```

The runtime dispatch resolves the stable name to the registered typed View and encodes its
typed output only at the API transport boundary.

A View:

- runs only after AW core read authorization;
- may apply stricter product visibility, never weaker visibility;
- returns disclosure/provenance dependencies with any value later admitted into model-
  visible or durable Agent context;
- cannot mutate/grant authority;
- owns no AW resource revision/lifecycle;
- remains bounded;
- does not perturb model/history/cache identity unless its output is explicitly admitted
  into model-visible context.

Views are enrichment only. Once the core resource read is authorized, **no View failure
invalidates or hides the authoritative core snapshot**. Each requested name returns either
its bounded typed result or a bounded per-View error under the `extensions` result:
`unknown`, `denied`, `unavailable`, `stale_schema` or execution failure as appropriate.
`unknown` still catches caller typos, but a removed/reloading Extension therefore cannot
turn a formerly valid `agent.get/project.get/...` into total resource-read failure. There
is no strict all-or-nothing View mode in v1.

#### EventHandlers

`EventHandlerRegistry` registers durable post-commit handler **definitions** over the same
immutable `commit_log`. Definition registration creates no cursor and pins no retention.
A trusted host explicitly enables a Handler with its bounded service authority and one
private start policy:

```text
start = now                 // default; atomically baseline at current Event tail
      | earliest_available  // explicit historical replay requirement
```

EventHandler cursors use the same StoreId + **authority-scoped** selector placement-
generation affinity under their explicit service authority. Hidden topology outside that
authority cannot perturb their state. Store/placement mismatch within the authorized scope
stops delivery and retention pinning and marks the Handler `lagged` until explicitly
rebaselined; Handlers never create an implicit cross-store merge.

For a product read-model bootstrap, the race-safe default is: enable an **active** `now`
cursor first so its bounded backlog participates in retention, build the product's current
state from authoritative AW snapshots, then drain delivery from that baseline. Snapshot/
Event overlap is tolerated by Event.id/product idempotency. If bootstrap runs beyond the
configured active-handler retention ceiling it becomes `lagged` explicitly rather than
silently losing changes. `earliest_available` never promises history older than physically
retained Event truth; absence fails/starts lagged explicitly rather than fabricating a
complete history.

AW persists only delivery state:

```text
event_handler_cursors_current
    extension_id + handler_id PRIMARY KEY
    handler revision/schema
    normalized pattern hash
    StoreId + selector placement generation
    private commit_log position
    status = active | suspended | lagged
    authenticated service/host authority ref + generation
```

The Handler owns any external/product state it builds; AW owns Event truth, activation
baseline and cursor.

Delivery is at least once. The cursor advances only after acknowledged/idempotently
accepted handling. `Event.id` is the idempotency/provenance key. External-effect handlers
own effect receipts/idempotency; Event replay never makes an uncertain effect magically
safe to retry.

An active Handler's unacknowledged range participates in bounded host-managed Event
retention. Removing/disabling the Extension or Handler changes the cursor to `suspended`:
it stops delivery and stops pinning Event retention beyond the ordinary public/host floor.
A compatible reinstall may resume from the retained cursor only if that cursor still lies
within available Event history; otherwise it becomes `lagged` and requires explicit
recovery. Falling beyond the configured lag/storage ceiling while active likewise marks
it `lagged` rather than silently skipping facts.

Every Handler delivery reauthorizes current service authority against the exact concrete
Event source **and its DisclosureManifest closure**. A trusted product Handler does not
receive protected Event payload merely because its pattern matches. Any AW mutation
returns through an ordinary authorized host API.

This remains distinct from Agent subscription:

```text
events.subscribe       Event -> recipient Agent EventInput
EventHandlerRegistry   Event -> trusted host/product handlers
```

### 25.8 Pi capability coverage under the registry model

```text
Pi capability                         AW extension shape
──────────────────────────────────    ─────────────────────────────────────────────
lifecycle/input/context hooks         HookRegistry
post-commit lifecycle/event observe   AW Events + EventHandlerRegistry
tool_call/tool_result hooks           HookRegistry + ToolRevisionPolicy
custom compaction                     CompactorRegistry
registerTool                          ToolRegistry
active/deferred/hidden tools          ctx.tools visible/callable/registered
tool-to-tool invocation               ctx.tools.call
commands                              ActionRegistry
keyboard shortcuts                    UiRegistry binding -> Action
CLI flags                             temporary Setting/Action binding
command autocomplete                  Action typed completions
sendUserMessage                       canonical Agent input port
sendMessage display-only              ctx.state + UI
sendMessage trigger Turn              Agent input port with extension provenance
appendEntry                           StateRegistry + ctx.state
skills/prompts/themes                 ResourceRegistry
settings                              SettingRegistry; secrets -> Vault
dialogs/widgets/status/editor         UiRegistry / ctx.ui
message/tool/state renderers          UiRegistry
provider registration                 ProviderRegistry
provider request/header/stream hook   ProviderHookRegistry
virtual models                        ProviderRegistry/model routing
model/thinking selection              ctx.models
MCP server/tool sources               existing MCP adapter + exact grants; secrets via Vault
exec/bash spawn customization         ProcessHookRegistry + ctx.process + Sandbox/Jobs
SSH/container/VM execution            SandboxDriverRegistry
project trust                         TrustPolicyRegistry; operator/global only
reload                                atomic ExtensionRegistry generation replacement
extension-to-extension events         AW Events or ephemeral local typed channels
session fork/new/resume               agent.create / checkpoint.restore / navigation
session name                          agent.edit(title=...)
session labels/bookmarks              Agent fields or ctx.state
```

No extension-specific Session API is reintroduced.

### 25.9 Fully typed Histate example

Histate can be a normal native Rust `Extension` with concrete domain types at
registration time.

Conceptually:

```rust
struct HistateExtension {
    store: Arc<HistateStore>,
}

struct SummaryView {
    store: Arc<HistateStore>,
}
impl View for SummaryView {
    type Resource = AgentResource;
    type Query = SummaryQuery;
    type Output = HistateSummary;
    // ...
}

struct ForkDecision;
impl Action for ForkDecision {
    type Input = ForkDecisionInput;
    type Output = ForkDecisionResult;
    // ...
}

struct DecisionIndexer {
    store: Arc<HistateStore>,
}
impl EventHandler for DecisionIndexer {
    // broad/system Event pattern + typed Event envelope
}

struct DecisionAccepted {
    decision: DecisionId,
    branch: DecisionBranchId,
}

impl Extension for HistateExtension {
    fn id(&self) -> ExtensionId { "histate".into() }
    fn revision(&self) -> ExtensionRevision { /* stable code/config revision */ }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<()> {
        registry.state()
            .register::<HistateLocalState>("local", schema_version(1))?;

        registry.views()
            .register(SummaryView { store: self.store.clone() })?;

        registry.views()
            .register(DecisionView { store: self.store.clone() })?;

        let decision_accepted: EventKey<DecisionAccepted> = registry.event_types()
            .register("decision_accepted", schema_version(1))?;

        registry.event_handlers()
            .register(DecisionIndexer { store: self.store.clone() })?;

        // Histate callbacks may later publish the typed payload through
        // ctx.events.publish(decision_accepted, DecisionAccepted { ... }).

        registry.actions()
            .register(ForkDecision)?;

        registry.ui()
            .register(HistateUi::new(self.store.clone()))?;

        registry.tools()
            .register(HistateDecisionTool::new(self.store.clone()))?;

        registry.hooks()
            .register(HistateContextHook::new(self.store.clone()))?;

        Ok(())
    }
}
```

Nothing in that Rust registration path requires a `serde_json::Value`-typed Histate
domain model.

At boundaries:

- model Tool protocol -> schema/JSON adapter;
- HTTP/web/RPC -> typed value serialized by transport adapter;
- durable extension State -> explicit typed codec/schema version;
- dynamic child-process extension protocol -> validated manifest/schema + wire encoding.

Histate's durable semantic graph remains in Histate's typed store:

```text
Histate owns
    Build
    product Session
    Decision / Option / Evidence
    summaries
    semantic/decision graphs
    commit classification
    artifacts / semantic edges
```

AW owns the execution/history/source/sandbox/vault/Event primitives plus the typed
registry contracts.

A Histate product Session may reference many AW Agents and zero or more AW Sandboxes
(often one primary/shared Sandbox plus derived/fresh experiment Sandboxes). It remains
distinct from AW's internal SessionStore.

The three DAGs stay independent but cross-referenceable:

```text
AW History DAG
Source DAG
Histate Decision DAG
```

A Histate checkpoint composes typed Histate decision/summary/graph heads with **one or
more** AW Checkpoint refs according to product scope. A simple single-Agent Session may
need one AW checkpoint; a coordinated product snapshot spanning several Agents/Sandboxes
may reference several. Histate owns that multi-checkpoint consistency/product transaction
semantics; AW checkpoints remain independently valid execution-state anchors.

Histate product forks resolve to the same minimal AW primitives:

- history alternative -> `agent.create(from=...)`;
- exact history+source continuation -> `checkpoint.restore(...)`;
- shared mutable workspace -> same Sandbox;
- isolated workspace branch -> `sandbox.create(from=...)`;
- fresh workspace -> `sandbox.create(project=...)`.

AW does **not** promise an atomic transaction across an Extension/product database and the
AW store. A product Action that must update Histate state and create/mutate AW resources
uses an idempotent product-owned saga/outbox keyed by the same authenticated mutation
identity:

```text
product intent/outbox -> canonical AW mutation -> persist returned AW ref/Event.id/receipt
                      -> complete product state
```

On crash/retry the product replays/reconciles from its intent plus AW's factual idempotent
result rather than repeating an uncertain effect. AW Events/EventHandlers may accelerate
reconciliation but are not a distributed transaction coordinator. ExtensionRegistry owns
no cross-store commit protocol.

No Histate semantic graph row belongs in Agent-local `ctx.state` or the AW Agent record.

### 25.10 Dynamic extensions: process first, WASM later

The minimal dynamic-extension strategy is **not** to embed a JavaScript runtime or a
WebAssembly runtime in v1.

AW already has a bounded external-process extension path in `aw-extensions`: explicit
absolute executable selection, no shell/PATH lookup, cleared environment, bounded
line-framed JSON requests/responses, manifest validation, exact tool/hook opt-in and
connection poisoning after uncertain/incomplete calls. DL-200 should evolve that existing
path into the dynamic form of the same ExtensionRegistry contract rather than create a
third extension system.

The conceptual host split is:

```text
ExtensionHost
├── Native       Rust Extension registered directly in-process
├── Process      any executable speaking the AW Extension protocol over stdio
└── Wasm         optional future embedded Wasm Component host
```

`ExtensionHost` is an implementation/runtime adapter only. All three feed the same
ExtensionRegistry and receive the same logical scoped `ctx.*` capabilities. There is no
Native API, JavaScript API and WASM API with different semantics.

#### V1: process extensions are the universal dynamic ABI

A dynamic Extension is simply an executable implementing the versioned AW Extension
protocol over stdio.

That executable can be:

```text
/absolute/path/to/node /absolute/path/extension.js
/absolute/path/to/bun /absolute/path/extension.ts
/absolute/path/to/python /absolute/path/extension.py
/absolute/path/extension-rust
/absolute/path/to/wasmtime run /absolute/path/extension.wasm
```

Therefore language/package ecosystems remain external to AW. AW does **not** need an npm,
pip, cargo or extension-marketplace package manager in v1. Users install/update an
extension with the ecosystem that owns it; AW is configured with the explicitly selected
executable/arguments and stable Extension ID.

The existing process ABI should be generalized rather than discarded:

```text
current aw-extensions v2
    describe
    tool.execute
    prompt/tool hooks
    bounded JSONL stdio

DL-200 process host
    describe Extension + registry contribution manifest
    invoke registered Actions/Views/Tools/Hooks/UI/etc.
    child -> host scoped ctx.* capability calls
    host -> child callbacks/invocations
    bounded full-duplex versioned stdio protocol
```

The minimal wire remains the current bounded one-JSON-message-per-line envelope. Do not
rewrite it to JSON-RPC merely for naming familiarity. Extend the existing
`v/id/method/params` + response shape symmetrically in both directions, with disjoint peer
request-ID spaces and explicit protocol errors. There is no need for HTTP, sockets, gRPC,
protobuf or another daemon just to load an Extension.

Keep the existing process safety scale in v1: maximum frame 256 KiB and manifest 128 KiB;
every manifest list/schema/result also has its role-specific native/protocol bound. A
single top-level callback permits at most 8 nested cross-extension callback levels and 256
nested `ctx.*` requests total under the same root deadline/cancellation scope. Exceeding a
bound fails explicitly and never partially grants new registrations/effects.

The protocol must be full duplex because an external Action/Hook/Handler may need
host-mediated runtime ports while it is executing:

```text
child callback
    -> ctx.state.get/append
    -> ctx.tools.call / inspect visible tools
    -> ctx.events.publish/read
    -> ctx.settings.get
    -> ctx.models.read/select only when granted
    -> ctx.process launch/control only when granted
    -> ctx.ui...
```

Keep concurrency minimal in v1: one top-level host->Extension callback is active per
process connection at a time. While that callback is active, the child may issue nested
ctx.* requests; unsolicited child requests outside an active callback are rejected. This
avoids building a general concurrent message broker merely to support extensions.

V1 also rejects **same-process re-entry**: while Extension process X is handling a host
callback, a nested `ctx.tools.call`/Action path that would synchronously dispatch another
callback back into X fails with a bounded reentrant/conflict error. Calls into other
Extensions/host Tools are allowed subject to normal authority. This prevents deadlock
without requiring multiplexed recursive callback execution.

Every top-level callback carries an opaque host-minted invocation scope. Nested ctx.*
requests must echo that scope; the host resolves caller/Agent/Turn/Extension/generation
and exact allowed ports from its own scope table rather than trusting identities supplied
by the child. The scope expires when the callback completes/cancels/times out or the
connection is poisoned.

Those are capability calls into AW, not ambient OS authority. Effectful ctx.* calls reuse
the existing AW receipt/uncertainty rules; if an effect may have happened but the process
connection is lost, AW/SDK must not automatically replay it.

The process manifest is a **bounded subset** of ExtensionRegistry definition registration.
Advertising a contribution never activates/grants it; the host independently selects the
allowed Tool/Hook/EventHandler/UI/etc. definitions for the relevant scope before callback
or delivery. Do not make the first dynamic ABI mirror every privileged native registry
merely because the native type exists.

V1 dynamic contributions are:

```text
Tools
Hooks (safe Agent Hook stages only)
Actions
Resources
Views
EventTypes
EventHandlers
semantic UI
Settings
State keys
```

Defer dynamic registration of:

```text
Providers / ProviderHooks
ProcessHooks
Compactors
SourceDrivers
SandboxDrivers
VaultProviders
TrustPolicy
```

until there is a concrete external-extension requirement and a reviewed authority/wire
contract. Native trusted Rust Extensions may still use those registries immediately.
This keeps the initial JS/TS ABI small without closing the protocol to later additions.

The host validates the entire advertised contribution set and publishes it atomically as
one ExtensionRegistry generation, exactly like native `Extension::register`.

#### JavaScript / TypeScript SDK

The first ecosystem SDK should be TypeScript/JavaScript because it gives the lowest
friction for third-party extensions.

Conceptually:

```ts
defineExtension({
  id: "histate",
  revision: "1",
  register(registry) {
    registry.actions.register(forkDecision);
    registry.views.register(summaryView);
    registry.eventTypes.register(decisionAccepted);
    registry.eventHandlers.register(decisionIndexer);
  }
});
```

Runtime callbacks receive the same logical shape:

```ts
ctx.tools
ctx.state
ctx.events
ctx.settings
ctx.models
ctx.process
ctx.ui
```

TypeScript generics/types provide author-time typing; runtime schemas validate the dynamic
wire. Plain JavaScript gets the same runtime validation without static TypeScript checks.
The SDK owns framing, request IDs, invocation-scope propagation, reconnect/poison rules,
schema encoding and protocol versioning so extension authors do not hand-write IPC.
Callback deadlines/cancellation are host-minted and role-specific; the current fixed
process timeout is only migration behavior, not the final universal contract.

One process connection represents exactly one host-selected stable Extension ID; the
child manifest must match that configured identity and cannot rename itself. If it exits
or becomes poisoned, its current executable definitions become unavailable, its durable
EventHandlers become `suspended`, and in-flight effectful calls are never blindly retried.
Host lifecycle policy may start a fresh process, but it must rerun `describe`, validate the
complete manifest/revision and publish a new ExtensionRegistry generation atomically
before new callbacks use it. Compatible handlers/state keys may then resume under their
normal schema/cursor rules. The manifest revision is compatibility/code metadata, not
proof of executable integrity.

Rust/Python/Go SDKs can implement the exact same protocol later without any AW-core
semantic changes.

#### Security and Sandbox

The current process extension implementation explicitly treats the selected child as
OS-user-trusted code. That is acceptable as the minimal compatibility/bootstrapping mode,
but it must remain explicit.

The target DL-200 model is:

- dynamic process Extensions run in a host-selected Sandbox environment with a fresh
  Process Scope by default once the lower Sandbox implementation is promoted; the Sandbox
  constrains environment/authority while Scope owns lifetime/Terminal semantics. Extensions
  do not become Agents or new Sandbox authority subjects merely because they are Extensions;
- a dedicated logical Sandbox is optional only when an Extension genuinely needs durable
  mutable source state;
- no host environment/credentials/filesystem/network are ambiently inherited;
- code/package/runtime files required to launch Node/Bun/Python/etc. are mounted/readable
  by host policy;
- extension capabilities are primarily the explicit `ctx.*` protocol ports;
- Vault/network/source access is independently granted/revalidated;
- an explicit trusted-host override may run a process with broader native OS authority.

This composes with, but does not promote, the frozen sandbox/mount implementation plan.

#### Why WASM is not the first implementation

WASM Components are attractive for a later host because they can provide a portable
single artifact, capability-oriented imports and strong runtime isolation. They are not
the minimum path to an accessible extension ecosystem:

- AW would need to add/embed and maintain a WASM runtime;
- AW would need to design/version a WIT component world in addition to the existing
  extension protocol;
- JavaScript/TypeScript authors would need a componentization toolchain instead of simply
  running Node/Bun/Deno;
- arbitrary npm/Node packages are not automatically WASI-compatible;
- the extension semantics would still need the exact same Registry and `ctx.*` capability
  model underneath.

Therefore **do not add Wasmtime to AW v1 solely for extension loading**.

A WASI/Wasm extension can already participate without an embedded runtime if the user
launches an external runtime as the selected process command and the component/program
speaks the AW Extension protocol over stdio:

```text
aw -> process host -> wasmtime run extension.wasm -> AW Extension protocol
```

That gives an immediate experimentation path with zero AW runtime dependency.

Later, if startup/isolation/distribution pressure justifies it, add an embedded
`WasmExtensionHost` behind the same ExtensionRegistry contract. WIT bindings may then be
generated from the stable dynamic extension capability surface; native/process Extensions
remain unchanged.

#### Minimal implementation order

```text
1. Generalize existing aw-extensions process ABI to the DL-200 Registry manifest.
2. Make the stdio protocol full duplex and expose bounded host ctx.* capability calls.
3. Add a small TypeScript SDK + one example Extension.
4. Support explicit executable/args configuration; no package manager/marketplace.
5. Run external Extensions in the promoted Sandbox by default when that layer exists.
6. Only then evaluate an embedded WasmExtensionHost if real demand remains.
```

This path gets JavaScript/TypeScript, Python, Rust and any other process-capable language
with one small host implementation, while preserving an easy future WASM path.

### 25.11 Process is one supervised execution scope

The reusable lesson from release/0.17.6 is broader than a better process-group guard:
**AW owns one Process execution scope, not one leader PID.** The main command is only one
member of that scope.

Keep the semantic surface deliberately small:

```rust
pub enum Occupancy {
    Empty,
    Live,
    Unknown,
}

pub struct ProcessOutputMeta {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_complete: bool,
    pub stderr_complete: bool,
}

pub struct ProcessTerminal {
    pub exit: ProcessExit,
    pub stop: Option<StopRecord>,
    pub occupancy: Occupancy,
    pub output: ProcessOutputMeta,
}
```

`Occupancy` is the immutable **pre-cleanup observation** of the semantic Process scope.
It is captured once after the main-command terminal/stop boundary is established and
**before cleanup changes the scope**, then carried unchanged into `ProcessTerminal`.
`StopRecord` separately records what semantic/housekeeping cleanup proved. Cleanup success
must never rewrite an earlier `Occupancy::Live` to `Empty`.

`Occupancy` describes the **semantic Process scope**, not merely whichever lower containment
primitive a backend happens to expose:

```text
Empty    AW proved no other live workload remained in the Process scope
Live     AW proved other live workload remained in the Process scope
Unknown  AW could not prove complete Process-scope membership/occupancy
```

Keep this meaning backend-independent. A weaker backend does **not** weaken `Empty` into
"the observed process group is empty." `aw-platform::DirectScope` exposes synchronous,
containment-local OS observations; `aw-process` is responsible for converting those facts
into semantic `Occupancy` using the backend's containment strength. Direct Unix
process-group/procfs observation can positively prove `Live`, but a negative group-local
scan maps to `Occupancy::Unknown` whenever an escaped descendant (for example via `setsid`)
cannot be excluded. That same narrower negative may still be used privately for safe
cleanup decisions without being promoted to semantic occupancy evidence. Strong
closed-containment backends such as a PID-namespace supervisor or Windows Job may normally
prove `Empty|Live`.

A successful kill never implies `Occupancy::Live`; occupancy and stop evidence remain
independent facts. The R5 **Rust source** still uses `Tree`; R6 renames the shared type and
terminal field at the Phase-0 cutover. Dotlink's `tree` JSON is a compatibility projection
only, never a second AW fact.

#### One private `Scope` owns lifetime

The dedicated **`aw-process`** crate owns one private `Scope` for each dispatch and
provides the single Agent-neutral async spawn, pipe lifetime, stop, wait, drain and
terminal implementation defined in section 0.2. AW and dotlink consume it rather than
keeping separate guard/wait/reap/pipe-drain state machines. `aw-platform` continues to
own synchronous OS facts. Policy, deadlines, output storage/retention, MCP projection
and operator logs stay with the embedding host.

The private Scope owns its one Process lifetime:

```text
Process
├── Scope            private lifetime owner
├── stdout/stderr
└── ProcessTerminal
```

`Scope` owns the main child plus the platform containment identity. Higher layers do not
own or manipulate a leader PID, process group, Job Object, pidfd, readiness primitive,
occupancy observer or cleanup guard independently.

The compiled direct-host backend is an implementation detail called `DirectScope`, not a
second semantic Process API:

```text
Linux direct     pidfd when available (unreaped child PID fallback) + process group + trustworthy procfs fallback observation
macOS direct     owned child identity + process group
Windows direct   duplicated exact process HANDLE + Job Object
```

`DirectScope` provides synchronous kernel facts/operations to the private async `Scope`.
There is no Tokio/runtime dependency in `aw-platform`, no public `child_exit_ready`, and no
higher-layer `disarm`/`release` operation. The platform scope owns its bounded final
termination attempt.

The async runtime owns waiting policy:

```text
wake hint / readiness
-> scope fact
-> observe
-> settle remaining scope
-> reap main command
-> settle output
-> ProcessTerminal
```

Wake mechanisms are not authority/truth. Linux may use SIGCHLD today and pidfd async
readiness later without changing Process semantics. **A wake never authorizes cleanup.** For the
owned main child, Linux direct settlement must confirm terminal state with a non-reaping parent
fact (`waitid(..., WEXITED | WNOHANG | WNOWAIT)`, preferably by the already-owned pidfd) before
observing/sweeping the process group. Pidfd poll/AsyncFd readability is only a wake hint; a
spurious readable edge must simply recheck the non-reaping child fact. The more general
`pid_identity` support for nonchildren may use pidfd readiness to say an identity is no longer
live, but that weaker observation must never be reused as the cleanup-authorizing main-child fact.
If the strict parent fact is unavailable/`ECHILD` where `DirectScope` expected its own child, the
settlement is uncertain; do not infer exit and sweep a possibly live command.

#### Sandbox and Scope compose; neither owns the other

The Process design must work identically with and without Sandbox isolation:

```text
                         Process
                            │
                 ┌──────────┴──────────┐
                 │                     │
            Environment             Lifetime
                 │                     │
              Sandbox                Scope
                 │                     │
      fs/network/credentials     direct / PID-1 /
       mounts/devices/etc.         Job / remote
                                         │
                                  main + descendants
                                         │
                                  ProcessTerminal
```

The important invariants are:

1. **Sandbox does not own Process lifecycle.** It may prepare an environment and provide an
   opaque launch capability; it does not reap children, author `ProcessTerminal`, or invent
   another stop/occupancy model.
2. **Scope does not own Sandbox state.** It owns only one dispatch lifetime and cannot mutate
   Sandbox bindings, Sources, Vault authority or durable Sandbox lineage.
3. **Sandboxing is optional for Process semantics.** Unsandboxed/direct execution remains a
   valid first-class path using `DirectScope`.
4. **Sandboxed does not imply PID namespace.** A filesystem/network-only Sandbox may still
   use `DirectScope`; a Sandbox/launcher with stronger containment may provide a stronger
   Scope backend.
5. **Scope backend selection is private.** Do not expose a model/user-facing
   `direct|namespace|job|remote` selector merely to describe implementation. The trusted
   launch plan carries the required containment guarantee and the host chooses an
   authorized backend that satisfies it. The ordinary default may use the best available
   backend, but when policy/grant requires **closed descendant containment**, lack/failure
   of a PID-namespace/Job/equivalent closed backend rejects launch before effects; it never
   silently downgrades to weaker `DirectScope` or retries through it.
6. **Containment strength is factual.** `Occupancy::Empty` always means the full semantic Process
   scope is proven empty. A weaker backend therefore maps inconclusive negative containment
   observations to `Unknown`; a strong closed-containment backend should usually yield
   `Empty|Live`. Never upgrade evidence because a Sandbox happens to exist.

Typical realizations:

```text
Linux unsandboxed/direct             -> DirectScope
Linux fs/network sandbox only        -> DirectScope
Linux Sandbox + PID namespace        -> PID-1 supervisor Scope
Windows sandboxed or unsandboxed     -> Job Object Scope
macOS sandboxed or unsandboxed       -> direct process-group Scope
remote/container/VM SandboxDriver    -> host-provided private Scope backend
```

Launch should establish environment and lifetime ownership **before the command can run**
where the platform permits:

```text
prepare authorized Sandbox environment
-> prepare/select Scope backend
-> launch main command into that Scope
-> publish Process ownership
```

Do not intentionally design a generic `spawn first -> attach containment later` contract.
Windows already uses suspended launch -> Job attach -> resume; the strong Linux supervisor
naturally launches the main command from inside the owned namespace. Direct Unix fallback
uses pre-exec process-group setup and captures stable leader identity immediately after
spawn because the platform does not offer the same atomic primitive.

Changing Sandbox technology must not change Process semantics, and improving Process
containment must not change Sandbox semantics.

#### Shared Process launch/settlement release-correction amendment — origin v0.17.7

Linux launch has one additional lifetime fact that is below Sandbox policy but above the raw
`fork/exec`: **the thread that forks a launcher can itself be part of the launch contract**.
Bubblewrap `--die-with-parent` is implemented with `PR_SET_PDEATHSIG`, and on Linux that parent is
the specific forking thread. A Tokio worker may retire while the host process is perfectly alive;
therefore directly calling `tokio::process::Command::spawn()` from an arbitrary runtime worker can
SIGKILL an otherwise healthy sandbox seconds later.

**SEALED Spawn Facility core (origin: dotlink v0.17.7, 2026-11-21).** R6 does **not** require one OS thread
per Process. The invariant is only that the thread which performs a parent-death-sensitive
`fork/exec` remains alive for at least as long as every child it parented. Agent/Job execution
continues to use ordinary `Send` futures and may migrate freely across Tokio workers before and
after launch.

R6 therefore requires one private logical **Linux Spawn Facility** inside `aw-process` with a
small fixed/add-only set of process-lifetime **Spawn Lanes**. A lane is an OS thread used only for
the short launch critical section; it never waits for, reads from, cancels, cleans up or otherwise
runs the Process after ownership transfer:

```text
many ordinary Tokio tasks / Jobs                    process lifetime
      │        │        │                                │
      └────────┴────────┴── bounded async admission ─────┤
                                                        ▼
                                             Linux Spawn Facility
                                          ┌─────────────┴─────────────┐
                                          │ fixed stable Spawn Lanes │
                                          │ N=1 initially            │
                                          └─────────────┬─────────────┘
                                                        │
                                      fork/spawn + exact DirectScope attach
                                                        │
                                                        ▼
                                                  Spawned RAII
                                           Child + DirectScope + pid
                                                        │
                                                        ▼
                                            ordinary private Process Scope
                                                        │
                               wait / stop / observe / reap / drain on Tokio
```

The complexity target is explicit:

```text
stable Spawn Lane OS threads = O(1)      (initially exactly 1)
running Processes             = O(n)
Tokio Job/Process tasks        = O(n), lightweight and scheduler-migratable
```

A long-running command occupies a Spawn Lane only for spawn/custody establishment, **not** for the
command lifetime. There is no affinity between a live Process and the Tokio worker that submitted
it, and no requirement that later stdin/wait/cancel/output operations execute on its Spawn Lane.
`Process` must remain `Send`; worker stealing/migration is part of the intended high-concurrency
server design.

The shared spawn entry is asynchronous. Hosts already have an async `PipeProcess::spawn` boundary;
`aw-process` should expose/consume an async spawn handoff rather than blocking a Tokio worker with
`block_in_place`, `spawn_blocking` or a synchronous rendezvous. The facility owns **bounded async
admission/backpressure** and transfers the already-authorized `tokio::process::Command` by value.
Each accepted request carries the originating Tokio `Handle` only so the stable lane can enter that
runtime for `Command::spawn` registration; the lane is not a runtime worker and does no unrelated
async work.

The ownership transfer is cancellation-safe, deadline-safe and effect-safe. The staged
dotlink **v0.17.9 (`576d0a1`)** adds an important refinement: a launch may become stale while
waiting behind another Spawn Lane request, so cancellation/expiry must be rechecked on the lane
**immediately before fork**, not only before queue admission.

Use one private linearizable launch gate per request. Conceptually:

```text
LaunchState = Pending | Cancelled | TimedOut | Launching
```

The async spawn future and the Spawn Lane race only on this private state. The caller supplies the
same semantic cancellation signal and one **absolute host-selected execution deadline** used for the
subsequent Process lifetime. No AgentCore type crosses into `aw-process`: the async side can drive the
private atomic state from any cancellation future, while the lane sees only that tiny state plus the
absolute deadline.

```text
caller owns prepared Command
        │
        ▼
runner establishes absolute execution deadline
        │
        ▼
bounded async Spawn Facility admission
        │
        ▼
request waits in lane queue as Pending
        │
        ├── cancellation wins Pending -> Cancelled
        ├── deadline wins Pending -> TimedOut
        │
        ▼
Spawn Lane immediately before fork:
        compare deadline + CAS Pending -> Launching
        │
        ├── already Cancelled -> no spawn, CancelledBeforeSpawn
        ├── already TimedOut / deadline expired -> no spawn, TimedOutBeforeSpawn
        └── Launching wins -> spawn exactly once
              -> obtain child id
              -> establish exact DirectScope / stable kernel identity
              -> build private Spawned { Child, DirectScope, pid }
              -> reply
                    ├── receiver accepts -> Process takes ownership
                    └── receiver disappeared -> Spawned RAII cleans scope/child
```

A skipped stale launch created **no Process**. It therefore produces no synthetic
`ProcessTerminal`, no `StopRecord`, no "killed" evidence and no fake signal/occupancy fact. The
host Job may settle as cancelled/timed-out from its own pre-spawn admission fact, but it must not
claim a Process was stopped.

Once `Pending -> Launching` wins, cancellation/deadline can no longer retroactively claim "never
started". A race after that point may create a real child; the complete RAII package is retained and
immediately enters the normal Process stop/settlement path (or is cleaned on failed reply transfer),
so any actual Process has factual Process semantics. The command is never implicitly retried.
`kill_on_drop` remains a final child fail-safe; `DirectScope` remains the containment cleanup owner.

The absolute deadline is also the one used after spawn; Spawn-Lane queueing after the Job enters
Running must **consume** that execution budget rather than silently granting a fresh full timeout
when `Process::wait` begins. Prefer an absolute-deadline shared API (`wait_until`/equivalent) or
explicitly derive remaining duration from that same deadline exactly once. Do not create separate
spawn and run timeout clocks.

This does **not** mean all Job age consumes execution timeout. Product/job queueing and preparation
before the runner enters Running may still count toward observational `elapsed_ms` while not
consuming `timeout_secs`. The owner that transitions to Running establishes the execution deadline;
from that point, Spawn Facility queueing + actual execution share one deadline.

A request handler panic must not kill the lifetime anchor. Lane request processing should be
contained with a panic boundary: partially-created RAII state unwinds/cleans, the **facility is
poisoned/fails closed for new launches**, but the Spawn Lane thread itself stays alive/parked so
existing `PDEATHSIG` children are not killed merely because launch code panicked. Likewise, an
established lane/facility failure or a lost reply after an effectful submission is
`SetupUncertain`/restart-required; it is never repaired by silently respawning the lane and
retrying the command. Failure to create a lane before any request is accepted may be retried
because no external effect occurred.

Spawn Lanes are **process-lifetime anchors**. They may never be dynamically retired, shrunk or
recycled while the host process lives: doing so can deliver `PDEATHSIG` to every qualifying child
that lane parented. Future implementations may use a fixed `N>1` or add lanes monotonically, but
must never shrink the set. Lane count is an internal performance property, not a user/model/API
configuration knob.

Initial implementation is intentionally **N=1**. One lane serializes only process creation and
custody establishment, which is normally tiny relative to Process lifetime and avoids needless
fork/exec contention and permanent threads. Do not build a spawn-pool scheduler pre-emptively. If
real server benchmarks show admission queueing is material, `N=2` or `N=4` may be adopted without
changing `Process`, `Scope`, `ProcessTerminal`, dotlink, Sandbox or any public API. The focused
benchmark matrix is concurrency `1/8/32/128` against lanes `1/2/4`, measuring only spawn throughput,
queue-wait p50/p99, spawn-handshake p50/p99 and CPU. Stop at `N=1` unless those measurements justify
more lanes.

`spawn_blocking` and `block_in_place` are explicitly insufficient: their pool threads are still
runtime-managed and may retire, which is exactly the lifetime property this amendment removes. A
separate launcher **process** is also deliberately rejected for this fallback path: making another
process the kernel parent would move wait/reap/pipe/pidfd ownership across IPC/SCM_RIGHTS and
recreate much of the future strong PID-1 supervisor. That stronger backend remains separately
promoted and should reuse its own clear parent/terminal protocol rather than complicating direct
spawn merely to fix thread lifetime.

The Linux Spawn Facility is deliberately **not** a public service, Job, Sandbox, Actor, scheduler,
runtime owner, `SpawnBroker` trait or new execution framework. It is a private implementation
mechanic of the already-required `aw-process` owner. `aw-platform` remains synchronous and
Tokio-free; it owns launch flags/kernel facts and constructs/attaches `DirectScope`. The facility
makes any future host-prepared launcher carrying `PDEATHSIG` safe without teaching `aw-process`
Bubblewrap/product semantics.

Whole-process death still ends every Spawn Lane and therefore preserves launcher parent-death
protection; Tokio worker retirement does not. This amendment does **not** promote the frozen 017
Sandbox/mount design and does not make the Spawn Facility the strong Linux containment backend.
The future PID-1 supervisor remains separately promoted. PTY custody remains `aw-terminal` and is
not forced through this non-PTY Process launch path.

On Windows, the corresponding noninteractive Process launch primitive is
`CREATE_SUSPENDED | CREATE_NO_WINDOW`, followed by Job Object assignment and primary-thread
resume. `DETACHED_PROCESS` is not an acceptable substitute: PowerShell can successfully exit
without executing the requested command. `CREATE_NO_WINDOW` is a launch-detail of the existing
non-PTY Process backend; PTY/interactive terminal launch remains separate.

A post-v0.17.8 release-branch correction (`81ef891`) adds one more **shared Occupancy** rule.
`CREATE_NO_WINDOW` can transiently keep console/job infrastructure inside the Job Object after the
main process has become terminal, and Windows accounting may clear the exited leader slightly after
`wait` returns. An immediate `ActiveProcesses != 0` query can therefore falsely publish
`Occupancy::Live` for a clean command. Do not fix this with process-name/image exemptions.

After the exact main-command terminal fact is established, Windows pre-cleanup occupancy uses a
bounded **settling observation** over the owned Job Object:

```text
query active Job members
    0                         -> Occupancy::Empty immediately
    >0 before settle bound    -> wait briefly, query again
    >0 for full settle bound  -> Occupancy::Live
    query failure             -> Occupancy::Unknown
```

The initial reference bound is two seconds with a short (~10 ms) poll cadence, matching the
release evidence. v0.17.9 (`576d0a1`) sharpens this further: this is a bounded
**natural-exit observation grace**, not extra authority to delay a cancellation or execution
deadline. The settle future must remain inside the same outer cancellation/deadline selection as
natural Process settlement.

If cancellation or the execution deadline fires while a nonzero Job count is still settling:

```text
Empty was already proven      -> preserve Occupancy::Empty
nonzero/error was inconclusive -> Occupancy::Unknown
then sweep/stop immediately
```

Do not wait out the remaining two seconds before honoring the stop. A real leftover is
`Occupancy::Live` only when nonzero membership survives the **entire uninterrupted settle window**.
Likewise, a nonblocking/`try_wait` path that sees the main command terminal may return Empty on an
immediate zero, but an immediate nonzero must remain Unknown unless the full settle proof is
performed; transient infrastructure is not yet evidence of semantic leftovers. The main command's
already-established physical exit is preserved regardless of whether the settle is interrupted.

Keep the ownership split clean: `aw-platform-win::DirectScope` exposes the cheap synchronous exact
Job-accounting query/kernel handles; `aw-process` owns the async bounded settle loop and the outer
cancel/deadline race, converting only completed evidence to semantic `Occupancy`. Never sleep for
the settle window directly on a Tokio worker, and do not add Windows process-name knowledge to
shared Process semantics. Cleanup follows either the completed stabilized observation or an
interrupt immediately; it never rewrites the pre-cleanup fact afterward.

#### Strong Linux backend: tiny PID-1 supervisor

The preferred strong Linux realization is a per-command AW-owned supervisor inside a
per-command PID namespace:

```text
host AW
└── Process scope / PID namespace
    ├── PID 1  tiny AW supervisor
    ├── PID 2  main command (for shell Tools, Bash)
    └── PID N  descendants
```

The supervisor is an implementation detail, not an Agent, Job, daemon or public resource.
It only owns Process lifetime: spawn the main command, reap children, retain the main
command's physical exit, observe whether other workload remained, stop/reap the rest, and
report the bounded terminal facts over one private bounded **unidirectional supervisor→host**
channel independent of user stdin/stdout/stderr. The channel is not a general RPC protocol.
Only the supervisor may own a writable report endpoint: before `exec` of the main command,
the launcher/supervisor closes/removes that writer from the command and all descendant-
inheritable fd tables (CLOEXEC plus explicit close/dup discipline as appropriate). The host
owns only the read endpoint. User code must never be able to forge ProcessTerminal facts.
The host validates bounded/versioned framing, exactly one terminal record for the launch,
and the expected supervisor/launch identity established by the launcher/owned channel before
accepting terminal facts; malformed, duplicated or unexpected reports are treated as report
failure, not command facts.

If the supervisor exits, crashes, or its report channel breaks before AW receives the main
command's physical terminal fact, the supervisor's own exit must **never** masquerade as the
command exit: `ProcessExit::Unknown` is the factual result. `Occupancy` remains any pre-cleanup
occupancy already reported; if none was established, it is `Unknown`. Later namespace
destruction may prove final containment quiescence/cleanup, but it never rewrites the
historical pre-cleanup occupancy or reconstructs the lost command exit.

This backend is stronger and simpler because daemonization/double-fork/`setsid` cannot
escape the PID namespace; orphaned descendants remain owned by namespace PID 1. Supervisor
failure also fails closed because terminating namespace PID 1 terminates remaining
namespace processes.

Do **not** force Bash itself to be PID 1.

Because AW is a reusable Rust library, do **not** implement the PID-1 supervisor by running
arbitrary Rust after `fork`, assuming a host utility exists, or teaching every embedding
binary a hidden mode. The strong backend is instantiated by a real host/Sandbox launcher
that can provide the tiny supervisor executable/entrypoint safely. The currently frozen
017 mount/Bubblewrap plan is not implicitly promoted by this design.

Until that launcher is promoted, direct execution uses the private `DirectScope` fallback.
The fallback may be weaker (for example a Unix descendant may escape a process group with
`setsid`), so `Occupancy::Unknown` and capability truth remain meaningful. Replacing
`DirectScope` with the PID-namespace supervisor must require **no** change to
`ProcessTerminal`, Jobs or dotlink's semantic API.

Windows Job Objects already provide the strong containment shape without requiring a
userspace PID-1 process. macOS remains a weaker direct supervisor/process-group backend.

#### One terminal truth

AW `ProcessReceipt` stores `ProcessTerminal` directly and may additionally carry its
AW-specific output snapshot/spool access. `Outcome`, aggregate output completeness, stop
helpers and Process status are derived projections rather than independently authored facts.
Dotlink derives its product completion from the same `ProcessTerminal`; no cross-host
`SharedProcessReceipt` is introduced.

`ProcessSettlement` may still carry an independent Job-level `Status` when the Job result
itself failed outside the Process (for example exact result retention failed). That is not
a duplicate Process truth.

`StopRecord` / `StopReason` / `StopEvidence` / `StopTracker` remain the one stop model. Do
not add Cleanup/TreeStop/TargetGone/LeaderState types.

A v0.17.7 correction sharpens what that model means: a `StopRecord` is factual evidence about a
semantic/housekeeping **stop operation against the containment scope**. Even
`StopEvidence::Stopped` does not, by itself, prove that stop caused the main command's physical
`ProcessExit`. The two facts remain independent until the Process owner reconciles them. An exit
status already established before a later stop remains authoritative; likewise a physical ending
incompatible with the runtime's delivered termination cannot be relabelled as timeout/cancel/
runtime-error merely because a group/Job stop also reached some live member. Cleanup of leftovers
never rewrites the main ending.

Do not add another stored cause field merely to express this. Causal `stopped_by`/operator wording
is a **derived projection over the whole `ProcessTerminal`**, not `StopRecord::terminal_reason()`
read in isolation. The shared implementation should retire/avoid any helper whose name implies
that `StopRecord` alone proves the main command's terminal cause. A signal with no reconciled
shared stop is **unattributed**; it is not proof of an "external sender" because kernel mechanisms
such as `PDEATHSIG` can generate the signal in the parent's kernel context without a userspace
`kill()` caller.

Host-specific cancellation origin remains outside the shared Process fact. If a product needs to
distinguish caller vs connector/shutdown, the host records the **first requester before firing the
cancellation token** and later projects that origin only when the shared terminal facts support a
cancel stop. A later requester cannot overwrite it. `aw-process` continues to receive only the
semantic `StopReason::Cancelled`.

The Process invariant is:

```text
Process is terminal only after
    main command terminal fact is retained
    + remaining execution scope is settled as far as the backend can prove
    + output producers are settled/bounded
```

No model-facing Jobs tool and no DL-200 host method is added by this section.

#### Dotlink is a thin consumer of the same Process owner

Dotlink reuses the same shared Process lifetime/settlement owner that produces
`ProcessTerminal`; it does **not** retain a second guard/wait/reap/cleanup-retry runtime and
merely copy the resulting fields. If the current crate boundary is too AW-host-specific,
move/factor that owner downward to the smallest sensible shared library boundary rather
than duplicating it.

Dotlink may retain shell/product concerns that are genuinely its own: launch/configuration
projection, output transport, MCP/result shape, operator lifecycle wording and bounded-turn
timeout policy.

AW does **not** gain `tree="leftovers"`, `success`, signal-name JSON, lifecycle prose or a
turn-horizon API. Dotlink retains the **wire** name `tree` for compatibility, derived only from the shared
`ProcessTerminal.occupancy` fact. AW never stores a second tree fact:

```text
Occupancy::Empty    -> omit tree
Occupancy::Live     -> tree="leftovers"
Occupancy::Unknown  -> tree="unknown"

stdout_drain_incomplete = !output.stdout_complete
stderr_drain_incomplete = !output.stderr_complete
```

Dotlink may make `Occupancy::Live` clear its product `success` flag without rewriting the
physical exit code or turning the MCP call into a generic tool error. Its
`tree="leftovers"` projection means **live workload existed at the immutable pre-cleanup
settlement boundary**; it does not claim that workload remains alive after cleanup.
`StopRecord`/terminal completeness carry the cleanup evidence. Signal names remain a
dotlink edge projection using the existing AW signal-name helper. One concise operator
lifecycle line is likewise dotlink presentation, not shared process truth.

Qualification is behavioral, not merely serialized-field parity: AW and dotlink must share
exit/stop/occupancy/output semantics for clean/nonzero/signal exits, timeout/cancel, remaining
children, inherited-pipe output and direct-Unix escaped-descendant uncertainty. The post-v0.17.7
Process gate additionally proves, without fixture proliferation: (1) force retirement of the Tokio
worker that requested a Linux spawn and verify a `PDEATHSIG` child remains alive under the shared
Spawn Facility while the `Process: Send` future continues on another worker; (2) process death still terminates a parent-death launcher; (3) an artificial/spurious
pidfd wake cannot make a live child cleanup-eligible; (4) a previously established exit code or
incompatible signal is not overwritten by a racing stop; (5) Windows noninteractive managed launch
actually executes under `CREATE_NO_WINDOW` while preserving suspended Job attachment; and (6) a
clean Windows command whose transient console/job infrastructure briefly remains does **not** become
`Occupancy::Live`, while a real leftover that remains through the bounded settle window does.
Dotlink should reuse its v0.17.7 Bubblewrap/process-lifetime and post-v0.17.8 Windows occupancy tests
as real consumer gates rather than duplicating a second local launcher/observation implementation.

#### Dotlink bounded-turn surface policy — v0.17.8 convergence

The audited dotlink **v0.17.8** release (`60e04d3`) refines the bounded conversational policy and
remains deliberately **dotlink-owned** rather than AW Process/Job truth:

```text
turn horizon             = 35 minutes
hand-back/report reserve = 10 minutes
new-command ceiling      = 25 minutes = horizon - reserve
checkpoint mode begins   = minute 25
```

A bounded turn is useful only if the result still has time to be reported. A single command may
therefore never consume the whole 35-minute horizon. On the bounded surface, effective new-command
timeout is:

```text
min(configured shell timeout, 25 minutes)
```

The final 10 minutes belong to landing/parking current work, leaving the tree/report state coherent,
reading retained output and handing the turn back. `background=true` changes observation/handoff,
not the execution deadline already assigned to the Job. An uncapped/local-only runtime keeps the
configured Process timeout and emits no turn/checkpoint advisory.

This policy composes with AW through **facts and projection**, not a generic callback framework.
The canonical split is:

```text
aw-tools::JobManager / aw-process
    lifecycle, revision, ProcessTerminal, retained output, ACK/release truth
                         │
                         │ typed facts
                         ▼
dotlink Job host projection
    local monotonic created-at + immutable timeout_secs
    + current RuntimeSurfacePolicy
    + transport/product wording
                         │
                         ▼
    server instructions / shell ceiling / job_get.next / operator warning
```

`aw-tools::JobManager` remains clock-free and product-policy-free. It must not learn a turn horizon,
hand-back reserve, OpenAI tunnel identity or prose such as "checkpoint what it produced". The
explicit Process deadline still reaches `aw-process` as the host-selected absolute deadline/
equivalent remaining budget; AW enforces it without knowing why the host chose it. Dotlink may
retain narrow local monotonic **timing projection** because that is host runtime observation/policy,
not a second Job lifecycle or Process outcome owner:

```text
created: Instant                  // Job age / elapsed_ms, includes product queueing
execution_deadline: Option<Instant> // set once when the runner enters Running
immutable timeout_secs            // execution budget selected for this Job
```

Do not derive `execution_deadline = created + timeout_secs`: queue/preparation time before Running
is observational age but is not execution budget.

Dotlink should converge on one small local **`RuntimeSurfacePolicy`** (name descriptive, not a
public API) derived once from `ActiveTransports.constraints()`. Conceptually it owns only:

```text
sync_hold: Option<Duration>       // can this runtime hand a call back mid-work?
turn_horizon: Option<Duration>    // 35m only when bounded
handback_reserve: Duration        // 10m when bounded
effective_command_ceiling(profile_timeout)
turn_advice_enabled()
```

The active transport set contributes capabilities, not deployment names. `sync_hold.is_some()` is
the relevant fact: a runtime that can hand a durable handle back mid-work has a conversational turn
to checkpoint into. If multiple transports share one `LocalMachine`, the tightest active transport
constraint wins for the shared schema/execution environment. A stdio/HTTP-only runtime remains
uncapped unless one of its active transports itself declares such a bound.

The **effective command ceiling is execution authority**. It (or the transport constraint from which
it is deterministically derived) participates in dotlink's prepared execution-authority fingerprint
for **fresh effects**. Changing the active bounded-surface constraint may therefore change fresh
command admission. It must **not** make a previously consumed request identity executable again:
exact retained Ready/Unknown replay remains factual/no-rerun under the existing replay owner, and
only current disclosure/presentation is re-evaluated. By contrast, whether the current surface
renders a near-deadline advisory is **presentation state**, not replay identity: bind/rebind it on
every `LocalMachine` construction so a surviving/reused Job/session never inherits stale advice
after transport changes. An already-running Job retains the `timeout_secs` it was admitted with;
current presentation never rewrites it.

The v0.17.8 advisory plus v0.17.9 correction is also dotlink projection only. It is computed
from current surface capability + the **runner's actual execution deadline** + immutable
`timeout_secs` + canonical shared lifecycle. Queue age must never be subtracted from the execution
budget:

- a Queued Job has no execution deadline and never warns, even if `elapsed_ms` is large;
- when the runner enters Running, publish/store the exact absolute execution deadline before launch;
- only a known Running, non-cancelling Job with that deadline can warn;
- compute remaining time as `execution_deadline - now`, not `timeout_secs - elapsed_ms`;
- warn inside `min(timeout_secs / 5, 300s)` of the actual execution deadline;
- do not warn for a one-second/no-actionable-margin cap or once the deadline is already reached;
- wording should name the **execution deadline** so it cannot be confused with Job age/turn age;
- prepend the deadline/checkpoint sentence to the ordinary `jobs_get.next` follow-up;
- do **not** repeat it on `jobs_list`, `jobs_read`, `jobs_ack` or cancellation receipts;
- terminal, cancelling, revoked or journal-Unknown state suppresses the predictive advisory.

No rendered shared receipt is parsed to implement this. Dotlink reads canonical typed Job facts
(`runtime_snapshot`/equivalent), combines them with its narrow local timing projection
(`created`/`execution_deadline`/`timeout_secs`) and surface policy, and renders its own product
metadata. If the later DL-200 cutover uses the shared AW `jobs_get` adapter so directly
that composition is blocked, factor only the smallest **typed factual projection seam** needed by
the first real consumer: shared code produces the Job status facts/default rendering, and dotlink
may add bounded host-owned metadata such as a `next` prefix before final serialization. Do not add
a generic renderer plugin, arbitrary string callback, Job-policy trait, clock to `aw-tools`, or a
parallel dotlink lifecycle state machine.

Dotlink's typed AW receipt profile should provide the factual timestamp needed to anchor the turn
(`unix_ts=true` on the relevant Process/Jobs receipts), but **must not simultaneously emit a second
generic checkpoint schedule**. For this bounded dotlink runtime use `checkpoint_minutes=None` and
let `RuntimeSurfacePolicy` own the exact 25/35/10 guidance in one place. Other AW embeddings may
continue using generic `ReceiptProfile.checkpoint_minutes` when that simpler advisory is appropriate.

One dotlink-owned policy source must derive all product projections that need these numbers:

1. server/model instructions (35m horizon, checkpoint mode from 25m, 10m reporting reserve);
2. effective shell timeout/schema/runtime validation (max 1500s on the bounded surface);
3. startup warning when operator configuration is narrowed;
4. current-surface `jobs_get.next` near-deadline warning;
5. execution-authority fingerprint input for new effects.

This prevents 25/35/10 from drifting across descriptions, runtime enforcement and Job projection,
while preserving AW's reusable clock-free semantics.

Direct and Bubblewrap dotlink Bash invocation should converge with AW on non-login,
non-profile execution (`--noprofile --norc -c`).

No model-facing Jobs tool and no DL-200 host method is added by this section.

---

## 26. Shared Events API

DL-200 has one Event concept and one NATS-style subject-routing model for system Events,
Agent-published Events, host observation and durable Agent subscriptions.

Do not create separate signal, topic, Agent, Principal/Group, Project, input-plan or Job
notification systems.

The substrate projects authoritative committed transitions; it is not a second source of
truth.

### 26.1 Event subject, envelope and protocol bounds

Every Event has one concrete subject from section 3.3:

```text
<exact resource prefix>.e:<event>
```

Examples:

```text
d:acme.e:changed
d:acme.i:compiler-team.e:changed
d:acme.i:alice.p:core.e:changed
d:acme.i:alice.p:core.a:2.e:#ready
d:acme.i:alice.p:core.a:2.t:7.e:settled
d:acme.i:alice.p:core.a:2.t:7.j:test.e:result
```

Conceptually:

```text
Event {
    id           // opaque stable immutable Event identity
    subject      // canonical concrete Event subject
    event_rev    // source-local monotonic Event revision
    data?        // bounded JSON object
    actor?       // immutable emitter/transition provenance
    turn?        // emitting Turn when applicable
    causal_root? // opaque root id for causally triggered work
    causal_hop   // Event-generation hop within that root; 0 for fresh root
    schema?      // optional registered EventType key + schema version; typing, not auth
    disclosure_manifest? // internal immutable manifest ref/digest; not a routing dimension
}
```

The source resource and Event name are parsed from `subject`; they are not second
routing dimensions. `disclosure_manifest` is internal authorization/provenance metadata and is not exposed as
another subject/grant namespace. The commit-log Event envelope stores only the bounded
manifest ref/digest, not an arbitrarily growing dependency array. There is only one Event
concept.

V1 shared protocol bounds are normative:

```text
concrete subject / pattern    <= 2048 UTF-8 bytes, canonical ASCII tokens after handle encoding
custom event token            #[a-z][a-z0-9_-]{0,62}   // 2..64 bytes total
custom data                   JSON object or omitted, canonical encoding <= 4096 bytes
custom data nesting           <= 8
events.read limit             1..256
events.read wait_ms           0..30000
active subscriptions/Agent    <= 128
model custom publishes/Turn   <= 32
Event causal hop depth        <= 16
EventInput deliveries/root    <= 256
```

System Event names are always **bare** final values (`changed`, `result`, `settled`,
`attention`, and any future bare name). Agent/host-published custom Events are always
prefixed with `#`: `#ready`, `#tests_passed`, `#review_requested`,
`#phase_complete`.

This structural split removes the reserved-name registry entirely:

```text
e:changed   -> system-owned
e:#ready    -> custom Agent Event
```

Agents can publish only `#...` names and therefore cannot forge present or future system
Events. `#` is an AW literal custom-event marker, not an MQTT-style wildcard. There is
no partial-token `#*` matching; `e:*` matches any one Event token.

`event_rev` is not the same thing as owner `rev`.

- `rev` versions current mutable owner state.
- `event_rev` orders factual Events emitted by one concrete source.
- `Event.id` identifies one exact immutable Event envelope.
- an opaque stream `cursor` records a reader's private scan position.

`Event.id` is server-minted, opaque and stable across replay. It is unique per Event
envelope even when one transaction emits several names at the same `event_rev`; it is
not sortable and reveals no private commit-log position.

One source transaction that emits multiple factual names may assign them the same
`event_rev`, preserving the current SourceEventBook behavior. No one compares Event
revisions across unrelated sources.

### 26.2 `events.read` — inotify-style subject read

Host/SDK API:

```text
events.read(
    subject,
    after   = "now" | <opaque cursor>,
    limit?,
    wait_ms?
)
```

`subject` is an exact Event subject or NATS-style Event pattern from section 3.3.

`after="now"` atomically captures the current durable Event-log position and returns a
cursor whose future scan begins after that position. The response contains only Events
authorized for the caller; the internal position is never exposed.

A read cursor is a random opaque store record bound to:

```text
authenticated principal
normalized subject/pattern hash
StoreId + RestoreGeneration  // existing StoreScope
selector_placement_generation
internal Event scan position
created / last-used time
```

`StoreId` is the stable identity of the authoritative ordered `commit_log`/Event
index; it is paired with existing `restore_generation` because backup restoration
preserves StoreId but replaces log history. An old-incarnation cursor is stale, not
replayed against a reused position. The placement token remains an independent
**authority-scoped** selector-affinity check. `selector_placement_generation` is an **authority-
scoped store-affinity token**, not a global topology generation: the existing Domain/
placement owner derives it from the caller's currently authorized selector scope plus the
bound StoreId. Hidden/unauthorized Domains/resources are excluded and must not change
caller-observable cursor validity merely because they are created, moved or deleted. The
token changes only when the caller's currently authorized selector scope can no longer be
proven wholly served by the bound store (for example the caller gains authority to a
matching Domain on another store, or an authorized Domain migrates). This is private
cursor metadata, not a federation API or second Event owner.

It is not bound to a frozen ACL snapshot. Authorization order is normative and privacy-
preserving:

1. resolve the cheap/current **exact source visibility gate first**;
2. a proven source denial/nonmatch advances privately and **must not evaluate deeper
   DisclosureManifest/provider dependencies**;
3. only after the caller is permitted to observe that concrete source does AW evaluate the
   Event's current audience plus immutable DisclosureManifest authority roots/dependencies;
4. store-local authority is read under the current store snapshot/transactional owner;
   externally mutable authority must supply either immutable/non-revocable authorization
   evidence or a provider-enforced fence held valid through the authorization/read decision.
   If neither is available, the result is indeterminate/`authorization_unavailable`.

Thus every visible Event is authorized against **current** resource authority and its
immutable DisclosureManifest closure, while hidden sources cannot trigger deeper provider
work or observable transient errors merely by existing. A pattern/cursor never bypasses
protected payload provenance. Therefore:

- losing access hides unread matching Events immediately;
- gaining access does not retroactively reveal rows the cursor already advanced past;
- wildcard patterns grant no discovery/access and dynamically include future resources;
- hidden/nonmatching rows advance only the private scan position and reveal no counts,
  sequence numbers or gaps.

One call scans at most 4096 underlying Event-index rows to produce up to the requested
256 visible Events. Reaching that scan bound after only proven nonmatch/deny rows may return
an empty page with a new opaque cursor; it does not disclose how many hidden rows were
crossed.

If the next candidate whose **source is already visible** cannot be authoritatively
classified because group/security/DisclosureManifest/provider/placement authority is
transiently unavailable or indeterminate, return the single retryable stream-level result/
error **`authorization_unavailable`** and do **not** advance past that Event. The result is
candidate-oblivious: it includes no Event ID, subject, source ref, position, hidden-row count,
dependency identity or indication of which exact row caused the stall. The supplied opaque
cursor remains valid/unchanged; when the call started with `after="now"`, the captured cursor
is returned/retained as the retry point. This condition must never masquerade as an empty
page or a proven denial.

Read cursors expire after 24 hours of inactivity and are bounded to 64 live cursors per
principal after opportunistic cleanup. The logical public Event retention floor keeps
**all Events from the last 7 days and at least the most recent 65,536 Event rows**,
whichever is more permissive. The immutable commit log may physically retain data
longer; that does not promise indefinite public replay.

If a cursor falls behind the logical retention floor, return `cursor_expired`. The
caller rereads authoritative resource snapshots and resumes from `after="now"`.

With `wait_ms`, the store may wait on the relevant authority/dependency wake plus the normal
Event wake/deadline. If the blocking visible-source candidate is still indeterminate at the
deadline, return `authorization_unavailable` with unchanged progress. This intentional head-
of-line blocking preserves ordered no-loss semantics.

The privacy guarantee is deliberately precise rather than absolute: AW directly exposes no
hidden Event identity, subject, source, exact count or private log position. Because reads
are bounded to finite scans/waits, callers may still infer **coarse aggregate timing/traffic
volume** from latency, empty-page frequency or number of calls. V1 does not add per-user
Event indexes, padding or constant-time scanning absent a stronger reviewed threat model.

Events remain factual invalidation/coordination hints. Callers reread authoritative
mutable resource state before acting.

Examples:

```text
subject=d:acme.i:alice.p:core.a:2.e:changed
data={aspect:"inputs", inputs_rev:12}

subject=d:acme.i:alice.p:core.a:2.e:changed
data={aspect:"state", rev:84}

subject=d:acme.i:compiler-team.e:changed
data={aspect:"members", rev:5}
```

Cross-Project, cross-Principal and **same-Event-store** cross-Domain observation uses the
same grammar; only the concrete resource authorizer changes. V1 does not merge multiple
independent `commit_log` positions into one cursor. A requested cross-store selector is
explicitly unsupported/fails closed rather than silently returning an incomplete page.

Every cursor use revalidates its StoreId + authority-scoped selector placement
generation both before scanning **and before committing/returning cursor progress**. A
concurrent scope/placement change therefore cannot advance the cursor through a page built
under stale completeness assumptions: discard that page/progress and return `cursor_stale`.
"Future matching resources" means future resources in the host's **authoritatively known
Domain placement universe that are currently authorized for this caller** and remain wholly
served by the bound Event store. Hidden Domains never perturb the token or reveal topology.
If the caller later gains authority that makes the supported selector cross-store, or an
authorized matching Domain moves stores, return `cursor_stale`; never ignore the newly
authorized store or fabricate a merged position. For a broad selector such as `d:*`, v1
never claims global exhaustiveness beyond the host's authoritative placement universe; if
the placement owner cannot prove complete store placement for a Domain the caller is
actually authorized to include, admission/progress fails closed rather than returning an
apparently complete partial world.

### 26.3 Durable `events.subscribe`

A durable subscription asks that future matching Events be delivered into an Agent's
authoritative input owner. Creating/enabling/changing one requires recipient Agent
lifecycle `open` plus current `control` or `manage` authority (or the equivalent exact
model tool grant). Disabling/removing remains allowed during close for idempotent cleanup.
The committed subscription retains the factual creating principal/tool-authority
provenance used for later disclosure revalidation.

**V1 Agent delivery is transactionally store-local.** The subscription/EventStore/causal
budget owner and the recipient Agent's authoritative Input owner must participate in the
same AW store transaction. Subscription creation/re-enable validates that internal store
affinity. Cross-Project/Principal/Domain delivery remains supported when both endpoints are
served by that same store; cross-store Agent delivery is explicitly unsupported/fails
closed rather than weakening the single EventInput admission transaction. A future
cross-store delivery protocol is separate design work, not an implicit saga hidden in v1.

Host form:

```text
events.subscribe(
    subject,
    to=<Agent ref>,
    enabled=true
)
```

`enabled` has exact idempotent semantics; no reset/seek/update verb is needed:

```text
current row                         request                 result
active, same normalized subject     enabled=true            unchanged; preserve generation + baseline + scan
absent/disabled                     enabled=true            create fresh generation + baseline at current Event tail
lagged                              enabled=true            explicit rebaseline: fresh generation + fresh tail baseline
active or lagged                    enabled=false           disable/remove and fence later admissions
absent/disabled                     enabled=false           unchanged
```

A different normalized subject is a different subscription key, not an in-place edit.
Repeated `enabled=true` on an active row must never reset the baseline or discard backlog.
Replacement/removal/rebaseline is generation-fenced internally.

Durable subscription state is inspectable without adding get/edit/delete micro-verbs:

```text
events.subscriptions.list(
    to=<Agent ref>,
    limit?,
    cursor?
)
```

Inspection requires current `control`/`manage` authority on the recipient Agent (or the
exact host subscription-management capability); Agent `view` alone does not disclose
subscription patterns that may name otherwise-private resources. Rows expose bounded
subject/generation/state/creator metadata but never private scan/baseline positions.
`events.subscribe(..., enabled=false)` remains the idempotent removal operation.

A wildcard subscription is one durable pattern row; it is **never** expanded into rules
on every matching source. Its current state is conceptually:

```text
EventSubscription {
    recipient Agent
    normalized pattern
    StoreId + RestoreGeneration
    selector_placement_generation
    generation
    baseline_position
    scan_position
    created_by / authority_ref
    recipient_authority_evidence
    state = active | lagged
}
```

The committed baseline is the durable Event-log tail in the same transaction that
creates/enables the subscription, so delivery is future-only with no subscribe race.
Before every scan and again at admission/progress commit, the subscription revalidates its
StoreId + **authority-scoped** selector placement generation under the creating-
principal/tool authority retained by the row. Hidden Domains that authority cannot observe
must not change the subscription's externally visible state. If the currently authorized
selector moves stores or becomes cross-store, stop delivery and retention pinning, mark the
existing row `lagged` with an internal stale-store reason, and require explicit
re-enable/rebaseline. Never silently scan only the old store or merge positions.

No queue/steer/interrupt/wake mode is part of the public subscription. A matched Event
becomes one typed immutable `EventInput` in the recipient's existing Agent input owner;
the host scheduler decides when/how that input is admitted.

Backlog is **not copied per subscriber**. The shared immutable Event log plus each
subscription's scan position is the backlog. For each candidate, scanning may perform optimistic/preliminary checks, but the **only
admission linearization point** is the existing replay-safe Agent input-owner transaction.
That transaction receives the deterministic delivery identity plus expected subscription
identity/generation/store-affinity evidence and atomically:

1. confirms the subscription is still the same active generation and store-affinity scope;
2. revalidates the recipient Agent is still open and the creating-principal/tool authority
   still permits delivery;
3. resolves the exact **source visibility gate first**; proven source denial returns a
   terminal skip without touching deeper manifest/provider dependencies;
4. for a visible source, reauthorizes the current recipient audience against every immutable
   Event DisclosureManifest authority root/dependency, including any source `share`
   requirement created by the current audience. Store-local authority is checked inside the
   transaction. Any externally mutable dependency must provide immutable/non-revocable
   authorization evidence or a provider-enforced fence held valid through **EventInput
   transaction commit**. If neither is available, admission is `authorization_unavailable`/
   Unknown and neither EventInput nor scan progress is committed;
5. deduplicates the deterministic delivery identity
   `hash(subscription, generation, Event-log identity)`;
6. charges the causal delivery budget only for a newly accepted input;
7. appends the immutable `EventInput` with source + transitive disclosure provenance and
   current recipient disclosure principals; and
8. returns the terminal accepted/deduplicated/proven-denied/authorization-unavailable result
   used to advance or hold the subscription scan.

Unsubscribe, Agent close, authority revocation or incompatible audience change committed
**before** this transaction denies new admission. An EventInput committed first remains a
factual existing input and is never retroactively rewritten; later audience broadening is
still subject to the retained disclosure dependencies before a new viewer can see it. This
is ordinary transaction-winner semantics, not a second queue/notification subsystem.

A crash after accepted input but before scan-position persistence safely replays the same
deterministic delivery and deduplicates it. There is no second pending-message database.

Only **proven** nonmatching/unauthorized concrete Events advance the private subscription
scan just as they do for `events.read`. Authorization/disclosure evaluation is internally
three-way even though no public enum is required:

```text
allow      -> candidate may be returned/admitted
 deny       -> proven hidden/unauthorized; advance private scan without disclosure
 unknown    -> authority/dependency/store unavailable or indeterminate; DO NOT advance
```

A transient source-authorizer, group/security, disclosure-dependency or placement lookup
failure is `unknown`, not denial. `events.read` returns the retryable
`authorization_unavailable` result above without moving its cursor beyond that candidate. A
durable subscription or Handler leaves its scan position at that candidate and retries with
bounded backoff/wake-driven scheduling rather than hot-looping. This creates intentional
head-of-line blocking for that ordered consumer. If retention eventually overtakes the
blocked position, the existing `lagged` semantics take over; the Event is never silently
skipped as though authorization were denied. Later authority restoration after a **proven denial** does
not retroactively disclose the skipped interval. Recipient ACL broadening is separately
checked against every retained EventInput/content disclosure dependency before a new viewer
is admitted.

A subscription whose recipient Agent enters `closing` is generation-fenced and removed/
tombstoned equivalently to `enabled=false` by Agent lifecycle ownership; live subscription
rows therefore remain only `active | lagged`. It no longer scans or pins backlog. A subscription that otherwise
falls behind the Event retention floor becomes `lagged`; it does not silently skip. The
recipient gets one bounded host-visible `attention` fact, and
the subscription must be explicitly re-enabled to establish a fresh future baseline.

Self-delivery is suppressed in v1 whenever concrete source Agent == recipient Agent,
including matches through broad wildcards. Host `events.read` can still observe those
Events.

Wildcard patterns do not create a side channel: hidden sources are skipped by exact
authorization and do not reveal hidden counts/positions.

### 26.4 Model-facing `events_subscribe`

```text
events_subscribe(
    subject,
    enabled=true
)
```

The recipient is implicitly the current Agent. The model cannot name another recipient
or choose delivery/scheduling mode.

`enabled=false` unsubscribes that normalized subject pattern.

Relative patterns resolve against the current Agent. Explicit cross-Project/Principal/
Domain patterns are accepted only when the model's current host grant permits that
Event scope; the wildcard itself never supplies authority. Every eventual concrete
match is still reauthorized at delivery time.

Examples:

```text
events_subscribe(subject="a:2.e:#ready")

events_subscribe(
    subject="p:core.a:*.e:#phase_complete"
)

events_subscribe(
    subject="a:2.>"
)
```

This supersedes `jobs_subscribe`; Event subscription is not intrinsically a Job
operation.

### 26.5 Event publication: `events.publish` / `events_publish`

DL-200 deliberately does **not** add topic creation, topic deletion or a separate signal
system. Publishing an Event is enough.

Host form:

```text
events.publish(
    ref,
    event,
    data?
)
```

`ref` must resolve to one exact **open** Agent for caller-published custom Events. The
caller must hold current `control` authority (or an equivalent exact host capability) on
that Agent because publication acts as that Agent and can trigger durable reactions.
`closing/closed` Agents emit only authoritative system lifecycle Events; they cannot be
used as custom Event speakers. The host cannot use this API to publish a bare/system Event or publish as a wildcard,
Domain, Principal, Project, Turn or Job source.

Model form:

```text
events_publish(
    event,
    data?
)
```

The source is implicitly the current Agent. The runtime constructs the exact subject:

```text
<current Agent subject prefix>.e:<event>
```

A model cannot publish as another Agent. Its `event` argument must be a valid `#...`
custom token; bare/system names are rejected structurally. The tool exists only when the
current Turn carries the exact host `events_publish` capability;
ordinary Agent `view/submit` authority does not manufacture it.

Publication commits:

```text
id      = <stable opaque Event identity>
subject = <current Agent prefix>.e:<bounded #custom event token>
data    = <small bounded JSON metadata>
actor / turn / tool-call provenance
disclosure_manifest = <host-derived immutable dependency-manifest ref/digest>
event_rev++
```

The underlying dependency closure/manifest is never caller-selected to be weaker. A model/Extension publication
inherits the current invocation/Turn disclosure dependencies that could have influenced
its payload. A system Event containing only owner-state metadata normally has only its
source-resource policy; if an owner emits protected content/reference metadata it attaches
the corresponding dependencies. A trusted host custom publication uses the source
resource authorizer/current host provenance to derive the allowed disclosure ceiling; it
cannot mark arbitrary protected bytes as public merely by omitting metadata.

There is no create/delete topic lifecycle. The first publication simply uses that Event
subject.

Custom publication obeys the section 26.1 grammar and 4096-byte canonical JSON-object
limit. Model publication is additionally capped at 32 successful custom Event
publications per Turn. System Events are instead bounded by the authoritative state
transitions that create them; they are not charged as model custom publications.

Use custom Events for coordination/facts:

```text
#ready
#tests_passed
#review_requested
#candidate_found
#phase_complete
```

Do **not** use Events as a bulk chat/file transport. Directed substantive input remains
`jobs_send`; large outputs remain in Jobs/results and can be referenced from Event
metadata.

The distinction is:

```text
jobs_send        = directed input/message
events_publish   = source-scoped Event publication
events_subscribe = decoupled Event reaction/fanout
events.read      = host/SDK observation and catch-up
```

Delivery enters the recipient Agent's typed EventInput path with original subject/emitter
and causal provenance. It is not rewritten as a user-authored chat message.

Every EventInput chain carries a root causal ID and hop/delivery counters. The root is
host-minted only for a genuinely fresh causal publication. AW-mediated work descended
from an EventInput automatically retains that root:

- a Turn admitted from EventInput carries the root in its causal execution scope;
- Jobs/Tools/Actions/EventHandlers invoked under that scope retain it;
- any custom/system Event emitted from that causal work inherits the root rather than
  minting a new one;
- fanout deliveries from one Event share the same Event-hop depth while each accepted
  EventInput increments the root delivery count;
- when a descendant causes another Event generation, the Event hop advances exactly once.

Causal budget accounting is durable/replay-safe, not an in-memory counter. A small current
projection may be:

```text
event_causal_roots_current
    root_id PRIMARY KEY
    max_event_hop
    accepted_event_input_count
    state = active | exhausted
    updated_at
```

The deterministic delivery admission transaction checks/deduplicates the EventInput ID and
increments the root count **only for a newly accepted delivery** in the same authoritative
transaction. A replayed delivery therefore cannot double-charge, and a crash/restart
cannot reset the budget. Event publication under an existing causal scope records the
inherited root/hop in the immutable Event envelope before any subscribers can observe it.
The current row is a safety projection, not Event truth, and may be rebuilt from retained
causal Event/EventInput facts; cleanup happens only after no active/retained causal work can
reference the root.

V1 stops delivery after 16 Event generations/hops or 256 total EventInput deliveries from
one causal root. The exhausted chain records one host-visible attention fact; that fact is
non-deliverable as a new causal Event and cannot reset/escape the exhausted budget. There
is no model/Extension API to request a fresh causal root from inside an existing causal
scope. This bounds cross-Agent/Handler cycles without adding a scheduler concept to Events.

Cross-resource delivery is a disclosure path, but DL-200 does **not** add a second
`EventLink` grant service. The exact source resource authorizer, recipient Agent
authority and existing Domain/federation policy are sufficient:

- Agent/Turn/Job/Node Event -> current source Agent `view` authority;
- Sandbox Event -> current source Sandbox visibility through its Project/Agent binding
  authority;
- Principal/Project/Domain Event -> that resource's existing metadata/domain authority;
- delivery -> current recipient `submit` authority plus subscription ownership **and**
  recipient-audience disclosure closure; broadening source content to recipient viewers
  requires source `share`/equivalent disclosure authority;
- cross-Domain -> explicit current federation/Event-read authority in addition to both
  endpoint checks.

If inherited history/content is actually transferred, ordinary `derive/share`
HistoryGrant rules still apply. A small factual Event by itself does not mint history
disclosure.

### 26.6 One shared durable implementation

The existing immutable `commit_log` is the durable ordering/history owner for DL-200
Events **within one authoritative Event store**, including multiple authorized Domains
resident in that store. Do not create another Event database or pretend independent remote
stores share one ordered scan position. Cross-store federation is deferred, not implicit. The section 26.2 Event replay floor is a
logical Event-read/consumer availability contract; it is **not** authorization to prune
non-Event commit/audit/recovery history that another store owner still retains. Event
projection/index compaction and core commit-log retention remain separate concerns.

Generalize the current Job Event code by **splitting its responsibilities**:

```text
EventSourceState
    exact concrete source
    next source-local event_rev
    delivery_cutover_rev?      // first revision whose delivery owner is DL-200

commit_log
    immutable factual Event envelope + global private scan position
    // existing event_id is Event.id; canonical subject/envelope lives in bounded payload

event_projection_index
    rebuildable parsed subject columns keyed by commit_log position

event_projection_state
    StoreId + RestoreGeneration       // existing StoreScope for indexed history
    current index generation
    indexed_through_commit_position   // contiguous, gap-free safe prefix

event_subscriptions_current
    recipient-owned normalized patterns + StoreScope + selector placement generation
    + subscription generation + scan position

event_read_cursors_current
    opaque reader/pattern + StoreScope + selector placement generation
    -> private scan position
```

The single private **EventScan** mechanism under `events.read`, Agent subscriptions and
EventHandlers performs bounded ordered projection scans, checks `StoreId + StorePos`
affinity/placement and contiguous indexed-through progress, then checks source visibility
**before** deeper disclosure dependencies. It yields one of: preliminary authorized
candidate, proven skip, blocked/unknown, stale or end. It owns neither Handler execution
nor EventInput admission, output projection, acknowledgment or cursor-progress commit;
those remain in the distinct consumers and their existing owners. Every candidate must be
revalidated at the consumer's final publication/disclosure transaction, including held
external authority evidence. A blocked public response remains candidate-oblivious. Reuse
one opaque cursor-envelope validator for Event readers, subscription/Handler positions and
Jobs pages; retain their distinct scan algorithms and disposition rules. This is an
internal pure/transaction-helper boundary, not a new Event worker or queue.

The shared EventScan and all Event reader/subscription/Handler positions check
`StoreScope` before advancing. On authoritative same-Store backup restore, old
incarnation cursors fail stale; durable consumers reconcile or become lagged until
explicit safe rebaseline. An index from the restored image may be validated/rebuilt
from its restored authoritative commit log, but cannot silently claim coverage across
the incarnation change. No new store ID/epoch or replay log is introduced.

`event_projection_index` is an index/projection only; deleting/rebuilding it never
deletes Event truth. Its correctness contract is nevertheless **gap-free progress**:
readers/subscriptions/Handlers may never advance a durable scan position beyond
`event_projection_state.indexed_through_commit_position`. That watermark advances only
across a contiguous commit-log prefix for which every DL-200 Event row has its deterministic
projection entry (non-Event commit rows are explicitly classified/skipped). If an Event row
is missing at/below the watermark, treat the projection as corrupt/unavailable and do not
advance any consumer.

For newly authored DL-200 Events, write the Event envelope and its projection row in the
same authoritative store transaction whenever practical; advance the contiguous watermark
only when no preceding gap exists. Recovery/backfill scans `commit_log` in order and moves
the same watermark forward only after each prefix is complete. During a rebuild, consumers
whose desired position is beyond the safe watermark wait/fail temporarily **without
advancing or fabricating an empty page**. A rebuild may use a fresh internal index generation
and atomically publish its safe prefix; it is not a second Event log/service.

Reuse the existing `commit_log.event_id` as stable `Event.id`. Do not stuff a potentially
2048-byte canonical resource ref/subject into legacy `commit_log.entity_id` (currently a
compact internal identifier with a smaller bound): a DL-200 Event row stores its bounded
canonical subject/data/provenance envelope in `payload_json`, while
`kind/entity_type/entity_id` remain compact internal classification/index metadata. Existing
historical commit rows are not rewritten; rows that have no DL-200 Event envelope are
classified as non-Events and skipped by the indexer without creating a gap.

Source lifecycle fields such as current Agent settled/result state remain with their
resource owner, not in the generic Event service.

An authoritative transition increments its source Event revision and writes the Event
envelope to `commit_log` in the **same store transaction** as the state transition.
That preserves the current rule that Turn `result/settled` facts appear only after
authoritative attachment.

Ephemeral `tokio::watch`/notify channels may wake local readers/deliverers, but they are
never durable Event truth.

### 26.7 Migration from the v36 Job-only Event substrate

The current `agent_job_event_sources` rows mix three concerns: source Event revision,
exact-source subscriptions, and copied pending deliveries. Migrate them without running
two delivery owners for the same Event revision.

Cutover is serialized by the **existing source Event owner**. Inside that source-owner
cutover transaction set, exactly:

```text
delivery_cutover_rev = current next_event_rev
```

before releasing the source owner. `delivery_cutover_rev` is therefore the first source-
local Event revision whose delivery belongs exclusively to DL-200. Because one source
transaction may emit several Event names at the same `event_rev`, the **entire revision** is
owned by one side; individual Event names within a revision are never split across legacy
and DL-200 delivery:

```text
event_rev <  delivery_cutover_rev  -> legacy delivery owner only
 event_rev >= delivery_cutover_rev  -> DL-200 Event delivery owner only
```

The same cutover transaction reads the current `next_event_rev`, records that exact value as
`delivery_cutover_rev`, installs/translates the DL-200 subscriptions/baseline needed for
that revision and later, and fences the legacy admission path from accepting the cutover
revision or later **before releasing source-owner serialization**. Event envelope projection
may be introduced earlier for compatibility/inspection, but delivery ownership is determined
only by this source-owner cutover revision, so envelope dual-writing cannot double-admit an
Event.

Migration then:

1. classify legacy pending/accepted deliveries only for revisions **before** the cutover;
2. accepted legacy Agent Inputs keep their original factual identity and are **never
   re-admitted or re-minted**. Migration attaches one immutable compatibility disclosure/
   provenance sidecar keyed by that existing input/fact identity. Derive the narrowest exact
   source Event/resource dependency, already-disclosed audience and policy evidence available
   from the legacy delivery. If complete reconstruction is impossible, attach a conservative
   `legacy_protected`/source-root authority dependency whose ceiling preserves the already-
   valid historical audience but causes any future audience expansion/fork/share check to
   fail closed unless current source authority can explicitly authorize it. Missing legacy
   provenance is never interpreted as unrestricted/declassified content;
3. each genuinely pending, not-yet-accepted legacy delivery is converted at most once using
   a deterministic bridge EventInput identity derived from the legacy delivery identity;
   retain its old queue/steer/interrupt mode only as a one-time host scheduling hint/
   provenance, never as a public Event field;
4. post-cutover source revisions are delivered only through DL-200 subscriptions;
5. once all pre-cutover compatibility deliveries are acknowledged/drained and restart proof
   is green, stop writing the old subscription/pending JSON and contract it later.

The compatibility sidecar is migration provenance owned with the existing AgentInput/history
fact, not a second Input or grant service. Historical readers within the already-valid
legacy audience continue to see the factual Input; future audience expansion is governed by
the reconstructed/conservative DisclosureManifest ceiling.

Crash/restart simply rereads `delivery_cutover_rev` and the legacy delivery's factual
accepted/pending state; it never guesses which path owns a revision. There is never a second
semantic Event bus or a migration service.

---

## 27. Consolidated model-facing Jobs API

The final model Jobs surface is six tools.

### 27.1 `jobs_list`

```text
jobs_list(
    ref    = ".",
    scope  = "children" | "tree" | "turns",
    limit?,
    cursor?
)
```

- `children`: natural direct work children.
- `tree`: recursive natural work tree.
- `turns`: Agent Turn timeline.
- rows contain `ref kind state rev` plus bounded kind-specific facts;
- every returned row/subtree is exact-authorized per section 5.4; lineage never grants
  visibility;
- inherited Turn rows may additionally carry stable `history_fact_id` + authorized
  original provenance per sections 5.3/19.8;
- no initial `kind` or `state` query filter.

### 27.2 `jobs_get`

```text
jobs_get(
    ref     = ".",
    rev?,
    wait_ms?,
    tail?
)
```

Compact status/observation.

`rev + wait_ms` means "I already observed this owner revision; wait efficiently for
it to change, then reauthorize and reread." Use the authoritative Agent/Process/Graph
owner's notification and a **register-then-recheck** revision sequence; never busy-poll
Session/branch listings every 10 ms. Canonical Agent refs resolve through indexed exact
Agent ownership. `jobs_list` uses indexed, cursor-bounded ordered exposure reads with
per-row current authority, not full per-principal Session/branch scans and sorting.

### 27.3 `jobs_read`

```text
jobs_read(
    ref      = ".",
    view?,
    filter?,
    cursor?,
    offset?,
    limit?,
    encoding?,
    wait_ms?
)
```

Detailed/paged result, transcript or stream read according to target kind.

### 27.4 `jobs_send`

```text
jobs_send(
    ref   = ".",
    input = { text } | { eof:true }
)
```

- Agent -> ordinary new Agent input; host owns queue/scheduling.
- Process -> stdin text/bytes or EOF.
- Turn/Graph/Node -> reject.

No `queue/steer/interrupt/pending` mode.

### 27.5 `jobs_ack`

```text
jobs_ack(
    ref,
    view?,
    offset?
)
```

Explicit target required. Releases only proven delivered retained output according to
the existing D5/D6 ACK rules.

### 27.6 `jobs_cancel`

```text
jobs_cancel(ref)
```

Explicit exact target required.

Ordinary model-facing cancel does not require a generic revision CAS; exact identity,
current owner state and replay/idempotence own correctness.

---

## 28. Consolidated Host Agent API

Keep the host Agent surface compact:

```text
agent.list
agent.get
agent.create
agent.edit
agent.close
agent.admission
agent.access.list
agent.access.set
agent.vault.list
agent.vault.set
```

plus the five `agent.inputs.*` operations from section 15.

The destination Project ref already contains `/d:<domain>/i:<principal>/p:<project>`, so
Agent construction does not take a second owner identity. The Project's `i:`
principal is the canonical Agent owner namespace.

### 28.1 `agent.list`

```text
agent.list(
    project = <Project ref; default current Project>,
    limit?,
    cursor?
)
```

Lists authorized Agents in the selected/current Project with bounded status. Cross-Project
enumeration is composed from authorized `project.list` + project-scoped `agent.list`
rather than a hidden global Agent query.

### 28.2 `agent.get`

```text
agent.get(ref=".", include?)
```

Returns:

```text
ref
rev
owner principal
Project
caller effective-permission/access summary
access_generation
lifecycle
admission state
active Turn
parent Agent?          // delegation/creator lineage; ref only when caller may view parent
Sandbox ref
immutable history head / stable history fact identity
input summary
work summary
```

Agent changes are observed through the shared Events API; `agent.get` remains a pure
authoritative snapshot read. Its ordinary access summary is caller-relative and does not
enumerate other ACL principals/group membership. The ordinary snapshot does not expose lazy/internal
`HistoryBaseId`; callers use the stable immutable history head/fact identity, while
checkpoint/library surfaces may expose an already-sealed typed HistoryBaseRef where
needed. `parent Agent` is an authorized projection: if the caller may
view the child but not the parent, the parent ref is omitted/redacted and `^` remains
disallowed rather than leaking a private Agent identity.

### 28.3 Host `agent.create`

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

`from` omitted creates fresh history. `from` present creates an immutable-history fork
under section 22. Host access defaults to private; the Project's `i:` principal is the
canonical owner. Exact observed-history selection uses a settled Turn ref rather than a
mutable source `rev`.

The Sandbox is deliberately independent from history ancestry. An explicit Sandbox ref
shares that logical mutable environment and must resolve under the exact destination
Project in v1; `auto` resolves through trusted host/delegation policy. To request a particular isolated branch/fresh environment, create a Sandbox
first.

### 28.4 `agent.edit`

```text
agent.edit(
    ref,
    title?,
    rev
)
```

This is the bounded Agent-owned presentation/configuration edit surface in v1 and requires
current Agent `manage` authority. A successful edit advances Agent `rev` exactly once and emits `changed` with an exact
data aspect. It does not edit history, ACLs, Project/owner, parent lineage, Sandbox
binding/state, Vault grants or model-visible content. Additional fields should be added here only when they are truly Agent-owned
metadata rather than creating new one-off verbs.

### 28.5 Model-facing `agent`

```text
agent(
    task,
    title?,
    background=false,
    from? = <Agent ref | settled Turn ref>
)
```

No `from` -> fresh Agent according to trusted host/delegation destination-Project,
access and Sandbox policy.

`from` -> exact permitted immutable-history fork.

The model never chooses destination Project/owner, Agent ACL, Sandbox, source authority,
Vault grants or cache domain.

### 28.6 Agent access

#### `agent.access.list`

```text
agent.access.list(ref=".")
```

Requires current Agent `manage` authority (or stronger owner/admin authority); ordinary
`view` does not reveal the collaborator roster. Returns owner principal, direct/group ACL
entries, effective audience summary, Agent
`rev`, Agent-owned `acl_rev` and the caller's opaque derived `access_generation`.
The last value may change when referenced group/security state changes even when Agent
`rev` does not.

#### `agent.access.set`

```text
agent.access.set(
    ref,
    principal,
    permissions=[...] | null,
    rev
)
```

Permission array creates/replaces that principal's complete grant; `null` removes it.
The caller requires current `manage`; any audience expansion additionally passes the
section 19.5 disclosure closure and current `share`/source-equivalent authority where
needed. Narrowing/removal requires no `share` grant.

The canonical owner (`i:`) is not changed through ACL edits.

### 28.7 `agent.close`

```text
agent.close(
    ref,
    rev?
)
```

Close requires current Agent `control` authority (or stronger owner/admin authority). It
is an idempotent terminal **execution/admission** lifecycle, never delete/purge.
The observable lifecycle is `open | closing | closed`.

On the first successful close request AW atomically:

- changes admission to held and refuses new runtime input custody/Turns;
- retires/disables Event subscriptions whose recipient is this Agent so they stop scanning
  or pinning future Event backlog;
- requests cancellation/settlement of the Agent's active Turn and directly owned finite
  work through the existing factual cancellation/uncertainty owners;
- advances Agent `rev` for the lifecycle transition.

The Agent becomes `closed` only after owned active work reaches factual terminal state;
that terminal transition advances `rev` again when distinct. A close call never fabricates
success for an uncertain external effect. Child Agents are separate globally addressable
lifecycles and are not implicitly deleted by parent close.

Closed Agents remain readable according to ACL, keep immutable history/provenance and
remain history retention roots in v1. Title/access/checkpoint/history management may still
operate when separately authorized because close is not archive/delete. The staged draft
snapshot remains inspectable but frozen; new input/subscriptions/custom Events/new Vault
attachments are rejected. Repeated close on `closing/closed` is idempotent. `rev` is an optional
"only if unchanged since observed" fence.

### 28.8 `agent.admission`

One operation replaces hold/resume micro-verbs:

```text
agent.admission(
    ref,
    state = "open" | "held",
    rev?
)
```

Changing admission requires current Agent `manage` authority. `held` lets the current Turn
settle and prevents future Turn admission while inputs may remain staged/queued. `agent.admission` is valid only while lifecycle is `open`; a
`closing/closed` Agent cannot be reopened through admission.

### 28.9 Agent input plan

Exactly five operations:

```text
agent.inputs.list(ref=".")
agent.inputs.add(ref=".", items=[...], rev?)
agent.inputs.edit(ref=".", id, text?, intent?, before?|after?, rev)
agent.inputs.delete(ref=".", ids=[...], rev)
agent.inputs.send(ref=".", ids?, rev)
```

Intent kinds:

```text
followup(after?)
steer(turn)
interrupt(turn)
```

`edit` includes reorder.

Omitted `ids` on `send` means all staged items. Permissions are those from section 15.7:
`view` for list; `contribute` for permitted draft add/edit/delete; `submit` for followup
Send; `control` for steer/interrupt staging/Send; `manage` for other users' drafts and
admission management. A transaction containing mixed intents requires the union of their
permissions and fails atomically if any selected item is unauthorized.

### 28.10 Agent Vault bindings

`agent.vault.list` requires current Agent `manage` plus Vault-binding metadata authority.
It always returns bounded logical binding name/projection/status metadata; provider IDs/
entry refs are returned only when the caller has the corresponding VaultProvider metadata
authority. It never returns secret bytes.

`agent.vault.set` attaches/replaces one named VaultGrant from `binding={...}` only while
the Agent lifecycle is `open`, requiring Agent `manage` plus exact Vault attach/grant
authority. `binding=null` remains allowed while closing/closed for idempotent capability
cleanup. It never accepts secret bytes directly and never treats Sandbox sharing,
history ancestry or Agent ACL inheritance as Vault authority.

The required `rev` is the observed Agent revision. The successful binding mutation
advances Agent `rev` exactly once. The VaultProvider revalidates the selected
entry/projection when a Process actually launches.

### 28.11 Legacy observer/queue/process/pause/control convergence

DL-200 performs a semantic cutover, not a synonym expansion. The final model registry
does not expose compatibility aliases.

```text
legacy model surface                     DL-200 owner/surface
──────────────────────────────────────   ─────────────────────────────────────────
jobs_subscribe                           events_subscribe
Agent/queue/Process observer tools       jobs_list / jobs_get / jobs_read + Events
queue_enqueue/update/delete              host agent.inputs.add/edit/delete/send
agent_pause_after_turn                   host agent.admission(state="held")
Process stdin                            jobs_send(text=...)
Process close_stdin                      jobs_send(eof=true)
Process terminate                        jobs_cancel
arbitrary Process signal/control         trusted host Process control only
model steer/interrupt queue verbs        host agent.inputs steer/interrupt
Signal type/API                          Event type/API
Session public identity/API              Agent public identity/API
```

The lower safety facts are retained, not weakened:

- existing ActorQueue private-payload custody, claim/ACK/finalization and uncertainty
  handling remain the runtime input owner below `agent.inputs.send`;
- existing ActorControl Prepared/Applied/Rejected/Unknown facts remain available where a
  real non-idempotent control boundary still needs them;
- Process stdin/EOF/cancel preserve the exact process-owner CAS/no-retry semantics;
- arbitrary OS signals do not get smuggled into `jobs_send`;
- public receipts use canonical Agent/Job refs while legacy Session IDs may remain
  internal audit locators.

A language SDK may temporarily offer **client-side source migration helpers** that call
the canonical DL-200 operations, but those helpers are not protocol methods, are not
registered as model tools and own no state. There is one wire/runtime semantic surface.

Checkpoint operations are a separate top-level `checkpoint.*` host namespace.

---

## 29. Fork/cache/access examples

### 29.1 Shared seed Turn -> cheap specialists

```text
Agent a:0
  t:1 common initialization
          │
          ▼
      HistoryBase H1
      PrefixContentKey P1
          │
          ├── agent.create(from=".", access="inherit") -> t:2 security
          ├── agent.create(from=".", access="private") -> t:2 performance
          └── agent.create(from=".", access=[...])      -> t:2 API review
```

All exact forks preserve the identity/content/provenance of t:1 and may share the same
local KV prefix where the backend supports it.

### 29.2 Team group

```text
principal compiler-team (kind=group)
    Alice
    Bob
    Carol

Agent owner = i:alice
Agent access:
    i:compiler-team -> [view, submit, control]
```

Adding/removing a group member changes effective Agent access without duplicating or
rewriting the Agent ACL.

For genuine team co-ownership:

```text
Agent owner = i:compiler-team  // principal kind = group
```

Every active group member then has Agent-owner authority.

### 29.3 Fine-grained exception

```text
i:engineering -> [view, submit]             // group principal
i:alice       -> [view, control, manage]    // user principal
```

Alice receives the allow-only union. No special one-person group is needed.

### 29.4 Independent custom fork

```text
source Agent:
    /d:acme/i:alice/p:core/a:2
    access:
      Bob view

agent.create(from=source Agent):
    project=/d:acme/i:carol/p:core
    access:
      Dave  view
      Ellen view + submit + control
```

The destination security envelope is independent while exact inherited history and
original Alice/Bob/Agent attribution remain unchanged.

### 29.5 Checkpoint restore

```text
checkpoint cp:17
├── history H
└── SourceSet
    ├── code -> Git anchor G1
    └── data -> object-manifest anchor G2

checkpoint.restore(cp:17, access="private")
```

creates a new private Agent from H and a new Sandbox whose named sources are restored
from G1/G2. It does not rewind the original Agent, Sandbox, Git worktree or object
source.

---

## 30. Normative invariant index

This section is deliberately an **index, not a second statement of the rules**. A contract is
defined once in its owning section; if any summary, example, historical handoff or ADR differs,
the owning section wins.

- **Architecture law / locality / shared mechanics:** sections 0.1–0.3.
- **Jobs identity, traversal, paging and control:** sections 2–10 and consolidated section 27.
- **Owner revisions, staged input and replay/publication:** sections 12, 15–17.
- **Checkpoint, SourceDriver and reproducible source state:** section 18.
- **History/fork/disclosure/cache/provenance:** sections 13 and 19–23.
- **Principals, groups and Agent access:** section 24.
- **Agent envelope / Session compatibility / Sandbox / Vault / Extensions / Process:** sections
  25.1–25.11.
- **Events, EventScan consumers, delivery, retention and migration:** section 26.
- **Consolidated model/host APIs and legacy convergence:** sections 27–28.
- **Examples:** section 29 is explanatory only; it creates no new contract.

The non-collapsible distinctions in section 0.3 are the review checklist for attempted
simplifications. Do not duplicate the full invariant set here.

---

## 31. Decision history

R1–R5 design-resolution history is preserved intact in
[DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md](../../docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md).
It is historical evidence, not a second normative specification.

R6's proposed simplification is defined only by the current normative sections—especially
0.1–0.3—and the phased implementation plan. Keep future rationale/alternatives in dated
handoffs or ADR-style decision records rather than restating normative contracts here.

---

## 32. Seal

DL-200 **R6 + the shared Process/release-correction amendment are SEALED / IMPLEMENTATION PROMOTED — PHASE-0 CORRECTIVE IMPLEMENTATION GATE PENDING**.

- R1 original seal: explicit user approval on **2026-11-20** after the full pre-seal audit.
- R2-R4 amendment/reseals: 2026-11-21 prior Process/Event/replay/registry/construction/Jobs/
  DisclosureManifest/store-recovery reviews.
- R5 amendment/reseal: explicit user direction on **2026-11-21** to resolve the latest
  external Event authority, supervisor-report, legacy-input-provenance, driver-startup,
  Event-privacy/cardinality/descriptor and exact-cutover review cleanly in the plan.
  **R5 is authoritative.**

**R6 seal (2026-11-21):** explicitly approved after the final live-owner review.
Sections 0.1–0.3 consolidate the internal architecture into one-owner/store-local
consistency, six descriptive roles, one `StorePos`, replay/Prepared and external-evidence
rules, projection publication, opaque cursor validation, EventScan, durable DriverKey,
typed registry staging and one required shared `aw-process` runtime. Section 25.11 renames
`Tree` → `Occupancy` at the source cutover. The final review additionally fixed the
implementation boundary: `ProcessTerminal` is the cross-host Fact while AW/dotlink retain
their product receipt/output owners; hosts pass prepared commands by value; semantic weak-
Unix negative occupancy maps to Unknown in `aw-process`; and transferable `aw-terminal`
PTY custody remains a distinct Terminal resource rather than a second Process Job owner.

**Shared Process/release-correction amendment seal (2026-11-21):** explicitly approved through the
v0.17.7 parent-lifetime review and subsequent user-directed imports of the newer 0.17.x fixes.
Section 25.11 fixes the Linux launch contract as one private logical Spawn Facility with O(1)
fixed/add-only process-lifetime lanes (N=1 initially), async bounded admission, complete RAII
custody transfer, freely worker-migratable `Process: Send` lifetimes, no lane retirement/shrink,
poison-without-anchor-exit failure handling, and benchmark-only internal scaling. It rejects
thread-per-Process, `spawn_blocking`/`block_in_place` lifetime anchors, public pool configuration and
a launcher-process detour.

The same sealed amendment now also includes the later audited release corrections: strict
non-reaping child-terminal proof, independent stop-vs-exit attribution, Windows
`CREATE_SUSPENDED | CREATE_NO_WINDOW`, bounded/interruptible Windows Job-accounting settlement,
and the v0.17.9 stale-launch rule. One absolute execution deadline is established when the
runner enters Running and is shared by Spawn-Facility queueing + Process execution; cancellation/
expiry is rechecked on the lane immediately before fork. A stale launch skipped before
`Launching` creates no Process and therefore no ProcessTerminal/StopRecord/kill claim. Product Job
age remains separate from the execution deadline, and dotlink deadline advice counts down from the
runner's actual deadline rather than from Job age. **This amendment is authoritative and sealed.**

The strong Linux PID-1 backend contract remains normative but its executable qualification
belongs to the separately promoted launcher/backend gate. The public Agent/Jobs/Event/
Sandbox/Checkpoint APIs, existing authoritative owners, seven phases and all R5 safety/
retention/ACL/causal contracts stay unchanged. **R6 is authoritative; R1–R5 are historical
design provenance.**

The phased implementation plan is [`IMPLEMENTATION.md`](IMPLEMENTATION.md). Source
implementation was separately promoted by the user on 2026-11-21; Phase 0 is active.
Commit, push, release and deployment remain separately gated. The frozen
`017-sandbox-mount-convergence-2026-11-18` implementation remains outside this promotion
unless separately promoted.
