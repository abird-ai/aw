# DL-200 — unified Jobs, Agents, Events, History and Checkpoints

Author: Prasanna Loganathar
Date: 2026-11-20
Status: **SEALED / IMPLEMENTATION PLAN READY — NOT PROMOTED**

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
  source visibility and host-granted Event capability.

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

Pagination cursors retain any hidden scan position privately. Hidden rows do not change
visible counts/offsets or create observable gaps. Direct `jobs_get/read/send/cancel` still
reauthorize the exact target independently.

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

Conceptually:

```text
MutationReplayKey = authenticated caller/scope + semantic operation + request identity
request_fingerprint = canonical hash of the complete semantic request
```

Rules:

- same replay key + same fingerprint -> return the same factual result/created resource IDs
  or the same terminal receipt; do not repeat the mutation;
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
- do not create a second generic replay ledger merely for DL-200.

This applies uniformly to resource construction and additive mutations that can otherwise
duplicate on retry, including `agent.create`, `sandbox.create`, `checkpoint.create`,
`group.create`, staged input append/Send, custom Event publication and Extension Actions
that invoke those canonical owners. A replayed custom Event returns the same persisted
`Event.id`; a replayed constructor returns the same Agent/Sandbox/Checkpoint/Principal.

### 17.2 Prepared construction and atomic public publication

Constructors that span AW rows plus filesystem/provider materialization use one shared
prepare/publish discipline. Do not expose a half-created resource and do not require a
cross-provider distributed transaction.

```text
1. reserve/mint operation-owned candidate IDs privately
2. preflight all current authority, disclosure and quota constraints
3. prepare external/materialized state under non-public operation custody
4. revalidate exact source identities/authority/fingerprints
5. in one AW store transaction publish all authoritative current rows/grants/relations,
   initial queued input where requested, commit-log Events and mutation-replay result
6. only after commit make the canonical refs discoverable/dispatchable
7. release temporary barriers; asynchronously/reconciliably clean abandoned preparation
```

Prepared objects are **not** public Sandboxes/Agents/checkpoints and cannot be addressed by
Jobs/Events/Views while preparation is incomplete. A crash before step 5 leaves no
published AW resource; a reconciler identifies operation-owned prepared worktrees/provider
materializations/temporary retention roots that are not reachable from a committed current
row and cleans them idempotently. If an external prepare effect has an uncertain outcome,
the operation remains factual `Unknown`/needs-inspection and is not blindly retried.

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
    capture(binding) -> immutable SourceAnchor
    restore(anchor)  -> fresh mutable SourceBinding
    retain/release/reconcile(anchor)

SourceAnchor
    driver kind + immutable driver-specific identity/provenance
```

A source driver is valid only if it can fail closed and provide stable immutable restore
semantics. There is deliberately no generic "tar whatever is at this path" fallback.

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
    source_resource / source_fact
    already_disclosed_principals[]   // user/group principals, groups not expanded
    source_policy/provenance evidence
}

DisclosureManifest {
    immutable content-addressed id/digest
    normalized deduplicated DisclosureDependency closure
}
```

`DisclosureManifest` is an internal immutable provenance structure, not another ACL/grant
service or public resource API. HistoryGrant, Event, committed local facts and trusted
projections may reference the same manifest/digest; unions create/reuse a normalized
content-addressed manifest rather than copying an unbounded dependency array into every
row. Retention follows the immutable facts/grants/Events that reference it.

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
- combining protected inputs unions dependencies rather than overwriting them;
- an Assistant/model output inherits the union of disclosure dependencies from the
  model-visible context/tool results that could have influenced it, because AW cannot
  prove the model did not paraphrase or reproduce them.

This is conservative **capability/provenance propagation**, not byte-level dynamic taint
analysis. AW does not claim to infer arbitrary information flow through code, files or
network side effects inside a Process; Sandbox/Vault/source capability separation remains
the primary control for those effects. Dependencies may be removed only by an explicit
trusted redaction/declassification/materialization operation producing a new immutable
projection/base with its own provenance. Ordinary Hooks, model generation,
summaries and exact forks never silently declassify.

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

Add one authoritative current Agent projection rather than teaching every legacy Session
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

Backfill one `agent_state_current` row for every existing public Agent binding and
freeze its canonical handles. Seed its initial `rev` from the exact value the current
bridge would return (`MAX(audit.seq)+1` for that backing Session) so a cutover does not
manufacture an immediate stale-value discontinuity.

