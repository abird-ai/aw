# Final implementation specification: AI-native operations, Jobs, Agents, Control and wake

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **DESIGN FINAL, 2026-11-10. Implementation is not yet authorized or performed by this plan.**

This file defines the proposed **vNext** semantics. It is not a retrospective statement that the APIs already exist, not a migration of sealed DL-150, and not a substitute for current source/tests. If this proposal is accepted, its implementation must intentionally change and test the old wire contract, rather than silently editing the earlier plan.

## 0. Scope, constraints and authority

### Goals

1. A compact, predictable model tool API: the existing 13 orthogonal filesystem tools; explicit bash/pwsh, batch and grouped agents constructors; one grouped jobs management tool.
2. One canonical semantic call language: `[operation,args]`; Graph nodes are `[operation,args,when?]`. Grouped object-wire `agents({op,...})` / `jobs({op,...})` is the only secondary projection when a transport requires object-shaped arguments.
3. The **same** typed operation semantics and authority for model calls, QuickJS/Python scripts, host Rust SDK and product adapters.
4. Five host-classified effects: Read, Write, Process, Control, Unknown. No Composite authority category and no conflation with Job lifecycle outcome unknown.
5. First-class read-only filesystem sessions that can run bounded read-only Graphs and independently permitted Job observation/ACK/cancel/wake operations; bounded Agent delegation is separately grantable without arbitrary shell authority.
6. One Agent implementation. Root, child, recursive child, team member and planned worker differ by parent/delegation/session bindings, not by a second Agent runtime.
7. One immutable Graph/Node execution definition with optional, independently controlled Agent execution as a node; no Graph topology/dataflow rewrite from steering.
8. One parent-owned event-to-wake vocabulary, used on creation and updated by jobs.subscribe; reliable shell, Agent and Graph settlement.
9. Typed retained Graph results (text, exact source, binary, image/audio/resource) and precise get/output/ACK/long-poll contracts.
10. Optional host-configured dynamic policy executors (local rules/program/QuickJS/Python/MCP/model/pipeline/human), always narrowing the static grant ceiling.
11. No second Agent, TurnMachine, Process supervisor, actor message bus, JobManager, Session journal, replay ledger, event bus or policy authority.

### Existing invariants retained

- Shared semantics and lifecycle in AW's aw-tools, secured filesystems in aw-tools-fs-unix and aw-tools-fs-win; product-specific AW and dotlink adapters supply authority, context, process custody, media delivery and transport/replay.
- Existing model-facing 13 native filesystem operations: ls, find, grep, read, write, edit, patch, cp, mv, rm, bread, bwrite, bedit. ls remains shallow; find recursive; no default shell compatibility.
- Canonical outcome grammar: ok, unchanged, fail, mixed, unknown, skip, plus orthogonal more and incomplete. An aggregate with known failures and unknown components is mixed, not uniformly unknown. Job lifecycle running/cancelling/cancelled is separately tracked.
- Precise atomic publication, bounded partial read and paging, deliberate mutation outcome ambiguity, replay protection, compact successful results and raw-byte result ownership/ACK semantics.
- AW Runtime, Actor, Message, Envelope are the only four runtime primitives; Next/Defer the only hook verbs. The same Agent/Session/TurnJournal owns child model calls and paid-effect facts.
- Exact source kind/id/epoch, principal, execution site, current owner/Session/writer epoch, Envelope and ToolPolicy checks; no implicit grant on tool registration or MCP hint.
- No startup daemon, listener, model call, credentials or background process created solely by loading the shared library or inspecting tool schemas. no_std+alloc core and optional hosted features remain separable.
- Dotlink currently scopes its Job namespace to a live runtime/profile session rather than a multi-principal user database; do not invent an ACL without a concrete deployment requirement. AW requires its separate authenticated Actor/Session parent binding for model wake.
- Do not replace provider-private transcript handling with a generic Job event; no child transcript publication without original parent authorization.

### Out of scope

Arbitrary Graph loops and value dataflow; arbitrary nested Graph/Job node creation; arbitrary model-written OS policy bypass; event-sourced second database; universal crash-resumable dotlink workflows; raw user-directed external notifications; arbitrary remote Agent spawn; additional default tools team/plan/review; blanket privileged tool registries; gratuitous fixture/evidence pinning.

## 1. Canonical AI-native operation API

### One semantic call language

The canonical representation is:

~~~text
Call       = [operation, args]
Graph node = [operation, args, when?]
~~~

Examples:

~~~javascript
["read", {"path":"README.md"}]
["bash", {"command":"cargo test", "background":true}]
["agents.run", {"task":"Review cancellation", "background":true}]
["jobs.get", {"id":"j42", "wait_ms":10000}]
["jobs.send", {"id":"j42", "text":"Focus on the race", "at":"step"}]
~~~

This compact tuple is the semantic/API/call-IR source of truth for Graph nodes, PreparedOperation input, policy input, structured audit/tests and generic script calls. Preparation then partitions execution semantics from title/background/wake before semantic execution hashing and product replay checks.

Standard model transports commonly require one named tool plus an object argument.
For those only, use the grouped object-wire projection:

~~~javascript
agents({op:"run", task, id?, title?, background?, interactive?, wake?})

jobs({op:"list", offset?, limit?})
jobs({op:"get", id, node?, wait_ms?})
jobs({op:"output", id, node?, stream?, offset?, limit?, encoding?, wait_ms?})
jobs({op:"ack", id, node?, stream?, offset})
jobs({op:"send", id, node?, text?, eof?, at?})
jobs({op:"cancel", id})
jobs({op:"subscribe", id, node?, wake})
~~~

The adapter immediately normalizes these to `["agents.run",args]` or
`["jobs.<op>",args]`. This is the **only** secondary spelling. Do not add
nested command objects, `call`, `job_*` aliases to the new schema, or another
canonical representation.

### Constructors

Canonical direct constructors are:

~~~javascript
["bash", {command, dir?, stdin?, timeout_secs?, max_output_bytes?,
          writes?, title?, background?, interactive?, wake?}]