After the projection is activated:

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
Bubblewrap/container/VM namespace.

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
Sandbox audience model. Sandbox lifecycle/binding changes and completed AW-owned write-capable source mutations
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

`ExtensionRegistry` is the composition root and exposes typed subregistries:

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
authority. They are not duplicate databases/authority engines.

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

The `ExtensionRegistry` passed to `Extension::register` is an extension-scoped **staging
transaction**, not a set of immediately-mutating registry references. All contributions
from that call are validated together and published atomically as one generation. A
collision, schema error, invalid authority declaration or failed subregistry validation
aborts the whole registration; no half-installed Extension is visible.

A later dynamic update uses the same extension-scoped, generation-fenced transaction
model with an expected registry revision. This covers Pi-style dynamic registration
without giving runtime callbacks unrestricted mutable access to global registries.

Register/remove/reload advances one ExtensionRegistry generation. Subregistry `revision()`
is the relevant typed projection of that root generation plus its local change revision;
it is not a separate authority clock. In-flight work remains pinned to the exact registry
+ authority generation under which it was admitted; newly admitted work sees the new
generation.

Removing one registration or an entire Extension withdraws registered definitions but
**does not destroy durable extension State, EventHandler cursors, product data, source
anchors or secret custody**. Those records are retained/tombstoned according to their own
owner. Reinstalling the same stable Extension/local IDs and compatible schema may resume
them only after current authority/schema revalidation. Destructive data cleanup is a
separate explicit host maintenance operation, never a side effect of registry removal.

A definition registry may refuse removal when the implementation itself is required to
keep live durable state valid. In particular, SourceDriver removal is refused while live
SourceBindings/checkpoints depend on it unless a compatible replacement is published in
the same atomic generation; SandboxDriver removal is refused while live execution depends
on it. VaultProvider removal may make grants unavailable/fail-closed but never deletes or
exports provider-held secret material.

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
does not own Sandbox authority.

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

- dynamic process Extensions run under host-selected isolated **process containment** by
  default once the lower Sandbox implementation is promoted; they do not become Agents or
  new Sandbox authority subjects merely because they are Extensions;
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

### 25.11 Shared Process settlement adopted from 0.17.6

The release/0.17 worktree established one additional shared fact that belongs in AW rather
than dotlink presentation: when the waited command ended, did any live work remain in its
contained process tree before cleanup?

Keep the shared API deliberately small.

#### One new semantic fact: `Tree`

`aw-tools` owns the host-neutral fact:

```rust
pub enum Tree {
    Empty,
    Live,
    Unknown,
}
```

Meaning:

```text
Empty    AW proved no live contained work remained before cleanup
Live     AW proved at least one live contained member remained
Unknown  AW could not prove either
```

Do not infer `Live` from a successful kill. Tree occupancy and cleanup evidence are
orthogonal.

The platform layer stays simpler and does not depend on aw-tools. For directly managed
process groups/Job Objects it exposes the existing tri-state I/O shape:

```rust
ManagedProcessGuard::observe() -> io::Result<bool>

Ok(true)  -> Tree::Live
Ok(false) -> Tree::Empty
Err(_)    -> Tree::Unknown
```

Namespace/sandbox execution may provide the same observation through its backend-specific
private mechanism; the adapter maps it into the same `Tree` value. No second public
observation enum is added.

#### Keep output metadata flat

Do not introduce a separate stream metadata hierarchy. Refine the existing shared output
record only enough to retain per-stream EOF/quiescence:

```rust
pub struct ProcessOutputMeta {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_complete: bool,
    pub stderr_complete: bool,
}

impl ProcessOutputMeta {
    pub const fn complete(&self) -> bool {
        self.stdout_complete && self.stderr_complete
    }
}
```

`*_bytes` means bytes observed by AW. When that stream is incomplete it is not a claim
about the exact number of bytes the command produced. `truncated` continues to mean
retention truncation only; it is independent from drain completeness.

#### `ProcessTerminal` is the one final factual result

The canonical shared terminal value becomes:

```rust
pub struct ProcessTerminal {
    pub exit: ProcessExit,
    pub stop: Option<StopRecord>,
    pub tree: Tree,
    pub output: ProcessOutputMeta,
}
```

These four fields answer four independent questions:

```text
exit    how the waited command itself ended
stop    what semantic/housekeeping stop AW proved, if any
tree    whether live contained work existed before cleanup
output  which output streams AW observed completely
```

`StopRecord` / `StopReason` / `StopEvidence` / `StopTracker` remain unchanged. Do not add
parallel Cleanup/TreeStop/TargetGone types.

`ProcessReceipt` and `ProcessSettlement` should converge on `ProcessTerminal` as their
single factual terminal truth. Existing `Outcome`, aggregate `output_complete`, `status`
or product helpers may remain temporarily as compatibility projections during cutover, but
must be derived rather than independently authored. The implementation should delete
redundant stored facts where compatibility permits.

#### One settlement order

Normal exit, timeout, cancel, runtime error and explicit termination converge on one shared
settlement invariant:

```text
wait until command exit is ready / stop boundary reached
-> observe contained tree before cleanup
-> cleanup containment
-> if cleanup is Unknown, retry once immediately while the target identity is still safe
-> reap/finalize the waited command
-> drain stdout/stderr with bounded grace
-> publish ProcessTerminal
```

On Unix, do not reap the process-group leader before the final observation/cleanup signal;
use non-reaping exit readiness (`pidfd`/`waitid(...WNOWAIT)`/equivalent) so a reused numeric
PGID cannot be signalled accidentally. Positive live-member evidence is easy; a negative
Linux observation is accepted only when the observed `/proc` is the command's PID
namespace and visibility is complete (`hidepid`/partial scans fail to `Unknown`). Zombies
do not count as live, while live threads under an exited/zombie leader do.

On Windows, Job Object active-member accounting supplies the observation independently
from `TerminateJobObject`; successful termination alone is not occupancy evidence.

A racing forced cleanup must not overwrite an independently established command exit.
Preserve AW's existing graceful-stop semantics, but for forced settlement an exit code or
independently observed terminal signal that predates/contradicts the cleanup attempt remains
the command's physical `exit` fact.

An unconfirmed cleanup never gets hidden by a later successful output drain. The guard
remains fail-safe armed unless settlement has conclusively finalized/disarmed it.

Bubblewrap/PID-namespace observation is an implementation of this same contract and must
ultimately live in the shared platform/Sandbox execution layer. Do not restore the
release-local dotlink Guard/observer as a second semantic owner, and do not promote the
frozen 017 mount plan merely to land this process-settlement work.

#### Dotlink owns the projection and turn horizon

AW does **not** gain `tree="leftovers"`, `success`, signal-name JSON, lifecycle prose or a
turn-horizon API. Dotlink derives its product/wire fields from the shared facts:

```text
Tree::Empty    -> omit tree
Tree::Live     -> tree="leftovers"
Tree::Unknown  -> tree="unknown"

stdout_drain_incomplete = !output.stdout_complete
stderr_drain_incomplete = !output.stderr_complete
```

Dotlink may make `Tree::Live` clear its product `success` flag without rewriting the
physical exit code or turning the MCP call into a generic tool error. Signal names remain
a dotlink edge projection using the existing AW signal-name helper. One concise operator
lifecycle line is likewise dotlink presentation, not shared process truth.

Dotlink also restores the release runtime-horizon policy **at the dotlink layer only**:

```text
recommended coherent checkpoint  ~= 30 minutes
hard conversational command horizon = 35 minutes
```

For a host/runtime profile that declares the bounded conversational horizon, dotlink uses
one source of truth to derive server/model guidance, shell schema maximum and runtime
validation; the effective command deadline is the tighter of configured shell policy and
the 35-minute horizon. A runtime profile without a bounded conversational turn keeps its
normal configured Process limit and omits turn-horizon guidance. `background=true` changes
observation/hand-off behavior, not this command-lifetime bound.