["pwsh", {command, dir?, stdin?, timeout_secs?, max_output_bytes?,
          writes?, title?, background?, interactive?, wake?}]

["batch", {ops, title?, background?, wake?}]

["agents.run", {task, id?, title?, background?, interactive?, wake?}]
~~~

`powershell` may remain an adapter compatibility spelling where required but is
not a second canonical operation name.

`agents.run` with no `id` allocates a fresh authorized child Agent. With an
`id`, it starts a new authorized Turn on that durable Agent. The old Job/Turn is
historical and unchanged; continuation consumes fresh model/tool budgets and never
means "restart the old Job".

Root and child are instances of the same Agent/TurnMachine/ToolRegistry/
ToolPolicy/TurnJournal implementation. Child/subagent is a relationship and
narrowed binding, not a second Agent type. The `agents` namespace exposes only
Agents the caller is currently authorized to delegate/control; it does not grant
the current root Agent self-management automatically.

A direct fresh Agent run creates two identities: durable Agent ID and live Job ID. The Agent ID resolves the existing durable Session/lineage internally; Session remains authoritative storage but is not the primary model-facing handle.

A fresh `agents.run` result always makes the authorized durable Agent ID recoverable, even when the Turn completes synchronously and its internal Job never needs exposure. The Job ID is returned when the Job is exposed/handed off. For an exposed Agent Job, `jobs.list/get` include the Agent ID when the caller has continuation authority, so a lost constructor receipt can recover both live execution and durable conversation identity. A Graph Agent node discloses its Agent ID only when the caller is authorized to continue that Agent.

### Invocation fields

| Field | Semantics |
| --- | --- |
| title | Optional short human/audit intent label, 1..160 printable UTF-8 bytes; no desc/description/label alias |
| background | Direct-call response disposition only. false/omitted optimistically waits/hands off same Job; true exposes same Job immediately |
| interactive | Execution semantic: retain the constructor-specific authorized live input channel |
| wake | Separately revisioned parent-owned event-to-wake control sidecar |

`title` is never permission, effect, scheduling proof or semantic execution
identity. Store it with the initiating intent; list/get may show a bounded compact
projection, while routine constructor/output success never echoes it.

`background` never changes the operation/effect or creates another execution. A fast foreground execution and a background handoff are views of the same Job. **Graph nodes never advertise or accept `background`** because the enclosing Graph Job already owns asynchronous execution.

Preparation explicitly partitions constructor arguments:

~~~text
ExecutionArgs   operation semantics, including interactive where supported
Metadata        title
Disposition     background
Subscription    wake
~~~

The compact tuple is the canonical **call** IR, but the semantic execution identity/hash is derived from operation + normalized ExecutionArgs, not title/background/wake. Transport replay/dedup may additionally bind the complete normalized invocation envelope to reject a redelivery that changes metadata/disposition/subscription under the same external request identity; such a mismatch must never spawn another execution. The first successfully admitted creation metadata/subscription is authoritative for that creation. Later wake changes use `jobs.subscribe`, not constructor replay.

For Graph Agent/Process nodes, `interactive` remains semantic where the host supports it. A node-scoped `wake` is extracted from the call args and captured as subscription/control metadata before execution starts; it is not part of the immutable execution args and never rewrites the Graph.

`batch` is a top-level Graph constructor, not a Graph-native node: vNext does not introduce nested Graphs/Jobs. `jobs.*` management operations are likewise direct model/SDK control-plane operations, not Graph nodes initially. Graph-eligible leaves are the canonical native operations plus host-advertised Process/Agent nodes such as `bash` and `agents.run`.

Existing team/plan/review_release remain opt-in governed workflows with their
existing evidence/approval boundaries; they are not core primitives.

### Jobs: one live-execution control plane

- `jobs.list`: authorized exposed Jobs only; compact ID/kind/status and bounded
  title when present, never bulk output.
- `jobs.get`: status/lifecycle/progress only; whole Job or one node; never output,
  transcript or implicit ACK.
- `jobs.output`: outbound retained stdout/stderr/result data with exact raw-byte continuation/EOF/truncation. Agent output is the bounded public execution result, never private provider transcript or hidden reasoning. Agent provider token deltas are not an implicit inter-Agent stream; incremental public evidence would require an explicit future publication contract.
- `jobs.ack`: monotonic caller-owned retained-byte release; never another
  owner's replay/Task claim.
- `jobs.send`: inbound live execution input. Agent target requires nonempty text
  + `at`; shell target requires retained interactive stdin and text XOR eof and
  rejects `at`.
- `jobs.cancel`: idempotent whole-Job cancellation; no initial per-node cancel.
- `jobs.subscribe`: replace/clear one authenticated caller's future wake rule;
  no historical replay or separate subscription handle.

For one-node Jobs, node may be omitted. In a Graph, stable 1-based node coordinate
selects a child execution: `(job id,node)`. Durable Agent ID is separate.

A live Agent Turn is steered through `jobs.send`. A settled Agent cannot be sent
new Job input; use a fresh `agents.run({id,task})`, which creates a new Turn and
new Job.

No new advertised job_start/job_restart/job_list/job_get/job_output/job_ack/
job_cancel family exists once vNext is explicitly enabled. Legacy routes stay at
the compatibility boundary until intentional cutover.

### Naming normalization

- Semantic operation IDs: `agents.run`, `jobs.list`, `jobs.get`,
  `jobs.output`, `jobs.ack`, `jobs.send`, `jobs.cancel`,
  `jobs.subscribe`.
- Kernel-safe capability projection: `agents_run`, `jobs_list`,
  `jobs_get`, ... . Old `job_*` names are compatibility, not permanent
  canonical grant vocabulary.
- Use id for namespace-local resource ID, node for Graph coordinate, stream for
  stdout/stderr/result, offset/limit for paging, wait_ms for observation wait,
  at for Agent scheduling, wake for event-to-wake policy, title for audit metadata
  and background for direct-call immediate handoff.
- Command `timeout_secs` and observation `wait_ms` are independent.
- Existing native filesystem argument conventions remain DL-150 unless separately
  migrated.

## 2. Effect semantics and policy resolution

### The five effects

| Effect | Type of operation | Examples |
| --- | --- | --- |
| Read | Observe state or content without modifying the requested object | read, find, jobs.list, jobs.get, jobs.output |
| Write | Change filesystem/application data | write, edit, patch, cp, mv, rm |
| Process | Start or execute a process or model Turn | bash, pwsh, agents.run |
| Control | Manage an already-admitted execution, its input, retention, cancellation or wake configuration | jobs.ack, jobs.cancel, jobs.send, jobs.subscribe |
| Unknown | Cannot safely classify effects from trusted registry semantics | opaque untrusted external tool |

Control is an Effect classification and tool scheduling/journal attribute, **not** a new AW runtime primitive or a synonym for ActorMessage::Control. jobs.send to Agent is a Control-effect tool call that delivers ActorMessage::Input. Actual internal process termination remains an Actor/Process Control message/service; an event is Notify. Keep these dimensions orthogonal.

Control is a hard barrier for ordinary speculative Tool read waves, but authenticated out-of-band Runtime Control must remain serviceable while a Job/Agent waits, or cancellation/steering/approval can deadlock.

Unknown is intentionally conservative and never silently downgraded based on a remote read-only hint. An uncertain execution outcome named unknown is a different concept from Effect::Unknown.

No Composite authority effect. A Graph is a fixed container whose nodes have individually classified effects; graph creation preflights every known node, including branches that may later skip. A non-authoritative `EffectSet` may summarize the leaf effects for audit/scheduling/policy presentation (for example `{Read,Control}`), but it is never grantable and never replaces per-node authorization.

### Operation identities and exact grants

AW currently has one ToolSpec.effect per tool and a kernel capability Catalog requiring unique names, with kernel-safe names limited to alphanumeric, underscore and hyphen. vNext resolves a semantic operation **before** authority/journal admission.

Illustrative internal shape:

~~~rust
struct PreparedOperation {
    id: OperationId,         // "read", "agents.run", "jobs.cancel"
    internal_cap: CapName,   // "read", "agents_run", "jobs_cancel"
    effect: Effect,
    normalized_args: Args,
    requirements: Requirements,
    footprint: Footprint,
    // source/authority/runtime/result metadata captured by existing owners
}
~~~

For Graph/batch, preparation yields immutable prepared node operations plus a non-authoritative EffectSet summary. There is no `Effect::Composite` and no outer `Unknown` grant merely because several node effects are present.

- The canonical tuple provides the semantic operation directly. The object-wire grouped adapter validates `op`, removes it and resolves `jobs.<op>` / `agents.<op>` before authorization.
- Built-ins use one finite trusted operation table. A model cannot supply effect, capability identity or requirements.
- Exact grants use kernel-safe semantic projections such as `agents_run`, `jobs_get`, `jobs_output`, `jobs_ack`, `jobs_cancel`, `jobs_send`, `jobs_subscribe`, plus existing exact source/actor/site/effect/epoch.
- A bare public `jobs` or `agents` tool name grants nothing. Advertise each grouped tool only when at least one operation is currently allowed, and constrain its `op` enum to the current effective subset.
- Simple tools still resolve directly (`read` -> Read, `bash` -> Process). All paths carry the same PreparedOperation through ToolPolicy/Envelope, journal, scheduler and executor rather than creating a parallel admission system.
- Legacy `job_*` routes translate only at a bounded compatibility boundary during deliberate rollout; they do not become duplicate persisted grant identities.
- Source/epoch/site, catalog/authority revision, current writer generation and object-level ownership remain independently checked; no MCP/extension manifest may mint privileged built-in operation identities.

### Resource permission != effect

Operation effect does not itself confer resource access.

- jobs.cancel: Effect::Control, requires exact job.cancel for the particular authorized Job and current owner/session; no filesystem write or shell spawn grant.
- jobs.ack: Effect::Control, requires retained result owner/right, cannot release bytes required by another replay/MCP Task owner.
- jobs.send to Agent: Effect::Control plus exact agent.steer (target current generation/parent lineage, original Session writer and child delegation).
- jobs.send to shell: Effect::Control **plus** process.input for that live owned process, bound to original launcher/sandbox authority. Sending bytes can cause arbitrary shell effects; Control may not smuggle shell capability past current process and network/filesystem policy.
- jobs.subscribe: Effect::Control plus exact job.subscribe; the optional wake rule additionally requires agent.wake on the actual authenticated parent, revalidated at event delivery.
- bash/agents.run creation: Process effect, requires exact interpreter/process or Agent provider/delegation authority and budget, independently of filesystem ReadOnly.

For cp or edit, coarse Write still requires read-source + write-destination (or read+write) as defined in DL-150. Multiple resource checks do not imply another Effect::Composite category.

### Read-only sessions

Currently the AW envelope predicate rejects all non-Read effects in Access::ReadOnly and gates Process/Unknown on process permission. That is incompatible with independent Control and granted bounded child Agents; **do not merely weaken the conditional**.

Define access dimensions precisely so Access::ReadOnly continues forbidding native filesystem/data mutation while separate explicit grants/budgets govern Process and Control. Maintain the old denial behavior for all configurations that lack new explicit grants. Any new allowance requires exact host opt-in and current ancestor narrowing.

Read-only example:
- FS Read only; FS Write denied.
- Graph of read/find/grep allowed.
- Job list/get/output/ACK/cancel allowed on admitted owned/read-only Jobs if independently granted; no fs.write.
- Shell spawn denied unless separately granted, regardless of job.cancel permission.
- Bounded child Agent may be allowed without shell, filesystem Write or unauthorized model/provider selection.
- Agent wake and steering explicitly denied unless separately granted.

Because Control changes runtime state, it is not represented as a Read merely to pass a ReadOnly gate. Control, Write, Process and Unknown remain separate scheduler classifications and journal evidence.

### Static + dynamic admission

One authoritative execution path:

~~~text
canonical [operation,args]
    -> strict schema / normalize
    -> PreparedOperation: trusted operation + effect + requirements + footprint
    -> current exact ToolPolicy and narrow Envelope
    -> optional host-selected dynamic policy
    -> validate any narrowed proposal; re-resolve PreparedOperation
    -> fresh full static authority + source/epoch + owner/writer recheck
    -> original journal intent/attempt reservation
    -> original executor
    -> factual receipt and per-call disclosure recheck
~~~

Policy may use LocalRules, authorized Program, constrained QuickJS/Python, MCP, model reviewer, model pipeline or authenticated Human. Runtime Next/Defer and original owned continuations handle asynchronous decisions; do not add a second paid Agent loop or policy daemon.

Evaluator returns typed allow/deny/narrow/ask **data**. It can never supply a new tool, permission, model payer, root, site, exec authority or credential. A narrow proposal is not assumed monotone just because JSON fields were deleted: revalidate the original/revised operations fully under the same source/intent and exact host revision policy.

Program/model pipeline:
- Explicitly host-configured executable/route and permitted context, separate effect/budget/timeout/attempt evidence.
- No implicit OS sandbox for arbitrary same-UID native code; externally executed scripts get actual configured isolation where needed. Model/code reviewer output is untrusted regardless of provider.
- Deterministic cheap rules first. Paid review only when necessary. No recursive self-review except bounded, separately authorized arrangement.
- Timeouts, malformed output, revocation or unknown model cost fail closed; do not rerun possibly paid attempts.
- Cached reviewer decisions are tied to exact normalized request/source/effect/actor/site/Session/policy revision and expiry; reuse is evidence of a decision, not new grant.
- Host-initiated safety shutdown/cancellation remains available independently of an optional model-based review service; model-issued cancel still requires exact Control authority. No deadlock while a parent is waiting for reviewer/approval.
- Changed policy can narrow pending work; never widen captured authority for running Jobs/children. Revocation halts unstarted work, lets publication-critical native mutations settle, fences retained disclosure where required and records unknown hazards.

## 3. Job identity, lifecycle and observation

### Identity and owner

- One public Job ID per admitted top-level run; a Graph node is (id, coordinate), not another public Job.
- Durable Agent identity is retained across Turns and resolves the authoritative Session/lineage internally. It is distinct from Job execution identity. A later `agents.run({id,task})` creates a fresh Turn and Job on that Agent.
- Graph JobManager remains the only canonical Graph/Node ledger. AW Agent Session/TurnJournal remains the sole provider/effect ledger. A host Agent-node adapter reconciles the child Session's factual outcome into one Graph node checkpoint, never executes two model loops or pretends child Session is a second Graph database.
- Native-only dotlink Jobs remain scoped to the current runtime/profile Job session. A dotlink server with no authenticated AW parent may expose passive status/output but **must not promise model wake**.

### Job get/list

- jobs.list: newest exposed first, live logical offset/limit pagination; compact id/kind/status plus a bounded compact title when present, never bulk output or arguments.
- jobs.get({id}): small whole-Job lifecycle and Graph progress; ({id,node}) one node state/phase; no complete output, no automatic ACK, no implicit model wake.
- Lifecycle distinguishes running, cancelling, cancelled and terminal aggregate status from Graph node outcome ok/unchanged/fail/mixed/unknown/skip. Graph dependency conditions branch on immutable node outcomes, not lifecycle marker cancelled.
- Status and metadata returned only under current disclosure authority; revocation cannot revive previously authorized retained output.

### One bounded wait_ms convention

Both get and output accept an optional nonnegative bounded wait_ms; host configuration caps its maximum and transport deadline. Omitted/zero means nonblocking snapshot. The wait bound never alters command execution timeout, native deadline or Job cancellation.

- get with wait_ms: observe the initial status/progress revision, return immediately if already terminal, otherwise wait for a **meaningful** status/progress change or terminal event. On wait expiry, return current factual status (not an error pretending execution timed out). Avoid wakeups on implementation-only internal notification noise.
- output with wait_ms: return immediately if requested source bytes already exist; otherwise wait until new bytes at offset, definitive stream sealing/EOF, or wait expiry. Never treat a sealed but empty stream as missing output requiring indefinite poll.
- Use subscribe-before-snapshot or equivalent revision-recheck to avoid lost notify between first observation and wait registration. A status revision is an internal wake predicate, not an automatically new model-facing cursor.
- Each wait has a bounded cancellation path and releases subscriber/reader leases; long polls cannot hold per-Job/global execution locks or block processing of parent Actor Controls.

### Output and ACK

- jobs.output returns a typed stream page. Default stream for shell is stdout, for native/Agent node result; explicit invalid stream fails. Graph with >1 node requires node for output.
- Streams: shell stdout/stderr/result (result = compact terminal metadata, not stdout duplication); native and Agent result (typed where applicable). Status/result metadata is not a second complete MCP envelope.
- Output paging is over raw source bytes; text is an explicitly lossy presentation for arbitrary shell output, base64 gives exact bytes. For typed media, use kind, MIME, original byte length, hash and host-permitted typed content block.
- Accurate offset/next_offset/EOF/truncated/incomplete markers are preserved. A truncation of diagnostics is not a failure of a successful shell execution.
- ACK is monotonic, idempotent and bounded by bytes genuinely retained and delivered to that owner; read-only observation cannot silently ACK. Multi-owner native Task/replay delivery leases keep bytes until every owner releases them.
- Existing result-ceiling, output-spool and mutation-limit budgets stay enforced. No unbounded attachment cloning, repeated base64 in model context, or metadata echo.

## 4. Events, subscriptions and Agent wake

### Fundamental separation

A **factual event** is emitted by the authoritative Job/Agent/Process state owner. A **wake** is a separately authorized Input delivered to a receiving Agent. A subscription is the caller-owned rule mapping event kinds to wake boundary. These are views/services over existing Runtime/Actor/Message/Envelope, not another durable event engine.

Events are observable passively whether or not a wake rule exists. Subscription never implies a model invocation by itself. UI and script watch flows are passive (and must not become owner grants).

### Public shape