This policy is intentionally absent from AW. AW continues to accept explicit host-selected
Process deadlines and keeps `ReceiptProfile.checkpoint_minutes` advisory rather than an
execution-authority primitive.

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
internal Event scan position
created / last-used time
```

It is not bound to a frozen ACL snapshot. Every concrete Event is reauthorized against
**current** resource authority **and its immutable DisclosureManifest closure** at read
time. The caller must currently satisfy the concrete source policy plus every manifest
dependency; a pattern/cursor never bypasses protected payload provenance. Therefore:

- losing access hides unread matching Events immediately;
- gaining access does not retroactively reveal rows the cursor already advanced past;
- wildcard patterns grant no discovery/access and dynamically include future resources;
- hidden/nonmatching rows advance only the private scan position and reveal no counts,
  sequence numbers or gaps.

One call scans at most 4096 underlying Event-index rows to produce up to the requested
256 visible Events. Reaching that scan bound may return an empty page with a new opaque
cursor; it does not disclose how many hidden rows were crossed.

Read cursors expire after 24 hours of inactivity and are bounded to 64 live cursors per
principal after opportunistic cleanup. The logical public Event retention floor keeps
**all Events from the last 7 days and at least the most recent 65,536 Event rows**,
whichever is more permissive. The immutable commit log may physically retain data
longer; that does not promise indefinite public replay.

If a cursor falls behind the logical retention floor, return `cursor_expired`. The
caller rereads authoritative resource snapshots and resumes from `after="now"`.

With `wait_ms`, the store uses the same cursor semantics plus a local wake accelerator;
there is no poll race and no hidden source count leak.

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

Cross-Project, cross-Principal and cross-Domain observation uses the same grammar; only
the concrete resource authorizer changes.

### 26.3 Durable `events.subscribe`

A durable subscription asks that future matching Events be delivered into an Agent's
authoritative input owner. Creating/enabling/changing one requires recipient Agent
lifecycle `open` plus current `control` or `manage` authority (or the equivalent exact
model tool grant). Disabling/removing remains allowed during close for idempotent cleanup.
The committed subscription retains the factual creating principal/tool-authority
provenance used for later disclosure revalidation.

Host form:

```text
events.subscribe(
    subject,
    to=<Agent ref>,
    enabled=true
)
```

`enabled=false` removes that exact recipient+normalized-subject subscription.
Replacement/removal is idempotent and generation-fenced internally.

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

No queue/steer/interrupt/wake mode is part of the public subscription. A matched Event
becomes one typed immutable `EventInput` in the recipient's existing Agent input owner;
the host scheduler decides when/how that input is admitted.

Backlog is **not copied per subscriber**. The shared immutable Event log plus each
subscription's scan position is the backlog. For each candidate:

1. match the normalized pattern;
2. reauthorize the exact concrete source resource under the subscription's current
   creating-principal/tool authority;
3. revalidate the recipient Agent and the authority that installed the subscription;
4. mint deterministic delivery identity
   `hash(subscription, generation, Event-log identity)`;
5. validate disclosure into the **current recipient disclosure principals** (owner +
   direct/group ACL principals, with a group treated as one live principal rather than
   expanded members): each such principal must satisfy both the concrete source
   Event/resource disclosure policy **and every immutable Event disclosure dependency**;
   where a principal is not already disclosed, the installer must hold the required
   current source `share`/equivalent authorities and each dependency policy must permit
   expansion;
6. admit that EventInput through the existing replay-safe Agent queue/input owner with the
   union of the Event's source + transitive disclosure dependencies/provenance, recording
   the current recipient disclosure principals as already disclosed;
7. advance the subscription scan position only after accepted/deduplicated terminal
   admission.

A crash between 6 and 7 safely replays the same deterministic admission. There is no
second pending-message database.

Unauthorized/nonmatching concrete Events, including Events whose disclosure is not valid
for the recipient's current audience, advance the private subscription scan just as they
do for `events.read`; later authority restoration or audience narrowing never
retroactively discloses the skipped interval. Recipient ACL broadening later is separately
checked against every retained EventInput/content disclosure dependency before the new
viewer is admitted.

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
Events. Do not create another Event database. The section 26.2 Event replay floor is a
logical Event-read/consumer availability contract; it is **not** authorization to prune
non-Event commit/audit/recovery history that another store owner still retains. Event
projection/index compaction and core commit-log retention remain separate concerns.

Generalize the current Job Event code by **splitting its responsibilities**:

```text
EventSourceState
    exact concrete source
    next source-local event_rev

commit_log
    immutable factual Event envelope + global private scan position
    // existing event_id is Event.id; canonical subject/envelope lives in bounded payload

event_projection_index
    rebuildable parsed subject columns keyed by commit_log position

event_subscriptions_current
    recipient-owned normalized patterns + generation + scan position

event_read_cursors_current
    opaque reader/pattern -> private scan position
```

`event_projection_index` is an index/projection only; deleting/rebuilding it never
deletes Event truth. Reuse the existing `commit_log.event_id` as stable `Event.id`. Do not
stuff a potentially 2048-byte canonical resource ref/subject into legacy
`commit_log.entity_id` (currently a compact internal identifier with a smaller bound): a
DL-200 Event row stores its bounded canonical subject/data/provenance envelope in
`payload_json`, while `kind/entity_type/entity_id` remain compact internal classification/
index metadata. Existing historical commit rows are not rewritten; rows that have no
DL-200 Event projection are simply skipped by the Event index/scanner.

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
two long-lived Event runtimes:

1. start committing DL-200 Event envelopes for newly authoritative transitions while
   retaining the old rows as compatibility state;
2. translate each exact legacy rule into one exact normalized recipient subscription at
   a baseline that cannot replay older source history;
3. convert each already-pending legacy delivery once using a deterministic migration
   EventInput ID; retain its old queue/steer/interrupt mode only as a one-time host
   scheduling hint/provenance, never as a new public Event field;
4. preserve the source's next `event_rev` so revision continuity is not reset;
5. once all pending compatibility deliveries are acknowledged/drained and restart proof
   is green, stop writing the old subscription/pending JSON and contract it later.

The same deterministic input admission makes upgrade/restart replay safe. There is never
a second semantic Event bus.

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
it to change, then reauthorize and reread."

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

## 30. Architectural invariants

1. Model-facing Jobs remain exactly six tools; durable Agent reaction is generic
   `events_subscribe`.
2. Host change observation uses the shared `events.read` stream rather than
   resource-specific watch/long-poll APIs.
3. Exact forks structurally share immutable history; they do not copy prior Turns.
4. Agent is the public security/history/execution envelope; there is no public AW Session
   resource or `session.*` API.
5. Fresh and exact-history-fork construction use one host `agent.create(from=...)`
   primitive; every exact fork creates a new globally addressable Agent.
6. Exact forks preserve inherited identity/content/provenance.
7. Redaction/anonymization creates a new projected HistoryBase, not an exact fork.
8. Agent access uses one allow-only ACL over user/group principals.
9. Groups are explicit reusable user sets; no implicit groups and no nesting in v1.
10. Direct-user and group grants may coexist; effective permissions are an allow-only
    union.
11. Group-owned Agents are explicit team co-ownership; group management remains separate
    from Agent management.
12. Agent ACL/principal/security metadata stays outside PrefixContentKey unless actually
    model-visible.
13. Cache reuse is optimization, never authority.
14. New Agents created from inherited history mint fresh ACL/security state.
15. Active Turns, finite Job runtimes, watches and staged input drafts are never copied
    into a history fork.
16. Checkpoint identity is immutable history + immutable named SourceSet, never `rev`.
17. Checkpoint restore is non-destructive, recreates the full captured SourceSet in a new
    Sandbox and creates a new Agent.
18. Settled Turn boundaries are reusable fork/cache seed points.
19. Fanout needs no new API; repeated `agent.create(from=...)` calls structurally share
    immutable history, and derive the same cache identity only when their effective
    model-visible profiles are also identical.
20. Recursive subagents use the same Agent constructor, Sandbox composition and existing
    delegation limits.
21. System and Agent-emitted notifications are one Event concept: bare Event names are
    system-owned and `#name` is custom, so Agents cannot forge system Events.
22. `jobs_send` remains the directed-input path; `events_publish/events_subscribe` are
    source-scoped Event publication/reaction, not a bulk message transport.
23. One lower Agent/history/Event owner model serves model tools, host APIs and recursive
    delegation.
24. Agent `rev` is owner-local; referenced group/security changes never cause fanout
    writes to every Agent and are revalidated through independent access generations.
25. The immutable `commit_log` is the one Event history/order owner; Event indexes,
    cursors, subscriptions and durable consumer positions are projections/current state,
    not another Event log.
26. Event patterns never grant authority; every concrete read/delivery is authorized at
    the exact resource and recipient boundaries.
27. `i:<principal>` is one Domain handle namespace across user/group kinds; kind is
    metadata, never a second public identifier grammar.
28. Project is a principal-scoped public view over existing Workspace backing in v1;
    DL-200 does not duplicate Workspace truth into a parallel Project service.
29. Checkpoint SourceSet is typed and plural. Git is the mandatory first SourceDriver;
    every driver must provide immutable fail-closed capture/restore semantics and there
    is no generic archive snapshot fallback.
30. Host staged input state owns `inputs_rev`; only Send crosses into Agent runtime
    custody and Agent `rev`.
31. History ancestry, Sandbox ancestry and Vault authority are orthogonal; none grants
    the others.
32. A Sandbox is logical mutable execution state, not a persistent OS namespace; each
    Process receives fresh platform containment from current state + authority.
33. Multiple Agents may share one Sandbox while retaining different Sandbox/Vault
    authority.
34. Checkpoints capture SourceSet state, never live Sandbox/Vault authority or secret
    material.