~~~javascript
// Canonical compact form at direct creation:
["bash", {"command":"cargo test", "background":true,
          "title":"Run tests", "wake":{"settled":"turn"}}]

["agents.run", {"task":"Review concurrency", "background":true,
                "wake":{"settled":"turn","attention":"now"}}]

["batch", {"ops":[/* fixed graph */], "background":true,
           "wake":{"settled":"turn"}}]

// Canonical rule replacement on existing work:
["jobs.subscribe", {"id":"j42", "wake":{"settled":"step"}}]
["jobs.subscribe", {"id":"j42", "node":2,
                    "wake":{"settled":"step","attention":"now"}}]
["jobs.subscribe", {"id":"j42", "wake":{}}] // clear

// Object-wire fallback where required:
jobs({op:"subscribe", id:"j42", wake:{settled:"step"}})
~~~

Schema of wake is exactly:

~~~text
wake: { settled?: "now" | "step" | "turn",
        attention?: "now" | "step" | "turn" }
~~~

- settled fires once on the factual terminal outcome, including success, failure, cancellation and unknown.
- attention is an explicitly authenticated, bounded child Agent request for intervention. Arbitrary shell output, stderr, log text or external MCP data cannot forge this event. Each attention needs a unique source event identity and finite quota.
- No model wake on every progress tick/output chunk initially. Progress/output readiness remains passively observable with get/output and host SDK watch.
- No generic raw push-to-user notification destination is introduced. Recipient is the authenticated parent/Agent controller binding selected by the host, never a model-supplied arbitrary identifier.
- At creation, the Job/node rule is captured and registered atomically **before roots may start**. The same wake shape on Graph child node args is registered as node-scoped subscription metadata, not executable node argument rewriting.
- jobs.subscribe replaces one caller-scoped rule atomically; no additional subscription ID or model-facing unsubscribe tool. Empty map clears. A rule installed after terminal state does not retroactively invoke a model or deliver old events.
- A Graph-level rule observes the Graph; a node-level rule observes one node. No automatic propagation of node-level attention to an unrelated ancestor. A host may correlate node event (id,node) to its authorized parent without giving that child a parent-Control grant.

### Canonical at semantics

The only public values are now, step and turn, always relative to the **receiving Agent**:

| at | Active receiving Agent | Idle receiving Agent |
| --- | --- | --- |
| now | Earliest safe interruption/admission point; never mid native commit or erased paid attempt | Next authorized Turn |
| step | After the current Step (model invocation + complete proposed tool batch and receipts), before next Step if still valid | Next authorized Turn |
| turn | Queue as a new Turn after the current Turn factually settles | Next authorized Turn |

- An input with a requested exact active Step/Turn that already passed must be rejected as too late, not silently pushed into another cycle. Explicit relative-at for an idle Agent may map to its next Turn.
- Do not equate after-step with a generic before-tool-batch checkpoint: AW Step includes the tool batch, and safe continuation must be defined/implemented honestly.
- A now wake may ask for urgent steering, but interruption of an in-flight paid provider, tool effect or approval does not erase its original receipt or prove non-execution; preserve unknown hazards and controlled settle.
- A wake may be admitted/queued/deferred/rejected; only a separately journaled original Agent Turn proves it was consumed. Control/Notify/queued Input classes remain distinct from effect Control.
- Child event producer **cannot steer its parent**; only the authenticated parent-owned subscription/runtime can convert a factual event to a permitted Input.

### Exactly-once wake request, not fictional exactly-once inference

Give each factual event an immutable identity derived from (authoritative origin generation, Job ID, optional node, event kind, source sequence/terminal revision). Pair it with the scoped subscription revision and recipient identity. Use that as the idempotency key for **one wake admission**.

Critical ordering:
1. Constructor validates wake permission/recipient and registers the rule with the reserved Job identity before roots/runner launch.
2. Execution owner records the authoritative event (for shell, only after Process exit classification and stdout/stderr writer quiescence/Job settlement; for Agent, original child Session/Turn journal settles first).
3. The adapter observes that committed transition and projects bounded event metadata, not raw stdout or private answer.
4. Reconcile the initiating response disposition: if terminal result was already delivered synchronously to that same parent, mark the event covered and **do not wake again**. If handoff occurs, deliver the event once even when it settled before creation response/handoff.
5. Evaluate the current parent's exact wake grant, owner writer epoch, subscription revision, budget and optional dynamic policy; prepare source-labeled Actor Input with the chosen at boundary. Persist/admit through existing AW owner queue/notice receipts, not another queue.
6. Acknowledge only actual enqueue/admission, then separately report consumed/settled parent Turn. If outcome is ambiguous, fence/deduplicate and require inspection; never create another paid Turn to compensate blindly.

No hook can synthesize terminal success from stream progress. No missed wake on fast Job completion, dropped caller response or the create-to-register gap. A cancelled/incompatible authority may suppress wake but must leave factual terminal state accessible to the authorized owner.

### Shell-specific completion integration

Dotlink currently has a host Job completion record and internal waiter notification, including JobFinisher writer quiescence and shared JobManager settlement. **There is no automatic AW parent Agent wake today.**

Implement a narrow host adapter/event projection from **the same** final settlement transition:
- shell exit/status/timeout/cancellation/unknown -> shared settled event with Job ID, node coordinate, status and optional exit code; no stdout/stderr duplication.
- Process finishing before output streams seal is not yet settled for wake. Do not send early.
- Delivery to AW Actor requires an explicitly configured, authenticated parent owner/Session/site/generation binding. Standalone dotlink has no implied parent; reject wake at validation or omit unsupported field from advertised schema.
- Native MCP Task completion/status and old OpenAI replay ownership stay supported and must not trigger a second execution or a double wake.
- Dotlink live Job session replacement cannot resurrect old Jobs; a different runtime session cannot inherit old wake grants merely by matching a display ID.

### Event delivery / restart limits