35. Extension is only the installed composition unit; ExtensionRegistry and its typed
    subregistries register contributions but own no second authority/runtime.
36. View is a namespaced bounded read-only pull facet; rich product data remains owned by
    the embedding product.
37. Durable EventHandlers consume the same `commit_log` at-least-once; they are not a
    second Event bus and public `events.read` retention is not the product-indexing
    contract.
38. Agent-local extension State is bounded, namespaced and non-model-visible by default;
    stored State/seq stays outside HistoryBase and Agent `rev` and is not inherited across
    forks, while exact State-derived bytes explicitly projected to the model participate in
    PrefixContentKey like any other model-visible context.
39. Actions are explicit host/user operations; slash commands, shortcuts, buttons and RPC
    bindings never create alternate mutation owners.
40. `ctx.tools.set_visible` only selects among registered/currently authorized Tools; it
    never manufactures Tool grants.
41. Provider-sensitive interception uses ProviderHook; Process interception uses
    ProcessHook, and extension execution always remains inside Sandbox/Jobs authority.
42. Hook stages are interceptable in-flight boundaries only (`Turn/Step/Input/Context/Tool`);
    committed Event/lifecycle observation uses EventHandler/AW Events, and Prompt/Signal
    hook aliases do not exist.
43. Native and dynamic Extensions feed the same ExtensionRegistry; process/WASM hosts are
    runtime adapters, never parallel extension APIs or authority owners.
44. The v1 dynamic ABI is the existing bounded stdio process direction generalized to one
    active top-level callback plus scoped nested `ctx.*` calls; unsolicited child calls,
    ambient credentials and blind retry after uncertainty are forbidden.
45. V1 requires no npm/pip/cargo resolver, marketplace, embedded JavaScript runtime,
    Wasmtime dependency or WIT world. WASM remains an optional later ExtensionHost.
46. Parent/delegation lineage, namespace placement and immutable-history ancestry are
    distinct; lineage never grants visibility, and exact history forks never fabricate
    destination-local Job/Node runtimes for inherited historical work.
47. HistoryBase identity contains immutable history/provenance only. PrefixContentKey is a
    derived invocation projection, and lazy HistoryBase sealing/indexing is idempotent
    internal memoization that never advances Agent `rev`.
48. Public resource handles keep the canonical ExecutionRef grammar. Event routing uses a
    reversible canonical token encoding (including `%2E` for literal `.`) rather than
    weakening resource handles or ambiguously splitting tokens.
49. Checkpoints retain exact SourceBinding layout metadata plus immutable source anchors;
    every live Git checkpoint pins clean or dirty commit objects independently of user
    branch reachability, and submodule contents require their own SourceBinding when they
    must be retained.
50. Agent->Sandbox binding is immutable in v1. Sandbox lifecycle is `open|closed`, source
    rebinding is quiescent/all-or-nothing, and SandboxGrant/VaultGrant authority remains
    actor-sensitive and independently revalidated.
51. Vault projection kinds are provider/host-defined safe profiles, not arbitrary mount/env
    injection. Grant removal governs future projections and never claims retroactive
    erasure from already-running Processes.
52. Disclosure dependencies are immutable transitive provenance represented by internal
    content-addressed DisclosureManifests. History, Events, EventInputs, protected
    Resources/Views/tool results and model outputs cannot silently drop/declassify them.
53. Event read, Agent delivery and durable Handler delivery all authorize the exact source
    plus the Event DisclosureManifest. Durable Event causal root/hop/delivery budgets
    survive replay/restart and cannot be reset by descendants.
54. One authenticated mutation replay identity/fingerprint protects constructors/additive
    mutations from duplicate retry. Same identity + changed semantics conflicts; uncertain
    external effects remain Unknown/no-blind-retry. No second replay ledger exists.
55. Multi-resource construction uses private prepare -> one atomic AW publication ->
    reconciled cleanup. Half-restored/half-created Agents/Sandboxes/checkpoints are never
    public, and caller-owned explicit resources are never rolled back on unrelated failure.
56. Extension registries own definitions, not activation/grants. Runtime bindings are
    scoped; typed handles stale only on their own registration/type/schema change, not an
    unrelated root generation change.
57. Dynamic process Extensions reuse the bounded existing stdio ABI with explicit absolute
    executables, one callback scope, same-process re-entry rejection and bounded nested
    capability calls; manifest declarations never manufacture authority or narrow an
    untrusted Tool effect classification.
58. Views are optional enrichment: no View failure can hide/fail an already-authorized core
    snapshot. EventHandler registration is inert until explicit activation and suspended
    handlers do not pin Event retention indefinitely.
59. AW promises atomicity only within its own authoritative store. Product/Extension stores
    such as Histate compose AW mutations through idempotent saga/outbox reconciliation,
    never a hidden distributed transaction.
60. `commit_log.event_id` is reused as Event.id; bounded Event subject/envelope data lives
    in payload/projection fields rather than overloading legacy compact `entity_id`, and
    Event replay retention never authorizes pruning unrelated commit/audit truth.
61. Closed Agents are terminal execution objects but remain readable/shareable history
    roots; execution-adjacent mutable surfaces freeze at closing while title/access/
    checkpoint/history management may continue under their independent authority.
62. Tool/operation availability and resource authority are conjunctive; a broad Jobs or
    Extension capability never grants cross-resource discovery/control by itself.
63. Shared Process settlement has one additional fact only: `Tree::{Empty,Live,Unknown}`.
    Tree occupancy is independent from StopEvidence; a delivered kill never proves that
    live contained work existed.
64. `ProcessTerminal` is the single shared factual terminal value: physical `exit`, final
    `Option<StopRecord>`, pre-cleanup `tree` and per-stream output metadata. Compatibility
    outcome/status/completeness projections are derived rather than independently authored.
65. Process settlement observes/cleans containment before final reap/drain, retries one
    Unknown cleanup immediately while target identity is safe, and never hides unresolved
    cleanup behind a successful drain. Unix must not signal a reused PGID; trustworthy
    negative Linux observation requires correct namespace + complete procfs visibility.
66. Conversational turn horizons are dotlink product/runtime policy, not AW Process
    semantics. Dotlink uses ~30-minute checkpoint guidance and a 35-minute hard command
    horizon only for runtime profiles that declare a bounded conversational turn.


---

## 31. Resolved design items before seal

The pre-implementation design questions are closed in this draft. They remain
**unsealed** until explicit user review/approval.

1. **Cross-Agent HistoryBase/HistoryGrant storage/migration** — sections 19.2-19.3 define
   immutable HistorySpan/Base/Grant links above the existing Session journal, lazy legacy
   sealing, structural sharing and reachability GC. Session recovery remains the local
   Turn/effect owner.
2. **Owner-local Agent rev + group-driven ACL revision** — sections 12 and 24.5 keep
   Agent `rev` strictly owner-local. Direct ACL edits bump it; group membership changes
   only group `rev` and derived `access_generation`, with no Agent fanout writes.
3. **Checkpoint SourceSet retention/restore semantics** — section 18 makes checkpoints
   exact history + a plural named typed SourceSet. Git keeps the private-index synthetic
   commit path; typed drivers admit versioned object/bucket manifests without a generic
   archive fallback; multi-source capture is all-or-nothing and restore creates a fresh
   Sandbox.
4. **Model exact-fork default Project/access** — section 19.9 fixes the default to source
   Project + inherited source direct ACL. Trusted host/delegation policy may narrow it;
   redirect/broadening requires ordinary authority and is never model-selected.
5. **Migration away from Session-audit-derived Agent rev** — section 25.1 defines
   `agent_state_current`, one-time seed from the current bridge revision, then
   transaction-owned persisted Agent rev independent from audit.
6. **Generic Events substrate** — sections 26.3/26.6 make existing immutable
   `commit_log` the single Event history/order, split source Event revision from
   recipient-owned pattern subscriptions, and reuse deterministic Agent input admission.
7. **Event bounds/disclosure/wildcards** — sections 3.3 and 26.1-26.5 fix grammar,
   payload/read/subscription/causal limits, self-delivery suppression and exact
   per-resource authorization. Bare names are system-owned; `#name` is structurally
   custom, eliminating a reserved-name registry.
8. **Event cursors/retention/authorization projection** — section 26.2 defines opaque
   caller+pattern-bound cursors, hidden-row scanning, 24h idle cursor retention, the
   7-day/65,536-row Event replay floor and `cursor_expired`; subscriptions use the same
   floor and become explicitly `lagged`.
9. **Legacy SDK observer/queue/process/pause/control convergence** — section 28.11 maps
   every old semantic family to the canonical Jobs/Events/Agent-input/admission owners
   while retaining lower Prepared/Unknown/CAS/no-retry facts.
10. **Minimal Project/Principal/Domain metadata APIs** — section 24.7 defines only bounded
    `list/get` host reads; Project is a principal-scoped projection of existing Workspace
    backing rather than a second Project service.