- Treat UI/MCP progress/broadcast as advisory and coalescible; terminal/attention facts must survive via existing factual Job/Session state and appropriate ACK/owner custody. Avoid a new general-purpose event log.
- A pending AW accepted Input may be handled by its current durable encrypted queue/writer as already designed; a lost/unknown accepted Input must not be replayed as a fresh prompt without inspection.
- An AW Session can be durable without the old external shell Process remaining alive across process crash; do not claim physical process resurrection or cross-process Job continuation absent a new separately approved durability design.
- If the wake recipient has no active/writable owner, record a passive terminal outcome and explicit delivery failure/blocked state as supported, not "wake succeeded". Return fail/unsupported for a requested wake that cannot be safely represented at creation.

## 5. Interactive Process and child Agent input

### jobs.send

Agent case:
~~~javascript
["jobs.send", {"id":"j42", "node":1,
               "text":"Focus on symlink races", "at":"step"}]
~~~

- Valid only for an authorized active interactive Agent execution. Relationship alone does not grant peer/parent steering.
- Uses original ActorMessage::Input and current Session writer, with source sequence/idempotency, budget, safe-point admission and optional dynamic policy.
- Not a prompt rewrite or a direct mutation of an already signed/paid model message.
- Default child->parent collaboration remains factual attention/settlement + parent-owned wake subscription. Direct child/peer->parent `jobs.send` needs an explicit communication topology/grant.
- Only the current active Turn can be steered. For a settled Agent, a new authorized `agents.run({id,task})` creates a new Turn and distinct Job without changing the settled Graph result.

Shell case:
~~~javascript
["jobs.send", {"id":"j43", "text":"yes\n"}]
["jobs.send", {"id":"j43", "eof":true}]
~~~

- Requires interactive stdin explicitly held open at creation and exact current owner/process input permission. Reject at on shells and eof on Agents.
- Text input is bounded and the host journals intent before writing; a successful tool call proves host-side write/flush as supported, not that the child app consumed the bytes.
- eof closes owned stdin as a distinct idempotent request. Never re-open original stdin or replay ambiguous writes.
- PTY/resize/freeze are optional specialized Process controls, not needed to overload the minimal jobs API. No shell syntax '&' treated as durable Job creation.

### Control authority and safety

- Control effect is a model Tool scheduling/journal category; the authenticated Runtime's out-of-band host cancellation path stays serviceable while a suspended Model or Job is awaiting changes.
- jobs.cancel is whole Job-wide, propagating to live child Agent/Process nodes while preserving settled and unknown evidence. Cancel is not successful rollback and cannot claim process killed without real custody proof.
- If a Graph Agent node is waiting on parent attention, the parent must still accept wake/Input and inspect output. Graph scheduling cannot hold the only Actor admission lock while awaiting child/approval.

## 6. Immutable Graphs with typed Agent nodes

### Graph invariants

- Keep existing fixed coordinate graph grammar, deterministic dependency expression/when, static node references, bounded per-node work, outcome branches, topology and conflict footprints.
- The requested Graph is immutable after admission: node set, initial args, dependency edges, resource/authority ceiling and semantic work budgets.
- An eligible host-registered `agents.run` node starts one bounded Turn on a fresh or explicitly authorized existing Agent. Node output is a typed bounded public result; provider-private transcript stays in its original Session.
- An opted-in interactive child may receive separate authorized Actor Input or Control while running. Its actual model/tool choices can vary without changing Graph structure.
- Node terminal status commits only after actual child Session settling and reconciliation, including provider/effect Unknown; downstream Graph conditions use that one committed node outcome.
- Graph cancellation is propagated to children but cannot erase a completed child, duplicate an Agent invocation or bypass a native publication checkpoint.
- Graph Plan immutable and node Input stream mutable are **different objects**; subscription changes are out-of-band control metadata. Retain original event/input provenance.

### Proposed Node adapter (host-neutral)

aw-tools does not depend on AW Agent and must not embed an LLM provider. Extend the existing host Node executor/registry with a transport-neutral trusted host-node capability bound at Graph preflight to exact callbacks, budgets and authority. AW registers `agents.run` through that adapter. Only AW-capable hosts advertise it; standalone dotlink must not pretend it can spawn Agents without an explicit authenticated AW integration.

One host Node adapter:
1. validates child task/interactivity/title and parent lineage;
2. reserves child capacity and immutable narrowed Envelope/provider/account routing revision;
3. binds a fresh child Session or authorized continued Session under original AW SessionStore;
4. invokes existing Agent/TurnJournal once per admitted Turn via the existing controller;
5. exposes get/output/status through a Job node projection, never a second provider execution owner;
6. implements send/cancel/wake through exact Actor/Session control operations;
7. publishes a single terminal Graph node outcome from factual child settlement.

Initial Graph Agent `interactive=false` by default. Graph nodes never accept `background`. Node subscription wake may be declared in its args at Graph creation and is captured as control metadata before execution starts. A Graph node cannot register/execute an arbitrary dynamic tool outside the host-approved delegated Agent grant.

### No ambiguity in nested identities

- Graph Job ID + node coordinate selects an in-flight child node for jobs.get/output/send/subscribe.
- Host may disclose child Session ID only to an authorized caller who can legitimately continue it.
- `agents.run({id,...})` after settlement is a new Job/Turn on the same durable Agent, not a modification or restart of the old node; old Graph dependencies/status never change.

### Existing team and planner

Leave opted-in team and plan services intact. They may construct/coordinate ordinary Agents under their existing approved routes and review gates, but the normal model-facing agents+batch+jobs surface is sufficient for baseline delegation. Do not duplicate planner execution as a second Graph/Agent runtime.

## 7. Typed Graph result retention, including bread

aw-tools direct results already distinguish Text, exact Read with independent status, and MediaPacket from bread(format=mcp). Current graph_text(Output) returns None for media, making bread direct-only.

Do not keep that blanket exclusion. Generalize retained Graph node results to a compact typed body/descriptor:
- Text: exact/adapted text as originally supplied.
- Read: exact source bytes plus separate status, continuation and numbering contract.
- Bytes: exact retained bytes/window with encoding presentation selected later.
- Media: verified MediaPacket descriptor + retained exact bytes, kind/MIME/byte count/hash; no unvalidated provider attachment.
- Result metadata/outcome remains separate from stdout/stderr and does not duplicate a public MCP CallToolResult.

Retain original media policy: signature is not full codec validation; provider/host accepted MIME, size, signed custody, security and destination disclosure checks before delivery. MIME and path labels are never authority.

Graph and native-node statuses are computed from canonical direct semantics before result presentation. Media representation must not silently flatten to text or change the node's success/unknown decision. Retention is bounded; release/ACK uses original raw-byte ownership and transport replay leases.

Graph remains an execution DAG, not a dynamically typed dataflow language that passes node content into arbitrary next-node arguments.

## 8. QuickJS/Python and Rust SDK projection

The compact tuple is the generic SDK contract:

~~~javascript
await tools.call(["read", {path:"README.md"}])

const run = await tools.call(["agents.run", {
  task:"Review the parser", title:"Review parser",
  background:true, wake:{settled:"turn"}
}])

await tools.call(["jobs.get", {id:run.job, wait_ms:10000}])
~~~

Typed ergonomic methods are sugar over the same resolver:

~~~javascript
const run = await tools.agents.run({
  task:"Review the parser", background:true
})
await tools.jobs.send({id:run.job, text:"Check cancellation too", at:"step"})
await tools.jobs.output({id:run.job, offset:0, limit:8192, wait_ms:10000})
await tools.jobs.subscribe({id:run.job, wake:{settled:"step"}})
~~~

Python exposes equivalent typed methods and `tools.call([op,args])`. Atomic typed SDK names stay as flat as their canonical operations (`tools.read`, `tools.bash`, `tools.batch`); resource families use `tools.agents.run` / `tools.jobs.get` etc. No wrapper may implement independent semantics: all paths resolve the same canonical operation, schema/effect/requirements, ToolPolicy/Envelope, dynamic policy, journal and executor.

A passive script iterator such as `tools.jobs.watch(id)` is optional observation sugar over the same Job revisions and must not create another model tool, durable subscription engine or authority source.

Use host-mediated file/process/network APIs only. No ambient QuickJS/Python filesystem, subprocess, network, model/payer, unauthorized plugin or foreign runtime escape. Any future model-facing `code(...)` requires a separate sandbox/admission decision and does not block core vNext.

## 9. Transport/product integration and migration

### AW

- Extend pure kernel Effect with Control, exact-grant and scheduler mapping; preserve no_std+alloc.
- Add trusted operation resolution in aw-agent-core ToolRegistry/ToolPolicy and independently verify user-owned Envelope and Session authority. Existing journal pipeline remains the only paid/effect proof.
- Thin aw-tools-adapter adds grouped jobs/agents wire projections and host Process/Agent-node adapters; reuse the original Agent, child Actor relation service, Process supervisor, SessionStore/TurnJournal and provider media bridge.
- Child Graph nodes register only where AW host can provide authentic child execution; default minimal/stateless builds do not suddenly require a database, actor owner daemon, or policy model.
- Dynamic evaluator composes existing typed hooks/reviewer execution and cannot replace mandatory grants/receipts.

### dotlink

- Consume aw-tools via pinned Git rev as currently; do not develop a duplicate Graph/Job or filesystem implementation.
- Map profile/capability grants into new exact trusted operation table; retain existing sandbox/launcher, OpenAI replay and request transport ownership.
- Surface grouped jobs once op-level schema, replay, output and cancellation migration gates are green. `agents.run` is advertised only by a real AW-capable host; standalone dotlink does not invent it. Until cutover preserve job_* and MCP Task mapping. Old schemas may deserialize at a bounded compatibility boundary but must not leak aliases into the new advertised schema.
- Use same factual Job settlement event. Only advertise wake when the *actual AW hosting adapter* has authenticated a parent Agent; standalone tunnels cannot infer one from client-request title/session strings.
- Do not implicitly run a shell/process in an existing read-only profile because Control effect or Job observation is granted.

### Rollout order

1. Introduce internal effects/operation resolution with compatibility projection; tests prove no former denial gets silently widened.
2. Shared Job observation and typed result consistency; dual old/new adapter conformance before name cutover.
3. AW parent wake and read-only management, with shell finish -> event and child Agent settlement -> same event.
4. Interactive shell/Agent host controls and Graph Agent nodes; verify child authority and Graph immutability.
5. Dotlink vNext public schema opt-in/migration; remove old advertising only at explicit approved boundary, preserve wire compatibility as intentionally required.
6. Optional script APIs only after same Rust dispatcher/permission semantics have passed existing platform and product validation.

No version bump, push, release or automatic migration is authorized by this finalized design.

## 10. Failure matrix and acceptance tests

Use a compact contract-focused suite with existing fixtures and unit/integration suites, not a large per-field evidence matrix.

### Security/authority

- ReadOnly FS + permitted jobs.get/output/cancel/ack/subscribe works without any file write or shell execution; denied Control op remains denied even when another jobs op is available.
- Full exact grant on internal jobs_* name/source/epoch/effect/actor/site; forged op, stale source, stale owner/writer epoch, wrong Job or node, revoked policy, broader child Envelope all fail closed.
- jobs.send shell input cannot bypass original Process authority; child Agents cannot publish private transcript or steer parent merely by sending Notify.
- Model/program policy allow/narrow/ask cannot grant absent Process/file/Control rights; malformed, timeout, paid-unknown and policy-revision changes are fenced; no recursive unbudgeted reviewer.

### Execution and Graph

- Graph of read-only nodes passes in read-only mode, while a single unauthorized conditional mutating node prevents Graph admission before any root starts.
- Static Graph is immutable while an authorized interactive child receives ordered now/step/turn messages. Stale exact boundary is rejected; unknown effect not auto-retried.
- Agent subtree cancel settles accurately, descendants no new effects after revoke, pending Graph nodes become skip as appropriate; no deadlock while parent awaits child attention/approval.

### Completion and wake