11. **Principal-kind storage under one `i:` namespace** — sections 24.1/24.7 define one
    Domain principal registry keyed by a single handle across user/group kinds, with
    group membership as an extension keyed by that same principal identity.
12. **Unified Agent constructor** — sections 19/22/28 remove host `agent.fork`; fresh and
    exact-history construction use `agent.create(from=...)`, while
    `checkpoint.restore` remains the stronger exact history+SourceSet recovery API.
13. **Logical Sandbox composition** — section 25.4 defines Project-base, shared, derived
    and fresh Sandboxes independently from history, with per-Agent monotonic authority
    and fresh physical containment for each Process.
14. **Vault capability projection** — section 25.5 keeps secret values/provider lifecycle
    out of Agent history/checkpoints while allowing revalidated broker/socket/file/env
    projections through Agent-scoped VaultGrants.
15. **Typed ExtensionRegistry composition / Pi parity / Histate seams** — sections
    25.6-25.9 define one ExtensionRegistry with typed Tool/Hook/Action/Resource/View/
    EventType/EventHandler/Provider/UI/Setting/State/Compactor/SourceDriver/SandboxDriver/
    VaultProvider subregistries. Native
    Rust registration preserves concrete types and returns typed handles; stable
    Extension/local IDs + schema versions remain durable/wire identity and type erasure
    occurs only at heterogeneous registry internals or real boundaries. Runtime callbacks
    still use scoped `ctx.tools`, `ctx.state`, `ctx.ui`, etc. Histate keeps its durable
    Build/Session/Decision/summary/semantic graph in its own typed store.
16. **Custom Event namespace and inspection** — sections 3.3/26 use `e:#ready` for custom
    Events, preserve NATS-style wildcard cardinality, expose stable Event identity for
    provenance/idempotency and add bounded host subscription inspection without another
    Event owner.
17. **Minimal dynamic Extension host** — section 25.10 reuses/generalizes the existing
    `aw-extensions` bounded stdio process ABI as the universal v1 dynamic host. TypeScript/
    JavaScript is the first SDK, package installation stays with external ecosystems, v1
    dynamic registrations are intentionally a safe subset of native registries, nested
    child->host capability calls are bound to host-minted invocation scopes, and embedded
    WASM/Wasmtime/WIT are deferred until real demand.
18. **Fresh full pre-seal architecture audit** — the end-to-end audit hardened the design
    across identity/privacy, revision ownership, exact history/fork identity, cache keys,
    source/checkpoint retention, Sandbox/Vault revocation, mutation replay + atomic
    publication, transitive disclosure manifests, Event token encoding/causal persistence,
    commit-log compatibility, typed Extension activation/reload, dynamic process bounds,
    State CAS, View failure isolation and Histate/product cross-store composition. The
    audit removed public cache/revision/HistoryBase constructor knobs, resolved Graph-Node
    cancel to reject in v1, and introduced no duplicate owner/service/runtime.
19. **0.17.6 Process settlement adoption** — section 25.11 adopts only the reusable shared
    facts learned from the release worktree: one `Tree` fact, per-stream completeness and
    one `ProcessTerminal` truth with settlement-before-drain ordering. Platform observation
    stays minimal; dotlink retains wire/log/success projection and its 30-minute advisory /
    35-minute hard conversational runtime horizon. No new model or host API is introduced.

The fresh **full pre-seal** contradiction/correctness/minimality review is complete and
clean after the earlier naming, typed-registry and dynamic-host reviews. Live source/schema
checks confirmed the current ToolBindingAuthority, Agent constructor replay, compaction
projection, commit-log transaction and Job Event transaction seams can support the design
without requiring parallel semantic owners.

The subsequent 0.17.6 forensic review was folded in through section 25.11 with the smaller
shared Process API above; no release-local semantic owner was restored.

---

## 32. Seal

DL-200 is **SEALED / IMPLEMENTATION PLAN READY — NOT PROMOTED**.

Explicit user approval to seal was given on **2026-11-20** in this conversation after the
full pre-seal audit and the simplified 0.17.6 Process-settlement refinement.

The phased implementation plan is
[`IMPLEMENTATION.md`](IMPLEMENTATION.md). Sealing authorizes that plan to exist; it does
**not** authorize source implementation, commit, push, release or deployment.

Implementation begins only after a separate explicit user promotion, and may be promoted
phase-by-phase. The frozen `017-sandbox-mount-convergence-2026-11-18` implementation remains
outside this seal unless separately promoted.