- Very fast bash exit before constructor response; synchronous result suppresses duplicate parent wake; handoff guarantees one wake request when subscribed. Both success and failure/timeout/cancel/unknown terminal outcomes report accurately.
- Shell Process exit before stream writer quiescence does not wake early. Unknown OS stop is reported unknown, not successful cancellation.
- Graph-level vs node-level wake, duplicate event/redelivery, subscribe replacement/removal, subscribe after terminal, immediate policy revoke, missing recipient owner, full parent mailbox and parent restart/unknown Input are all handled without forged exactly-once inference.
- No child/log/stdout content can impersonate Agent attention; no unbounded progress-triggered paid model Turn loops.

### Observation/output

- get and output wait_ms return immediately when status/data already exists, wake on genuine change/data, return truthful unchanged/end-of-stream state after bounded timeout, and never block controls while waiting.
- Typed bread(format=mcp) inside Graph retains image/audio/resource bytes with correct host media disclosure; output/ACK raw offsets and replay/native Task owners preserved; no duplicates, truncated stream counted separately.
- Original 13 filesystem semantics, DL-150 status/modifiers, Graph predicates and native mutation safety unchanged.

### Product/runtime gates

- Run existing shared aw-tools/Unix/Windows source/adapter/Agent tests, targeted Rust lint/format/check and product integration tests after each affected implementation phase, not a ceremonial excessive collection of fixtures.
- Keep pending Windows/macOS native and Bubblewrap kernel smoke tests explicitly pending if the host cannot execute them. Do not claim full platform certification from source inspection.
- Prove minimal/stateless, no-std kernel builds and dotlink pinned Git build without new default dependencies. Measure total advertised schema bytes/token estimate before/after; justify significant size growth by real model-task benefit.
- Preserve dirty/unrelated work, existing seal/checkpoints, versions and owner-controlled plan indexes until explicit implementation/commit/push approval.

## 11. Final decision record + implementation-time checks

### Finalized design decisions

- Canonical semantic/API/IR call is `[operation,args]`; Graph node is
  `[operation,args,when?]`.
- Grouped object-wire `jobs({op,...})` and `agents({op,...})` is the only
  secondary representation; no nested command-object form.
- `agents.run` replaces subagent as the semantic model operation. Root and
  delegated children use one Agent implementation; child/subagent is lineage.
- One Agent Turn = one live Job. Durable Agent identity continues through new
  Turns; a settled follow-up is a fresh `agents.run({id,task})`, not Job restart.
- Direct bash/pwsh, batch and agents.run may use `background`; it is response
  disposition only. Graph nodes never contain background.
- Jobs is the sole live-execution control plane:
  list/get/output/ack/send/cancel/subscribe.
- `jobs.send` is inbound input; `jobs.output` is outbound
  stdout/stderr/result. Agent result never exposes the private transcript.
- `wait_ms` belongs on get/output; no model-facing wait/watch handle.
- Wake is parent-owned mapping of factual settled/attention events to authenticated
  Agent Input at now/step/turn; no progress/output-driven paid wake initially.
- Five effects include Control; no Composite authority. Internal EffectSet is
  summary only.
- Semantic capability identities are `jobs.*`, `agents.run`; kernel-safe
  projections are `jobs_*`, `agents_run`. Old `job_*` is compatibility.
- Read-only filesystem authority is orthogonal to separately granted
  Control/Process/Agent delegation.
- Graph remains immutable while interactive Agent/Process execution may receive
  separately authorized live input.
- Typed result retention makes all canonical filesystem operations, including
  `bread(format=mcp)`, Graph-eligible.
- QuickJS/Python/Rust use the same PreparedOperation path; wrappers cannot bypass
  schemas, policy, journal or executor.
- Team/plan/review remain host-opt-in governed workflows, not core primitives.
- `title` stays optional <=160 UTF-8 bytes, non-authoritative, available in
  bounded list/get metadata and not echoed in routine results.

### Implementation-time checks, not design questions

1. Verify the exact source/ABI migration required to add Control across
   aw-agent-kernel/aw-agent-core without widening legacy grants.
2. Confirm current AW/dotlink Process owners can retain interactive stdin on each
   supported platform. Where not provable, hide/reject interactive send.
3. Bind wake only when a real current authenticated parent Agent/owner/writer is
   available; otherwise hide/reject wake.
4. Fit the generic host-node callback into existing NodePlan/JobManager lifetime
   checkpoints without another Agent/Graph manager.
5. Implement get/output wait revisions and stream sealing through one race-free
   notification mechanism, not a persistent universal event bus.
6. Measure grouped object-wire schema bytes/tokens against current job_* surface;
   preserve compact model success while keeping strict per-op validation.
7. Re-read live source/dirty work at implementation start; this finalized design
   does not authorize source edits, compatibility cutover or release.

## 12. Next-agent source map

Read (1) both AGENTS.md; (2) both .agents/plans/README.md and relevant current TODO; (3) dotlink current canonical cross-repo handoff; (4) sealed DL-150 README and SHARED-IMPLEMENTATION; (5) AW shared-tool convergence and two AW runtime design documents; (6) live source. Most relevant implementation points:

AW:
- crates/aw-agent-kernel/src/capability.rs, message.rs, envelope.rs, turn.rs
- crates/aw-agent-core/src/tool.rs, agent.rs, control.rs, runtime.rs, hooks.rs
- crates/aw-tools/src/filesystem/{schema.rs,direct.rs,batch.rs,job_manager.rs}
- crates/aw-tools/src/media.rs
- crates/aw-tools-adapter/src/{lib.rs,job_tools.rs,job_host.rs,job_runtime.rs,process.rs,shell.rs}
- src/{subagents.rs,child_actors.rs,actor_owner.rs,model_management.rs,subagent_teams.rs,subagent_planner.rs}

dotlink:
- src/{jobs.rs,mcp.rs}
- src/mcp/tools/{jobs.rs,shell.rs,graph.rs,shared_native.rs}
- current transport replay and authority code as discovered from live source.

Use **live source/tests** rather than assuming the path list is complete or implementation state is frozen at these historical baselines. The old implementation and current dirty work must not be overwritten merely because this finalized design describes the future API.
